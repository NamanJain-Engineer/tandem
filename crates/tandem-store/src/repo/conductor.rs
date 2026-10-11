//! Conductor's call log (F-CN-04, UI-66): `conductor_calls` and
//! `conductor_call_events`, kept locally for 7 days by default.
//!
//! A call's id is the API's voice session id, so a call the app never
//! settled (it crashed mid-call) can be settled at the next start: its row
//! has no `credits` yet, and `connected_seconds` holds its last progress.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CallRow {
    pub id: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub connected_seconds: Option<i64>,
    /// `None` until the API confirmed the settle.
    pub credits: Option<i64>,
    pub end_reason: Option<String>,
    pub actions: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CallEventKind {
    User,
    Assistant,
    Action,
}

impl CallEventKind {
    fn as_str(self) -> &'static str {
        match self {
            CallEventKind::User => "user",
            CallEventKind::Assistant => "assistant",
            CallEventKind::Action => "action",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "assistant" => CallEventKind::Assistant,
            "action" => CallEventKind::Action,
            _ => CallEventKind::User,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CallEventRow {
    pub ts: i64,
    pub kind: CallEventKind,
    pub text: String,
    pub ok: Option<bool>,
}

const COLS: &str = "c.id, c.started_at, c.ended_at, c.connected_seconds, c.credits, c.end_reason,
    (SELECT COUNT(*) FROM conductor_call_events e WHERE e.call_id = c.id AND e.kind = 'action')";

fn row(r: &rusqlite::Row) -> rusqlite::Result<CallRow> {
    Ok(CallRow {
        id: r.get(0)?,
        started_at: r.get(1)?,
        ended_at: r.get(2)?,
        connected_seconds: r.get(3)?,
        credits: r.get(4)?,
        end_reason: r.get(5)?,
        actions: r.get(6)?,
    })
}

pub async fn started(store: &Store, id: &str, started_at: i64) -> Result<(), StoreError> {
    let id = id.to_string();
    store
        .write(move |t| {
            t.execute(
                "INSERT OR IGNORE INTO conductor_calls (id, started_at, connected_seconds) VALUES (?1, ?2, 0)",
                params![id, started_at],
            )?;
            Ok(())
        })
        .await
}

pub async fn progress(store: &Store, id: &str, seconds: i64) -> Result<(), StoreError> {
    let id = id.to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE conductor_calls SET connected_seconds = ?2 WHERE id = ?1 AND ended_at IS NULL",
                params![id, seconds],
            )?;
            Ok(())
        })
        .await
}

pub async fn event(
    store: &Store,
    id: &str,
    ts: i64,
    kind: CallEventKind,
    text: &str,
    ok: Option<bool>,
) -> Result<(), StoreError> {
    let (id, text) = (id.to_string(), text.to_string());
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO conductor_call_events (call_id, ts, kind, text, ok) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, ts, kind.as_str(), text, ok],
            )?;
            Ok(())
        })
        .await
}

pub async fn ended(
    store: &Store,
    id: &str,
    ended_at: i64,
    seconds: i64,
    credits: Option<i64>,
    reason: &str,
) -> Result<(), StoreError> {
    let (id, reason) = (id.to_string(), reason.to_string());
    store
        .write(move |t| {
            t.execute(
                "UPDATE conductor_calls SET ended_at = ?2, connected_seconds = ?3, credits = ?4, end_reason = ?5 WHERE id = ?1",
                params![id, ended_at, seconds, credits, reason],
            )?;
            Ok(())
        })
        .await
}

/// A settle the API confirmed later.
pub async fn settled(store: &Store, id: &str, credits: i64, now: i64) -> Result<(), StoreError> {
    let id = id.to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE conductor_calls SET credits = ?2, ended_at = COALESCE(ended_at, ?3),
                   end_reason = COALESCE(end_reason, 'interrupted') WHERE id = ?1",
                params![id, credits, now],
            )?;
            Ok(())
        })
        .await
}

/// Calls never settled, with their last known connected seconds.
pub async fn unsettled(store: &Store) -> Result<Vec<CallRow>, StoreError> {
    store
        .read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLS} FROM conductor_calls c WHERE c.credits IS NULL ORDER BY c.started_at"
            ))?;
            let rows = stmt.query_map([], row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// Calls, newest first.
pub async fn list(store: &Store, limit: u32) -> Result<Vec<CallRow>, StoreError> {
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLS} FROM conductor_calls c ORDER BY c.started_at DESC LIMIT ?1"
            ))?;
            let rows = stmt.query_map([limit], row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

pub async fn get(store: &Store, id: &str) -> Result<Option<CallRow>, StoreError> {
    let id = id.to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLS} FROM conductor_calls c WHERE c.id = ?1"),
                [id],
                row,
            )
            .optional()?)
        })
        .await
}

/// One call's turns and actions, in order.
pub async fn events(store: &Store, id: &str) -> Result<Vec<CallEventRow>, StoreError> {
    let id = id.to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT ts, kind, text, ok FROM conductor_call_events WHERE call_id = ?1 ORDER BY id",
            )?;
            let rows = stmt.query_map([id], |r| {
                Ok(CallEventRow {
                    ts: r.get(0)?,
                    kind: CallEventKind::parse(&r.get::<_, String>(1)?),
                    text: r.get(2)?,
                    ok: r.get(3)?,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

pub async fn delete(store: &Store, id: &str) -> Result<(), StoreError> {
    let id = id.to_string();
    store
        .write(move |t| {
            t.execute(
                "DELETE FROM conductor_call_events WHERE call_id = ?1",
                [&id],
            )?;
            t.execute("DELETE FROM conductor_calls WHERE id = ?1", [&id])?;
            Ok(())
        })
        .await
}

/// Retention (F-CN-04: 7 days by default): settled calls that started
/// before `before` go, with their events. Unsettled ones stay until settled.
pub async fn purge(store: &Store, before: i64) -> Result<usize, StoreError> {
    store
        .write(move |t| {
            t.execute(
                "DELETE FROM conductor_call_events WHERE call_id IN
                   (SELECT id FROM conductor_calls WHERE started_at < ?1 AND credits IS NOT NULL)",
                [before],
            )?;
            Ok(t.execute(
                "DELETE FROM conductor_calls WHERE started_at < ?1 AND credits IS NOT NULL",
                [before],
            )?)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;

    #[tokio::test]
    async fn f_cn_04_a_call_and_its_turns_are_kept_in_order() {
        let (_d, store) = crate::tests::temp_store().await;
        started(&store, "vs_1", 1_000).await.expect("start");
        event(
            &store,
            "vs_1",
            1_100,
            CallEventKind::User,
            "open code mode",
            None,
        )
        .await
        .expect("e");
        event(
            &store,
            "vs_1",
            1_200,
            CallEventKind::Action,
            "Code mode: Done.",
            Some(true),
        )
        .await
        .expect("e");
        event(
            &store,
            "vs_1",
            1_300,
            CallEventKind::Assistant,
            "Done.",
            None,
        )
        .await
        .expect("e");
        progress(&store, "vs_1", 30).await.expect("p");
        assert_eq!(
            unsettled(&store).await.expect("u")[0].connected_seconds,
            Some(30)
        );
        ended(&store, "vs_1", 66_000, 65, Some(140), "hung_up")
            .await
            .expect("end");
        let c = get(&store, "vs_1").await.expect("get").expect("row");
        assert_eq!(
            (c.connected_seconds, c.credits, c.actions),
            (Some(65), Some(140), 1)
        );
        let ev = events(&store, "vs_1").await.expect("events");
        assert_eq!(ev.len(), 3);
        assert_eq!(ev[1].kind, CallEventKind::Action);
        assert_eq!(ev[1].ok, Some(true));
        assert!(unsettled(&store).await.expect("u").is_empty());
    }

    #[tokio::test]
    async fn f_cn_04_calls_are_kept_seven_days() {
        let (_d, store) = crate::tests::temp_store().await;
        let now = 100 * DAY;
        for (id, at) in [
            ("old", now - 8 * DAY),
            ("new", now - DAY),
            ("open", now - 9 * DAY),
        ] {
            started(&store, id, at).await.expect("start");
            event(&store, id, at, CallEventKind::User, "hi", None)
                .await
                .expect("e");
        }
        ended(&store, "old", now - 8 * DAY, 10, Some(20), "hung_up")
            .await
            .expect("e");
        ended(&store, "new", now - DAY, 10, Some(20), "hung_up")
            .await
            .expect("e");
        assert_eq!(purge(&store, now - 7 * DAY).await.expect("purge"), 1);
        let ids: Vec<String> = list(&store, 10)
            .await
            .expect("list")
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, vec!["new".to_string(), "open".to_string()]);
        // An unsettled call stays until it is settled.
        settled(&store, "open", 0, now).await.expect("settled");
        assert_eq!(purge(&store, now - 7 * DAY).await.expect("purge"), 1);
        delete(&store, "new").await.expect("delete");
        assert!(list(&store, 10).await.expect("list").is_empty());
    }
}
