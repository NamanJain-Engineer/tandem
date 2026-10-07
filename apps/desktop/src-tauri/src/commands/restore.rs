//! Session restore commands (ARCHITECTURE §3.2 "Restore", F-SH-07).

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use tauri::State;

use tandem_core::ids::PaneId;
use tandem_core::{ErrorCode, IpcError};
use tandem_pty::PtyInfo;

use crate::services::restore::RestoreSnapshot;
use crate::services::terminal::LaunchMode;
use crate::state::AppState;

#[tauri::command]
pub async fn restore_get_snapshot(state: State<'_, AppState>) -> Result<RestoreSnapshot, IpcError> {
    state
        .restore
        .snapshot(&state.terminals, &state.data.scrollback())
        .await
}

async fn launch(
    state: &AppState,
    pane_id: PaneId,
    cols: Option<u16>,
    rows: Option<u16>,
    mode: LaunchMode,
) -> Result<PtyInfo, IpcError> {
    let size = cols.zip(rows);
    let max_live = state.entitlements.limits().panes;
    state
        .restore
        .launch(&state.terminals, &pane_id, size, max_live, mode)
        .await
}

/// Resume on a paused card: the saved session in its saved provider home.
#[tauri::command]
pub async fn restore_resume(
    state: State<'_, AppState>,
    pane_id: PaneId,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<PtyInfo, IpcError> {
    launch(&state, pane_id, cols, rows, LaunchMode::Resume).await
}

/// Start fresh on a card: a new conversation in the pane.
#[tauri::command]
pub async fn restore_start_fresh(
    state: State<'_, AppState>,
    pane_id: PaneId,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<PtyInfo, IpcError> {
    launch(&state, pane_id, cols, rows, LaunchMode::Fresh).await
}

/// "Resume agents automatically": the UI calls this once its shell shows.
/// Returns at once; the queue starts agents in the background.
#[tauri::command]
pub fn restore_auto_resume(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let state = app.state::<AppState>();
        state.terminals.login_env().await;
        let max_live = state.entitlements.limits().panes;
        state.restore.auto_resume(&state.terminals, max_live).await;
    });
}

/// Ctrl+C on a card.
#[tauri::command]
pub async fn restore_cancel(state: State<'_, AppState>, pane_id: PaneId) -> Result<(), IpcError> {
    state.restore.cancel(&pane_id).await
}

/// The pane's scrollback from the last run (base64), shown read-only.
#[tauri::command]
pub async fn restore_scrollback(
    state: State<'_, AppState>,
    pane_id: PaneId,
) -> Result<String, IpcError> {
    let id = pane_id.as_str();
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(IpcError::new(ErrorCode::Invalid, "Not a pane id"));
    }
    let path = state.data.scrollback().join(format!("{id}.bin"));
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| IpcError::new(ErrorCode::NotFound, "No saved output for this pane"))?;
    Ok(STANDARD.encode(bytes))
}
