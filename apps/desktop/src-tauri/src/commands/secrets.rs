//! Agent-proof secrets (F-TL-12): the Secrets page, and Run with secrets.

use tauri::State;

use tandem_core::ids::{PaneId, TabId};
use tandem_core::IpcError;
use tandem_store::repo::panes::{self, PaneRecord};

use crate::services::secrets::{self, RunPlan, SecretInfo};
use crate::state::AppState;

#[tauri::command]
pub async fn secrets_list(state: State<'_, AppState>) -> Result<Vec<SecretInfo>, IpcError> {
    secrets::list(&state.store).await
}

#[tauri::command]
pub async fn secrets_set(
    state: State<'_, AppState>,
    name: String,
    value: String,
) -> Result<SecretInfo, IpcError> {
    let value = zeroize::Zeroizing::new(value);
    let now = state.clock.now_ms();
    secrets::set(&state.store, state.keychain.as_ref(), &name, &value, now).await
}

#[tauri::command]
pub async fn secrets_delete(state: State<'_, AppState>, name: String) -> Result<(), IpcError> {
    secrets::delete(&state.store, state.keychain.as_ref(), &name).await
}

#[tauri::command]
pub async fn secrets_run_plan(
    state: State<'_, AppState>,
    pane_id: PaneId,
) -> Result<RunPlan, IpcError> {
    let blocked = state.terminals.run_blocked().await;
    secrets::plan(&state.store, &pane_id, blocked).await
}

/// A new terminal pane in `pane_id`'s folder that runs `command` with the
/// folder's `.env`, references resolved and masked. The caller places it in
/// `tab_id`'s layout.
#[tauri::command]
pub async fn secrets_run(
    state: State<'_, AppState>,
    pane_id: PaneId,
    tab_id: TabId,
    command: String,
) -> Result<PaneRecord, IpcError> {
    state
        .terminals
        .check_live_limit(state.entitlements.limits().panes)?;
    if let Some(why) = state.terminals.run_blocked().await {
        return Err(IpcError::new(
            tandem_core::error::ErrorCode::Unavailable,
            why,
        ));
    }
    secrets::check_tab(&state.store, &pane_id, &tab_id).await?;
    let (folder, run) =
        secrets::prepare(&state.store, state.keychain.as_ref(), &pane_id, &command).await?;
    let pane = panes::create_with(
        &state.store,
        &tab_id,
        "terminal",
        Some(folder.to_string_lossy().into_owned()),
        None,
    )
    .await?;
    let name = secrets::pane_name(&run.command);
    let pane = match panes::rename(&state.store, &pane.id, &name).await {
        Ok(_) => panes::get(&state.store, &pane.id).await?,
        Err(_) => pane,
    };
    state.terminals.set_pending_run(&pane.id, run);
    tracing::info!(code = "secrets.run", "a run with secrets started");
    Ok(pane)
}
