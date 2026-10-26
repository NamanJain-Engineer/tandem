//! `activity` (F-EX-07): the ledger's own events. Rows hold redacted one-line
//! summaries and names or numbers in `detail_json`, never prompt text, file
//! contents, output or secret values.

use rusqlite::params;

use crate::{Store, StoreError};

/// Events kept this long.
pub const KEEP_DAYS: i64 = 14;

/// At most this many events.
pub const KEEP_ROWS: i64 = 50_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityRow {
    pub id: i64,
    /// `pane` | `thread`
    pub source_kind: String,
    pub source_id: String,
    /// `turn` | `tool` | `subagent` | `approval` | `run`
    pub kind: String,
    pub at: i64,
    pub summary: String,
    pub detail: serde_json::Value,
    pub checkpoint_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewActivity {
    pub source_kind: &'static str,
    pub source_id: String,
    pub kind: &'static str,
    pub at: i64,
    pub summary: String,
    pub detail: serde_json::Value,
    pub checkpoint_id: Option<String>,
    /// Pruned by age only, never by count: a decided run, an allowed
    /// approval, a note that events weren't kept.
    pub kept: bool,
}

pub async fn insert(store: &Store, row: NewActivity) -> Result<i64, StoreError> {
    let detail = row.detail.to_string();
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO activity (source_kind, source_id, kind, at, summary, detail_json, checkpoint_id, kept)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    row.source_kind,
                    row.source_id,
                    row.kind,
                    row.at,
                    row.summary,
                    detail,
                    row.checkpoint_id,
                    row.kept
                ],
            )?;
            Ok(t.last_insert_rowid())
        })
        .await
}

/// A window of a source's history: the newest `limit` at or before `until`
/// (ms, inclusive, so events sharing the boundary millisecond aren't
/// skipped; `None` for now), returned oldest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub until: Option<i64>,
    pub limit: i64,
}

impl Page {
    /// Everything (Export).
    pub const ALL: Page = Page {
        until: None,
        limit: i64::MAX,
    };

    fn before(&self) -> i64 {
        self.until.unwrap_or(i64::MAX)
    }
}

/// A source's events in `page`, oldest first.
pub async fn list(
    store: &Store,
    source_kind: &str,
    source_id: &str,
    page: Page,
) -> Result<Vec<ActivityRow>, StoreError> {
    let (kind, id) = (source_kind.to_string(), source_id.to_string());
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, source_kind, source_id, kind, at, summary, detail_json, checkpoint_id
                 FROM activity WHERE source_kind = ?1 AND source_id = ?2 AND at <= ?3
                 ORDER BY at DESC, id DESC LIMIT ?4",
            )?;
            let mut rows = stmt
                .query_map(params![kind, id, page.before(), page.limit], |r| {
                    let detail: String = r.get(6)?;
                    Ok(ActivityRow {
                        id: r.get(0)?,
                        source_kind: r.get(1)?,
                        source_id: r.get(2)?,
                        kind: r.get(3)?,
                        at: r.get(4)?,
                        summary: r.get(5)?,
                        detail: serde_json::from_str(&detail).unwrap_or_default(),
                        checkpoint_id: r.get(7)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.reverse();
            Ok(rows)
        })
        .await
}

/// A checkpoint of the source, and whether its folder had one before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointMark {
    pub id: String,
    pub at: i64,
    /// `turn` | `prompt` | `manual` | `restore`
    pub reason: String,
    pub first_of_folder: bool,
    /// Its workspace, so a link opens the right folder's Checkpoints.
    pub workspace_id: Option<String>,
}

/// A command the source's pane ran (command recall, redacted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandMark {
    pub id: i64,
    pub at: i64,
    pub command: String,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<i64>,
}

/// A thread's approval as the approvals table has it (redacted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalMark {
    pub id: String,
    pub at: i64,
    pub summary: String,
    /// `pending` | `allowed` | `denied` | `expired` | `cancelled`
    pub state: String,
    pub scope: Option<String>,
    pub via: Option<String>,
}

/// The checkpoints a pane or thread took in `page`, oldest first.
pub async fn checkpoints(
    store: &Store,
    source_kind: &str,
    source_id: &str,
    page: Page,
) -> Result<Vec<CheckpointMark>, StoreError> {
    let column = if source_kind == "thread" {
        "thread_id"
    } else {
        "pane_id"
    };
    let sql = format!(
        "SELECT c.id, c.created_at, c.reason,
                NOT EXISTS (SELECT 1 FROM checkpoints e WHERE e.folder = c.folder AND e.created_at < c.created_at),
                c.workspace_id
         FROM checkpoints c WHERE c.{column} = ?1 AND c.created_at <= ?2
         ORDER BY c.created_at DESC, c.id DESC LIMIT ?3"
    );
    let id = source_id.to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(&sql)?;
            let mut rows = stmt
                .query_map(params![id, page.before(), page.limit], |r| {
                    Ok(CheckpointMark {
                        id: r.get(0)?,
                        at: r.get(1)?,
                        reason: r.get(2)?,
                        first_of_folder: r.get(3)?,
                        workspace_id: r.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.reverse();
            Ok(rows)
        })
        .await
}

/// The commands a pane ran in `page`, oldest first; a command is dated by
/// when it ended.
pub async fn commands(
    store: &Store,
    pane_id: &str,
    page: Page,
) -> Result<Vec<CommandMark>, StoreError> {
    let id = pane_id.to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, started_at + COALESCE(duration_ms, 0) AS ended, command, exit_code, duration_ms
                 FROM command_history WHERE pane_id = ?1 AND started_at + COALESCE(duration_ms, 0) <= ?2
                 ORDER BY ended DESC, id DESC LIMIT ?3",
            )?;
            let mut rows = stmt
                .query_map(params![id, page.before(), page.limit], |r| {
                    Ok(CommandMark {
                        id: r.get(0)?,
                        at: r.get(1)?,
                        command: r.get(2)?,
                        exit_code: r.get(3)?,
                        duration_ms: r.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.reverse();
            Ok(rows)
        })
        .await
}

/// A thread's approvals in `page`, by when they were asked.
pub async fn approvals(
    store: &Store,
    thread_id: &str,
    page: Page,
) -> Result<Vec<ApprovalMark>, StoreError> {
    let id = thread_id.to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, created_at, summary, state, scope, decided_via FROM approvals
                 WHERE source_kind = 'thread' AND source_id = ?1 AND created_at <= ?2
                 ORDER BY created_at DESC, id DESC LIMIT ?3",
            )?;
            let mut rows = stmt
                .query_map(params![id, page.before(), page.limit], |r| {
                    Ok(ApprovalMark {
                        id: r.get(0)?,
                        at: r.get(1)?,
                        summary: r.get(2)?,
                        state: r.get(3)?,
                        scope: r.get(4)?,
                        via: r.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.reverse();
            Ok(rows)
        })
        .await
}

/// At most this many events per pane or thread: one looping agent can't
/// push every other session's history out.
pub const KEEP_PER_SOURCE: i64 = 10_000;

/// Drops events older than [`KEEP_DAYS`], then each source's oldest past
/// [`KEEP_PER_SOURCE`], then the oldest past [`KEEP_ROWS`]. Rows marked
/// `kept` (decided runs, allowed approvals, notes) go by age only: the agent
/// can sign tool reports and trigger refusals, so a flood of those must not
/// push its real runs out.
pub async fn prune(store: &Store, now: i64) -> Result<usize, StoreError> {
    prune_to(store, now, KEEP_PER_SOURCE, KEEP_ROWS).await
}

async fn prune_to(
    store: &Store,
    now: i64,
    per_source: i64,
    total: i64,
) -> Result<usize, StoreError> {
    let cutoff = now - KEEP_DAYS * 24 * 60 * 60 * 1000;
    store
        .write(move |t| {
            let old = t.execute("DELETE FROM activity WHERE at < ?1", [cutoff])?;
            let crowded = t.execute(
                "DELETE FROM activity WHERE id IN (
                   SELECT id FROM (
                     SELECT id, ROW_NUMBER() OVER (
                       PARTITION BY source_kind, source_id ORDER BY at DESC, id DESC) AS n
                     FROM activity WHERE kept = 0)
                   WHERE n > ?1)",
                [per_source],
            )?;
            let over = t.execute(
                "DELETE FROM activity WHERE id IN (SELECT id FROM activity WHERE kept = 0 ORDER BY at DESC, id DESC LIMIT -1 OFFSET ?1)",
                [total],
            )?;
            Ok(old + crowded + over)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
        (dir, Store::open(&data).expect("store"))
    }

    fn event(source: &str, kind: &'static str, at: i64) -> NewActivity {
        NewActivity {
            source_kind: "pane",
            source_id: source.into(),
            kind,
            at,
            summary: format!("{kind} at {at}"),
            detail: serde_json::json!({ "n": at }),
            checkpoint_id: None,
            kept: false,
        }
    }

    #[tokio::test]
    async fn f_ex_07_events_list_per_source_oldest_first() {
        let (_d, store) = store();
        insert(&store, event("pn_1", "tool", 20))
            .await
            .expect("insert");
        insert(&store, event("pn_2", "tool", 10))
            .await
            .expect("insert");
        insert(&store, event("pn_1", "turn", 10))
            .await
            .expect("insert");
        let rows = list(&store, "pane", "pn_1", Page::ALL).await.expect("list");
        let kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(kinds, ["turn", "tool"]);
        assert_eq!(rows[1].detail, serde_json::json!({ "n": 20 }));
        assert!(list(&store, "thread", "pn_1", Page::ALL)
            .await
            .expect("list")
            .is_empty());
        // A page is the newest events at or before its cursor, oldest first.
        for at in 30..40 {
            insert(&store, event("pn_1", "tool", at))
                .await
                .expect("insert");
        }
        let page = list(
            &store,
            "pane",
            "pn_1",
            Page {
                until: Some(38),
                limit: 3,
            },
        )
        .await
        .expect("page");
        let ats: Vec<i64> = page.iter().map(|r| r.at).collect();
        assert_eq!(ats, vec![36, 37, 38]);
    }

    #[tokio::test]
    async fn f_ex_07_old_events_are_pruned() {
        let (_d, store) = store();
        let day = 24 * 60 * 60 * 1000;
        let now = 100 * day;
        insert(&store, event("pn_1", "tool", now - 15 * day))
            .await
            .expect("insert");
        insert(&store, event("pn_1", "tool", now - day))
            .await
            .expect("insert");
        assert_eq!(prune(&store, now).await.expect("prune"), 1);
        assert_eq!(
            list(&store, "pane", "pn_1", Page::ALL)
                .await
                .expect("list")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn f_ex_07_retention_by_count_keeps_each_sessions_newest_and_the_newest_overall() {
        let (_d, store) = store();
        let now = 1_000_000;
        for at in 0..10 {
            insert(&store, event("pn_loop", "tool", now - 100 + at))
                .await
                .expect("insert");
        }
        for at in 0..3 {
            insert(&store, event("pn_quiet", "tool", now - 1000 + at))
                .await
                .expect("insert");
        }
        // Per session 4, 6 in all: the loop keeps its newest 4, the quiet
        // pane its 3 (it isn't pushed out by the loop's flood).
        assert_eq!(prune_to(&store, now, 4, 6).await.expect("prune"), 7);
        let looping = list(&store, "pane", "pn_loop", Page::ALL)
            .await
            .expect("list");
        let ats: Vec<i64> = looping.iter().map(|r| r.at - (now - 100)).collect();
        assert_eq!(ats, vec![6, 7, 8, 9]);
        let quiet = list(&store, "pane", "pn_quiet", Page::ALL)
            .await
            .expect("list");
        assert_eq!(quiet.len(), 2);
    }

    #[tokio::test]
    async fn f_ex_07_a_flood_of_tool_rows_never_prunes_kept_runs_or_approvals() {
        let (_d, store) = store();
        let now = 1_000_000;
        let kept = |kind| NewActivity {
            kept: true,
            ..event("pn_1", kind, now - 500)
        };
        insert(&store, kept("run")).await.expect("insert");
        insert(&store, kept("approval")).await.expect("insert");
        // A refused run isn't kept: it goes with the count like a tool row.
        insert(&store, event("pn_1", "run", now - 400))
            .await
            .expect("insert");
        for at in 0..20 {
            insert(&store, event("pn_1", "tool", now - 100 + at))
                .await
                .expect("insert");
        }
        // Per session 4, 6 in all: the other rows go down to 4 (the refused
        // run among the first out); the kept run and approval stay, however
        // many rows came after them.
        prune_to(&store, now, 4, 6).await.expect("prune");
        let kinds: Vec<String> = list(&store, "pane", "pn_1", Page::ALL)
            .await
            .expect("list")
            .into_iter()
            .map(|r| r.kind)
            .collect();
        assert_eq!(kinds, ["run", "approval", "tool", "tool", "tool", "tool"]);
        // Age still applies to them.
        let later = now + 15 * 24 * 60 * 60 * 1000;
        prune_to(&store, later, 4, 6).await.expect("prune");
        assert!(list(&store, "pane", "pn_1", Page::ALL)
            .await
            .expect("list")
            .is_empty());
    }

    #[tokio::test]
    async fn f_ex_07_unknown_kinds_are_refused() {
        let (_d, store) = store();
        let mut bad = event("pn_1", "tool", 1);
        bad.kind = "output";
        assert!(insert(&store, bad).await.is_err());
    }
}
