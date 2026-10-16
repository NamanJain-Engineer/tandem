//! The runaway guard's cards (F-AC-09, UI-170).

use tandem_core::IpcError;
use tauri::State;

use crate::services::runaway::GuardCard;
use crate::state::AppState;

/// Every pane's guard card.
#[tauri::command]
pub fn runaway_list(state: State<'_, AppState>) -> Vec<GuardCard> {
    state.runaway.list()
}

/// Dismiss: the card goes, and its limit stays quiet until the next prompt.
#[tauri::command]
pub fn runaway_dismiss(state: State<'_, AppState>, pane_id: String) {
    state.runaway.dismiss(&pane_id);
}

/// Pause: the interrupt key, then a stop if it's still working 5 s later.
#[tauri::command]
pub async fn runaway_pause(state: State<'_, AppState>, pane_id: String) -> Result<(), IpcError> {
    let svc = state.runaway.clone();
    svc.pause(&state, &pane_id).await
}

/// Resume: continues a paused agent.
#[tauri::command]
pub fn runaway_resume(state: State<'_, AppState>, pane_id: String) -> Result<(), IpcError> {
    state.runaway.resume(&pane_id)
}
