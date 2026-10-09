//! The browser (ARCHITECTURE §3.2 "Files, editor, browser, scratchpad";
//! F-CD-17). Opening and navigating read the workspaces (for `file:` pages)
//! and the search engine setting; the rest are main-thread commands, so a
//! page's calls reach it in the order the UI made them.

use std::path::PathBuf;
use std::sync::Arc;

use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::{settings, workspaces};
use tauri::State;

use crate::browser::{BrowserCmd, BrowserService, BrowserState, Device, Navigated, Rect};
use crate::state::AppState;

fn service(app: &AppState) -> Result<Arc<BrowserService>, IpcError> {
    app.browser.get().cloned().ok_or_else(|| {
        IpcError::new(
            ErrorCode::Unavailable,
            crate::browser::wry_host::UNAVAILABLE,
        )
        .with_stage("browser")
    })
}

/// The workspaces' folders, for the `file:` rule.
async fn refresh_roots(app: &AppState, browser: &BrowserService) {
    if let Ok(list) = workspaces::list(&app.store).await {
        browser.set_roots(
            list.into_iter()
                .filter(|w| w.kind == "local")
                .map(|w| PathBuf::from(w.root))
                .collect(),
        );
    }
}

#[tauri::command]
pub async fn browser_open(
    app: State<'_, AppState>,
    browser_id: String,
    url: Option<String>,
    bounds: Rect,
    visible: bool,
) -> Result<BrowserState, IpcError> {
    let browser = service(&app)?;
    refresh_roots(&app, &browser).await;
    browser.open(&browser_id, url.as_deref(), bounds, visible)
}

#[tauri::command]
pub async fn browser_navigate(
    app: State<'_, AppState>,
    browser_id: String,
    input: String,
) -> Result<Navigated, IpcError> {
    let browser = service(&app)?;
    refresh_roots(&app, &browser).await;
    let engine = settings::get(&app.store, SettingKey::SearchEngine)
        .await
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "duckduckgo".into());
    browser.navigate(&browser_id, &input, &engine)
}

#[tauri::command]
pub fn browser_command(
    app: State<'_, AppState>,
    browser_id: String,
    cmd: BrowserCmd,
) -> Result<(), IpcError> {
    service(&app)?.command(&browser_id, cmd)
}

#[tauri::command]
pub fn browser_set_bounds(
    app: State<'_, AppState>,
    browser_id: String,
    bounds: Rect,
    visible: bool,
) -> Result<(), IpcError> {
    service(&app)?.set_bounds(&browser_id, bounds, visible)
}

#[tauri::command]
pub fn browser_set_device(
    app: State<'_, AppState>,
    browser_id: String,
    device: Device,
    rotated: bool,
) -> Result<BrowserState, IpcError> {
    service(&app)?.set_device(&browser_id, device, rotated)
}

#[tauri::command]
pub fn browser_close(app: State<'_, AppState>, browser_id: String) -> Result<(), IpcError> {
    service(&app)?.close(&browser_id);
    Ok(())
}

#[tauri::command]
pub fn browser_clear_data(app: State<'_, AppState>) -> Result<(), IpcError> {
    service(&app)?.clear_data();
    Ok(())
}
