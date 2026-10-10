//! Plugins and grants (ARCHITECTURE §3.2 "Plugins and grants"; F-PL-05):
//! the cards of the modules this build has and their connections, and
//! who each connection is granted to. Connecting (OAuth, API keys) arrives
//! with M12; what's here lists and grants.

use serde::{Deserialize, Serialize};
use tandem_core::ids::ConnectionId;
use tandem_core::IpcError;
use tandem_store::repo::plugins::{self, ConnectionStatus, GrantSubject};
use tauri::State;
use ts_rs::TS;

use crate::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PluginState {
    Connected,
    Available,
    NeedsAttention,
    BlockedByOrg,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginCard {
    pub plugin_id: String,
    pub label: String,
    pub category: String,
    pub description: String,
    /// "ready" or "preview".
    pub status: String,
    pub state: PluginState,
    pub account_label: Option<String>,
    pub connection_id: Option<ConnectionId>,
}

/// One card per connection, and one "available" card for a module with
/// none (F-PL-03's grid; F-AG-03's nudge reads the connected ones).
pub async fn catalog(state: &AppState) -> Result<Vec<PluginCard>, IpcError> {
    let connections = plugins::connections(&state.store).await?;
    let mut cards = Vec::new();
    for module in tandem_gateway::vendors::registry() {
        let m = module.manifest();
        let category = serde_json::to_value(m.category)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let status = match m.status {
            tandem_gateway::vendors::ModuleStatus::Ready => "ready",
            tandem_gateway::vendors::ModuleStatus::Preview => "preview",
        };
        let card = |state, account_label, connection_id| PluginCard {
            plugin_id: m.id.to_string(),
            label: m.label.to_string(),
            category: category.clone(),
            description: m.description.to_string(),
            status: status.to_string(),
            state,
            account_label,
            connection_id,
        };
        let mine: Vec<_> = connections
            .iter()
            .filter(|c| c.plugin_id == m.id && c.status != ConnectionStatus::Disconnected)
            .collect();
        if mine.is_empty() {
            cards.push(card(PluginState::Available, None, None));
        }
        for c in mine {
            let s = match c.status {
                ConnectionStatus::NeedsAttention => PluginState::NeedsAttention,
                _ => PluginState::Connected,
            };
            cards.push(card(s, c.account_label.clone(), Some(c.id.clone())));
        }
    }
    Ok(cards)
}

#[tauri::command]
pub async fn plugins_catalog(state: State<'_, AppState>) -> Result<Vec<PluginCard>, IpcError> {
    catalog(&state).await
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
    Ok(())
}
