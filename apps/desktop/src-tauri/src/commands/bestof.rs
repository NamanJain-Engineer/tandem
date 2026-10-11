//! Best of N (ARCHITECTURE §3.2 "Extras"; F-EX-04).

use tandem_core::ids::WorkspaceId;
use tandem_core::origin::Origin;
use tandem_core::{ErrorCode, IpcError};
use tandem_git::MergeReport;
use tandem_store::repo::bestof::BestOfRun;
use tauri::{AppHandle, State, WebviewWindow};

use crate::commands::lanes::confirm_native;
use crate::services::bestof::{self, BestOfDetail, BestOfSeat};
use crate::state::AppState;

fn origin(window: &WebviewWindow) -> Origin {
    Origin::Local {
        window: window.label().to_string(),
    }
}

#[tauri::command]
pub async fn bestof_start(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    prompt: String,
    seats: Vec<BestOfSeat>,
    check_command: Option<String>,
) -> Result<BestOfRun, IpcError> {
    bestof::start(&state, &workspace_id, &prompt, &seats, check_command).await
}

#[tauri::command]
pub async fn bestof_list(
    state: State<'_, AppState>,
    workspace_id: Option<String>,
) -> Result<Vec<BestOfRun>, IpcError> {
    Ok(tandem_store::repo::bestof::list(&state.store, workspace_id).await?)
}

#[tauri::command]
pub async fn bestof_get(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<BestOfDetail, IpcError> {
    bestof::detail(&state, &run_id).await
}

#[tauri::command]
pub async fn bestof_check(state: State<'_, AppState>, run_id: String) -> Result<(), IpcError> {
    bestof::check(&state, &run_id).await
}

#[tauri::command]
pub async fn bestof_check_stop(state: State<'_, AppState>, run_id: String) -> Result<(), IpcError> {
    bestof::stop_checks(&state, &run_id).await;
    Ok(())
}

/// Removing the other worktrees discards their uncommitted work, so the
/// native confirm (TL-06 rule 5) is the one confirmation, with the F-EX-04
/// copy (D-710); a dirty base refuses before it asks.
#[tauri::command]
pub async fn bestof_keep(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    run_id: String,
    seat: u32,
) -> Result<MergeReport, IpcError> {
    let (title, body) = bestof::keep_question(&state, &run_id, seat).await?;
    if !confirm_native(&app, &title, &body, "Merge and clean up").await {
        return Err(IpcError::new(ErrorCode::Cancelled, "Kept every worktree"));
    }
    bestof::keep(&state, &origin(&window), &run_id, seat).await
}

#[tauri::command]
pub async fn bestof_discard(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    run_id: String,
) -> Result<(), IpcError> {
    let (title, body) = bestof::discard_question(&state, &run_id).await?;
    if !confirm_native(&app, &title, &body, "Discard all").await {
        return Err(IpcError::new(ErrorCode::Cancelled, "Kept every worktree"));
    }
    bestof::discard(&state, &origin(&window), &run_id).await
}
