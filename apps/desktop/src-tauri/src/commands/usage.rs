//! The Usage page (F-AC-07): the report and its share card.

use base64::Engine;
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::{accounts, workspaces};
use tauri::{AppHandle, State};

use crate::services::usage_report::{self, UsageReport};
use crate::state::AppState;

/// Tokens from the engines' local logs for the last 12 weeks.
#[tauri::command]
pub async fn usage_report(state: State<'_, AppState>) -> Result<UsageReport, IpcError> {
    let accounts = accounts::list(&state.store).await?;
    let workspaces = workspaces::list(&state.store).await?;
    let registry = state.terminals.engines.base_registry();
    let now = chrono::Local::now();
    let (today, tz) = (now.date_naive(), *now.offset());
    tokio::task::spawn_blocking(move || {
        usage_report::report(&accounts, &workspaces, &registry, today, tz)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))
}

/// Each account's sessions that spent its tokens in the last hour, from
/// the engines' local logs (F-AC-08); only token counts are read.
#[tauri::command]
pub async fn usage_sessions(
    state: State<'_, AppState>,
    account_id: Option<tandem_core::ids::AccountId>,
) -> Result<Vec<crate::services::usage::AccountSessions>, IpcError> {
    let all = accounts::list(&state.store).await?;
    let accounts: Vec<_> = all
        .into_iter()
        .filter(|a| account_id.as_ref().is_none_or(|id| *id == a.id))
        .collect();
    let registry = state.terminals.engines.base_registry();
    let since = state.clock.now_ms() - crate::services::usage::SESSIONS_SPAN_MS;
    let panes = state.terminals.status.engine_sessions();
    tokio::task::spawn_blocking(move || {
        let sources = usage_report::sources(&accounts, &registry);
        let tokens = tandem_engines::usage_log::recent_sessions(&sources, since);
        let readable = |engine: &str| {
            registry
                .get(engine)
                .is_some_and(|e| !e.usage_log_globs.is_empty())
        };
        crate::services::usage::shares(&accounts, &readable, &tokens, &panes)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))
}

/// Saves the share card (a PNG the page drew) where the user picks.
/// Returns the path, or `None` when they cancel.
#[tauri::command]
pub async fn usage_share_save(
    app: AppHandle,
    png_base64: String,
) -> Result<Option<String>, IpcError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_base64.as_bytes())
        .map_err(|_| IpcError::new(ErrorCode::Invalid, "That image couldn't be read."))?;
    if bytes.len() > 10 * 1024 * 1024 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(IpcError::new(ErrorCode::Invalid, "That isn't a PNG image."));
    }
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
                .set_file_name("vibezeo-usage.png")
                .add_filter("PNG image", &["png"])
                .save_file(move |picked| {
                    let _ = tx.send(picked.and_then(|p| p.into_path().ok()));
                });
            rx.await.unwrap_or(None)
        }
    };
    let Some(path) = path else { return Ok(None) };
    tandem_core::atomic::atomic_write(&path, &bytes, None)
        .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
