//! The one approval broker (F-TH-11, F-PL-07, F-DB-01). Threads, the gateway
//! and plugins ask it; the Dashboard, inline cards and notifications answer
//! through it. A request nobody answers is denied after 30 minutes.
//!
//! The broker keeps pending requests in memory and publishes every change;
//! the app persists them (`approvals` table), forwards `approvals:changed`
//! and raises notifications from that stream.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, oneshot};
use ts_rs::TS;

use crate::clock::Clock;
use crate::error::{ErrorCode, IpcError};
use crate::ids::ApprovalId;
use crate::source::SourceRef;

/// How long a request waits before it's denied (F-PL-07 AC2).
pub const APPROVAL_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ApprovalKind {
    Command,
    FileWrite,
    FileDelete,
    Network,
    Mcp,
    PluginAction,
    Spend,
    Question,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ApprovalScope {
    Once,
    Thread,
    Conversation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ApprovalDecision {
    Allow { scope: ApprovalScope },
    Deny { reason: Option<String> },
}

/// Where a decision was made (the `decided_via` column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ApprovalVia {
    Inline,
    Dashboard,
    Notification,
    Phone,
    Voice,
    Rule,
    Expiry,
}

/// What the requester gets back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalOutcome {
    Allowed {
        scope: ApprovalScope,
        via: ApprovalVia,
    },
    Denied {
        reason: Option<String>,
        via: ApprovalVia,
    },
    Expired,
    /// The source went away (thread closed, pane stopped).
    Cancelled,
}

impl ApprovalOutcome {
    pub fn allowed(&self) -> bool {
        matches!(self, ApprovalOutcome::Allowed { .. })
    }
}

/// A new request.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub source: SourceRef,
    pub kind: ApprovalKind,
    /// Redacted, one line ("Run `npm test`").
    pub summary: String,
    /// Redacted, secrets masked.
    pub detail: String,
    /// Allow scopes offered; Deny is always offered.
    pub options: Vec<ApprovalScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApprovalView {
    pub id: ApprovalId,
    pub source: SourceRef,
    pub kind: ApprovalKind,
    pub summary: String,
    pub detail: String,
    pub options: Vec<ApprovalScope>,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ApprovalState {
    Allowed,
    Denied,
    Expired,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalEvent {
    Requested(ApprovalView),
    Resolved {
        view: ApprovalView,
        state: ApprovalState,
        decision: Option<ApprovalDecision>,
        via: Option<ApprovalVia>,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ApprovalError {
    #[error("That approval isn't pending anymore")]
    NotFound,
    #[error("That option isn't offered for this request")]
    ScopeNotOffered,
}

impl From<ApprovalError> for IpcError {
    fn from(e: ApprovalError) -> Self {
        let code = match e {
            ApprovalError::NotFound => ErrorCode::Conflict,
            ApprovalError::ScopeNotOffered => ErrorCode::Invalid,
        };
        IpcError::new(code, e.to_string())
    }
}

struct Pending {
    view: ApprovalView,
    reply: oneshot::Sender<ApprovalOutcome>,
}

pub struct ApprovalBroker {
    clock: Arc<dyn Clock>,
    ttl: Duration,
    pending: Mutex<HashMap<ApprovalId, Pending>>,
    tx: broadcast::Sender<ApprovalEvent>,
}

/// Awaits the decision on one request.
pub struct ApprovalTicket {
    rx: oneshot::Receiver<ApprovalOutcome>,
}

impl ApprovalTicket {
    pub async fn outcome(self) -> ApprovalOutcome {
        self.rx.await.unwrap_or(ApprovalOutcome::Cancelled)
    }
}

impl ApprovalBroker {
    pub fn new(clock: Arc<dyn Clock>) -> Arc<Self> {
        Self::with_ttl(clock, APPROVAL_TTL)
    }

    pub fn with_ttl(clock: Arc<dyn Clock>, ttl: Duration) -> Arc<Self> {
        let (tx, _) = broadcast::channel(256);
        Arc::new(Self {
            clock,
            ttl,
            pending: Mutex::default(),
            tx,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ApprovalEvent> {
        self.tx.subscribe()
    }

    /// Opens a request; expiry runs on the broker's clock (needs a runtime).
    pub fn request(self: &Arc<Self>, req: ApprovalRequest) -> (ApprovalView, ApprovalTicket) {
        let now = self.clock.now_ms();
        let ttl_ms = i64::try_from(self.ttl.as_millis()).unwrap_or(i64::MAX);
        let view = ApprovalView {
            id: ApprovalId::new(),
            source: req.source,
            kind: req.kind,
            summary: req.summary,
            detail: req.detail,
            options: req.options,
            created_at: now,
            expires_at: now.saturating_add(ttl_ms),
        };
        let (reply, rx) = oneshot::channel();
        if let Ok(mut p) = self.pending.lock() {
            p.insert(
                view.id.clone(),
                Pending {
                    view: view.clone(),
                    reply,
                },
            );
        }
        let _ = self.tx.send(ApprovalEvent::Requested(view.clone()));
        let weak: Weak<Self> = Arc::downgrade(self);
        let id = view.id.clone();
        let (clock, ttl) = (self.clock.clone(), self.ttl);
        // The timer is made inside the task: a caller off the runtime's
        // threads (a sync command) must not need a reactor.
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn(async move {
                    clock.sleep(ttl).await;
                    if let Some(broker) = weak.upgrade() {
                        broker.finish(&id, ApprovalState::Expired, None, Some(ApprovalVia::Expiry));
                    }
                });
            }
            Err(_) => tracing::warn!(
                code = "approvals.no_runtime",
                "an approval was opened outside a runtime; it won't expire"
            ),
        }
        (view, ApprovalTicket { rx })
    }

    /// Answers a pending request. A request answered elsewhere already is
    /// `NotFound` (shown as Conflict: "isn't pending anymore").
    pub fn resolve(
        &self,
        id: &ApprovalId,
        decision: ApprovalDecision,
        via: ApprovalVia,
    ) -> Result<ApprovalView, ApprovalError> {
        if let ApprovalDecision::Allow { scope } = &decision {
            let offered = self
                .pending
                .lock()
                .map_err(|_| ApprovalError::NotFound)?
                .get(id)
                .map(|p| p.view.options.contains(scope))
                .ok_or(ApprovalError::NotFound)?;
            if !offered {
                return Err(ApprovalError::ScopeNotOffered);
            }
        }
        let state = match decision {
            ApprovalDecision::Allow { .. } => ApprovalState::Allowed,
            ApprovalDecision::Deny { .. } => ApprovalState::Denied,
        };
        self.finish(id, state, Some(decision), Some(via))
            .ok_or(ApprovalError::NotFound)
    }

    /// Cancels every pending request from `source` (it closed or stopped).
    pub fn cancel_source(&self, source: &SourceRef) -> Vec<ApprovalId> {
        let ids: Vec<ApprovalId> = self
            .pending
            .lock()
            .map(|p| {
                p.values()
                    .filter(|x| &x.view.source == source)
                    .map(|x| x.view.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        for id in &ids {
            self.finish(id, ApprovalState::Cancelled, None, None);
        }
        ids
    }

    /// Pending requests, oldest first.
    pub fn pending(&self) -> Vec<ApprovalView> {
        let mut v: Vec<ApprovalView> = self
            .pending
            .lock()
            .map(|p| p.values().map(|x| x.view.clone()).collect())
            .unwrap_or_default();
        v.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        v
    }

    /// The oldest pending request of a source (Dashboard cards show one).
    pub fn pending_for(&self, source: &SourceRef) -> Option<ApprovalView> {
        self.pending().into_iter().find(|v| &v.source == source)
    }

    fn finish(
        &self,
        id: &ApprovalId,
        state: ApprovalState,
        decision: Option<ApprovalDecision>,
        via: Option<ApprovalVia>,
    ) -> Option<ApprovalView> {
        let entry = self.pending.lock().ok()?.remove(id)?;
        let outcome = match (&state, &decision, via) {
            (ApprovalState::Allowed, Some(ApprovalDecision::Allow { scope }), Some(via)) => {
                ApprovalOutcome::Allowed { scope: *scope, via }
            }
            (ApprovalState::Denied, Some(ApprovalDecision::Deny { reason }), Some(via)) => {
                ApprovalOutcome::Denied {
                    reason: reason.clone(),
                    via,
                }
            }
            (ApprovalState::Expired, _, _) => ApprovalOutcome::Expired,
            _ => ApprovalOutcome::Cancelled,
        };
        let _ = entry.reply.send(outcome);
        let _ = self.tx.send(ApprovalEvent::Resolved {
            view: entry.view.clone(),
            state,
            decision,
            via,
        });
        Some(entry.view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::MockClock;
    use chrono::TimeZone;

    fn broker() -> (Arc<ApprovalBroker>, MockClock) {
        let clock = MockClock::new(
            chrono::Utc
                .with_ymd_and_hms(2026, 10, 1, 9, 0, 0)
                .single()
                .expect("time"),
        );
        (ApprovalBroker::new(Arc::new(clock.clone())), clock)
    }

    fn req(source: &str) -> ApprovalRequest {
        ApprovalRequest {
            source: SourceRef::Thread {
                thread_id: source.into(),
            },
            kind: ApprovalKind::Command,
            summary: "Run `npm test`".into(),
            detail: "npm test".into(),
            options: vec![ApprovalScope::Once, ApprovalScope::Thread],
        }
    }

    #[tokio::test]
    async fn approvals_allow_once_reaches_the_requester_and_the_stream() {
        let (b, _c) = broker();
        let mut events = b.subscribe();
        let (view, ticket) = b.request(req("th_1"));
        assert!(matches!(events.recv().await, Ok(ApprovalEvent::Requested(v)) if v.id == view.id));
        assert_eq!(b.pending().len(), 1);
        b.resolve(
            &view.id,
            ApprovalDecision::Allow {
                scope: ApprovalScope::Once,
            },
            ApprovalVia::Dashboard,
        )
        .expect("resolve");
        assert_eq!(
            ticket.outcome().await,
            ApprovalOutcome::Allowed {
                scope: ApprovalScope::Once,
                via: ApprovalVia::Dashboard
            }
        );
        assert!(matches!(
            events.recv().await,
            Ok(ApprovalEvent::Resolved {
                state: ApprovalState::Allowed,
                via: Some(ApprovalVia::Dashboard),
                ..
            })
        ));
        assert!(b.pending().is_empty());
    }

    #[tokio::test]
    async fn approvals_answered_twice_is_a_conflict_and_scopes_must_be_offered() {
        let (b, _c) = broker();
        let (view, ticket) = b.request(ApprovalRequest {
            options: vec![ApprovalScope::Once],
            ..req("th_1")
        });
        assert_eq!(
            b.resolve(
                &view.id,
                ApprovalDecision::Allow {
                    scope: ApprovalScope::Thread
                },
                ApprovalVia::Inline
            ),
            Err(ApprovalError::ScopeNotOffered)
        );
        b.resolve(
            &view.id,
            ApprovalDecision::Deny {
                reason: Some("not now".into()),
            },
            ApprovalVia::Notification,
        )
        .expect("deny");
        assert_eq!(
            ticket.outcome().await,
            ApprovalOutcome::Denied {
                reason: Some("not now".into()),
                via: ApprovalVia::Notification
            }
        );
        let again = b.resolve(
            &view.id,
            ApprovalDecision::Deny { reason: None },
            ApprovalVia::Inline,
        );
        assert_eq!(again, Err(ApprovalError::NotFound));
        assert_eq!(
            IpcError::from(ApprovalError::NotFound).code,
            ErrorCode::Conflict
        );
    }

    #[tokio::test]
    async fn approvals_expire_after_30_minutes() {
        let (b, clock) = broker();
        let (view, ticket) = b.request(req("th_1"));
        assert_eq!(view.expires_at - view.created_at, 30 * 60 * 1000);
        tokio::task::yield_now().await;
        clock.advance(Duration::from_secs(29 * 60));
        tokio::task::yield_now().await;
        assert_eq!(b.pending().len(), 1);
        clock.advance(Duration::from_secs(61));
        assert_eq!(ticket.outcome().await, ApprovalOutcome::Expired);
        assert!(b.pending().is_empty());
    }

    #[tokio::test]
    async fn approvals_of_a_closed_source_are_cancelled() {
        let (b, _c) = broker();
        let (_a, ta) = b.request(req("th_1"));
        let (other, _tb) = b.request(req("th_2"));
        let gone = b.cancel_source(&SourceRef::Thread {
            thread_id: "th_1".into(),
        });
        assert_eq!(gone.len(), 1);
        assert_eq!(ta.outcome().await, ApprovalOutcome::Cancelled);
        assert_eq!(
            b.pending_for(&SourceRef::Thread {
                thread_id: "th_2".into()
            })
            .map(|v| v.id),
            Some(other.id)
        );
    }

    /// A sync caller (a command on the main thread) has no reactor: opening
    /// a request there must not panic (it crashed the app once).
    #[test]
    fn approvals_open_outside_a_runtime_without_panicking() {
        let clock = MockClock::new(
            chrono::Utc
                .with_ymd_and_hms(2026, 10, 1, 9, 0, 0)
                .single()
                .expect("time"),
        );
        let b = ApprovalBroker::new(Arc::new(clock));
        let (view, _ticket) = b.request(req("th_1"));
        assert_eq!(b.pending().first().map(|v| v.id.clone()), Some(view.id));
    }
}
