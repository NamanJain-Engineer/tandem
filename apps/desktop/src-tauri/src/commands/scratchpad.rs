//! The scratchpad (F-SH-12): local Markdown notes in `$DATA/scratchpad`.

use tandem_core::{ErrorCode, IpcError};
use tandem_store::scratchpad::{ScratchNote, ScratchpadStore};
use tauri::State;

use crate::state::AppState;

fn store(state: &AppState) -> ScratchpadStore {
    ScratchpadStore::new(state.data.scratchpad())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, IpcError> + Send + 'static,
) -> Result<T, IpcError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
}

#[tauri::command]
pub async fn scratchpad_list(state: State<'_, AppState>) -> Result<Vec<ScratchNote>, IpcError> {
    let store = store(&state);
    blocking(move || Ok(store.list()?)).await
}

#[tauri::command]
pub async fn scratchpad_read(state: State<'_, AppState>, id: String) -> Result<String, IpcError> {
    let store = store(&state);
    blocking(move || Ok(store.read(&id)?)).await
}

#[tauri::command]
pub async fn scratchpad_write(
    state: State<'_, AppState>,
    id: Option<String>,
    markdown: String,
) -> Result<ScratchNote, IpcError> {
    let store = store(&state);
    blocking(move || Ok(store.write(id.as_deref(), &markdown)?)).await
}

#[tauri::command]
pub async fn scratchpad_delete(state: State<'_, AppState>, id: String) -> Result<(), IpcError> {
    let store = store(&state);
    blocking(move || Ok(store.delete(&id)?)).await
}
