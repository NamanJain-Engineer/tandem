//! Teammates (ARCHITECTURE §3.2 "Teammates, memory, skills, mail"; F-AG-01,
//! F-AG-02, F-AG-04, F-AG-05).

use tandem_core::ids::TeammateId;
use tandem_core::IpcError;
use tandem_store::repo::teammates::Teammate;
use tauri::State;

use crate::services::teammates::{TeammateInput, TeammatePatch, TeammateSummary};
use crate::state::AppState;

#[tauri::command]
pub async fn teammate_list(state: State<'_, AppState>) -> Result<Vec<TeammateSummary>, IpcError> {
    state.teammates.list().await
}

#[tauri::command]
pub async fn teammate_get(
    state: State<'_, AppState>,
    id: TeammateId,
) -> Result<Teammate, IpcError> {
    state.teammates.get(&id).await
}

#[tauri::command]
pub async fn teammate_create(
    state: State<'_, AppState>,
    input: TeammateInput,
) -> Result<Teammate, IpcError> {
    state.teammates.create(input).await
}

#[tauri::command]
pub async fn teammate_update(
    state: State<'_, AppState>,
    id: TeammateId,
    patch: TeammatePatch,
) -> Result<Teammate, IpcError> {
    let before = state.teammates.get(&id).await?;
    let updated = state.teammates.update(&id, patch).await?;
    // New built-ins (May message) or engine settings reach the next turn.
    if updated.settings != before.settings || updated.engine_id != before.engine_id {
        state.refresh_teammate_sessions(&id).await;
    }
    Ok(updated)
}

#[tauri::command]
pub async fn teammate_delete(state: State<'_, AppState>, id: TeammateId) -> Result<(), IpcError> {
    state.teammates.delete(&id).await
}

#[tauri::command]
pub async fn teammate_duplicate(
    state: State<'_, AppState>,
    id: TeammateId,
) -> Result<Teammate, IpcError> {
    state.teammates.duplicate(&id).await
}

#[tauri::command]
pub async fn teammate_reorder(
    state: State<'_, AppState>,
    ids: Vec<TeammateId>,
) -> Result<(), IpcError> {
    state.teammates.reorder(&ids).await
}
