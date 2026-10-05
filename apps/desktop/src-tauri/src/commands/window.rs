use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use ts_rs::TS;

use tandem_core::{ErrorCode, IpcError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WindowMaximized {
    pub maximized: bool,
}

fn main_window(app: &AppHandle) -> Result<tauri::WebviewWindow, IpcError> {
    app.get_webview_window("main")
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "The main window is closed"))
}

fn io(e: tauri::Error) -> IpcError {
    IpcError::new(ErrorCode::Internal, e.to_string()).with_stage("window")
}

#[tauri::command]
pub fn window_toggle_maximize(app: AppHandle) -> Result<WindowMaximized, IpcError> {
    let w = main_window(&app)?;
    if w.is_maximized().map_err(io)? {
        w.unmaximize().map_err(io)?;
    } else {
        w.maximize().map_err(io)?;
    }
    Ok(WindowMaximized {
        maximized: w.is_maximized().map_err(io)?,
    })
}

#[tauri::command]
pub fn window_minimize(app: AppHandle) -> Result<(), IpcError> {
    main_window(&app)?.minimize().map_err(io)
}

#[tauri::command]
pub fn window_close(app: AppHandle) -> Result<(), IpcError> {
    main_window(&app)?.close().map_err(io)
}
