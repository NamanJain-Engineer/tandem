//! The git panel (ARCHITECTURE §3.2 "Launch, handoff, lanes, git";
//! F-CD-20): the Files tab's Changes view, its branch chip and the status
//! bar's branch.

use tandem_core::IpcError;
use tandem_git::cli::Git;
use tandem_git::{Branch, FileDiff};
use tauri::State;

use crate::services::files::FileContext;
use crate::services::git_panel::{Committed, GitPanel};
use crate::services::lanes;
use crate::state::AppState;

/// The user's git, found on the login shell's PATH.
async fn user_git(app: &AppState) -> Result<Git, IpcError> {
    let login = app.terminals.login_env().await;
    lanes::git_for(&app.data, &login)
}

#[tauri::command]
pub async fn git_status(
    app: State<'_, AppState>,
    context: FileContext,
) -> Result<Option<GitPanel>, IpcError> {
    app.git_panel.status(&context).await
}

#[tauri::command]
pub async fn git_diff(
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
    staged: bool,
) -> Result<FileDiff, IpcError> {
    app.git_panel.diff(&context, &path, staged).await
}

#[tauri::command]
pub async fn git_stage(
    app: State<'_, AppState>,
    context: FileContext,
    paths: Vec<String>,
) -> Result<(), IpcError> {
    let git = user_git(&app).await?;
    app.git_panel.stage(&git, &context, &paths).await
}

#[tauri::command]
pub async fn git_unstage(
    app: State<'_, AppState>,
    context: FileContext,
    paths: Vec<String>,
) -> Result<(), IpcError> {
    let git = user_git(&app).await?;
    app.git_panel.unstage(&git, &context, &paths).await
}

/// The UI confirms first ("This can't be undone").
#[tauri::command]
pub async fn git_discard(
    app: State<'_, AppState>,
    context: FileContext,
    paths: Vec<String>,
) -> Result<(), IpcError> {
    let git = user_git(&app).await?;
    app.git_panel.discard(&git, &context, &paths).await
}

#[tauri::command]
pub async fn git_commit(
    app: State<'_, AppState>,
    context: FileContext,
    message: String,
) -> Result<Committed, IpcError> {
    let git = user_git(&app).await?;
    app.git_panel.commit(&git, &context, &message).await
}

#[tauri::command]
pub async fn git_push(
    app: State<'_, AppState>,
    context: FileContext,
    publish: Option<bool>,
) -> Result<(), IpcError> {
    let git = user_git(&app).await?;
    app.git_panel
        .push(&git, &context, publish.unwrap_or(false))
        .await
}

#[tauri::command]
pub async fn git_pull(app: State<'_, AppState>, context: FileContext) -> Result<(), IpcError> {
    let git = user_git(&app).await?;
    app.git_panel.pull(&git, &context).await
}

#[tauri::command]
pub async fn git_branches(
    app: State<'_, AppState>,
    context: FileContext,
) -> Result<Vec<Branch>, IpcError> {
    app.git_panel.branches(&context).await
}

#[tauri::command]
pub async fn git_switch(
    app: State<'_, AppState>,
    context: FileContext,
    branch: String,
) -> Result<(), IpcError> {
    let git = user_git(&app).await?;
    app.git_panel.switch(&git, &context, &branch).await
}

#[tauri::command]
pub async fn git_create_branch(
    app: State<'_, AppState>,
    context: FileContext,
    name: String,
) -> Result<(), IpcError> {
    let git = user_git(&app).await?;
    app.git_panel.create_branch(&git, &context, &name).await
}
