//! Checkpoints and rewind (ARCHITECTURE §3.2 "Extras"; F-EX-01).

use tandem_core::{ErrorCode, IpcError};
use tandem_git::FileDiff;
use tandem_store::repo::checkpoints::{Checkpoint, CheckpointReason};
use tauri::State;

use crate::services::checkpoints::{
    CheckpointTimeline, CheckpointUsage, Request, RestoreReport, ON_HOST,
};
use crate::services::files::FileContext;
use crate::state::AppState;

/// The context's folder on this computer; a folder on an SSH host has no
/// checkpoints (D-833).
async fn local_folder(
    app: &AppState,
    context: &FileContext,
) -> Result<std::path::PathBuf, IpcError> {
    if app.files.remote_of(context).await?.is_some() {
        return Err(IpcError::new(ErrorCode::Unavailable, ON_HOST).with_stage("checkpoints"));
    }
    app.files.root(context).await
}

/// A thread sees its own checkpoints; a workspace or lane all of its folder's.
fn thread_of(context: &FileContext) -> Option<String> {
    match context {
        FileContext::Thread { thread_id } => Some(thread_id.as_str().to_string()),
        FileContext::Workspace { .. } => None,
    }
}

#[tauri::command]
pub async fn checkpoints_list(
    app: State<'_, AppState>,
    context: FileContext,
) -> Result<CheckpointTimeline, IpcError> {
    let folder = local_folder(&app, &context).await?;
    app.checkpoints.timeline(&folder, thread_of(&context)).await
}

#[tauri::command]
pub async fn checkpoint_create(
    app: State<'_, AppState>,
    context: FileContext,
) -> Result<Option<Checkpoint>, IpcError> {
    let folder = local_folder(&app, &context).await?;
    let workspace_id = match &context {
        FileContext::Workspace { workspace_id } => Some(workspace_id.as_str().to_string()),
        FileContext::Thread { .. } => None,
    };
    app.checkpoints
        .take(
            Request {
                folder,
                workspace_id,
                thread_id: thread_of(&context),
                ..Default::default()
            },
            CheckpointReason::Manual,
        )
        .await
}

#[tauri::command]
pub async fn checkpoint_diff(
    app: State<'_, AppState>,
    checkpoint_id: String,
) -> Result<Vec<FileDiff>, IpcError> {
    app.checkpoints.diff(&checkpoint_id).await
}

#[tauri::command]
pub async fn checkpoint_restore(
    app: State<'_, AppState>,
    checkpoint_id: String,
    paths: Option<Vec<String>>,
) -> Result<RestoreReport, IpcError> {
    app.checkpoints.restore(&checkpoint_id, paths).await
}

#[tauri::command]
pub async fn checkpoints_usage(app: State<'_, AppState>) -> Result<CheckpointUsage, IpcError> {
    Ok(app.checkpoints.usage().await)
}

#[tauri::command]
pub async fn checkpoints_delete_all(app: State<'_, AppState>) -> Result<(), IpcError> {
    app.checkpoints.delete_all().await
}
