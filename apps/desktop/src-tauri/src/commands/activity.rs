//! The activity ledger (F-EX-07): a pane's or thread's events, and Export.

use tandem_core::{ErrorCode, IpcError};
use tauri::{AppHandle, State};

use crate::services::activity::ActivityPage;
use crate::state::AppState;

/// `source` is `pane` or `thread`; `before` (ms) pages back from the
/// newest events.
#[tauri::command]
pub async fn activity_list(
    state: State<'_, AppState>,
    source: String,
    id: String,
    before: Option<i64>,
) -> Result<ActivityPage, IpcError> {
    state.terminals.activity.list(&source, &id, before).await
}

/// Writes the events as JSON Lines where the person picks. `None` when they
/// cancel; otherwise the file's path.
#[tauri::command]
pub async fn activity_export(
    app: AppHandle,
    state: State<'_, AppState>,
    source: String,
    id: String,
) -> Result<Option<String>, IpcError> {
    let text = state.terminals.activity.export(&source, &id).await?;
    let now = chrono::Local::now();
    let name = format!("vibezeo-activity-{}.jsonl", now.format("%Y-%m-%d-%H%M"));
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
                .add_filter("JSON Lines", &["jsonl"])
                .save_file(move |picked| {
                    let _ = tx.send(picked.and_then(|p| p.into_path().ok()));
                });
            rx.await.unwrap_or(None)
        }
    };
    let Some(path) = path else { return Ok(None) };
    tandem_core::atomic::atomic_write(&path, text.as_bytes(), Some(0o600)).map_err(|e| {
        IpcError::new(
            ErrorCode::Io,
            format!("The activity couldn't be written: {e}"),
        )
    })?;
    tracing::info!(code = "activity.exported", "activity exported");
    Ok(Some(path.to_string_lossy().into_owned()))
}
