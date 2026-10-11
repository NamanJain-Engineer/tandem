//! The phone companion (ARCHITECTURE §3 "Remote", §13.3, F-RM-03): the
//! running app as the phone's host, the API and relay as its network, and
//! the Settings › Remote commands. Arming, grants and the queue need a
//! person in the main window (`LocalReview`, F-TL-06 rule 5).

use std::sync::Arc;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use tandem_core::approvals::{ApprovalDecision, ApprovalScope, ApprovalVia, ApprovalView};
use tandem_core::ids::{ApprovalId, PaneId, RoutineId, ThreadId};
use tandem_core::origin::{LocalReview, Origin};
use tandem_core::remote::messages::{PhoneDecision, PhoneGrants, PushPlatform};
use tandem_core::source::SourceRef;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::events::ThreadEvent;
use tandem_store::repo::notifications::NotificationKind;
use tandem_store::repo::panes;
use tandem_store::repo::threads::ThreadKind;

use crate::services::api::ApiError;
use crate::services::dashboard;
use crate::services::phone::{
    PairVia, Pairing, PairingQr, PairingTicket, PhoneAgent, PhoneArmState, PhoneHost, PhoneNet,
    PhoneRoutine, PhoneThread, PhoneTranscript, PhoneTranscriptItem, PushOutcome,
    QueuedPhoneAction, RelayIn, RelayLink,
};
use crate::services::phone_lan::{pid_of_relay_url, LanConfig, LanState, WebFiles};
use crate::state::AppState;

/// The running app, as seen from the phone. Every call here comes from the
/// phone (`Origin::Phone`).
pub struct AppPhoneHost {
    pub app: AppHandle,
}

impl AppPhoneHost {
    fn state(&self) -> State<'_, AppState> {
        self.app.state::<AppState>()
    }
}

fn tool_line(event: &ThreadEvent) -> Option<(&'static str, String)> {
    match event {
        ThreadEvent::User { text, .. } => Some(("user", text.clone())),
        ThreadEvent::Assistant { markdown } => Some(("assistant", markdown.clone())),
        ThreadEvent::Tool {
            verb, arg_summary, ..
        } => Some((
            "tool",
            format!(
                "{} {arg_summary}",
                serde_json::to_value(verb)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default()
            ),
        )),
        _ => None,
    }
}

#[async_trait]
impl PhoneHost for AppPhoneHost {
    fn desktop_name(&self) -> String {
        crate::services::auth::device_name()
    }

    async fn agents(&self) -> Vec<PhoneAgent> {
        let state = self.state();
        dashboard::snapshot(&state)
            .await
            .into_iter()
            .map(|item| {
                let (pane_id, thread_id) = match &item.source {
                    SourceRef::Pane { pane_id } => (Some(pane_id.to_string()), None),
                    SourceRef::Thread { thread_id } => (None, Some(thread_id.to_string())),
                    _ => (None, None),
                };
                PhoneAgent {
                    key: item.key.0.clone(),
                    title: item
                        .title
                        .clone()
                        .or(item.human_name.clone())
                        .unwrap_or_else(|| "Agent".into()),
                    status: item.status,
                    engine: item.engine_label.clone(),
                    workspace: item.workspace.clone(),
                    pane_id,
                    thread_id,
                    preview: item.preview.clone(),
                    since: item.since,
                }
            })
            .collect()
    }

    fn approvals(&self) -> Vec<ApprovalView> {
        self.state().approvals.broker.pending()
    }

    async fn threads(&self) -> Vec<PhoneThread> {
        let state = self.state();
        let mut threads = Vec::new();
        for (kind, teammate) in [
            (ThreadKind::Thread, false),
            (ThreadKind::TeammateChat, true),
        ] {
            for t in state.threads.list(kind, false).await.unwrap_or_default() {
                threads.push(PhoneThread {
                    id: t.id.to_string(),
                    title: t.title,
                    state: t.state,
                    updated_at: t.updated_at,
                    teammate,
                });
            }
        }
        threads.sort_by_key(|t| std::cmp::Reverse(t.updated_at));
        threads
    }

    async fn thread_read(
        &self,
        thread_id: &str,
        before: Option<i64>,
        limit: u32,
    ) -> Result<PhoneTranscript, String> {
        let id: ThreadId = thread_id
            .parse()
            .map_err(|_| "No such thread".to_string())?;
        let state = self.state();
        let page = state
            .threads
            .events(&id, before, limit)
            .await
            .map_err(|e| e.message)?;
        Ok(PhoneTranscript {
            items: page
                .items
                .iter()
                .filter_map(|env| {
                    tool_line(&env.event).map(|(role, text)| PhoneTranscriptItem {
                        role: role.into(),
                        text: tandem_core::redact::redact(&text),
                        at: env.ts,
                    })
                })
                .collect(),
            before: page.next_before_id,
        })
    }

    async fn thread_send(&self, thread_id: &str, text: &str) -> Result<(), String> {
        let id: ThreadId = thread_id
            .parse()
            .map_err(|_| "No such thread".to_string())?;
        let state = self.state();
        state
            .threads
            .send(&id, text.to_string(), Vec::new(), true)
            .await
            .map(|_| ())
            .map_err(|e| e.message)
    }

    async fn pane_type(&self, pane_id: &str, text: &str) -> Result<(), String> {
        let id = PaneId::from_db(pane_id.to_string());
        let state = self.state();
        let record = panes::get_open(&state.store, &id)
            .await
            .map_err(|_| "That pane is closed".to_string())?;
        // Terminal control reaches agents only, never plain shells (F-RM-03).
        if record.kind != "agent" {
            return Err("Only agent panes take input from the phone".into());
        }
        state
            .terminals
            .type_prompt(&id, text)
            .await
            .map_err(|e| e.message)
    }

    fn approval_resolve(&self, approval_id: &str, decision: PhoneDecision) -> Result<(), String> {
        let id: ApprovalId = approval_id
            .parse()
            .map_err(|_| "No such approval".to_string())?;
        let decision = match decision {
            PhoneDecision::Allow => ApprovalDecision::Allow {
                scope: ApprovalScope::Once,
            },
            PhoneDecision::Deny => ApprovalDecision::Deny { reason: None },
        };
        self.state()
            .approvals
            .broker
            .resolve(&id, decision, ApprovalVia::Phone)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn routines(&self) -> Vec<PhoneRoutine> {
        self.state()
            .routines
            .list()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|v| PhoneRoutine {
                id: v.routine.id.to_string(),
                name: v.routine.name,
                paused: v.routine.paused || v.routine.paused_by_plan,
                running: v.running.is_some(),
            })
            .collect()
    }

    async fn routine_run_now(&self, routine_id: &str) -> Result<(), String> {
        let id: RoutineId = routine_id
            .parse()
            .map_err(|_| "No such routine".to_string())?;
        self.state()
            .routines
            .run_now(&id)
            .await
            .map(|_| ())
            .map_err(|e| e.message)
    }

    async fn queued_notice(&self, approval: &ApprovalView, device: &str) {
        let state = self.state();
        state
            .notifications
            .raise_in_app(
                NotificationKind::Approval,
                format!("{device} asked to allow this"),
                format!(
                    "{} · Confirm it in Settings › Remote.",
                    tandem_core::redact::redact(&approval.summary)
                ),
                approval.source.clone(),
                format!("phone-queue:{}", approval.id),
            )
            .await;
    }
}

/// The API's pairing routes and the relay socket, and the desktop's own
/// LAN relay for LAN pairings (F-RM-04).
pub struct AppPhoneNet {
    pub app: AppHandle,
}

fn pairing_error(err: ApiError) -> IpcError {
    match &err {
        ApiError::Http {
            status: 403, code, ..
        } if code == "FORBIDDEN_PLAN" => IpcError::new(
            ErrorCode::ForbiddenPlan,
            "Pairing a phone needs Vibezeo Pro.",
        ),
        ApiError::Http { status: 403, .. } => IpcError::new(
            ErrorCode::PermissionDenied,
            "Your organization turned the phone companion off.",
        ),
        ApiError::Http { status: 401, .. } => {
            IpcError::new(ErrorCode::Unauthenticated, "Sign in to pair a phone.")
        }
        _ => IpcError::new(
            ErrorCode::Unavailable,
            "Couldn't reach Vibezeo. Check your connection and try again.",
        ),
    }
}

#[async_trait]
impl PhoneNet for AppPhoneNet {
    async fn create_pairing(
        &self,
        pairing_id: &str,
        label: Option<&str>,
    ) -> Result<PairingTicket, IpcError> {
        let state = self.app.state::<AppState>();
        let api = state.api.clone();
        let label: Option<String> = label.map(|l| l.chars().take(60).collect());
        state
            .auth
            .with_token(|token| {
                let api = api.clone();
                let label = label.clone();
                async move {
                    api.pairing_create(&token, pairing_id, label.as_deref())
                        .await
                }
            })
            .await
            .map_err(pairing_error)
    }

    async fn delete_pairing(&self, pairing_id: &str) -> Result<(), String> {
        let state = self.app.state::<AppState>();
        if state.phone_lan.owns(pairing_id) {
            state.phone_lan.delete_pairing(pairing_id);
            return Ok(());
        }
        let api = state.api.clone();
        state
            .auth
            .with_token(|token| {
                let api = api.clone();
                async move { api.pairing_delete(&token, pairing_id).await }
            })
            .await
            .map_err(|e| e.to_string())
    }

    async fn create_lan_pairing(
        &self,
        pairing_id: &str,
        address: Option<&str>,
    ) -> Result<PairingTicket, IpcError> {
        let state = self.app.state::<AppState>();
        state.phone_lan.create_pairing(pairing_id, address)
    }

    fn forget_local(&self, pairing_id: &str) {
        let state = self.app.state::<AppState>();
        if state.phone_lan.owns(pairing_id) {
            state.phone_lan.delete_pairing(pairing_id);
        }
    }

    async fn push(
        &self,
        pairing_id: &str,
        platform: PushPlatform,
        token: &str,
        payload: &str,
    ) -> Result<PushOutcome, String> {
        let state = self.app.state::<AppState>();
        let api = state.api.clone();
        let result = state
            .auth
            .with_token(|access| {
                let api = api.clone();
                async move {
                    api.pairing_push(&access, pairing_id, platform.as_str(), token, payload)
                        .await
                }
            })
            .await;
        match result {
            Ok(()) => Ok(PushOutcome::Sent),
            Err(ApiError::Http { status: 410, .. }) => Ok(PushOutcome::Unregistered),
            Err(e) => Err(e.to_string()),
        }
    }

    async fn dial(&self, relay_url: &str) -> Result<RelayLink, String> {
        let state = self.app.state::<AppState>();
        if let Some(pid) = pid_of_relay_url(relay_url).filter(|p| state.phone_lan.owns(p)) {
            return state.phone_lan.dial(pid);
        }
        let token = state.auth.access_token().await.map_err(|e| e.to_string())?;
        let mut req = relay_url.into_client_request().map_err(|e| e.to_string())?;
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {token}")
                .parse()
                .map_err(|_| "bad token".to_string())?,
        );
        state.net.record(&format!(
            "WS {} open",
            relay_url.split('?').next().unwrap_or_default()
        ));
        let (ws, _) = tokio_tungstenite::connect_async(req)
            .await
            .map_err(|e| e.to_string())?;
        let (mut sink, mut stream) = ws.split();
        let (tx, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (in_tx, rx) = mpsc::unbounded_channel::<RelayIn>();
        tauri::async_runtime::spawn(async move {
            while let Some(frame) = out_rx.recv().await {
                if sink.send(Message::Binary(frame.into())).await.is_err() {
                    break;
                }
            }
            let _ = sink.close().await;
        });
        tauri::async_runtime::spawn(async move {
            let mut code = None;
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(Message::Binary(bytes)) => {
                        let _ = in_tx.send(RelayIn::Frame(bytes.to_vec()));
                    }
                    Ok(Message::Text(text)) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text.as_str()) {
                            if v["type"] == "peer" {
                                let _ = in_tx.send(RelayIn::Peer(v["up"] == true));
                            }
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        code = frame.map(|f| u16::from(f.code));
                        break;
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            let _ = in_tx.send(RelayIn::Closed(code));
        });
        Ok(RelayLink { tx, rx })
    }
}

fn review(window: &WebviewWindow, what: &str) -> Result<LocalReview, IpcError> {
    LocalReview::check(&Origin::Local {
        window: window.label().to_string(),
    })
    .ok_or_else(|| {
        IpcError::new(
            ErrorCode::PermissionDenied,
            format!("{what} needs the main window"),
        )
    })
}

fn org_forbids_phone(state: &AppState) -> bool {
    state.entitlements.policy().is_some_and(|p| p.disable_phone)
}

/// Settings › Remote › Pair a phone: through the hosted relay (Pro), or
/// with `via: "lan"` over the local network (F-RM-04; any plan, no account,
/// LAN mode on).
#[tauri::command]
pub async fn phone_pair_begin(
    window: WebviewWindow,
    state: State<'_, AppState>,
    via: Option<String>,
    address: Option<String>,
) -> Result<PairingQr, IpcError> {
    review(&window, "Pairing a phone")?;
    if via.as_deref() == Some("lan") {
        if org_forbids_phone(&state) {
            return Err(IpcError::new(
                ErrorCode::PermissionDenied,
                "Your organization turned the phone companion off.",
            ));
        }
        if !LanConfig::load(state.data.root()).enabled {
            return Err(IpcError::new(
                ErrorCode::Unavailable,
                "Turn on pairing over your local network first.",
            ));
        }
        return state.phone.pair_begin_via(PairVia::Lan { address }).await;
    }
    if !state.auth.is_signed_in() {
        return Err(IpcError::new(
            ErrorCode::Unauthenticated,
            "Sign in to pair a phone.",
        ));
    }
    if org_forbids_phone(&state) {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "Your organization turned the phone companion off.",
        ));
    }
    if !state.entitlements.limits().phone {
        return Err(IpcError::new(
            ErrorCode::ForbiddenPlan,
            "Pairing a phone needs Vibezeo Pro.",
        ));
    }
    state.phone.pair_begin().await
}

/// The pairing dialog closed before a phone answered.
#[tauri::command]
pub async fn phone_pair_cancel(
    state: State<'_, AppState>,
    pairing_id: String,
) -> Result<(), IpcError> {
    state.phone.pair_cancel(&pairing_id).await;
    Ok(())
}

#[tauri::command]
pub async fn phone_pairings_list(state: State<'_, AppState>) -> Result<Vec<Pairing>, IpcError> {
    state.phone.list().await
}

#[tauri::command]
pub async fn phone_pairing_set_grants(
    window: WebviewWindow,
    state: State<'_, AppState>,
    pairing_id: String,
    grants: PhoneGrants,
) -> Result<(), IpcError> {
    let review = review(&window, "Changing what a phone can do")?;
    state.phone.set_grants(&review, &pairing_id, grants).await
}

#[tauri::command]
pub async fn phone_pairing_revoke(
    state: State<'_, AppState>,
    pairing_id: String,
) -> Result<(), IpcError> {
    state.phone.revoke(&pairing_id, "revoked").await
}

/// "Reset remote access…": every pairing goes and the desktop's key is replaced.
#[tauri::command]
pub async fn phone_reset(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.phone.revoke_all("reset", true).await
}

/// "Allow approvals from phone for the next 8 hours" (`hours` 8), or off (0).
#[tauri::command]
pub fn phone_arm_approvals(
    window: WebviewWindow,
    state: State<'_, AppState>,
    hours: i64,
) -> Result<PhoneArmState, IpcError> {
    let review = review(&window, "Allowing approvals from the phone")?;
    state.phone.arm(&review, hours)
}

#[tauri::command]
pub fn phone_arm_state(state: State<'_, AppState>) -> PhoneArmState {
    state.phone.arm_state()
}

#[tauri::command]
pub async fn phone_queue_list(
    state: State<'_, AppState>,
) -> Result<Vec<QueuedPhoneAction>, IpcError> {
    state.phone.queue_list().await
}

#[tauri::command]
pub async fn phone_queue_decide(
    window: WebviewWindow,
    state: State<'_, AppState>,
    item_id: String,
    apply: bool,
) -> Result<(), IpcError> {
    let review = review(&window, "Confirming a phone's request")?;
    state.phone.queue_decide(&review, &item_id, apply).await
}

/// Settings › Remote › Local network.
#[tauri::command]
pub fn phone_lan_state(state: State<'_, AppState>) -> LanState {
    let config = LanConfig::load(state.data.root());
    let mut lan = state.phone_lan.server().state(config.enabled);
    if !config.enabled {
        lan.port = config.port;
    }
    lan
}

/// Turns LAN mode on or off (and picks its port). Listening on the network
/// is the person's choice at the desktop, so it needs the main window.
#[tauri::command]
pub async fn phone_lan_set(
    window: WebviewWindow,
    state: State<'_, AppState>,
    enabled: bool,
    port: Option<u16>,
) -> Result<LanState, IpcError> {
    review(&window, "Pairing over the local network")?;
    let mut config = LanConfig::load(state.data.root());
    if let Some(port) = port {
        if port < 1024 {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Pick a port from 1024 to 65535.",
            ));
        }
        config.port = port;
    }
    if enabled && org_forbids_phone(&state) {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "Your organization turned the phone companion off.",
        ));
    }
    config.enabled = enabled;
    config
        .save(state.data.root())
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    let server = state.phone_lan.server().clone();
    let lan = if enabled {
        server.start(config.port).await
    } else {
        server.stop();
        let mut lan = server.state(false);
        lan.port = config.port;
        lan
    };
    tracing::info!(code = "phone.lan_set", enabled, "LAN mode changed");
    Ok(lan)
}

/// The phone web app the LAN server serves: the bundled resources, or the
/// source tree's build in a dev run.
fn web_files(app: &AppHandle) -> WebFiles {
    let bundled = app.path().resource_dir().ok().map(|d| d.join("remote-web"));
    let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/remote-web");
    for dir in bundled.into_iter().chain(std::iter::once(dev)) {
        let files = WebFiles::load(&dir);
        if files.has_app() {
            return files;
        }
    }
    WebFiles::default()
}

/// Binds the phone service to the app, reopens live pairings and, if the
/// person turned it on, starts LAN mode.
pub fn start(app: &AppHandle) {
    let state = app.state::<AppState>();
    let phone = state.phone.clone();
    phone.bind(
        Arc::new(AppPhoneHost { app: app.clone() }),
        Arc::new(AppPhoneNet { app: app.clone() }),
    );
    let lan = state.phone_lan.server().clone();
    lan.set_web(web_files(app));
    let config = LanConfig::load(state.data.root());
    tauri::async_runtime::spawn(async move {
        if config.enabled {
            lan.start(config.port).await;
        }
        phone.start().await
    });
}
