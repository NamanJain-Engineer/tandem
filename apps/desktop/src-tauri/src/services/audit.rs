//! Metadata-only audit events for a Team org (F-TL-10, ARCHITECTURE §14.6).
//!
//! Only when the signed-in user's org policy turns audit on does anything
//! get queued; the API keeps it only for such a member as well. Events say
//! what happened (Full access used, a plugin write approved) and on which
//! engine or plugin: never prompts, paths, file contents or output. They go
//! out in batches; a network failure keeps them for the next try.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value};

use tandem_core::clock::Clock;

use super::api::{ApiClient, AuditEvent};
use super::auth::AuthService;
use super::entitlement::EntitlementService;

/// Events kept while offline; the oldest go first past this.
const MAX_QUEUED: usize = 500;
/// The API takes at most this many per request.
const BATCH: usize = 100;
const FLUSH_EVERY: Duration = Duration::from_secs(30);

/// Events the app sends (the API drops anything else).
pub const FULL_ACCESS_USED: &str = "full_access_used";
pub const PLUGIN_WRITE_APPROVED: &str = "plugin_write_approved";
pub const PLUGIN_CONNECTED: &str = "plugin_connected";

pub struct AuditService {
    auth: Arc<AuthService>,
    api: Arc<dyn ApiClient>,
    entitlements: Arc<EntitlementService>,
    clock: Arc<dyn Clock>,
    queue: Mutex<Vec<AuditEvent>>,
    kick: tokio::sync::Notify,
}

impl AuditService {
    pub fn new(
        auth: Arc<AuthService>,
        api: Arc<dyn ApiClient>,
        entitlements: Arc<EntitlementService>,
        clock: Arc<dyn Clock>,
    ) -> Arc<Self> {
        Arc::new(Self {
            auth,
            api,
            entitlements,
            clock,
            queue: Mutex::default(),
            kick: tokio::sync::Notify::new(),
        })
    }

    /// Queues `event` when the org's audit is on; returns whether it was.
    /// `meta` holds short identifiers only (an engine or plugin id).
    pub fn record(&self, event: &str, meta: &[(&str, &str)]) -> bool {
        if !self.entitlements.policy().is_some_and(|p| p.audit_enabled) {
            return false;
        }
        let meta: Map<String, Value> = meta
            .iter()
            .map(|(k, v)| {
                (
                    (*k).to_string(),
                    Value::String(v.chars().take(100).collect()),
                )
            })
            .collect();
        // UTC with a Z, the form every RFC 3339 parser takes.
        let ts = chrono::DateTime::from_timestamp_millis(self.clock.now_ms())
            .unwrap_or_default()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        if let Ok(mut q) = self.queue.lock() {
            if q.len() >= MAX_QUEUED {
                q.remove(0);
            }
            q.push(AuditEvent {
                event: event.to_string(),
                meta,
                ts,
            });
        }
        tracing::info!(code = "audit.recorded", event, "audit event queued");
        self.kick.notify_one();
        true
    }

    pub fn pending(&self) -> usize {
        self.queue.lock().map(|q| q.len()).unwrap_or(0)
    }

    /// Sends what's queued. Network errors keep it; anything the server
    /// refuses outright is dropped (it would never be accepted).
    pub async fn flush(&self) -> usize {
        let mut sent = 0;
        loop {
            let batch: Vec<AuditEvent> = match self.queue.lock() {
                Ok(q) if !q.is_empty() => q.iter().take(BATCH).cloned().collect(),
                _ => return sent,
            };
            if !self.auth.is_signed_in() {
                return sent;
            }
            let api = self.api.clone();
            let events = batch.clone();
            let result = self
                .auth
                .with_token(move |t| {
                    let api = api.clone();
                    let events = events.clone();
                    async move { api.audit_events(&t, &events).await }
                })
                .await;
            match result {
                Err(e) if e.is_network() || e.is_unauthorized() => return sent,
                Err(e) => {
                    tracing::warn!(code = "audit.refused", error = %e, "audit events refused; dropped");
                }
                Ok(()) => sent += batch.len(),
            }
            if let Ok(mut q) = self.queue.lock() {
                let n = batch.len().min(q.len());
                q.drain(..n);
            }
        }
    }

    /// Flushes soon after each event, and every 30 s while some wait.
    pub async fn run(self: Arc<Self>) {
        loop {
            tokio::select! {
                _ = self.kick.notified() => {}
                _ = tokio::time::sleep(FLUSH_EVERY) => {}
            }
            // Let a burst of events land in one request.
            tokio::time::sleep(Duration::from_millis(500)).await;
            self.flush().await;
        }
    }
}

/// What the thread launcher needs from the org: its policy and the audit log.
pub trait OrgGuard: Send + Sync {
    fn policy(&self) -> Option<tandem_core::policy::OrgPolicy>;
    fn audit(&self, event: &str, meta: &[(&str, &str)]);
}

impl OrgGuard for AuditService {
    fn policy(&self) -> Option<tandem_core::policy::OrgPolicy> {
        self.entitlements.policy()
    }

    fn audit(&self, event: &str, meta: &[(&str, &str)]) {
        self.record(event, meta);
    }
}
