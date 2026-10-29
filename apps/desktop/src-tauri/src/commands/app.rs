use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use ts_rs::TS;

use tandem_core::commands::{self as registry, CommandSpec};
use tandem_core::diagnostics::{self, DiagnosticsInfo};
use tandem_core::redact::Redactor;
use tandem_core::{ErrorCode, IpcError};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::services::boot::{self, AppBoot};
use crate::services::deeplink::DeepLink;
use crate::services::opener::is_external_url;
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

/// One topic's queue on the event bus.
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BusDepth {
    pub topic: tandem_core::bus::Topic,
    pub queued: u32,
}

/// Help › Diagnostics, the live panel (PRODUCT_SPEC §5.7): kept on this
/// machine, never sent anywhere.
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Diagnostics {
    #[ts(type = "number")]
    pub uptime_ms: u64,
    pub subsystems: Vec<tandem_core::supervisor::SubsystemHealth>,
    pub panes: Vec<tandem_pty::PaneMetrics>,
    pub bus: Vec<BusDepth>,
}

static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Marks the process start for the uptime (called at startup).
pub fn mark_started() {
    let _ = STARTED.set(std::time::Instant::now());
}

#[tauri::command]
pub fn app_diagnostics(state: State<'_, AppState>) -> Diagnostics {
    Diagnostics {
        uptime_ms: STARTED
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis() as u64,
        subsystems: tandem_core::supervisor::watchdog().snapshot(),
        panes: state.terminals.pty.metrics(),
        bus: state
            .bus
            .depths()
            .into_iter()
            .map(|(topic, n)| BusDepth {
                topic,
                queued: n as u32,
            })
            .collect(),
    }
}

/// Help → Copy diagnostics (F-SH-11): version, OS and the last 200
/// sanitized log lines, also placed on the clipboard.
#[tauri::command]
pub fn app_copy_diagnostics(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, IpcError> {
    let info = DiagnosticsInfo {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        build: boot::build_id(),
        engines: Vec::new(),
        flags: Vec::new(),
    };
    let bundle = diagnostics::bundle(&state.data.logs(), state.data.root(), &info);
    if let Err(err) = app.clipboard().write_text(bundle.clone()) {
        tracing::warn!(error = %err, "couldn't write diagnostics to the clipboard");
    }
    Ok(bundle)
}

/// Help → Open logs folder (F-SH-11).
#[tauri::command]
pub fn app_open_logs_folder(state: State<'_, AppState>) -> Result<(), IpcError> {
    let logs = state.data.logs();
    let _ = std::fs::create_dir_all(&logs);
    if state.e2e {
        return Ok(());
    }
    tauri_plugin_opener::open_path(&logs, None::<&str>)
        .map_err(|e| IpcError::new(ErrorCode::Unavailable, e.to_string()).with_stage("opener"))
}

/// Opens http(s) and mailto links in the system browser (F-CD-09).
#[tauri::command]
pub fn app_open_external(state: State<'_, AppState>, url: String) -> Result<(), IpcError> {
    if !is_external_url(&url) {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Only web and email links can be opened",
        ));
    }
    state
        .opener
        .open(&url)
        .map_err(|e| IpcError::new(ErrorCode::Unavailable, e).with_stage("browser"))
}

#[tauri::command]
pub fn app_quit(app: tauri::AppHandle, state: State<'_, AppState>) {
    state.terminals.shutdown(&state.data.scrollback());
    app.exit(0);
}

/// About › Licenses (F-TL-04 AC2): the third-party notices this build carries.
#[tauri::command]
pub fn app_licenses() -> String {
    include_str!(concat!(env!("OUT_DIR"), "/THIRD_PARTY_NOTICES.md")).to_string()
}

/// Links that arrived before the UI was ready (F-SH-13).
#[tauri::command]
pub fn deeplink_pending(state: State<'_, AppState>) -> Vec<DeepLink> {
    state.deeplinks.take()
}

static PTY_RESIZES: AtomicU64 = AtomicU64::new(0);
static PTY_RESIZES_BY_PANE: std::sync::Mutex<std::collections::BTreeMap<String, u32>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Counts `pty_resize` calls for the E2E harness (F-CD-02 AC1: one per PTY).
pub fn count_resize(pane: &str) {
    PTY_RESIZES.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut m) = PTY_RESIZES_BY_PANE.lock() {
        *m.entry(pane.to_string()).or_insert(0) += 1;
    }
}

pub fn resize_count(reset: bool) -> u64 {
    if reset {
        PTY_RESIZES.swap(0, Ordering::Relaxed)
    } else {
        PTY_RESIZES.load(Ordering::Relaxed)
    }
}

/// Resizes per pane since the last reset.
pub fn resize_counts_by_pane(reset: bool) -> std::collections::BTreeMap<String, u32> {
    PTY_RESIZES_BY_PANE
        .lock()
        .map(|mut m| {
            if reset {
                std::mem::take(&mut *m)
            } else {
                m.clone()
            }
        })
        .unwrap_or_default()
}

#[tauri::command]
pub fn commands_registry() -> Vec<CommandSpec> {
    registry::registry().to_vec()
}

/// `text` as a save's check reads it: redacted as written and past what's
/// removable when it holds a credential, else unchanged, so a caller that
/// compares the two learns what the save refuses (D-3171).
#[tauri::command]
pub fn redact_text(text: String) -> String {
    if tandem_core::untrusted::holds_credential(&text) {
        tandem_core::untrusted::redacted(&text)
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_tk_08_1_the_preview_finds_what_the_save_refuses() {
        // MemoryTab and SkillsTab compare redact_text's answer with what
        // they sent: it changes exactly the text a save would refuse, a
        // credential as written or past what's removable (D-3171).
        let z = '\u{200b}';
        for secret in [
            format!("ghp_0123456789{z}abcdefghijABCDEFGHIJ012345"),
            format!("abc{z}ghp_0123456789{z}abcdefghijABCDEFGHIJ012345"),
        ] {
            let text = format!("- pushed with {secret}");
            assert!(tandem_core::untrusted::holds_credential(&text));
            let shown = redact_text(text.clone());
            assert_ne!(shown, text);
            assert!(!shown.contains("0123456789"), "{shown:?}");
        }
        // No credential, an emoji joiner kept: unchanged, as the save keeps it.
        let fine = "- Ships on Thursdays \u{1F468}\u{200D}\u{1F469}".to_string();
        assert_eq!(redact_text(fine.clone()), fine);
    }
}
