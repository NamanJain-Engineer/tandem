//! `approvals` (F-TH-11, F-PL-07): the record of every request the broker
//! saw and how it ended. Pending rows exist only while their requester is
//! alive; a restart cancels whatever a crash left pending.

use rusqlite::params;
use tandem_core::approvals::{ApprovalDecision, ApprovalState, ApprovalVia, ApprovalView};
use tandem_core::source::SourceRef;

use crate::{now_ms, Store, StoreError};

/// The table's `(source_kind, source_id)`, or `None` for sources it doesn't
/// record (the test harness's fixtures).
fn source_cols(source: &SourceRef) -> Option<(&'static str, String)> {
    match source {
        SourceRef::Thread { thread_id } => Some(("thread", thread_id.clone())),
        SourceRef::TeammateChat { thread_id, .. } => Some(("teammate_chat", thread_id.clone())),
        SourceRef::Plugin { connection_id } => Some(("gateway", connection_id.clone())),
        _ => None,
    }
}

fn snake<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|j| j.as_str().map(str::to_string))
        .unwrap_or_default()
}

pub async fn insert_pending(store: &Store, view: &ApprovalView) -> Result<(), StoreError> {
    let Some((kind, id)) = source_cols(&view.source) else {
        return Ok(());
    };
    let v = view.clone();
    let options =
        serde_json::to_string(&v.options).map_err(|e| StoreError::Invalid(e.to_string()))?;
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO approvals (id, source_kind, source_id, kind, summary, detail_json, options_json, state, created_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9)",
                params![
                    v.id.as_str(),
                    kind,
                    id,
                    snake(&v.kind),
                    v.summary,
                    serde_json::json!({ "detail": v.detail }).to_string(),
                    options,
                    v.created_at,
                    v.expires_at
                ],
            )?;
            Ok(())
        })
        .await
}

pub async fn finish(
    store: &Store,
    view: &ApprovalView,
    state: &ApprovalState,
    decision: Option<&ApprovalDecision>,
    via: Option<ApprovalVia>,
) -> Result<(), StoreError> {
    if source_cols(&view.source).is_none() {
        return Ok(());
    }
    let id = view.id.as_str().to_string();
    let state = snake(state);
    let (scope, reason) = match decision {
        Some(ApprovalDecision::Allow { scope }) => (Some(snake(scope)), None),
        Some(ApprovalDecision::Deny { reason }) => (None, reason.clone()),
        None => (None, None),
    };
    let via = via.map(|v| snake(&v));
    store
        .write(move |t| {
            t.execute(
                "UPDATE approvals SET state = ?2, scope = ?3, decided_via = ?4, deny_reason = ?5, decided_at = ?6
                 WHERE id = ?1 AND state = 'pending'",
                params![id, state, scope, via, reason, now_ms()],
            )?;
            Ok(())
        })
        .await
}

/// At startup: nobody is waiting for these anymore.
pub async fn cancel_stale(store: &Store) -> Result<usize, StoreError> {
    store
        .write(|t| {
            Ok(t.execute(
                "UPDATE approvals SET state = 'cancelled', decided_at = ?1 WHERE state = 'pending'",
                [now_ms()],
            )?)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::approvals::{ApprovalKind, ApprovalScope};
    use tandem_core::ids::ApprovalId;

    fn view(source: SourceRef) -> ApprovalView {
        ApprovalView {
            id: ApprovalId::new(),
            source,
            kind: ApprovalKind::Command,
            summary: "Run `npm test`".into(),
            detail: "npm test".into(),
            options: vec![ApprovalScope::Once],
            created_at: 1,
            expires_at: 2,
        }
    }

    async fn state_of(
        store: &Store,
        id: &ApprovalId,
    ) -> Option<(String, Option<String>, Option<String>)> {
        let id = id.as_str().to_string();
        store
            .read(move |c| {
                Ok(c.query_row(
                    "SELECT state, scope, decided_via FROM approvals WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .ok())
            })
            .await
            .expect("read")
    }

    #[tokio::test]
    async fn approvals_are_recorded_and_settled_once() {
        let (_d, store) = crate::tests::temp_store().await;
        // The FK on threads isn't enforced for approvals: source ids are plain text.
        let v = view(SourceRef::Thread {
            thread_id: "th_1".into(),
        });
        insert_pending(&store, &v).await.expect("insert");
        assert_eq!(
            state_of(&store, &v.id).await.map(|s| s.0).as_deref(),
            Some("pending")
        );
        let allow = ApprovalDecision::Allow {
            scope: ApprovalScope::Once,
        };
        finish(
            &store,
            &v,
            &ApprovalState::Allowed,
            Some(&allow),
            Some(ApprovalVia::Dashboard),
        )
        .await
        .expect("finish");
        assert_eq!(
            state_of(&store, &v.id).await,
            Some((
                "allowed".into(),
                Some("once".into()),
                Some("dashboard".into())
            ))
        );
        // A late expiry doesn't overwrite the decision.
        finish(
            &store,
            &v,
            &ApprovalState::Expired,
            None,
            Some(ApprovalVia::Expiry),
        )
        .await
        .expect("finish");
        assert_eq!(
            state_of(&store, &v.id).await.map(|s| s.0).as_deref(),
            Some("allowed")
        );
    }

    #[tokio::test]
    async fn approvals_left_pending_by_a_crash_are_cancelled_and_fixtures_not_recorded() {
        let (_d, store) = crate::tests::temp_store().await;
        let v = view(SourceRef::Plugin {
            connection_id: "pc_1".into(),
        });
        insert_pending(&store, &v).await.expect("insert");
        let f = view(SourceRef::Fixture {
            fixture_id: "a".into(),
        });
        insert_pending(&store, &f).await.expect("fixture");
        assert_eq!(state_of(&store, &f.id).await, None);
        assert_eq!(cancel_stale(&store).await.expect("cancel"), 1);
        assert_eq!(
            state_of(&store, &v.id).await.map(|s| s.0).as_deref(),
            Some("cancelled")
        );
    }
}
