use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use ts_rs::TS;

use tandem_core::commands::{self as registry, CommandSpec};
use tandem_core::diagnostics::{self, DiagnosticsInfo};
use tandem_core::redact::Redactor;
use tandem_core::IpcError;

use crate::services::boot::{self, AppBoot};
use crate::state::AppState;

/// Called by the UI after its first paint: shows the main window and returns
/// boot info (ARCHITECTURE §1.3 step 5).
#[tauri::command]
pub async fn app_ready(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<AppBoot, IpcError> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
    }
    Ok(boot::app_boot(&state).await)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum LogField {
    Text(String),
    Number(f64),
    Flag(bool),
}

/// The UI's logger: codes and scalar fields only, redacted like Rust logs.
#[tauri::command]
pub fn app_log(level: LogLevel, target: String, code: String, fields: BTreeMap<String, LogField>) {
    let fields = Redactor::global()
        .redact(&serde_json::to_string(&fields).unwrap_or_default())
        .into_owned();
    match level {
        LogLevel::Error => {
            tracing::error!(target: "ui", ui_target = %target, code = %code, fields = %fields)
        }
        LogLevel::Warn => {
            tracing::warn!(target: "ui", ui_target = %target, code = %code, fields = %fields)
        }
        LogLevel::Info => {
            tracing::info!(target: "ui", ui_target = %target, code = %code, fields = %fields)
        }
        LogLevel::Debug => {
            tracing::debug!(target: "ui", ui_target = %target, code = %code, fields = %fields)
        }
    }
}

#[tauri::command]
pub fn app_copy_diagnostics(state: State<'_, AppState>) -> Result<String, IpcError> {
    let info = DiagnosticsInfo {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        build: boot::build_id(),
        engines: Vec::new(),
        flags: Vec::new(),
    };
    Ok(diagnostics::bundle(
        &state.data.logs(),
        state.data.root(),
        &info,
    ))
}

#[tauri::command]
pub fn commands_registry() -> Vec<CommandSpec> {
    registry::registry().to_vec()
}

#[tauri::command]
pub fn redact_text(text: String) -> String {
    Redactor::global().redact(&text).into_owned()
}
