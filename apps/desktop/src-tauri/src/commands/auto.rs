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
        Some(AutoEndReason::NoSubscription) => (
            NotificationKind::Failed,
            "Auto stopped: it comes with Vibezeo accounts",
        ),
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
                kind,
                title.into(),
                tandem_core::redact::redact(&body),
                view.pane_id.as_str(),
            )
            .await;
    }
}

/// Auto sends a pane's last lines to Vibezeo's servers, so it never runs on
/// a pane started on a local model (F-EN-11).
fn local_stays_here(record: &panes::PaneRecord) -> Result<(), IpcError> {
    if record.local_model.is_some() {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Auto sends this pane's last lines to Vibezeo's servers, so it doesn't run on a model on this computer.",
        ));
    }
    Ok(())
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
    local_stays_here(&record)?;
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
            "Auto comes with Vibezeo accounts. It uses Vibezeo's servers and switches on when Vibezeo accounts open.",
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

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::ids::{TabId, WorkspaceId};

    /// F-EN-11 review: Auto never reads a local model's pane.
    #[test]
    fn f_en_11_3_auto_refuses_a_local_model_pane() {
        let mut record = panes::PaneRecord {
            id: PaneId::from_db("pan_1".into()),
            tab_id: TabId::from_db("tab_1".into()),
            workspace_id: WorkspaceId::from_db("ws_1".into()),
            kind: "agent".into(),
            cwd: None,
            engine_id: Some("codex".into()),
            account_id: None,
            human_name: None,
            title: None,
            restore_state: "none".into(),
            created_at: 1,
            browser_url: None,
            local_model: None,
        };
        assert!(local_stays_here(&record).is_ok());
        record.local_model = Some("qwen3:8b · Ollama".into());
        assert_eq!(
            local_stays_here(&record).expect_err("refused").code,
            ErrorCode::Invalid
        );
    }
}
