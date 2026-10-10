//! Task runs (ARCHITECTURE §3.2 "Tasks"; F-TK-04).

use tandem_core::ids::AttemptId;
use tandem_core::IpcError;
use tauri::State;

use crate::services::tasks_exec::{AttemptView, ExecuteInput};
use crate::state::AppState;

/// Execute… (UI-82). Needs sign-in and Pro; never started by assigning.
#[tauri::command]
pub async fn task_execute(
    state: State<'_, AppState>,
    input: ExecuteInput,
) -> Result<AttemptView, IpcError> {
    state.task_exec.execute(input).await
}

#[tauri::command]
pub async fn task_attempts_list(state: State<'_, AppState>) -> Result<Vec<AttemptView>, IpcError> {
    state.task_exec.list().await
}

#[tauri::command]
pub async fn task_attempt_stop(
    state: State<'_, AppState>,
    attempt_id: AttemptId,
) -> Result<AttemptView, IpcError> {
    state.task_exec.stop(&attempt_id).await
}

/// Resume: the same session, with `text` (or the default retry
/// instructions) as its next turn.
#[tauri::command]
pub async fn task_attempt_resume(
    state: State<'_, AppState>,
    attempt_id: AttemptId,
    text: Option<String>,
) -> Result<AttemptView, IpcError> {
    state.task_exec.resume(&attempt_id, text).await
}

#[tauri::command]
pub async fn task_attempt_retry(
    state: State<'_, AppState>,
    attempt_id: AttemptId,
) -> Result<AttemptView, IpcError> {
    state.task_exec.retry(&attempt_id).await
}
