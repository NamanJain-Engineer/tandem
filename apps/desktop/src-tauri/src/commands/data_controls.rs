//! Settings › Privacy: "Export my data…" and "Delete local data…" (F-TL-05).

use tandem_core::{ErrorCode, IpcError};
use tauri::{AppHandle, State};

use crate::services::data_controls::{self, DataControlsError, DataExport};
use crate::state::AppState;

fn ipc(err: DataControlsError) -> IpcError {
    IpcError::new(
        ErrorCode::Io,
        format!("The export couldn't be written: {err}"),
    )
}

/// Writes the export zip where the user picks. Returns `None` when they
/// cancel the save dialog.
#[tauri::command]
pub async fn data_export(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<DataExport>, IpcError> {
    let now = chrono::Local::now();
    let name = format!("vibezeo-export-{}.zip", now.format("%Y-%m-%d"));
    let path = match std::env::var("TANDEM_FAKE_SAVE_PATH") {
        // Debug builds: the file names where to save; no file reads as cancel.
        Ok(fake) if cfg!(debug_assertions) => std::fs::read_to_string(&fake)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from),
        _ => {
            use tauri_plugin_dialog::DialogExt;
            let (tx, rx) = tokio::sync::oneshot::channel();
            app.dialog()
                .file()
                .set_file_name(&name)
                .add_filter("Zip archive", &["zip"])
                .save_file(move |picked| {
                    let _ = tx.send(picked.and_then(|p| p.into_path().ok()));
                });
            rx.await.unwrap_or(None)
        }
    };
    let Some(path) = path else { return Ok(None) };
    let (store, data) = (state.store.clone(), state.data.clone());
    let exported_at = now.to_rfc3339();
    tokio::task::spawn_blocking(move || {
        data_controls::export(
            &store,
            &data,
            &path,
            env!("CARGO_PKG_VERSION"),
            &exported_at,
        )
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
    .map(Some)
    .map_err(ipc)
}

/// Deletes this device's Vibezeo data: closes every terminal, forgets
/// Vibezeo's keychain secrets, leaves the delete marker and relaunches into
/// a first run (the delete itself happens at that start). Under the test
/// harness the marker is left and the app keeps running for the spec to
/// relaunch.
#[tauri::command]
pub async fn data_delete_local(
    app: AppHandle,
    state: State<'_, AppState>,
    remove_engine_sign_ins: bool,
) -> Result<(), IpcError> {
    data_controls::request_delete(state.data.root(), remove_engine_sign_ins).map_err(|e| {
        IpcError::new(
            ErrorCode::Io,
            format!("Vibezeo couldn't start the deletion: {e}"),
        )
    })?;
    data_controls::forget_secrets(state.keychain.as_ref());
    // The names go with the database; their values go now (F-TL-12).
    let names: Vec<String> = tandem_store::repo::secrets::list(&state.store)
        .await
        .map(|rows| rows.into_iter().map(|r| r.name).collect())
        .unwrap_or_default();
    crate::services::secrets::forget_all(state.keychain.as_ref(), &names);
    if state.e2e {
        tracing::info!(code = "data.delete_recorded", "test harness: no restart");
        return Ok(());
    }
    let terminals = state.terminals.clone();
    let scrollback = state.data.scrollback();
    tokio::task::spawn_blocking(move || terminals.shutdown(&scrollback))
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    app.restart();
}
