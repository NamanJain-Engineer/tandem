//! F-DB-04: the review queue (ARCHITECTURE §3.2 "Dashboard").

use std::sync::Arc;

use tauri::{State, WebviewWindow};

use tandem_core::ids::WorkspaceId;
use tandem_core::origin::{LocalReview, Origin};
use tandem_core::{ErrorCode, IpcError};
use tandem_git::FileDiff;

use crate::services::review_queue::{
    self as rq, RepoReview, ReviewItem, ReviewQueue, ReviewQueueView,
};
use crate::state::AppState;

fn queue(state: &AppState) -> Result<Arc<ReviewQueue>, IpcError> {
    state
        .review
        .get()
        .cloned()
        .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "The review queue is starting"))
}

#[tauri::command]
pub async fn review_queue(
    state: State<'_, AppState>,
    refresh: Option<bool>,
) -> Result<ReviewQueueView, IpcError> {
    Ok(queue(&state)?.list(refresh.unwrap_or(false)).await)
}

#[tauri::command]
pub async fn review_approve(
    state: State<'_, AppState>,
    key: String,
    diff_hash: String,
) -> Result<ReviewItem, IpcError> {
    queue(&state)?.approve(&key, &diff_hash)
}

#[tauri::command]
pub async fn review_unapprove(
    state: State<'_, AppState>,
    key: String,
) -> Result<ReviewItem, IpcError> {
    queue(&state)?.unapprove(&key)
}

#[tauri::command]
pub async fn review_snooze(
    state: State<'_, AppState>,
    key: String,
    minutes: Option<i64>,
) -> Result<ReviewItem, IpcError> {
    queue(&state)?.snooze(&key, minutes)
}

/// `scope`: "all" or "turn" (since the agent's latest checkpoint).
#[tauri::command]
pub async fn review_item_diff(
    state: State<'_, AppState>,
    key: String,
    scope: String,
) -> Result<Vec<FileDiff>, IpcError> {
    queue(&state)?.item_diff(&key, scope == "turn").await
}

#[tauri::command]
pub async fn review_run_check(state: State<'_, AppState>, key: String) -> Result<(), IpcError> {
    queue(&state)?.run_check(&key).await
}

#[tauri::command]
pub async fn review_repo_get(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> Result<RepoReview, IpcError> {
    let root = rq::workspace_repo_root(&state.store, &workspace_id).await?;
    Ok(rq::repo_review(&state.store, &root).await)
}

/// A command that runs in the repository's folders: only the main window
/// sets it (TL-06 rule 5).
#[tauri::command]
pub async fn review_repo_set(
    window: WebviewWindow,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    check: Option<String>,
    api_paths: Vec<String>,
) -> Result<RepoReview, IpcError> {
    let origin = Origin::Local {
        window: window.label().to_string(),
    };
    LocalReview::check(&origin).ok_or_else(|| {
        IpcError::new(
            ErrorCode::PermissionDenied,
            "Setting a check command needs the main window",
        )
    })?;
    let root = rq::workspace_repo_root(&state.store, &workspace_id).await?;
    rq::save_repo(&state.store, &root, check, api_paths).await
}
