//! Workspaces and tabs (ARCHITECTURE §3.2, F-CD-01).

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use tandem_core::ids::{PaneId, TabId, WorkspaceId};
use tandem_core::{ErrorCode, IpcError};
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
            // A file holds the folder to return, so a test can change it per
            // step. A first line `delay=<ms>` keeps the "dialog" open that long.
            let contents = match std::fs::read_to_string(&fake) {
                Ok(contents) if std::path::Path::new(&fake).is_file() => contents,
                _ => fake,
            };
            let mut lines = contents.lines().map(str::trim).filter(|l| !l.is_empty());
            let mut picked = lines.next().unwrap_or_default().to_string();
            if let Some(ms) = picked
                .strip_prefix("delay=")
                .and_then(|v| v.parse::<u64>().ok())
            {
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                picked = lines.next().unwrap_or_default().to_string();
            }
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
    kind: Option<String>,
    host_id: Option<tandem_core::ids::HostId>,
    wsl_distro: Option<String>,
) -> Result<AddedWorkspace, IpcError> {
    let limit = state.entitlements.limits().workspaces;
    match kind.as_deref().unwrap_or("local") {
        "local" => workspaces::add(&state.store, &root, limit).await,
        "ssh" => {
            let host = host_id.ok_or_else(|| {
                tandem_core::IpcError::new(tandem_core::ErrorCode::Invalid, "Choose the host first")
            })?;
            // The host says whether the folder exists, and its real path.
            let dir = state.remote.list_dir(&host, &root).await?;
            workspaces::add_remote(&state.store, "ssh", &host, &dir.path, None, limit).await
        }
        "wsl" => {
            let distro = wsl_distro.ok_or_else(|| {
                tandem_core::IpcError::new(
                    tandem_core::ErrorCode::Invalid,
                    "Choose the distribution first",
                )
            })?;
            let wsl = state.wsl.clone();
            let d = distro.clone();
            let root = tokio::task::spawn_blocking(move || wsl.checked_root(&d, &root))
                .await
                .map_err(|e| {
                    tandem_core::IpcError::new(tandem_core::ErrorCode::Internal, e.to_string())
                })??;
            let host = tandem_store::repo::hosts::ensure_wsl(&state.store, &distro).await?;
            let added = workspaces::add_remote(
                &state.store,
                "wsl",
                &host.id,
                &root,
                Some(distro.clone()),
                limit,
            )
            .await?;
            // The + menu lists the distro's engines: detect them now, as
            // connecting does for an SSH host.
            let (wsl, remote) = (state.wsl.clone(), state.remote.clone());
            let engines = state.terminals.engines.registry().all().to_vec();
            tauri::async_runtime::spawn(async move {
                match wsl.detect_report(&host.id, &distro, &engines).await {
                    Ok(report) => {
                        if let Err(e) = remote.publish_report(&host.id, &report).await {
                            tracing::warn!(code = "wsl.detect_failed", error = %e.message, "couldn't keep the distro's engines");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(code = "wsl.detect_failed", error = %e.message, "couldn't detect the distro's engines")
                    }
                }
            });
            Ok(added)
        }
        _ => Err(tandem_core::IpcError::new(
            tandem_core::ErrorCode::Invalid,
            "That kind of workspace doesn't exist",
        )),
    }
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

/// Points a workspace at its folder's new location ("Locate…", F-SH-03).
#[tauri::command]
pub async fn workspace_locate(
    state: State<'_, AppState>,
    id: WorkspaceId,
    new_root: String,
) -> Result<(), IpcError> {
    let root = workspaces::canonical_root(&new_root)?;
    Ok(repo::locate(&state.store, &id, root.to_string_lossy().into_owned()).await?)
}

#[tauri::command]
pub async fn workspace_set_flags(
    state: State<'_, AppState>,
    id: WorkspaceId,
    collapsed: Option<bool>,
    show_sessions_collapsed: Option<bool>,
) -> Result<(), IpcError> {
    Ok(repo::set_flags(&state.store, &id, collapsed, show_sessions_collapsed).await?)
}

/// Shows the workspace folder in the system file manager.
#[tauri::command]
pub async fn workspace_reveal(state: State<'_, AppState>, id: WorkspaceId) -> Result<(), IpcError> {
    let ws = repo::get(&state.store, &id).await?;
    tauri_plugin_opener::reveal_item_in_dir(&ws.root).map_err(|e| {
        tracing::warn!(code = "workspace.reveal_failed", error = %e, "reveal failed");
        IpcError::new(ErrorCode::NotFound, "This folder can't be shown.")
    })
}

/// Reorders or pins a rail row (F-SH-03). Code mode orders workspaces; the
/// other modes' rails arrive with their lists.
#[tauri::command]
pub async fn rail_move(
    state: State<'_, AppState>,
    mode: String,
    item_id: String,
    after_id: Option<String>,
    pinned: bool,
) -> Result<(), IpcError> {
    if mode != "code" {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "This rail can't be reordered yet",
        ));
    }
    let id = WorkspaceId::from_db(item_id);
    let after = after_id.map(WorkspaceId::from_db);
    Ok(repo::move_to(&state.store, &id, after, pinned).await?)
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
