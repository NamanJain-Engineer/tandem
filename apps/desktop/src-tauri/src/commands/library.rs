//! The session library (ARCHITECTURE §3.2 "Session library", F-TH-17).

use tauri::{State, WebviewWindow};

use tandem_core::ids::AccountId;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::library::TranscriptItem;

use crate::services::handoff::HandoffBrief;
use crate::services::library::{self, LibraryListing, LibraryOpened};
use crate::state::AppState;

/// Every Claude Code and Codex session on this computer, newest first.
#[tauri::command]
pub async fn library_list(state: State<'_, AppState>) -> Result<LibraryListing, IpcError> {
    library::list(&state).await
}

/// A listed session's prompts and replies, tools by name.
#[tauri::command]
pub async fn library_transcript(
    state: State<'_, AppState>,
    key: String,
) -> Result<Vec<TranscriptItem>, IpcError> {
    library::transcript(&state, &key).await
}

/// Resume: an agent pane in the session's folder and home.
#[tauri::command]
pub async fn library_resume(
    state: State<'_, AppState>,
    key: String,
) -> Result<LibraryOpened, IpcError> {
    library::resume(&state, &key).await
}

/// The brief for continuing a session with another engine.
#[tauri::command]
pub async fn library_brief(
    state: State<'_, AppState>,
    key: String,
) -> Result<HandoffBrief, IpcError> {
    library::brief(&state, &key).await
}

/// Continue with another engine: the target in the session's folder.
#[tauri::command]
pub async fn library_continue(
    state: State<'_, AppState>,
    key: String,
    engine_id: String,
    account_id: Option<AccountId>,
    brief: String,
) -> Result<LibraryOpened, IpcError> {
    library::continue_with(&state, &key, &engine_id, account_id, &brief).await
}

/// Full-text search (Pro, opt-in).
#[tauri::command]
pub async fn library_search(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<String>, IpcError> {
    library::search(&state, &query).await
}

/// Turns search on (Pro) or off, which deletes the index.
#[tauri::command]
pub async fn library_search_set(
    window: WebviewWindow,
    state: State<'_, AppState>,
    on: bool,
) -> Result<(), IpcError> {
    if window.label() != "main" {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "Changing search needs the main window",
        ));
    }
    library::set_search(&state, on).await
}
