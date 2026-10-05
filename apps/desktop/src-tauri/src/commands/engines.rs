//! Engines (ARCHITECTURE §3.2, F-EN-02). M3 slice: the bundled registry with
//! PATH-only detection, enough for the pane picker's engine tiles. M4 brings
//! the full detection report, versions and install commands.

use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

use tandem_engines::EngineSpec;

use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineInstall {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineView {
    pub spec: EngineSpec,
    /// Where it was found on the login PATH; absent when not installed.
    pub install: Option<EngineInstall>,
    pub chat_capable: bool,
    pub unsupported_reason: Option<String>,
}

#[tauri::command]
pub async fn engines_list(
    state: State<'_, AppState>,
    host_id: Option<String>,
) -> Result<Vec<EngineView>, tandem_core::IpcError> {
    if host_id.as_deref().is_some_and(|h| h != "local") {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for spec in state.terminals.engines.all() {
        let install = state
            .terminals
            .resolve_engine(spec)
            .await
            .map(|p| EngineInstall {
                path: p.to_string_lossy().into_owned(),
            });
        out.push(EngineView {
            spec: spec.clone(),
            install,
            chat_capable: false,
            unsupported_reason: None,
        });
    }
    Ok(out)
}
