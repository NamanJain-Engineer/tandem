//! Launch presets and lineups (ARCHITECTURE §3.2 "Launch, handoff, lanes,
//! git", F-CD-14).

use tauri::State;

use tandem_core::ids::{PresetId, TabId, WorkspaceId};
use tandem_core::IpcError;
use tandem_store::repo::presets::{self, Isolation, Preset, Seat};

use crate::services::launch::{self, LaunchReport, LaunchedSeat};
use crate::state::AppState;

#[tauri::command]
pub async fn presets_list(state: State<'_, AppState>) -> Result<Vec<Preset>, IpcError> {
    Ok(presets::list(&state.store).await?)
}

/// Saves the lineup as a new preset, or over the saved one `id` names.
#[tauri::command]
pub async fn preset_save(
    state: State<'_, AppState>,
    id: Option<PresetId>,
    name: String,
    seats: Vec<Seat>,
    isolation: Isolation,
) -> Result<Preset, IpcError> {
    Ok(presets::save(&state.store, id, &name, seats, isolation).await?)
}

#[tauri::command]
pub async fn preset_delete(state: State<'_, AppState>, id: PresetId) -> Result<(), IpcError> {
    Ok(presets::delete(&state.store, &id).await?)
}

#[tauri::command]
pub async fn launch_lineup(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    seats: Vec<Seat>,
    task: Option<String>,
    isolation: Isolation,
) -> Result<LaunchReport, IpcError> {
    launch::launch(&state, &workspace_id, &seats, task.as_deref(), isolation).await
}

#[tauri::command]
pub async fn launch_retry_seat(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    seat: Seat,
    task: Option<String>,
    isolation: Isolation,
    tab_id: Option<TabId>,
) -> Result<LaunchedSeat, IpcError> {
    launch::retry(
        &state,
        &workspace_id,
        &seat,
        task.as_deref(),
        isolation,
        tab_id,
    )
    .await
}
