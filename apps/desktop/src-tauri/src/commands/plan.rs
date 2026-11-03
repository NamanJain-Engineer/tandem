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

/// Runs `f` off the main thread and the async workers: it writes files.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, IpcError> + Send + 'static,
) -> Result<T, IpcError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
}

/// The edited draft becomes To do tasks in a new project.
#[tauri::command]
pub async fn plan_create_tasks(
    state: State<'_, AppState>,
    id: String,
    tasks: Vec<PlanTask>,
) -> Result<PlanView, IpcError> {
    let plans = plans(&state)?;
    blocking(move || plans.create_tasks(&id, tasks)).await
}

/// What Execute all starts, read now, for you to confirm.
#[tauri::command]
pub async fn plan_prepare(state: State<'_, AppState>, id: String) -> Result<PlanLaunch, IpcError> {
    plans(&state)?.prepare(&id).await
}

/// The runs `plan_prepare` showed, through F-TK-04's queue. With
/// `supervise`, Lead answers their routine questions (F-TK-08); with
/// `send_back`, a run's Changes requested goes back to it (F-TK-09).
#[tauri::command]
pub async fn plan_execute(
    state: State<'_, AppState>,
    id: String,
    supervise: bool,
    send_back: bool,
) -> Result<PlanView, IpcError> {
    plans(&state)?.execute(&id, supervise, send_back).await
}

/// Reviews that ask for changes go back to a plan's runs, or come to you
/// (F-TK-09).
#[tauri::command]
pub async fn plan_send_back(
    state: State<'_, AppState>,
    id: String,
    on: bool,
) -> Result<PlanView, IpcError> {
    plans(&state)?.send_back(&id, on)
}

/// Send back…: a review that came to you goes to its run, with your note
/// (F-TK-09).
#[tauri::command]
pub async fn plan_review_send(
    state: State<'_, AppState>,
    id: String,
    review_id: String,
    note: Option<String>,
) -> Result<PlanView, IpcError> {
    plans(&state)?.send_review(&id, &review_id, note).await
}

/// Lead's answers to a plan's runs, on or off (F-TK-08).
#[tauri::command]
pub async fn plan_supervise(
    state: State<'_, AppState>,
    id: String,
    on: bool,
) -> Result<PlanView, IpcError> {
    plans(&state)?.supervise(&id, on).await
}

/// Your answer to a plan run's question that came to you (F-TK-08).
#[tauri::command]
pub async fn plan_answer(
    state: State<'_, AppState>,
    id: String,
    question_id: String,
    text: String,
) -> Result<PlanView, IpcError> {
    plans(&state)?.answer(&id, &question_id, text).await
}

#[tauri::command]
pub async fn plan_discard(state: State<'_, AppState>, id: String) -> Result<(), IpcError> {
    let plans = plans(&state)?;
    blocking(move || plans.discard(&id)).await
}
