//! Plugins, grants and held drafts (ARCHITECTURE §3.2 "Plugins and grants",
//! "Approvals and held drafts"; F-PL-03, F-PL-05, F-PL-08). The work is in
//! `services::plugins`; credentials never cross this boundary.

use tandem_core::ids::ConnectionId;
use tandem_core::IpcError;
use tandem_gateway::PluginOptions;
use tandem_store::repo::held_drafts::HeldState;
use tandem_store::repo::plugins::{self, GrantSubject};
use tauri::State;

use crate::services::plugins::{ConnectOp, HeldDraft, PluginCard, PluginDetail, ToolResultView};
use crate::state::AppState;

/// One card per connection, and one "available" card for a module with
/// none (UI-90; F-AG-03's nudge reads the connected ones).
#[tauri::command]
pub async fn plugins_catalog(state: State<'_, AppState>) -> Result<Vec<PluginCard>, IpcError> {
    state.plugins.catalog().await
}

/// Starts the browser sign-in; progress arrives on `plugins:connect`.
#[tauri::command]
pub async fn plugin_connect_oauth(
    state: State<'_, AppState>,
    plugin_id: String,
    connection_id: Option<ConnectionId>,
) -> Result<ConnectOp, IpcError> {
    state.plugins.connect_oauth(&plugin_id, connection_id).await
}

#[tauri::command]
pub fn plugin_connect_cancel(state: State<'_, AppState>, op_id: String) {
    state.plugins.cancel(&op_id);
}

/// `null` when the key was empty (a cancel that changes nothing).
#[tauri::command]
pub async fn plugin_connect_api_key(
    state: State<'_, AppState>,
    plugin_id: String,
    key: String,
    connection_id: Option<ConnectionId>,
) -> Result<Option<PluginCard>, IpcError> {
    state
        .plugins
        .connect_api_key(&plugin_id, &key, connection_id)
        .await
}

/// A local-port plugin, or a built-in with no sign-in (`port` null).
#[tauri::command]
pub async fn plugin_connect_local_port(
    state: State<'_, AppState>,
    plugin_id: String,
    port: Option<u16>,
) -> Result<PluginCard, IpcError> {
    state.plugins.connect_local(&plugin_id, port).await
}

#[tauri::command]
pub async fn plugin_disconnect(
    state: State<'_, AppState>,
    connection_id: ConnectionId,
) -> Result<(), IpcError> {
    state.plugins.disconnect(&connection_id).await
}

#[tauri::command]
pub async fn plugin_detail(
    state: State<'_, AppState>,
    connection_id: ConnectionId,
) -> Result<PluginDetail, IpcError> {
    state.plugins.detail(&connection_id).await
}

#[tauri::command]
pub async fn plugin_set_option(
    state: State<'_, AppState>,
    connection_id: ConnectionId,
    allow_publish_in_full: bool,
    allow_writes_in_full: bool,
) -> Result<(), IpcError> {
    state
        .plugins
        .set_options(
            &connection_id,
            PluginOptions {
                allow_publish_in_full,
                allow_writes_in_full,
            },
        )
        .await
}

#[tauri::command]
pub async fn grants_list(
    state: State<'_, AppState>,
    subject: GrantSubject,
) -> Result<Vec<ConnectionId>, IpcError> {
    Ok(plugins::grants(&state.store, &subject).await?)
}

/// Grants take effect from the subject's next session (F-PL-05).
#[tauri::command]
pub async fn grants_set(
    state: State<'_, AppState>,
    subject: GrantSubject,
    connection_id: ConnectionId,
    granted: bool,
) -> Result<(), IpcError> {
    plugins::set_grant(&state.store, &subject, &connection_id, granted).await?;
    state.after_grants_changed(&subject).await;
    state.plugins.grants_changed();
    Ok(())
}

#[tauri::command]
pub async fn held_drafts_list(
    app: State<'_, AppState>,
    state: Option<HeldState>,
    origin_id: Option<String>,
) -> Result<Vec<HeldDraft>, IpcError> {
    app.plugins.held_list(state, origin_id).await
}

/// Approve & run (or Edit & run with `editedArgs`): exactly once.
#[tauri::command]
pub async fn held_draft_run(
    state: State<'_, AppState>,
    draft_id: String,
    edited_args: Option<serde_json::Value>,
) -> Result<ToolResultView, IpcError> {
    state.plugins.held_run(&draft_id, edited_args).await
}

#[tauri::command]
pub async fn held_drafts_discard(
    state: State<'_, AppState>,
    draft_ids: Vec<String>,
) -> Result<(), IpcError> {
    state.plugins.held_discard(draft_ids).await
}
