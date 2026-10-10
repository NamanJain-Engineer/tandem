//! `held_drafts` (F-PL-08): plugin calls an unattended run would have
//! needed approved, kept for the user. Each runs at most once: running
//! claims the draft (`held` → `executing`) in one statement, so a second
//! Approve & run, from anywhere, finds nothing to claim.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tandem_core::ids::ConnectionId;
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HeldState {
    Held,
    Executing,
    Executed,
    Discarded,
    Failed,
}

impl HeldState {
    fn as_str(self) -> &'static str {
        match self {
            HeldState::Held => "held",
            HeldState::Executing => "executing",
            HeldState::Executed => "executed",
            HeldState::Discarded => "discarded",
            HeldState::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "executing" => HeldState::Executing,
            "executed" => HeldState::Executed,
            "discarded" => HeldState::Discarded,
            "failed" => HeldState::Failed,
            _ => HeldState::Held,
        }
    }
}

/// Where a draft came from (the unattended run).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OriginKind {
    Routine,
    Task,
    Mail,
    Auto,
}

impl OriginKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OriginKind::Routine => "routine",
            OriginKind::Task => "task",
            OriginKind::Mail => "mail",
            OriginKind::Auto => "auto",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "routine" => Some(OriginKind::Routine),
            "task" => Some(OriginKind::Task),
            "mail" => Some(OriginKind::Mail),
            "auto" => Some(OriginKind::Auto),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HeldDraftRow {
    pub id: String,
    /// Shown as `D-<n>`.
    pub display_no: i64,
    pub connection_id: ConnectionId,
    pub plugin_id: String,
    pub tool: String,
    #[ts(type = "unknown")]
    pub args: serde_json::Value,
    pub risk_class: String,
    pub origin_kind: OriginKind,
    pub origin_id: String,
    pub run_id: Option<String>,
    pub thread_id: Option<String>,
    pub state: HeldState,
    /// What running it returned.
    pub result: Option<String>,
    pub created_at: i64,
    pub decided_at: Option<i64>,
}

pub struct NewDraft {
    pub connection_id: ConnectionId,
    pub plugin_id: String,
    pub tool: String,
    pub args: serde_json::Value,
    pub risk_class: String,
    pub origin_kind: OriginKind,
    pub origin_id: String,
    pub run_id: Option<String>,
    pub thread_id: Option<String>,
}

const COLS: &str = "id, display_no, connection_id, plugin_id, tool, args_json, risk_class, origin_kind, origin_id, run_id, thread_id, state, result_json, created_at, decided_at";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<HeldDraftRow> {
    Ok(HeldDraftRow {
        id: r.get(0)?,
        display_no: r.get(1)?,
        connection_id: ConnectionId::from_db(r.get(2)?),
        plugin_id: r.get(3)?,
        tool: r.get(4)?,
        args: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
        risk_class: r.get(6)?,
        origin_kind: OriginKind::parse(&r.get::<_, String>(7)?).unwrap_or(OriginKind::Routine),
        origin_id: r.get(8)?,
        run_id: r.get(9)?,
        thread_id: r.get(10)?,
        state: HeldState::parse(&r.get::<_, String>(11)?),
        result: r.get(12)?,
        created_at: r.get(13)?,
        decided_at: r.get(14)?,
    })
}

/// Stores a draft with the next display number.
pub async fn insert(store: &Store, d: NewDraft) -> Result<HeldDraftRow, StoreError> {
    let id = tandem_core::ids::HeldDraftId::new().as_str().to_string();
    let now = now_ms();
    let key = id.clone();
    store
        .write(move |t| {
            let next: i64 = t.query_row("SELECT COALESCE(MAX(display_no), 0) + 1 FROM held_drafts", [], |r| r.get(0))?;
            t.execute(
                "INSERT INTO held_drafts (id, display_no, connection_id, plugin_id, tool, args_json, risk_class, origin_kind, origin_id, run_id, thread_id, state, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'held', ?12)",
                params![
                    key,
                    next,
                    d.connection_id.as_str(),
                    d.plugin_id,
                    d.tool,
                    d.args.to_string(),
                    d.risk_class,
                    d.origin_kind.as_str(),
                    d.origin_id,
                    d.run_id,
                    d.thread_id,
                    now
                ],
            )?;
            Ok(())
        })
        .await?;
    get(store, &id)
        .await?
        .ok_or_else(|| StoreError::NotFound("held draft".into()))
}

pub async fn get(store: &Store, id: &str) -> Result<Option<HeldDraftRow>, StoreError> {
    let id = id.to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLS} FROM held_drafts WHERE id = ?1"),
                [id],
                row,
            )
            .optional()?)
        })
        .await
}

/// Drafts, newest first; `state` and `origin_id` narrow the list.
pub async fn list(
    store: &Store,
    state: Option<HeldState>,
    origin_id: Option<String>,
) -> Result<Vec<HeldDraftRow>, StoreError> {
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLS} FROM held_drafts
                 WHERE (?1 IS NULL OR state = ?1) AND (?2 IS NULL OR origin_id = ?2)
                 ORDER BY display_no DESC LIMIT 500"
            ))?;
            let rows = stmt.query_map(params![state.map(HeldState::as_str), origin_id], row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// Claims a held draft to run it. `false` when it isn't held any more
/// (already run, running or discarded).
pub async fn claim(
    store: &Store,
    id: &str,
    args: Option<serde_json::Value>,
) -> Result<bool, StoreError> {
    let id = id.to_string();
    store
        .write(move |t| {
            let n = match &args {
                Some(a) => t.execute(
                    "UPDATE held_drafts SET state = 'executing', args_json = ?2, decided_at = ?3 WHERE id = ?1 AND state = 'held'",
                    params![id, a.to_string(), now_ms()],
                )?,
                None => t.execute(
                    "UPDATE held_drafts SET state = 'executing', decided_at = ?2 WHERE id = ?1 AND state = 'held'",
                    params![id, now_ms()],
                )?,
            };
            Ok(n == 1)
        })
        .await
}

/// Records what running a claimed draft returned.
pub async fn finish(store: &Store, id: &str, ok: bool, result: &str) -> Result<(), StoreError> {
    let (id, result) = (id.to_string(), result.to_string());
    store
        .write(move |t| {
            t.execute(
                "UPDATE held_drafts SET state = ?2, result_json = ?3 WHERE id = ?1 AND state = 'executing'",
                params![id, if ok { "executed" } else { "failed" }, result],
            )?;
            Ok(())
        })
        .await
}

/// Discards held drafts; returns the ids that were held.
pub async fn discard(store: &Store, ids: Vec<String>) -> Result<Vec<String>, StoreError> {
    store
        .write(move |t| {
            let mut done = Vec::new();
            for id in ids {
                let n = t.execute(
                    "UPDATE held_drafts SET state = 'discarded', decided_at = ?2 WHERE id = ?1 AND state = 'held'",
                    params![id, now_ms()],
                )?;
                if n == 1 {
                    done.push(id);
                }
            }
            Ok(done)
        })
        .await
}

/// A crash mid-run leaves `executing`: it can't be known whether the
/// vendor got it, so it's marked failed (never run again by itself).
pub async fn settle_interrupted(store: &Store) -> Result<usize, StoreError> {
    store
        .write(|t| {
            Ok(t.execute(
                "UPDATE held_drafts SET state = 'failed', result_json = 'Vibezeo quit while this ran; check the vendor before running it again.' WHERE state = 'executing'",
                [],
            )?)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::plugins::insert_connection;

    #[tokio::test]
    async fn f_pl_08_drafts_number_up_and_run_at_most_once() {
        let (_d, store) = crate::tests::temp_store().await;
        let conn = insert_connection(&store, "github", Some("me"), "api_key")
            .await
            .expect("conn");
        let new = |tool: &str| NewDraft {
            connection_id: conn.id.clone(),
            plugin_id: "github".into(),
            tool: tool.into(),
            args: serde_json::json!({"title": "x"}),
            risk_class: "write".into(),
            origin_kind: OriginKind::Routine,
            origin_id: "rt_1".into(),
            run_id: Some("run_1".into()),
            thread_id: None,
        };
        let a = insert(&store, new("create_issue")).await.expect("a");
        let b = insert(&store, new("create_issue")).await.expect("b");
        assert_eq!((a.display_no, b.display_no), (1, 2));
        assert_eq!(a.state, HeldState::Held);
        assert!(claim(&store, &a.id, None).await.expect("claim"));
        assert!(
            !claim(&store, &a.id, None).await.expect("again"),
            "runs at most once"
        );
        finish(&store, &a.id, true, "ok").await.expect("finish");
        assert_eq!(
            get(&store, &a.id).await.expect("get").map(|d| d.state),
            Some(HeldState::Executed)
        );
        assert_eq!(
            discard(&store, vec![a.id.clone(), b.id.clone()])
                .await
                .expect("discard"),
            vec![b.id.clone()]
        );
        assert!(!claim(&store, &b.id, None).await.expect("discarded"));
        let held = list(&store, Some(HeldState::Held), None)
            .await
            .expect("list");
        assert!(held.is_empty());
        assert_eq!(
            list(&store, None, Some("rt_1".into()))
                .await
                .expect("all")
                .len(),
            2
        );
        // Edited arguments replace the stored ones when it's claimed.
        let c = insert(&store, new("create_issue")).await.expect("c");
        assert!(
            claim(&store, &c.id, Some(serde_json::json!({"title": "edited"})))
                .await
                .expect("claim")
        );
        assert_eq!(
            get(&store, &c.id)
                .await
                .expect("get")
                .map(|d| d.args["title"].clone()),
            Some(serde_json::json!("edited"))
        );
        assert_eq!(settle_interrupted(&store).await.expect("settle"), 1);
    }
}
