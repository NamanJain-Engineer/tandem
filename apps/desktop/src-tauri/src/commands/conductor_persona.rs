//! Settings › Conductor: memory and the wake phrase (F-CN-06, F-CN-07,
//! ARCHITECTURE §3 "Conductor").

use tauri::State;

use tandem_core::conductor_persona::ConductorMemory;
use tandem_core::{ErrorCode, IpcError};

use crate::services::wake::WakeStatus;
use crate::state::AppState;

#[tauri::command]
pub async fn conductor_memory_get(state: State<'_, AppState>) -> Result<ConductorMemory, IpcError> {
    state.persona.view().await
}

/// Saves the whole file from Settings' editor; credential-shaped text is refused.
#[tauri::command]
pub async fn conductor_memory_save(
    state: State<'_, AppState>,
    text: String,
) -> Result<ConductorMemory, IpcError> {
    if text.len() > 64 * 1024 {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "That's too long to remember.",
        ));
    }
    state.persona.save(&text).await
}

/// "Forget everything…".
#[tauri::command]
pub async fn conductor_memory_clear(
    state: State<'_, AppState>,
) -> Result<ConductorMemory, IpcError> {
    state.persona.clear().await
}

/// Keep or Discard proposed memories.
#[tauri::command]
pub async fn conductor_memory_resolve(
    state: State<'_, AppState>,
    proposal_ids: Vec<String>,
    keep: bool,
) -> Result<ConductorMemory, IpcError> {
    state.persona.resolve(&proposal_ids, keep).await
}

#[tauri::command]
pub async fn conductor_wake_status(state: State<'_, AppState>) -> Result<WakeStatus, IpcError> {
    state
        .wake
        .get()
        .map(|w| w.status())
        .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "The wake phrase is starting."))
}
