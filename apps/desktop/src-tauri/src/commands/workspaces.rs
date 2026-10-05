//! Workspaces and tabs (ARCHITECTURE §3.2, F-CD-01).

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use tandem_core::ids::{PaneId, TabId, WorkspaceId};
use tandem_core::IpcError;
use tandem_store::repo::layouts::{self, LayoutNode, TabLayout};
use tandem_store::repo::tabs::{self, Tab};
use tandem_store::repo::workspaces as repo;

use crate::services::workspaces::{self, AddedWorkspace, WorkspaceSummary};
use crate::state::AppState;

/// The native folder picker. Runs on the dialog thread, so other commands
/// keep working while it's open (F-CD-01 AC4).
#[tauri::command]
pub async fn workspace_pick_folder(app: AppHandle) -> Result<Option<String>, IpcError> {
    if cfg!(debug_assertions) {
        if let Ok(fake) = std::env::var("TANDEM_FAKE_PICK_FOLDER") {
            // A file holds the folder to return, so a test can change it per step.
            let picked = match std::fs::read_to_string(&fake) {
                Ok(contents) if std::path::Path::new(&fake).is_file() => {
                    contents.trim().to_string()
                }
                _ => fake,
            };
            return Ok(Some(picked).filter(|f| !f.is_empty()));
        }
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |picked| {
        let _ = tx.send(
            picked
                .and_then(|p| p.into_path().ok())
                .map(|p| p.to_string_lossy().into_owned()),
        );
    });
    Ok(rx.await.ok().flatten())
}

#[tauri::command]
pub async fn workspace_add(
    state: State<'_, AppState>,
    root: String,
) -> Result<AddedWorkspace, IpcError> {
    let limit = state.entitlements.limits().workspaces;
    workspaces::add(&state.store, &root, limit).await
}

#[tauri::command]
pub async fn workspace_list(state: State<'_, AppState>) -> Result<Vec<WorkspaceSummary>, IpcError> {
    workspaces::list(&state.store, &state.terminals.pty).await
}

#[tauri::command]
pub async fn workspace_rename(
    state: State<'_, AppState>,
    id: WorkspaceId,
    name: String,
) -> Result<(), IpcError> {
    Ok(repo::rename(&state.store, &id, name).await?)
}

#[tauri::command]
pub async fn workspace_remove(state: State<'_, AppState>, id: WorkspaceId) -> Result<(), IpcError> {
    workspaces::remove(&state.store, &state.terminals.pty, &id).await
}

#[tauri::command]
pub async fn tab_create(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    name: Option<String>,
) -> Result<Tab, IpcError> {
    Ok(tabs::create(&state.store, &workspace_id, name).await?)
}

#[tauri::command]
pub async fn tab_rename(
    state: State<'_, AppState>,
    id: TabId,
    name: String,
) -> Result<(), IpcError> {
    Ok(tabs::rename(&state.store, &id, name).await?)
}

#[tauri::command]
pub async fn tab_close(state: State<'_, AppState>, id: TabId) -> Result<(), IpcError> {
    for pane in tandem_store::repo::panes::list_open(&state.store, &id).await? {
        let _ = state.terminals.pty.kill(pane.id.as_str());
        tandem_store::repo::panes::close(&state.store, &pane.id).await?;
    }
    Ok(tabs::close(&state.store, &id).await?)
}

#[tauri::command]
pub async fn tab_reorder(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    ids: Vec<TabId>,
) -> Result<(), IpcError> {
    Ok(tabs::reorder(&state.store, &workspace_id, ids).await?)
}

/// Every tab's split tree, in tab order (F-CD-02).
#[tauri::command]
pub async fn layout_get(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> Result<Vec<TabLayout>, IpcError> {
    let list = tabs::list(&state.store, &workspace_id).await?;
    Ok(layouts::for_workspace(&state.store, list).await?)
}

/// Saves a tab's tree after the store validates it (the UI debounces).
#[tauri::command]
pub async fn layout_save(
    state: State<'_, AppState>,
    tab_id: TabId,
    tree: LayoutNode,
    active_pane_id: Option<PaneId>,
    maximized_pane_id: Option<PaneId>,
) -> Result<(), IpcError> {
    Ok(layouts::save(
        &state.store,
        &tab_id,
        tree,
        active_pane_id,
        maximized_pane_id,
    )
    .await?)
}
