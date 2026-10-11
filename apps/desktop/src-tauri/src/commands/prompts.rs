//! The prompt library (ARCHITECTURE §3.2 "Extras"; F-EX-05).

use std::collections::BTreeMap;
use std::path::PathBuf;

use tandem_core::IpcError;
use tauri::State;

use crate::services::files::FileContext;
use crate::services::prompts::{self, PromptList, PromptScope, Rendered};
use crate::state::AppState;

/// The context's folder; a thread without one (or no context) has none.
async fn folder(app: &AppState, context: Option<FileContext>) -> Option<PathBuf> {
    match context {
        Some(c) => app.files.root(&c).await.ok(),
        None => None,
    }
}

#[tauri::command]
pub async fn prompts_list(
    app: State<'_, AppState>,
    context: Option<FileContext>,
) -> Result<PromptList, IpcError> {
    let folder = folder(&app, context).await;
    let data = app.data.clone();
    tokio::task::spawn_blocking(move || prompts::list(&data, folder.as_deref()))
        .await
        .map_err(|e| IpcError::new(tandem_core::ErrorCode::Internal, e.to_string()))
}

#[tauri::command]
pub async fn prompts_render(
    app: State<'_, AppState>,
    path: String,
    context: Option<FileContext>,
    values: BTreeMap<String, String>,
) -> Result<Rendered, IpcError> {
    let folder = folder(&app, context).await;
    prompts::render(&app.data, folder.as_deref(), &path, values).await
}

#[tauri::command]
pub async fn prompts_create(
    app: State<'_, AppState>,
    context: Option<FileContext>,
    scope: PromptScope,
    name: String,
) -> Result<serde_json::Value, IpcError> {
    let folder = folder(&app, context).await;
    let path = prompts::create(&app.data, folder.as_deref(), scope, &name)?;
    Ok(serde_json::json!({ "path": path.to_string_lossy() }))
}

#[tauri::command]
pub async fn prompts_share_repo(
    app: State<'_, AppState>,
    context: FileContext,
) -> Result<(), IpcError> {
    let root = app.files.root(&context).await?;
    prompts::share(&root)
}
