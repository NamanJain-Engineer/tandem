//! Worktree lanes (ARCHITECTURE §3.2 "Launch, handoff, lanes, git",
//! F-CD-15). Copy the user sees says "worktree".

use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use tandem_core::ids::{TabId, WorkspaceId};
use tandem_core::origin::{LocalReview, Origin};
use tandem_core::{ErrorCode, IpcError};
use tandem_git::{FileDiff, ForceConfirm, MergeReport};
use tandem_store::repo::panes::PaneRecord;
use tandem_store::repo::workspaces::{self as repo, Workspace};

use crate::services::lanes::{self, StaleLanes};
use crate::state::AppState;

#[tauri::command]
pub async fn lane_create(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    slug_source: String,
) -> Result<Workspace, IpcError> {
    let login = state.terminals.login_env().await;
    let git = lanes::git_for(&state.data, &login)?;
    lanes::create(&state.store, &git, &workspace_id, &slug_source).await
}

#[tauri::command]
pub async fn lane_diff(
    state: State<'_, AppState>,
    lane_id: WorkspaceId,
) -> Result<Vec<FileDiff>, IpcError> {
    lanes::diff(&state.store, &lane_id).await
}

#[tauri::command]
pub async fn lane_merge(
    state: State<'_, AppState>,
    lane_id: WorkspaceId,
) -> Result<MergeReport, IpcError> {
    let login = state.terminals.login_env().await;
    let git = lanes::git_for(&state.data, &login)?;
    lanes::merge(&state.store, &git, &lane_id).await
}

/// A shell pane in the worktree that runs `gh pr create` once its prompt is
/// up. The caller places the pane in `tab_id`'s layout.
#[tauri::command]
pub async fn lane_create_pr(
    state: State<'_, AppState>,
    lane_id: WorkspaceId,
    tab_id: TabId,
) -> Result<PaneRecord, IpcError> {
    state
        .terminals
        .check_live_limit(state.entitlements.limits().panes)?;
    let login = state.terminals.login_env().await;
    let pane = lanes::create_pr_pane(&state.store, &login, &lane_id, &tab_id).await?;
    let terminals = state.terminals.clone();
    let id = pane.id.clone();
    tauri::async_runtime::spawn(async move {
        if !terminals
            .type_command_when_ready(&id, lanes::PR_COMMAND, lanes::PR_READY_TIMEOUT)
            .await
        {
            tracing::warn!("the pull request shell didn't become ready");
        }
    });
    Ok(pane)
}

/// The native "Discard {branch}?" confirm (UI_SPEC confirmations). Debug
/// builds take `TANDEM_FAKE_CONFIRM=yes|no` (or a file holding it) instead.
async fn confirm_discard(app: &AppHandle, branch: &str) -> bool {
    if cfg!(debug_assertions) {
        if let Ok(fake) = std::env::var("TANDEM_FAKE_CONFIRM") {
            let answer = match std::fs::read_to_string(&fake) {
                Ok(contents) if std::path::Path::new(&fake).is_file() => contents,
                _ => fake,
            };
            return answer.trim() == "yes";
        }
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(
            "Deletes the worktree folder and its uncommitted changes. Commits on the branch stay.",
        )
        .title(format!("Discard {branch}?"))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Discard worktree".into(),
            "Cancel".into(),
        ))
        .show(move |ok| {
            let _ = tx.send(ok);
        });
    rx.await.unwrap_or(false)
}

/// `force` comes only from the main window and still needs the native
/// confirm, so neither another window, voice nor the phone can discard
/// uncommitted work (TL-06 rule 5).
#[tauri::command]
pub async fn lane_remove(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    lane_id: WorkspaceId,
    force: bool,
) -> Result<(), IpcError> {
    let confirm = if force {
        let origin = Origin::Local {
            window: window.label().to_string(),
        };
        let review = LocalReview::check(&origin).ok_or_else(|| {
            IpcError::new(
                ErrorCode::PermissionDenied,
                "Discarding a worktree needs a confirmation in the main window",
            )
        })?;
        let branch = repo::get(&state.store, &lane_id)
            .await?
            .lane_branch
            .unwrap_or_default();
        if !confirm_discard(&app, &branch).await {
            return Err(IpcError::new(ErrorCode::Cancelled, "Kept the worktree"));
        }
        Some(ForceConfirm::new(review))
    } else {
        None
    };
    let login = state.terminals.login_env().await;
    let git = lanes::git_for(&state.data, &login)?;
    lanes::remove(&state.store, &state.terminals.pty, &git, &lane_id, confirm).await
}

#[tauri::command]
pub async fn lanes_sweep(state: State<'_, AppState>) -> Result<Vec<StaleLanes>, IpcError> {
    lanes::sweep_and_notify(&state).await
}
