//! Conductor (ARCHITECTURE §3 "Conductor", §12.6; F-CN-01…04, F-CN-08):
//! the running app as the call's host, and the commands the HUD, the ring
//! and the palette call.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State, Window};
use tokio::sync::mpsc;
use ts_rs::TS;

use tandem_core::bus::{BusEvent, Topic};
use tandem_core::ids::PaneId;
use tandem_core::origin::{LocalReview, Origin};
use tandem_core::settings::SettingKey;
use tandem_core::source::SourceRef;
use tandem_core::status::{AgentStatus, SessionKey, WaitingReason};
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::conductor::{self as log, CallEventRow, CallRow};
use tandem_store::repo::{panes, settings, teammates, threads, workspaces};
use tandem_voice::capture::{CaptureEvent, StreamTap};
use tandem_voice::conductor::call::{
    ActRequest, ActResponse, CallController, CallHost, CallOrigin, CallRecord, CallTiming,
    HostedError, MicEvent, SessionGrant, SessionRequest, Settlement, Speaker,
};
use tandem_voice::conductor::exec::{PaneFacts, PaneKind, ToolHost};
use tandem_voice::conductor::memory::RememberRefusal;
use tandem_voice::conductor::persona::PersonaBrief;
use tandem_voice::conductor::view::{CallView, UiAction};
use tandem_voice::realtime::RealtimeProvider;

use crate::services::conductor::{hosted_error, CallMic, SnapPane, Snapshot};
use crate::state::AppState;

/// The running app as a call's host.
pub struct AppHost {
    app: AppHandle,
    speaker: OnceLock<Arc<dyn Speaker>>,
    mic: OnceLock<CallMic>,
}

impl AppHost {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            speaker: OnceLock::new(),
            mic: OnceLock::new(),
        }
    }

    fn state(&self) -> State<'_, AppState> {
        self.app.state::<AppState>()
    }

    fn mic(&self) -> &CallMic {
        self.mic
            .get_or_init(|| CallMic::new(self.state().mic.clone()))
    }

    fn publish(&self, name: &str, payload: Value) {
        self.state()
            .bus
            .publish(BusEvent::new(Topic::Conductor, name, payload));
    }

    async fn hosted<T, F, Fut>(&self, f: F) -> Result<T, HostedError>
    where
        F: Fn(Arc<dyn crate::services::api::ApiClient>, String) -> Fut,
        Fut: std::future::Future<Output = Result<T, crate::services::api::ApiError>>,
    {
        let state = self.state();
        let api = state.api.clone();
        state
            .auth
            .with_token(|token| f(api.clone(), token))
            .await
            .map_err(|e| hosted_error(&e))
    }

    async fn pane_name(&self, record: &panes::PaneRecord) -> String {
        let state = self.state();
        record
            .human_name
            .clone()
            .or_else(|| record.title.clone())
            .or_else(|| {
                state
                    .terminals
                    .pty
                    .state(record.id.as_str())
                    .and_then(|s| s.title)
            })
            .or_else(|| {
                record
                    .engine_id
                    .as_deref()
                    .and_then(|e| state.terminals.engines.get(e))
                    .map(|e| e.label.clone())
            })
            .unwrap_or_else(|| "Terminal".into())
    }

    async fn snapshot_of(&self) -> Snapshot {
        let state = self.state();
        let store = &state.store;
        let mut snap = Snapshot::default();
        for w in workspaces::list(store).await.unwrap_or_default() {
            snap.workspaces.push((w.id.as_str().to_string(), w.name));
        }
        for p in panes::list_all_open(store).await.unwrap_or_default() {
            let status = state
                .terminals
                .status
                .store
                .get(&SessionKey::pane(p.id.as_str()))
                .map(|r| status_word(r.status).to_string());
            snap.panes.push(SnapPane {
                id: p.id.as_str().to_string(),
                name: self.pane_name(&p).await,
                engine: p.engine_id.clone(),
                kind: p.kind.clone(),
                status,
                workspace: p.workspace_id.as_str().to_string(),
            });
        }
        for t in threads::list(store, threads::ThreadKind::Thread, false)
            .await
            .unwrap_or_default()
        {
            snap.threads.push((t.id.as_str().to_string(), t.title));
        }
        for t in teammates::list(store).await.unwrap_or_default() {
            snap.teammates.push((t.id.as_str().to_string(), t.name));
        }
        snap.waiting = state
            .terminals
            .status
            .snapshot()
            .iter()
            .filter(|r| r.status == AgentStatus::Waiting)
            .count()
            + state.approvals.broker.pending().len();
        snap
    }
}

fn status_word(s: AgentStatus) -> &'static str {
    match s {
        AgentStatus::Working => "working",
        AgentStatus::Waiting => "waiting",
        AgentStatus::Done => "done",
        AgentStatus::Idle => "idle",
        AgentStatus::Failed => "failed",
    }
}

#[async_trait]
impl ToolHost for AppHost {
    async fn pane(&self, pane: &str) -> Option<PaneFacts> {
        let state = self.state();
        let record = panes::get_open(&state.store, &PaneId::from_db(pane.into()))
            .await
            .ok()?;
        let status = state.terminals.status.store.get(&SessionKey::pane(pane));
        Some(PaneFacts {
            title: self.pane_name(&record).await,
            kind: if record.kind == "agent" {
                PaneKind::Agent
            } else {
                PaneKind::Shell
            },
            // A prompt waiting for typed input takes typing; an approval
            // or a question never does (F-CN-03, F-EN-06 AC2).
            waiting: status.as_ref().is_some_and(|r| {
                r.status == AgentStatus::Waiting && r.reason != Some(WaitingReason::Input)
            }),
            working: status.is_some_and(|r| r.status == AgentStatus::Working),
        })
    }

    async fn type_prompt(&self, pane: &str, text: &str, send: bool) -> Result<(), String> {
        self.state()
            .terminals
            .type_prompt_with(&PaneId::from_db(pane.into()), text, send)
            .await
            .map_err(|e| e.message)
    }

    async fn run_in_shell(
        &self,
        pane: &str,
        cmd: &str,
        _review: LocalReview,
    ) -> Result<(), String> {
        self.state()
            .terminals
            .pty
            .write(pane, format!("{cmd}\r").as_bytes())
            .map_err(|e| e.to_string())
    }

    async fn read_pane(&self, pane: &str, lines: usize) -> Result<String, String> {
        self.state()
            .terminals
            .pty
            .plain_tail(pane, (lines as u16).saturating_mul(2))
            .map_err(|_| format!("There's no running pane {pane}."))
    }

    async fn get_setting(&self, key: SettingKey) -> Result<Value, String> {
        settings::get(&self.state().store, key)
            .await
            .map_err(|e| e.to_string())
    }

    async fn set_setting(&self, key: SettingKey, value: Value) -> Result<(), String> {
        let state = self.state();
        crate::commands::settings::store_and_apply(&self.app, &state, key, value)
            .await
            .map_err(|e| e.message)
    }

    async fn run_tool(&self, name: &str, args: &Value) -> Result<String, String> {
        let state = self.state();
        let s = |k: &str| args[k].as_str().unwrap_or_default().to_string();
        match name {
            "list_workspaces" | "list_panes" | "list_threads" | "list_teammates" => {
                let snap = self.snapshot_of().await;
                let j = snap.to_json();
                let key = name.trim_start_matches("list_");
                let mut out = j[key].clone();
                if name == "list_panes" && !s("workspace").is_empty() {
                    let ws = s("workspace");
                    out = Value::Array(
                        out.as_array()
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|p| p["workspace"] == ws.as_str())
                            .collect(),
                    );
                }
                Ok(out.to_string())
            }
            "get_waiting" => {
                let mut items = Vec::new();
                for r in state.terminals.status.snapshot() {
                    if r.status == AgentStatus::Waiting {
                        items.push(json!({"pane": r.key, "reason": r.reason, "what": r.preview}));
                    }
                }
                for a in state.approvals.broker.pending() {
                    items.push(json!({"approval": a.summary}));
                }
                Ok(Value::Array(items).to_string())
            }
            "focus_pane" => {
                crate::commands::shell::focus(
                    &self.app,
                    &state,
                    SourceRef::Pane { pane_id: s("pane") },
                )
                .await
                .map_err(|e| e.message)?;
                Ok("Focused.".into())
            }
            "close_pane" => {
                // The window closes it the way a click would, so its layout
                // follows.
                panes::get_open(&state.store, &PaneId::from_db(s("pane")))
                    .await
                    .map_err(|_| format!("There's no pane {}.", s("pane")))?;
                self.ui_action(UiAction {
                    action: "close_pane".into(),
                    args: args.clone(),
                });
                Ok("Closed.".into())
            }
            "find_work" => {
                let found = crate::commands::find_work::find_work(self.app.clone(), s("query"))
                    .await
                    .map_err(|e| e.message)?;
                if found.clear {
                    if let Some(m) = found.matches.first() {
                        let _ = crate::commands::shell::focus(&self.app, &state, m.source.clone())
                            .await;
                    }
                }
                let top: Vec<Value> = found
                    .matches
                    .iter()
                    .take(5)
                    .map(|m| json!({"label": m.label, "doing": m.detail}))
                    .collect();
                Ok(json!({"clear": found.clear, "matches": top}).to_string())
            }
            "get_credits" => {
                let view = state.entitlements.view();
                Ok(match view.credits {
                    Some(c) => format!("{} credits left.", c.monthly + c.purchased),
                    None => "Credits aren't known right now.".into(),
                })
            }
            other => {
                crate::commands::conductor_tools::run(self.app.state(), self, other, args).await
            }
        }
    }

    fn ui_action(&self, action: UiAction) {
        self.publish(
            "conductor:ui-action",
            serde_json::to_value(action).unwrap_or(Value::Null),
        );
    }
}

#[async_trait]
impl crate::commands::conductor_tools::ToolWindow for AppHost {
    fn ui_action(&self, action: UiAction) {
        ToolHost::ui_action(self, action)
    }

    async fn focus(&self, source: SourceRef) -> Result<(), IpcError> {
        crate::commands::shell::focus(&self.app, &self.state(), source).await
    }
}

#[async_trait]
impl CallHost for AppHost {
    async fn mint(&self, req: &SessionRequest) -> Result<SessionGrant, HostedError> {
        self.hosted(|api, token| {
            let req = req.clone();
            async move { api.voice_session(&token, &req).await }
        })
        .await
    }

    async fn act(&self, req: &ActRequest) -> Result<ActResponse, HostedError> {
        self.hosted(|api, token| {
            let req = req.clone();
            async move { api.voice_act(&token, &req).await }
        })
        .await
    }

    async fn settle(&self, session_id: &str, seconds: u32) -> Result<Settlement, HostedError> {
        let r = self
            .hosted(|api, token| {
                let id = session_id.to_string();
                async move { api.voice_settle(&token, &id, seconds).await }
            })
            .await;
        if r.is_ok() {
            // The balance in the title bar follows.
            let _ = self.state().entitlements.refresh().await;
        }
        r
    }

    async fn snapshot(&self) -> (Value, String) {
        let snap = self.snapshot_of().await;
        (snap.to_json(), snap.summary())
    }

    async fn persona(&self) -> PersonaBrief {
        self.state().persona.call_brief().await
    }

    fn provider(&self, name: &str) -> Option<Arc<dyn RealtimeProvider>> {
        tandem_voice::realtime::for_provider(name).map(Arc::from)
    }

    fn mic_start(&self, tx: mpsc::UnboundedSender<MicEvent>) -> Result<(), String> {
        let events = tx.clone();
        let state = self.state();
        self.mic().take(|| {
            state.dictate.call_mic_start(StreamTap {
                samples: Box::new(move |s: &[f32], rate| {
                    let _ = tx.send(MicEvent::Samples(s.to_vec(), rate));
                }),
                events: Box::new(move |e| {
                    if e == CaptureEvent::NotDelivering {
                        let _ = events.send(MicEvent::NotDelivering);
                    }
                }),
            })
        })
    }

    fn mic_stop(&self) {
        let state = self.state();
        self.mic().give_back(|| state.dictate.call_mic_stop());
    }

    fn speaker(&self) -> Arc<dyn Speaker> {
        self.speaker
            .get_or_init(|| {
                if self.state().e2e {
                    Arc::new(tandem_voice::conductor::speaker::SilentSpeaker::default())
                } else {
                    tandem_voice::conductor::speaker::native()
                }
            })
            .clone()
    }

    fn now_ms(&self) -> i64 {
        let state = self.state();
        state.clock.now_ms()
            + state
                .conductor
                .test_offset_ms
                .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn emit(&self, view: &CallView) {
        let payload = serde_json::to_value(view).unwrap_or(Value::Null);
        // The floating ring follows the call too.
        let _ = self.app.emit_to(
            crate::services::conductor_ring::LABEL,
            "conductor:state",
            payload.clone(),
        );
        self.publish("conductor:state", payload);
        if let Some(q) = &view.question {
            self.publish(
                "conductor:confirm-request",
                serde_json::to_value(q).unwrap_or(Value::Null),
            );
        }
    }

    fn emit_level(&self, level: f32) {
        let _ = self.app.emit_to(
            crate::services::conductor_ring::LABEL,
            "conductor:level",
            json!(level),
        );
        self.publish("conductor:level", json!(level));
    }

    async fn record(&self, record: CallRecord) {
        crate::services::conductor::record(&self.state().store, record).await;
    }

    async fn remember(&self, fact: &str) -> Result<(), RememberRefusal> {
        self.state().persona.remember_in_call(fact).await
    }

    async fn recall(&self) -> (bool, Vec<String>) {
        let persona = &self.state().persona;
        (persona.enabled().await, persona.recall().await)
    }

    async fn forget(&self, query: &str) -> usize {
        self.state().persona.forget(query).await.unwrap_or(0)
    }

    async fn after_call(
        &self,
        proposals: Vec<String>,
        ended_at: i64,
        seconds: u32,
        tools: Vec<String>,
    ) {
        self.state()
            .persona
            .after_call(&proposals, ended_at, seconds, &tools)
            .await;
    }
    // F-CN-09: the review walk belongs to one call, and an item may be acted
    // on only after the act that described it answered (D-2423).
    fn call_started(&self) {
        if let Some(q) = self.app.state::<AppState>().review.get() {
            q.walk_reset();
        }
    }

    fn call_ended(&self) {
        if let Some(q) = self.app.state::<AppState>().review.get() {
            q.walk_reset();
        }
    }

    fn act_answered(&self) {
        if let Some(q) = self.app.state::<AppState>().review.get() {
            q.walk_heard();
        }
    }
}

/// Builds the controller once the app runs, and tidies the call log.
pub fn install(app: &AppHandle) {
    let state = app.state::<AppState>();
    let host = Arc::new(AppHost::new(app.clone()));
    let tauri::async_runtime::RuntimeHandle::Tokio(rt) = tauri::async_runtime::handle();
    state
        .conductor
        .install(CallController::on(rt, host.clone(), CallTiming::default()));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let now = state.clock.now_ms();
        let keep = crate::services::conductor::keep_days(
            &settings::get(&state.store, SettingKey::ConductorCallLogs)
                .await
                .unwrap_or(Value::Null),
        );
        crate::services::conductor::tidy_log(&state.store, now, keep, |id, seconds| {
            let host = host.clone();
            async move { host.settle(&id, seconds).await.ok().map(|s| s.charged) }
        })
        .await;
        sync_ring(&app).await;
    });
}

/// Shows the floating ring while "Floating ring" is on and the plan has
/// Conductor; hides it otherwise (F-CN-04).
pub async fn sync_ring(app: &AppHandle) {
    let state = app.state::<AppState>();
    let on = settings::get(&state.store, SettingKey::ConductorFloatingRing)
        .await
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let show = on && state.entitlements.limits().conductor;
    let saved = crate::services::conductor_ring::saved_position(&state.store).await;
    crate::services::conductor_ring::sync(app, show, saved);
}

/// Where the floating ring was dragged to (physical pixels).
#[tauri::command]
pub async fn conductor_ring_set_position(
    state: State<'_, AppState>,
    x: i32,
    y: i32,
) -> Result<(), IpcError> {
    crate::services::conductor_ring::save_position(&state.store, x, y).await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RingState {
    pub view: CallView,
    pub theme: String,
}

/// The floating ring's start: the call and the theme.
#[tauri::command]
pub async fn conductor_ring_state(state: State<'_, AppState>) -> Result<RingState, IpcError> {
    let theme = settings::get(&state.store, SettingKey::Theme)
        .await
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "dark".into());
    Ok(RingState {
        view: state.conductor.view(),
        theme,
    })
}

fn gate(state: &AppState) -> Result<(), IpcError> {
    if !crate::services::api::hosted_service() {
        return Err(IpcError::new(
            ErrorCode::Unavailable,
            "Conductor needs Vibezeo's hosted service, which this build doesn't use.",
        ));
    }
    if state
        .entitlements
        .policy()
        .is_some_and(|p| p.disable_conductor)
    {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "Your organization turned Conductor off.",
        ));
    }
    if !state.entitlements.limits().conductor {
        return Err(IpcError::new(
            ErrorCode::ForbiddenPlan,
            "Conductor comes with Vibezeo accounts. It uses Vibezeo's servers and switches on when Vibezeo accounts open.",
        ));
    }
    Ok(())
}

/// Starts a call, or returns the one running (F-CN-01). `wake`: the wake
/// phrase started it; `pushToTalk`: the ring is held.
#[tauri::command]
pub fn conductor_start_call(
    state: State<'_, AppState>,
    wake: bool,
    push_to_talk: Option<bool>,
) -> Result<CallView, IpcError> {
    let origin = if wake {
        CallOrigin::Wake
    } else if push_to_talk.unwrap_or(false) {
        CallOrigin::PushToTalk
    } else {
        CallOrigin::Button
    };
    start_call(&state, origin)
}

/// The plan gate, then the one entry point; a running call is returned.
fn start_call(state: &AppState, origin: CallOrigin) -> Result<CallView, IpcError> {
    if state.conductor.is_live() {
        return Ok(state.conductor.view());
    }
    gate(state)?;
    state.conductor.start(origin)
}

/// "Hey Conductor" starts calls here (F-CN-07): the same gate and entry
/// point as the ring, as [`CallOrigin::Wake`].
pub struct WakeStart(pub AppHandle);

impl crate::services::wake::CallStarter for WakeStart {
    fn start_call_from_wake(&self) -> Result<(), String> {
        start_call(&self.0.state::<AppState>(), CallOrigin::Wake)
            .map(|_| ())
            .map_err(|e| e.message)
    }
}

/// Hangs up; with no call, clears an ended call's receipt or error.
#[tauri::command]
pub fn conductor_end_call(state: State<'_, AppState>) {
    if let Some(c) = state.conductor.controller() {
        c.end();
    }
}

#[tauri::command]
pub fn conductor_mute(state: State<'_, AppState>, muted: bool) {
    if let Some(c) = state.conductor.controller() {
        c.mute(muted);
    }
}

/// The HUD's Yes or No (F-CN-03): only a click in the main window answers
/// a question this way, and only the one it names.
#[tauri::command]
pub fn conductor_confirm(
    window: Window,
    state: State<'_, AppState>,
    request_id: String,
    yes: bool,
) -> Result<(), IpcError> {
    let review = LocalReview::check(&Origin::Local {
        window: window.label().to_string(),
    });
    if yes && review.is_none() {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "Confirm in the main window.",
        ));
    }
    let c = state
        .conductor
        .controller()
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "There's no call."))?;
    c.confirm_on_screen(&request_id, if yes { review } else { None })
        .map_err(|e| IpcError::new(ErrorCode::NotFound, e.to_string()))
}

/// The HUD after a reload.
#[tauri::command]
pub fn conductor_state(state: State<'_, AppState>) -> CallView {
    state.conductor.view()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CallLogEntry {
    pub call: CallRow,
    pub events: Vec<CallEventRow>,
}

/// The call log (UI-66): previous calls, newest first, and one call's turns.
#[tauri::command]
pub async fn conductor_call_log(
    state: State<'_, AppState>,
    call_id: Option<String>,
) -> Result<Vec<CallLogEntry>, IpcError> {
    let calls = log::list(&state.store, 200).await?;
    let mut out = Vec::new();
    for (i, call) in calls.into_iter().enumerate() {
        let wanted = match &call_id {
            Some(id) => *id == call.id,
            None => i == 0,
        };
        let events = if wanted {
            log::events(&state.store, &call.id).await?
        } else {
            Vec::new()
        };
        out.push(CallLogEntry { call, events });
    }
    Ok(out)
}

#[tauri::command]
pub async fn conductor_call_delete(
    state: State<'_, AppState>,
    call_id: String,
) -> Result<(), IpcError> {
    Ok(log::delete(&state.store, &call_id).await?)
}

/// The E2E harness: moves the call's clock forward (the cap, F-CN-01 AC3).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn test_conductor_clock(state: State<'_, AppState>, advance_ms: i64) -> Result<i64, IpcError> {
    if !state.e2e {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "The test harness is off",
        ));
    }
    Ok(state
        .conductor
        .test_offset_ms
        .fetch_add(advance_ms, std::sync::atomic::Ordering::Relaxed)
        + advance_ms)
}

/// The E2E harness: tool calls as if the doer had returned them.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn test_conductor_inject(
    state: State<'_, AppState>,
    calls: Vec<Value>,
) -> Result<bool, IpcError> {
    if !state.e2e {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "The test harness is off",
        ));
    }
    let calls: Vec<tandem_voice::conductor::exec::ToolCall> = calls
        .into_iter()
        .filter_map(|c| serde_json::from_value(c).ok())
        .collect();
    Ok(state
        .conductor
        .controller()
        .is_some_and(|c| c.inject(calls)))
}

/// The E2E harness: the wake phrase was heard (F-CN-07). `false` before the
/// wake service is up.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn test_conductor_wake(state: State<'_, AppState>) -> Result<bool, IpcError> {
    if !state.e2e {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "The test harness is off",
        ));
    }
    Ok(state.wake.get().map(|w| w.heard()).is_some())
}

/// The E2E harness: who holds the microphone (F-CN-07).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn test_mic_owner(
    state: State<'_, AppState>,
) -> Result<Option<tandem_voice::mic_owner::MicOwner>, IpcError> {
    if !state.e2e {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "The test harness is off",
        ));
    }
    Ok(state.mic.current())
}
