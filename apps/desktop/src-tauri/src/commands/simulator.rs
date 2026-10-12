//! The Simulator pane's commands (ARCHITECTURE §3 "Simulator", F-CD-21).
//! Registered on macOS only (`lib.rs`); the module also builds in tests so
//! the Linux gate type-checks it (D-1400).

use std::sync::Arc;

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, State};

use tandem_core::{ErrorCode, IpcError};

use crate::services::simulator::{SimDevice, SimKey, SimScreen, SimStatus, SimulatorService};
use crate::state::AppState;

fn sim(state: &AppState) -> Result<Arc<SimulatorService>, IpcError> {
    state
        .simulator
        .clone()
        .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "The Simulator runs on macOS only."))
}

#[tauri::command]
pub async fn sim_status(state: State<'_, AppState>) -> Result<SimStatus, IpcError> {
    Ok(sim(&state)?.status().await)
}

#[tauri::command]
pub async fn sim_boot(state: State<'_, AppState>, udid: String) -> Result<SimDevice, IpcError> {
    sim(&state)?.boot(&udid).await
}

#[tauri::command]
pub async fn sim_shutdown(state: State<'_, AppState>, udid: String) -> Result<SimDevice, IpcError> {
    sim(&state)?.shutdown(&udid).await
}

/// Picks a built `.app` and installs it on the (booted) device. Returns
/// `false` when the picker is cancelled.
#[tauri::command]
pub async fn sim_install(
    app: AppHandle,
    state: State<'_, AppState>,
    udid: String,
) -> Result<bool, IpcError> {
    use tauri_plugin_dialog::DialogExt;
    let sim = sim(&state)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose a built .app")
        .pick_folder(move |picked| {
            let _ = tx.send(picked.and_then(|p| p.into_path().ok()));
        });
    let Some(path) = rx.await.ok().flatten() else {
        return Ok(false);
    };
    sim.install(&udid, &path).await?;
    Ok(true)
}

/// Starts sending the pane PNG frames (raw bytes) until `sim_stream_stop`.
#[tauri::command]
pub fn sim_stream_start(
    state: State<'_, AppState>,
    pane_id: String,
    udid: String,
    channel: Channel<InvokeResponseBody>,
) -> Result<(), IpcError> {
    sim(&state)?.stream_start(
        &pane_id,
        &udid,
        Box::new(move |png| channel.send(InvokeResponseBody::Raw(png)).is_ok()),
    )
}

#[tauri::command]
pub fn sim_stream_stop(state: State<'_, AppState>, pane_id: String) -> Result<(), IpcError> {
    sim(&state)?.stream_stop(&pane_id);
    Ok(())
}

#[tauri::command]
pub async fn sim_screen(state: State<'_, AppState>, udid: String) -> Result<SimScreen, IpcError> {
    sim(&state)?.screen(&udid).await
}

#[tauri::command]
pub async fn sim_tap(
    state: State<'_, AppState>,
    udid: String,
    x: f64,
    y: f64,
) -> Result<(), IpcError> {
    sim(&state)?.tap(&udid, x, y).await
}

#[tauri::command]
pub async fn sim_text(
    state: State<'_, AppState>,
    udid: String,
    text: String,
) -> Result<(), IpcError> {
    sim(&state)?.text(&udid, &text).await
}

#[tauri::command]
pub async fn sim_key(
    state: State<'_, AppState>,
    udid: String,
    key: SimKey,
) -> Result<(), IpcError> {
    sim(&state)?.key(&udid, key).await
}

#[tauri::command]
pub async fn sim_home(state: State<'_, AppState>, udid: String) -> Result<(), IpcError> {
    sim(&state)?.home(&udid).await
}
