//! The agent sandbox (F-TL-11): whether it can run here, and each agent
//! pane's note.

use tauri::State;

use crate::services::terminal::PaneSandbox;
use crate::state::AppState;
use tandem_engines::sandbox::SandboxStatus;

/// Whether agent panes can be sandboxed on this computer, and if not, why.
#[tauri::command]
pub async fn sandbox_status(
    state: State<'_, AppState>,
) -> Result<SandboxStatus, tandem_core::IpcError> {
    Ok(state.terminals.sandboxer().await.status())
}

/// Each agent pane started while the setting was on: sandboxed, or why not.
#[tauri::command]
pub fn sandbox_panes(state: State<'_, AppState>) -> Vec<PaneSandbox> {
    state.terminals.sandbox_panes()
}
