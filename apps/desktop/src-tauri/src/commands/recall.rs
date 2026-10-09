//! Command recall (ARCHITECTURE §3.2 "Extras"; F-EX-02).

use tandem_core::ids::PaneId;
use tandem_core::IpcError;
use tandem_store::repo::command_history::HistoryEntry;
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn history_search(
    app: State<'_, AppState>,
    query: String,
    workspace_id: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<HistoryEntry>, IpcError> {
    app.recall
        .search(query, workspace_id, limit.unwrap_or(100))
        .await
}

/// Types the command and Enter into a shell pane at its prompt; the UI
/// offers it only on an explicit Run.
#[tauri::command]
pub async fn history_run(
    app: State<'_, AppState>,
    id: i64,
    pane_id: PaneId,
) -> Result<(), IpcError> {
    app.recall.run(&app.terminals.pty, id, &pane_id).await
}

#[tauri::command]
pub async fn history_delete(app: State<'_, AppState>, id: i64) -> Result<(), IpcError> {
    app.recall.delete(id).await
}

#[tauri::command]
pub async fn history_clear(app: State<'_, AppState>) -> Result<u32, IpcError> {
    app.recall.clear().await
}
