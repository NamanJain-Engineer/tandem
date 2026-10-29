//! Local models (ARCHITECTURE §3.2 "Engines, accounts, usage", F-EN-11,
//! UI-164).

use tauri::{State, WebviewWindow};

use crate::services::library::LibraryOpened;
use crate::services::local_models::{self, LocalServer};
use crate::state::AppState;
use tandem_core::ids::TabId;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::local_models::ServerKind;

fn main_only(window: &WebviewWindow) -> Result<(), IpcError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "Changing local models needs the main window",
        ))
    }
}

/// The model servers on this computer, asked now.
#[tauri::command]
pub async fn local_models_list(state: State<'_, AppState>) -> Result<Vec<LocalServer>, IpcError> {
    local_models::list(&state).await
}

/// Another port to ask, on this computer only.
#[tauri::command]
pub async fn local_models_add_port(
    window: WebviewWindow,
    state: State<'_, AppState>,
    host: String,
    port: u16,
    kind: ServerKind,
) -> Result<(), IpcError> {
    main_only(&window)?;
    local_models::add_port(&state, &host, port, kind).await
}

#[tauri::command]
pub async fn local_models_remove_port(
    window: WebviewWindow,
    state: State<'_, AppState>,
    port: u16,
) -> Result<(), IpcError> {
    main_only(&window)?;
    local_models::remove_port(&state, port).await
}

/// A Codex pane in `tab_id` using `model` on that server.
#[tauri::command]
pub async fn local_model_start(
    window: WebviewWindow,
    state: State<'_, AppState>,
    tab_id: TabId,
    kind: ServerKind,
    port: u16,
    model: String,
) -> Result<LibraryOpened, IpcError> {
    main_only(&window)?;
    local_models::start(&state, &tab_id, kind, port, &model).await
}
