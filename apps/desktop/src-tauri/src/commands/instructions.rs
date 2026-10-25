//! One instructions file for every engine (ARCHITECTURE §3 "Engines,
//! accounts, usage", F-EN-07). Writes need the main window (F-TL-06 rule 5).

use tandem_core::ids::WorkspaceId;
use tandem_core::{ErrorCode, IpcError};
use tandem_git::instructions::{
    self, EngineInput, InstructionsChange, InstructionsPlan, InstructionsReport,
};
use tandem_store::repo::workspaces;
use tauri::{State, WebviewWindow};

use crate::services::instructions as svc;
use crate::services::worktree_setup::{self, SetupState};
use crate::state::AppState;

fn main_window(window: &WebviewWindow) -> Result<(), IpcError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "Changing instructions files needs the main window",
        ))
    }
}

async fn context(
    state: &AppState,
    workspace_id: &WorkspaceId,
) -> Result<(std::path::PathBuf, Vec<EngineInput>), IpcError> {
    let ws = workspaces::get(&state.store, workspace_id).await?;
    let root = svc::root_of(&ws)?;
    let engines = &state.terminals.engines;
    let inputs = svc::installed(engines.base_registry().all(), |id| {
        engines.installed(id).is_some()
    });
    Ok((root, inputs))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, tandem_git::GitError> + Send + 'static,
) -> Result<T, IpcError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
        .map_err(IpcError::from)
}

#[tauri::command]
pub async fn instructions_get(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> Result<InstructionsReport, IpcError> {
    let (root, engines) = context(&state, &workspace_id).await?;
    blocking(move || instructions::inspect(&root, &engines)).await
}

#[tauri::command]
pub async fn instructions_plan(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    source: Option<String>,
) -> Result<InstructionsPlan, IpcError> {
    let (root, engines) = context(&state, &workspace_id).await?;
    blocking(move || instructions::plan(&root, &engines, source.as_deref())).await
}

#[tauri::command]
pub async fn instructions_apply(
    window: WebviewWindow,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    source: Option<String>,
    digest: String,
) -> Result<InstructionsChange, IpcError> {
    main_window(&window)?;
    let (root, engines) = context(&state, &workspace_id).await?;
    let backups = svc::backups(&state.data);
    let done = blocking(move || {
        instructions::apply(&root, &engines, source.as_deref(), &digest, &backups)
    })
    .await?;
    tracing::info!(
        code = "instructions.applied",
        files = done.written.len(),
        "instructions pointed at AGENTS.md"
    );
    Ok(done)
}

#[tauri::command]
pub async fn instructions_undo(
    window: WebviewWindow,
    state: State<'_, AppState>,
    change_id: String,
) -> Result<(), IpcError> {
    main_window(&window)?;
    let backups = svc::backups(&state.data);
    blocking(move || instructions::undo(&backups, &change_id)).await
}

#[tauri::command]
pub async fn instructions_copy_to_worktrees(
    window: WebviewWindow,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    path: String,
) -> Result<SetupState, IpcError> {
    main_window(&window)?;
    let (root, engines) = context(&state, &workspace_id).await?;
    let main = root.clone();
    blocking(move || instructions::copy_to_worktrees(&root, &engines, &path)).await?;
    Ok(worktree_setup::state_at(&state.data, &main))
}
