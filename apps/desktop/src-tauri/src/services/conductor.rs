//! Conductor in the app (F-CN-01…04, F-CN-08; ARCHITECTURE §12.6): one
//! [`CallController`] per app, started through [`ConductorService::start`],
//! the single entry point the ring, the palette, the shortcut and the wake
//! phrase (F-CN-07) all use. The running app is the controller's host
//! (`commands::conductor::AppHost`); this module holds what doesn't need
//! a window: the plan gate, the hosted-error mapping, the workspace
//! snapshot and the call log's retention.

use std::sync::{Arc, OnceLock};

use serde_json::{json, Value};
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::conductor as log;
use tandem_store::Store;
use tandem_voice::conductor::call::{CallController, CallOrigin, HostedError};
use tandem_voice::conductor::view::CallView;
use tandem_voice::mic_owner::{MicClaim, MicOwner, MicOwners};

use crate::services::api::ApiError;

/// How long call logs are kept by default (UI-66, F-CN-04).
pub const KEEP_DAYS: i64 = 7;

/// "Keep call logs" (`conductor_call_logs`, UI-67): days, or `None` for
/// "Don't keep".
pub fn keep_days(value: &serde_json::Value) -> Option<i64> {
    match value.as_str() {
        Some("off") => None,
        Some(days) => days.parse().ok(),
        None => Some(KEEP_DAYS),
    }
}

#[derive(Default)]
pub struct ConductorService {
    controller: OnceLock<Arc<CallController>>,
    /// The E2E harness moves a call's clock (the 30-minute cap) without
    /// moving the app's; always 0 otherwise.
    pub test_offset_ms: std::sync::atomic::AtomicI64,
}

impl ConductorService {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Called once from setup with the running app as the host.
    pub fn install(&self, controller: Arc<CallController>) {
        let _ = self.controller.set(controller);
    }

    pub fn controller(&self) -> Option<&Arc<CallController>> {
        self.controller.get()
    }

    /// Starts a call (or returns the one running). Every way of starting a
    /// call comes here; `origin` says which.
    pub fn start(&self, origin: CallOrigin) -> Result<CallView, IpcError> {
        let c = self
            .controller
            .get()
            .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "Conductor isn't ready yet."))?;
        Ok(c.start(origin))
    }

    pub fn is_live(&self) -> bool {
        self.controller.get().is_some_and(|c| c.is_live())
    }

    pub fn view(&self) -> CallView {
        self.controller.get().map(|c| c.view()).unwrap_or_default()
    }
}

/// The call's hold on the microphone (F-CN-07): claimed as
/// [`MicOwner::Call`] before capture opens and held until the call lets go,
/// so the wake listener steps aside and Dictate waits for the whole call.
pub struct CallMic {
    owners: MicOwners,
    claim: parking_lot::Mutex<Option<MicClaim>>,
}

impl CallMic {
    pub fn new(owners: MicOwners) -> Self {
        Self {
            owners,
            claim: parking_lot::Mutex::new(None),
        }
    }

    /// Claims the mic, then runs `open`; a failed open gives it back.
    pub fn take(&self, open: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        let mut held = self.claim.lock();
        if held.is_none() {
            *held = Some(self.owners.claim(MicOwner::Call).map_err(|o| match o {
                MicOwner::Dictate => "Finish dictating first.".to_string(),
                MicOwner::Call => "Another call has the microphone.".to_string(),
            })?);
        }
        open().inspect_err(|_| *held = None)
    }

    /// Runs `close`, then lets go of the mic.
    pub fn give_back(&self, close: impl FnOnce()) {
        close();
        *self.claim.lock() = None;
    }
}

/// An API failure as what the user can do about it.
pub fn hosted_error(e: &ApiError) -> HostedError {
    match e {
        ApiError::Network(m) if m.starts_with("server error") => HostedError::Other(m.clone()),
        ApiError::Network(_) => HostedError::Offline,
        ApiError::Decode(m) => HostedError::Other(m.clone()),
        ApiError::Http { status, code, .. } => match (*status, code.as_str()) {
            (401, _) => HostedError::SignedOut,
            (402, _) => HostedError::OutOfCredits,
            (403, "FORBIDDEN_POLICY") => HostedError::TurnedOff,
            (403, _) => HostedError::NeedsPro,
            (409, _) => HostedError::SessionEnded,
            (429, _) => HostedError::RateLimited,
            _ => HostedError::Other(code.clone()),
        },
    }
}

/// What the talker and the doer know of the workspace: names, engines and
/// states, never file contents or output.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub workspaces: Vec<(String, String)>,
    /// id, name, engine, kind, status, workspace id
    pub panes: Vec<SnapPane>,
    pub threads: Vec<(String, String)>,
    pub teammates: Vec<(String, String)>,
    pub waiting: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SnapPane {
    pub id: String,
    pub name: String,
    pub engine: Option<String>,
    pub kind: String,
    pub status: Option<String>,
    pub workspace: String,
}

impl Snapshot {
    pub fn to_json(&self) -> Value {
        let pairs = |v: &[(String, String)], k: &str| -> Vec<Value> {
            v.iter()
                .map(|(id, name)| json!({"id": id, k: name}))
                .collect()
        };
        json!({
            "workspaces": pairs(&self.workspaces, "name"),
            "panes": self.panes.iter().map(|p| json!({
                "id": p.id, "name": p.name, "engine": p.engine, "kind": p.kind,
                "status": p.status, "workspace": p.workspace,
            })).collect::<Vec<_>>(),
            "threads": pairs(&self.threads, "title"),
            "teammates": pairs(&self.teammates, "name"),
            "waiting": self.waiting,
        })
    }

    /// One paragraph for the talker.
    pub fn summary(&self) -> String {
        let names = |v: Vec<&str>| {
            if v.is_empty() {
                "none".to_string()
            } else {
                v.join(", ")
            }
        };
        format!(
            "Workspaces: {}. Panes: {}. Threads: {}. Teammates: {}. Waiting on the user: {}.",
            names(self.workspaces.iter().map(|w| w.1.as_str()).collect()),
            names(self.panes.iter().map(|p| p.name.as_str()).collect()),
            names(self.threads.iter().map(|t| t.1.as_str()).collect()),
            names(self.teammates.iter().map(|t| t.1.as_str()).collect()),
            self.waiting
        )
    }
}

/// Startup: calls that never settled (the app stopped mid-call) settle now
/// with their last known seconds, and logs past their week go.
pub async fn tidy_log<F, Fut>(store: &Store, now: i64, keep: Option<i64>, settle: F)
where
    F: Fn(String, u32) -> Fut,
    Fut: std::future::Future<Output = Option<u32>>,
{
    if let Ok(open) = log::unsettled(store).await {
        for call in open {
            let seconds = call.connected_seconds.unwrap_or(0).max(0) as u32;
            if let Some(credits) = settle(call.id.clone(), seconds).await {
                let _ = log::settled(store, &call.id, i64::from(credits), now).await;
            }
        }
    }
    // "Don't keep" purges every settled call.
    let _ = log::purge(
        store,
        now - keep.unwrap_or(0) * 86_400_000 + i64::from(keep.is_none()),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cn_01_api_failures_map_to_what_the_user_can_do() {
        let http = |status, code: &str| ApiError::Http {
            status,
            code: code.into(),
            detail: None,
        };
        assert_eq!(
            hosted_error(&http(402, "CREDITS_EXHAUSTED")),
            HostedError::OutOfCredits
        );
        assert_eq!(
            hosted_error(&http(403, "FORBIDDEN_PLAN")),
            HostedError::NeedsPro
        );
        assert_eq!(
            hosted_error(&http(403, "FORBIDDEN_POLICY")),
            HostedError::TurnedOff
        );
        assert_eq!(
            hosted_error(&http(409, "SESSION_ENDED")),
            HostedError::SessionEnded
        );
        assert_eq!(
            hosted_error(&http(401, "UNAUTHORIZED")),
            HostedError::SignedOut
        );
        assert_eq!(
            hosted_error(&ApiError::Network("refused".into())),
            HostedError::Offline
        );
        assert!(matches!(
            hosted_error(&ApiError::Network("server error 503".into())),
            HostedError::Other(_)
        ));
    }

    #[test]
    fn f_cn_02_the_snapshot_has_names_and_states_only() {
        let s = Snapshot {
            workspaces: vec![("ws_1".into(), "tandem".into())],
            panes: vec![SnapPane {
                id: "pn_1".into(),
                name: "auth tests".into(),
                engine: Some("claude".into()),
                kind: "agent".into(),
                status: Some("working".into()),
                workspace: "ws_1".into(),
            }],
            threads: vec![],
            teammates: vec![("tm_1".into(), "Lead".into())],
            waiting: 1,
        };
        let j = s.to_json();
        assert_eq!(j["panes"][0]["name"], "auth tests");
        assert_eq!(j["waiting"], 1);
        assert_eq!(
            s.summary(),
            "Workspaces: tandem. Panes: auth tests. Threads: none. Teammates: Lead. Waiting on the user: 1."
        );
    }

    #[tokio::test]
    async fn f_cn_04_startup_settles_open_calls_and_drops_old_logs() {
        let dir = tempfile::tempdir().expect("dir");
        let store = Store::open_at(&dir.path().join("t.db"), &dir.path().join("b")).expect("store");
        let day = 86_400_000;
        let now = 30 * day;
        log::started(&store, "vs_open", now - day).await.expect("s");
        log::progress(&store, "vs_open", 42).await.expect("p");
        log::started(&store, "vs_old", now - 9 * day)
            .await
            .expect("s");
        log::ended(&store, "vs_old", now - 9 * day, 5, Some(20), "hung_up")
            .await
            .expect("e");
        let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let s2 = seen.clone();
        tidy_log(&store, now, Some(KEEP_DAYS), move |id, secs| {
            let s2 = s2.clone();
            async move {
                s2.lock().push((id, secs));
                Some(100)
            }
        })
        .await;
        assert_eq!(seen.lock().clone(), vec![("vs_open".to_string(), 42)]);
        let calls = log::list(&store, 10).await.expect("list");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].credits, Some(100));
        // "Don't keep": every settled call goes.
        tidy_log(&store, now, None, |_, _| async { None }).await;
        assert!(log::list(&store, 10).await.expect("list").is_empty());
    }

    #[test]
    fn f_cn_04_keep_call_logs_reads_days_or_off() {
        assert_eq!(keep_days(&serde_json::json!("7")), Some(7));
        assert_eq!(keep_days(&serde_json::json!("30")), Some(30));
        assert_eq!(keep_days(&serde_json::json!("off")), None);
        assert_eq!(keep_days(&serde_json::Value::Null), Some(KEEP_DAYS));
    }

    #[test]
    fn f_cn_07_a_call_holds_the_mic_until_it_gives_it_back() {
        let owners = MicOwners::new();
        let mic = CallMic::new(owners.clone());
        let opened = std::cell::Cell::new(false);
        mic.take(|| {
            // The claim is in place before capture opens.
            opened.set(owners.current() == Some(MicOwner::Call));
            Ok(())
        })
        .expect("test");
        assert!(opened.get());
        assert_eq!(owners.current(), Some(MicOwner::Call));
        let closed_while_held = std::cell::Cell::new(false);
        mic.give_back(|| closed_while_held.set(owners.current() == Some(MicOwner::Call)));
        assert!(closed_while_held.get());
        assert_eq!(owners.current(), None);
    }

    #[test]
    fn f_cn_07_a_failed_open_gives_the_mic_back() {
        let owners = MicOwners::new();
        let mic = CallMic::new(owners.clone());
        let err = mic.take(|| Err("Finish dictating first.".into()));
        assert_eq!(err, Err("Finish dictating first.".to_string()));
        assert_eq!(owners.current(), None);
        // And the next call can take it.
        mic.take(|| Ok(())).expect("test");
        assert_eq!(owners.current(), Some(MicOwner::Call));
    }

    #[test]
    fn f_cn_07_dictate_holding_the_mic_refuses_the_call() {
        let owners = MicOwners::new();
        let _dictate = owners.claim(MicOwner::Dictate).expect("test");
        let mic = CallMic::new(owners.clone());
        let mut opened = false;
        let err = mic.take(|| {
            opened = true;
            Ok(())
        });
        assert_eq!(err, Err("Finish dictating first.".to_string()));
        assert!(!opened);
        assert_eq!(owners.current(), Some(MicOwner::Dictate));
    }
}
