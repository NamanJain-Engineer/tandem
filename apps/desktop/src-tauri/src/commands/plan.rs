//! F-TK-06: Plan with Lead (ARCHITECTURE §3.2 "Tasks").

use std::sync::Arc;

use tauri::State;

use tandem_core::ids::WorkspaceId;
use tandem_core::{ErrorCode, IpcError};

use crate::services::plan::{PlanLaunch, PlanService, PlanTask, PlanView};
use crate::state::AppState;

fn plans(state: &AppState) -> Result<Arc<PlanService>, IpcError> {
    state
        .plan
        .get()
        .cloned()
        .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "Plans are starting"))
}

/// Opens Lead's planning chat in the workspace.
#[tauri::command]
pub async fn plan_start(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    goal: String,
) -> Result<PlanView, IpcError> {
    plans(&state)?.start(workspace_id, goal).await
}

#[tauri::command]
pub fn plan_list(state: State<'_, AppState>) -> Result<Vec<PlanView>, IpcError> {
    Ok(plans(&state)?.list())
}

#[tauri::command]
pub fn plan_get(state: State<'_, AppState>, id: String) -> Result<PlanView, IpcError> {
    plans(&state)?.get(&id)
}

/// The edited draft becomes To do tasks in a new project.
#[tauri::command]
pub fn plan_create_tasks(
    state: State<'_, AppState>,
    id: String,
    tasks: Vec<PlanTask>,
) -> Result<PlanView, IpcError> {
    plans(&state)?.create_tasks(&id, tasks)
}

/// What Execute all starts, read now, for you to confirm.
#[tauri::command]
pub async fn plan_prepare(state: State<'_, AppState>, id: String) -> Result<PlanLaunch, IpcError> {
    plans(&state)?.prepare(&id).await
}

/// The runs `plan_prepare` showed, through F-TK-04's queue.
#[tauri::command]
pub async fn plan_execute(state: State<'_, AppState>, id: String) -> Result<PlanView, IpcError> {
    plans(&state)?.execute(&id).await
}

#[tauri::command]
pub fn plan_discard(state: State<'_, AppState>, id: String) -> Result<(), IpcError> {
    plans(&state)?.discard(&id)
}
