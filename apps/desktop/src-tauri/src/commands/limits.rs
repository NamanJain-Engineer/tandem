//! Usage limits on panes (ARCHITECTURE §3.2 "Engines and accounts",
//! F-AC-05, UI-36).

use tauri::State;

use tandem_core::ids::{AccountId, PaneId};
use tandem_core::IpcError;

use crate::services::handoff::HandoffResult;
use crate::services::limits::{self, LimitView};
use crate::state::AppState;

/// The limits showing now (the webview's view after a reload).
#[tauri::command]
pub fn limits_list(state: State<'_, AppState>) -> Vec<LimitView> {
    state.terminals.status.limits.list()
}

/// "Resume at reset": types "continue" at the reset time if the app is open.
#[tauri::command]
pub async fn limit_resume_at_reset(
    state: State<'_, AppState>,
    pane_id: PaneId,
) -> Result<LimitView, IpcError> {
    limits::resume_at_reset(&state, &pane_id).await
}

#[tauri::command]
pub async fn limit_resume_now(state: State<'_, AppState>, pane_id: PaneId) -> Result<(), IpcError> {
    limits::resume_now(&state, &pane_id).await
}

#[tauri::command]
pub fn limit_cancel_resume(
    state: State<'_, AppState>,
    pane_id: PaneId,
) -> Result<LimitView, IpcError> {
    limits::cancel_resume(&state, &pane_id)
}

/// "Stop here".
#[tauri::command]
pub fn limit_dismiss(state: State<'_, AppState>, pane_id: PaneId) {
    limits::dismiss(&state, &pane_id)
}

/// "Continue on another account": a new pane split right with a summary.
#[tauri::command]
pub async fn limit_continue(
    state: State<'_, AppState>,
    pane_id: PaneId,
    engine_id: String,
    account_id: Option<AccountId>,
) -> Result<HandoffResult, IpcError> {
    limits::continue_elsewhere(&state, &pane_id, &engine_id, account_id).await
}
