//! The Ports panel (ARCHITECTURE §3.2 "Extras"; F-EX-03).

use tandem_core::ids::PaneId;
use tandem_core::IpcError;
use tauri::State;

use crate::services::ports::{ListeningPort, StopOutcome};
use crate::state::AppState;

#[tauri::command]
pub async fn ports_list(app: State<'_, AppState>) -> Result<Vec<ListeningPort>, IpcError> {
    app.ports.list(&app.terminals.pty).await
}

/// The UI asks first; the core checks the process is still the pane's.
#[tauri::command]
pub async fn ports_stop(
    app: State<'_, AppState>,
    pane_id: PaneId,
    pid: u32,
    start_time: u64,
    force: bool,
) -> Result<StopOutcome, IpcError> {
    app.ports
        .stop(&app.terminals.pty, &pane_id, pid, start_time, force)
        .await
}
