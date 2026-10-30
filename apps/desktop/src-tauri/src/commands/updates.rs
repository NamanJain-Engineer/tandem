//! Updates (ARCHITECTURE §3.2 "Auth, entitlements, billing, updates",
//! F-SH-06). None of these ask for a plan.

use tauri::State;

use tandem_core::{ErrorCode, IpcError};

use crate::services::updates::UpdateState;
use crate::state::AppState;

#[tauri::command]
pub fn updates_state(state: State<'_, AppState>) -> UpdateState {
    state.updates.state()
}

#[tauri::command]
pub async fn updates_check_now(state: State<'_, AppState>) -> Result<UpdateState, IpcError> {
    Ok(state.updates.check_now().await)
}

#[tauri::command]
pub fn updates_set_channel(
    state: State<'_, AppState>,
    channel: String,
) -> Result<UpdateState, IpcError> {
    state.updates.set_channel(&channel)
}

/// "Restart now" in the restart dialog: installs the staged release, stops
/// every terminal (agents can be resumed after the restart) and restarts.
/// Under the test harness the install is recorded and the app keeps running.
#[tauri::command]
pub async fn updates_install(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateState, IpcError> {
    let version = state.updates.install()?;
    if state.e2e {
        tracing::info!(code = "updates.install_recorded", version = %version, "test harness: no restart");
        return Ok(state.updates.state());
    }
    let terminals = state.terminals.clone();
    let scrollback = state.data.scrollback();
    // Agent runs end first (F-TL-12), off the runtime's workers. From an
    // async command restart() still delivers the exit event, which ends
    // them again: harmless, at most 3 s more. (On Windows the updater's
    // installer exits on its own: see `TauriUpdater`'s `before_exit`.)
    tokio::task::spawn_blocking(move || {
        crate::services::agent_run::mark_exiting();
        crate::services::agent_run::shutdown_all();
        terminals.shutdown(&scrollback)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    app.restart();
}
