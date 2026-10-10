//! Auto (ARCHITECTURE §3 "Auto", §8.8, F-AU-01).

use std::sync::Arc;

use async_trait::async_trait;
use tauri::{AppHandle, Manager, State};

use tandem_core::ids::{AutoRunId, PaneId};
use tandem_core::settings::SettingKey;
use tandem_core::status::{SessionKey, StatusRecord};
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::notifications::NotificationKind;
use tandem_store::repo::{panes, settings};

use crate::services::api::{ApiError, AutoNextRequest, AutoNextResponse};
use crate::services::auto::{AutoEndReason, AutoHost, AutoMode, AutoRunView, PaneCheck};
use crate::state::AppState;

/// The running app as Auto's host.
struct AppHost {
    app: AppHandle,
}

impl AppHost {
    fn state(&self) -> State<'_, AppState> {
        self.app.state::<AppState>()
    }
}

/// The words an ended run's notification uses.
fn ending(view: &AutoRunView) -> (NotificationKind, &'static str) {
    match view.end_reason {
        Some(AutoEndReason::Done) => (NotificationKind::Done, "Auto is done"),
        Some(AutoEndReason::Stuck) => (NotificationKind::Failed, "Auto is stuck"),
        Some(AutoEndReason::TurnCeiling) => (NotificationKind::Done, "Auto reached its turn limit"),
        Some(AutoEndReason::OutOfCredits) => {
            (NotificationKind::Failed, "Auto stopped: out of credits")
        }
        Some(AutoEndReason::NoSubscription) => {
            (NotificationKind::Failed, "Auto stopped: it needs Pro")
        }
        Some(AutoEndReason::TurnedOff) => (
            NotificationKind::Failed,
            "Auto stopped: your organization turned it off",
        ),
        Some(AutoEndReason::SignedOut) => {
            (NotificationKind::Failed, "Auto stopped: you're signed out")
        }
        Some(AutoEndReason::ApiUnreachable) => (
            NotificationKind::Failed,
            "Auto stopped: Vibezeo is unreachable",
        ),
        Some(AutoEndReason::PaneClosed) => {
            (NotificationKind::Failed, "Auto stopped: the pane closed")
        }
        Some(AutoEndReason::EngineExited) => {
            (NotificationKind::Failed, "Auto stopped: the agent exited")
        }
        Some(AutoEndReason::EngineChanged) => {
            (NotificationKind::Failed, "Auto stopped: the engine changed")
        }
        Some(AutoEndReason::UserStopped) | None => (NotificationKind::Done, "Auto stopped"),
    }
}

#[async_trait]
impl AutoHost for AppHost {
    async fn pane(&self, pane: &str) -> PaneCheck {
        let state = self.state();
        let Ok(record) = panes::get_open(&state.store, &PaneId::from_db(pane.into())).await else {
            return PaneCheck::Closed;
        };
        if record.kind != "agent" {
            return PaneCheck::Shell;
        }
        PaneCheck::Agent {
            engine_id: record.engine_id,
            alive: state.terminals.status.is_watched(pane) && state.terminals.pty.is_live(pane),
        }
    }

    fn status(&self, pane: &str) -> Option<StatusRecord> {
        self.state()
            .terminals
            .status
            .store
            .get(&SessionKey::pane(pane))
    }

    fn tail(&self, pane: &str) -> String {
        self.state()
            .terminals
            .pty
            .plain_tail(pane, crate::services::auto::SCROLLBACK_LINES as u16 * 2)
            .unwrap_or_default()
    }

    async fn type_prompt(&self, pane: &str, text: &str) -> Result<(), String> {
        let state = self.state();
        state
            .terminals
            .type_prompt(&PaneId::from_db(pane.into()), text)
            .await
            .map_err(|e| e.message)
    }

    async fn next(&self, req: &AutoNextRequest) -> Result<AutoNextResponse, ApiError> {
        let state = self.state();
        let api = state.api.clone();
        state
            .auth
            .with_token(|token| {
                let api = api.clone();
                let req = req.clone();
                async move { api.auto_next(&token, &req).await }
            })
            .await
    }

    fn now_ms(&self) -> i64 {
        self.state().clock.now_ms()
    }

    async fn ended(&self, view: &AutoRunView) {
        if view.end_reason == Some(AutoEndReason::UserStopped) {
            return;
        }
        let state = self.state();
        let (kind, title) = ending(view);
        let body = view.detail.clone().unwrap_or_else(|| {
            format!(
                "{} · {} of {} turns",
                view.mode.label(),
                view.turns,
                view.ceiling
            )
        });
        state
            .notifications
            .raise_for_pane(
                &state,
                kind,
                title.into(),
                tandem_core::redact::redact(&body),
                view.pane_id.as_str(),
            )
            .await;
    }
}

/// Starts Auto on an agent pane (F-AU-01).
#[tauri::command]
pub async fn auto_start(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: PaneId,
    mode: AutoMode,
    goal: Option<String>,
    ceiling: Option<u32>,
) -> Result<AutoRunView, IpcError> {
    let only_agents = || IpcError::new(ErrorCode::Invalid, "Auto runs only in agent panes");
    let record = panes::get_open(&state.store, &pane_id)
        .await
        .map_err(|_| only_agents())?;
    if record.kind != "agent" || record.engine_id.is_none() {
        return Err(only_agents());
    }
    if !state.terminals.status.is_watched(pane_id.as_str()) {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Start the agent in this pane before turning on Auto.",
        ));
    }
    if state.entitlements.policy().is_some_and(|p| p.disable_auto) {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "Your organization turned Auto off.",
        ));
    }
    if !state.entitlements.limits().auto {
        return Err(IpcError::new(
            ErrorCode::ForbiddenPlan,
            "Auto needs Vibezeo Pro.",
        ));
    }
    let consent = settings::get(&state.store, SettingKey::AutoConsent)
        .await
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !consent {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "Allow Auto to send this terminal's last lines first.",
        ));
    }
    state.auto.listen();
    state.auto.start(
        Arc::new(AppHost { app }),
        pane_id,
        record.engine_id,
        mode,
        goal,
        ceiling,
    )
}

/// Every run with a chip to show (after a reload).
#[tauri::command]
pub fn auto_list(state: State<'_, AppState>) -> Vec<AutoRunView> {
    state.auto.list()
}

#[tauri::command]
pub fn auto_pause(state: State<'_, AppState>, run_id: AutoRunId) -> Result<AutoRunView, IpcError> {
    state.auto.pause(&run_id)
}

#[tauri::command]
pub fn auto_resume(state: State<'_, AppState>, run_id: AutoRunId) -> Result<AutoRunView, IpcError> {
    state.auto.resume(&run_id)
}

#[tauri::command]
pub async fn auto_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: AutoRunId,
) -> Result<AutoRunView, IpcError> {
    state.auto.stop(&AppHost { app }, &run_id).await
}

/// Clears an ended run's chip.
#[tauri::command]
pub fn auto_dismiss(state: State<'_, AppState>, run_id: AutoRunId) {
    state.auto.dismiss(&run_id)
}
