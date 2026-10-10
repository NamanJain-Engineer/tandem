//! The Files tab (ARCHITECTURE §3.2 "Files, editor, browser, scratchpad";
//! F-CD-19, F-CD-20).

use std::sync::Arc;

use tandem_core::files::watch::FsChange;
use tandem_core::files::FsKind;
use tandem_core::{ErrorCode, IpcError};
use tandem_git::FileDiff;
use tandem_store::repo::editor_tabs::EditorTabs;
use tauri::ipc::Channel;
use tauri::State;

use crate::services::files::{FileContent, FileContext, FsEntry, Saved};
use crate::state::AppState;

#[tauri::command]
pub async fn fs_list(
    app: State<'_, AppState>,
    context: FileContext,
    dir: String,
    show_ignored: bool,
) -> Result<Vec<FsEntry>, IpcError> {
    app.files.list(&context, dir, show_ignored).await
}

#[tauri::command]
pub async fn fs_read(
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
) -> Result<FileContent, IpcError> {
    app.files.read(&context, path).await
}

/// Saves only while the file is what the editor opened (`expectedHash`);
/// without a hash it creates a new file.
#[tauri::command]
pub async fn fs_write(
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
    text: String,
    expected_hash: Option<String>,
) -> Result<Saved, IpcError> {
    app.files.write(&context, path, text, expected_hash).await
}

#[tauri::command]
pub async fn fs_compare(
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
    text: String,
) -> Result<FileDiff, IpcError> {
    app.files.compare(&context, path, text).await
}

#[tauri::command]
pub async fn fs_create(
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
    kind: FsKind,
) -> Result<(), IpcError> {
    app.files.create(&context, path, kind).await
}

#[tauri::command]
pub async fn fs_rename(
    app: State<'_, AppState>,
    context: FileContext,
    from: String,
    to: String,
) -> Result<(), IpcError> {
    app.files.rename(&context, from, to).await
}

#[tauri::command]
pub async fn fs_delete(
    app: State<'_, AppState>,
    context: FileContext,
    path: String,
) -> Result<(), IpcError> {
    app.files.delete(&context, path).await
}

/// Streams batched changes for the context's folder until `fs_unwatch`.
#[tauri::command]
pub async fn fs_watch(
    app: State<'_, AppState>,
    context: FileContext,
    channel: Channel<FsChange>,
) -> Result<String, IpcError> {
    let sink = Arc::new(move |change: &FsChange| channel.send(change.clone()).is_ok());
    app.files.watch(&context, sink).await
}

#[tauri::command]
pub fn fs_unwatch(app: State<'_, AppState>, sub_id: String) {
    app.files.unwatch(&sub_id);
}

#[tauri::command]
pub async fn editor_tabs_get(
    app: State<'_, AppState>,
    context_key: String,
) -> Result<EditorTabs, IpcError> {
    app.files.tabs_get(&context_key).await
}

#[tauri::command]
pub async fn editor_tabs_save(
    app: State<'_, AppState>,
    context_key: String,
    state: EditorTabs,
) -> Result<(), IpcError> {
    app.files.tabs_save(&context_key, state).await
}

/// Shows a file or folder in the system file manager (F-SH-03, the tree's
/// "Reveal in file manager").
#[tauri::command]
pub async fn app_reveal_path(path: String) -> Result<(), IpcError> {
    if !std::path::Path::new(&path).exists() {
        return Err(IpcError::new(
            ErrorCode::NotFound,
            "That file or folder doesn't exist",
        ));
    }
    tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|e| {
        tracing::warn!(code = "files.reveal_failed", error = %e, "reveal failed");
        IpcError::new(ErrorCode::NotFound, "This file can't be shown.")
    })
}
