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
}

pub async fn insert(store: &Store, row: NewActivity) -> Result<i64, StoreError> {
    let detail = row.detail.to_string();
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO activity (source_kind, source_id, kind, at, summary, detail_json, checkpoint_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    row.source_kind,
                    row.source_id,
                    row.kind,
                    row.at,
                    row.summary,
                    detail,
                    row.checkpoint_id
                ],
            )?;
            Ok(t.last_insert_rowid())
        })
        .await
}

/// A source's events, oldest first.
pub async fn list(
    store: &Store,
    source_kind: &str,
    source_id: &str,
) -> Result<Vec<ActivityRow>, StoreError> {
    let (kind, id) = (source_kind.to_string(), source_id.to_string());
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, source_kind, source_id, kind, at, summary, detail_json, checkpoint_id
                 FROM activity WHERE source_kind = ?1 AND source_id = ?2 ORDER BY at, id",
            )?;
            let rows = stmt
                .query_map(params![kind, id], |r| {
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

/// The checkpoints a pane or thread took, oldest first.
pub async fn checkpoints(
    store: &Store,
    source_kind: &str,
    source_id: &str,
) -> Result<Vec<CheckpointMark>, StoreError> {
    let column = if source_kind == "thread" {
        "thread_id"
    } else {
        "pane_id"
    };
    let sql = format!(
        "SELECT c.id, c.created_at, c.reason,
                NOT EXISTS (SELECT 1 FROM checkpoints e WHERE e.folder = c.folder AND e.created_at < c.created_at)
         FROM checkpoints c WHERE c.{column} = ?1 ORDER BY c.created_at, c.id"
    );
    let id = source_id.to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt
                .query_map([id], |r| {
                    Ok(CheckpointMark {
                        id: r.get(0)?,
                        at: r.get(1)?,
                        reason: r.get(2)?,
                        first_of_folder: r.get(3)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

/// The commands a pane ran, oldest first; ended commands are dated by
/// when they ended.
pub async fn commands(store: &Store, pane_id: &str) -> Result<Vec<CommandMark>, StoreError> {
    let id = pane_id.to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, started_at + COALESCE(duration_ms, 0), command, exit_code, duration_ms
                 FROM command_history WHERE pane_id = ?1 ORDER BY started_at, id",
            )?;
            let rows = stmt
                .query_map([id], |r| {
                    Ok(CommandMark {
                        id: r.get(0)?,
                        at: r.get(1)?,
                        command: r.get(2)?,
                        exit_code: r.get(3)?,
                        duration_ms: r.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

/// A thread's approvals, by when they were asked.
pub async fn approvals(store: &Store, thread_id: &str) -> Result<Vec<ApprovalMark>, StoreError> {
    let id = thread_id.to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, created_at, summary, state, scope, decided_via FROM approvals
                 WHERE source_kind = 'thread' AND source_id = ?1 ORDER BY created_at, id",
            )?;
            let rows = stmt
                .query_map([id], |r| {
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
            Ok(rows)
        })
        .await
}

/// Drops events older than [`KEEP_DAYS`], then the oldest past [`KEEP_ROWS`].
pub async fn prune(store: &Store, now: i64) -> Result<usize, StoreError> {
    let cutoff = now - KEEP_DAYS * 24 * 60 * 60 * 1000;
    store
        .write(move |t| {
            let old = t.execute("DELETE FROM activity WHERE at < ?1", [cutoff])?;
            let over = t.execute(
                "DELETE FROM activity WHERE id IN (SELECT id FROM activity ORDER BY at DESC, id DESC LIMIT -1 OFFSET ?1)",
                [KEEP_ROWS],
            )?;
            Ok(old + over)
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
        let rows = list(&store, "pane", "pn_1").await.expect("list");
        let kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(kinds, ["turn", "tool"]);
        assert_eq!(rows[1].detail, serde_json::json!({ "n": 20 }));
        assert!(list(&store, "thread", "pn_1")
            .await
            .expect("list")
            .is_empty());
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
        assert_eq!(list(&store, "pane", "pn_1").await.expect("list").len(), 1);
    }

    #[tokio::test]
    async fn f_ex_07_unknown_kinds_are_refused() {
        let (_d, store) = store();
        let mut bad = event("pn_1", "tool", 1);
        bad.kind = "output";
        assert!(insert(&store, bad).await.is_err());
    }
}
