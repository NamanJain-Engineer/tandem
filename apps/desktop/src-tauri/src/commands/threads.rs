//! Threads (ARCHITECTURE §3.2 "Threads and teammate chats", F-TH-01,
//! F-TH-02, F-TH-03).

use serde::Deserialize;
use tandem_core::access::{Access, ThreadMode};
use tandem_core::ids::{AccountId, AttachmentId, ThreadId, WorkspaceId};
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::chat::InterruptOutcome;
use tandem_engines::events::ThreadEventEnvelope;
use tandem_store::repo::threads::{NewThread, ThreadKind, ThreadRecord};
use tandem_store::repo::workspaces;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::broadcast::error::RecvError;

use crate::services::threads::{SendResult, ThreadDetail, ThreadEventPage};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCreate {
    pub kind: ThreadKind,
    pub workspace_id: Option<WorkspaceId>,
    pub engine_id: Option<String>,
    pub account_id: Option<AccountId>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub access: Option<Access>,
    pub mode: Option<ThreadMode>,
}

/// The engine a thread starts with: Claude Code when it's installed, else
/// the first installed engine that has a chat adapter.
fn default_engine(state: &AppState) -> Option<String> {
    let engines = &state.terminals.engines;
    let registry = engines.registry();
    let chat: Vec<&str> = registry
        .all()
        .iter()
        .filter(|e| e.chat_adapter.is_some() && engines.installed(&e.id).is_some())
        .map(|e| e.id.as_str())
        .collect();
    chat.iter()
        .find(|id| **id == "claude")
        .or_else(|| chat.first())
        .map(|id| id.to_string())
}

/// `thread_create`. A thread without a folder works in its own scratch
/// folder with Read access (F-TH-02); omitted settings take the defaults.
#[tauri::command]
pub async fn thread_create(
    state: State<'_, AppState>,
    args: ThreadCreate,
) -> Result<ThreadRecord, IpcError> {
    let engine_id = match args.engine_id {
        Some(id) => id,
        None => default_engine(&state).ok_or_else(|| {
            IpcError::new(
                ErrorCode::EngineMissing,
                "No coding agent that works in Thread mode is installed",
            )
        })?,
    };
    if state.terminals.engines.get(&engine_id).is_none() {
        return Err(IpcError::new(
            ErrorCode::EngineMissing,
            "This engine isn't known to Vibezeo",
        ));
    }
    let id = ThreadId::new();
    let (cwd, default_access) = match &args.workspace_id {
        Some(ws) => {
            let ws = workspaces::get(&state.store, ws)
                .await
                .map_err(|e| IpcError::new(ErrorCode::NotFound, e.to_string()))?;
            (ws.root, Access::Edits)
        }
        None => {
            let dir = state.data.scratch(id.as_str());
            std::fs::create_dir_all(&dir).map_err(|e| {
                IpcError::new(
                    ErrorCode::Io,
                    format!("Couldn't create the thread's folder: {e}"),
                )
            })?;
            (dir.to_string_lossy().into_owned(), Access::Read)
        }
    };
    state
        .threads
        .create(NewThread {
            id,
            kind: args.kind,
            workspace_id: args.workspace_id,
            cwd,
            engine_id,
            account_id: args.account_id,
            model: args.model,
            effort: args.effort,
            access: args.access.unwrap_or(default_access),
            mode: args.mode.unwrap_or(ThreadMode::Act),
            attended: true,
        })
        .await
}

#[tauri::command]
pub async fn thread_list(
    state: State<'_, AppState>,
    kind: ThreadKind,
    include_archived: Option<bool>,
) -> Result<Vec<ThreadRecord>, IpcError> {
    state
        .threads
        .list(kind, include_archived.unwrap_or(false))
        .await
}

#[tauri::command]
pub async fn thread_get(
    state: State<'_, AppState>,
    thread_id: ThreadId,
) -> Result<ThreadDetail, IpcError> {
    state.threads.get(&thread_id).await
}

#[tauri::command]
pub async fn thread_events(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    before_id: Option<i64>,
    limit: Option<u32>,
) -> Result<ThreadEventPage, IpcError> {
    state
        .threads
        .events(&thread_id, before_id, limit.unwrap_or(200))
        .await
}

/// Streams a thread's events to `channel`: the running turn so far, then
/// live events, until the webview drops the channel.
#[tauri::command]
pub async fn thread_subscribe(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    channel: Channel<ThreadEventEnvelope>,
) -> Result<(), IpcError> {
    let threads = state.threads.clone();
    let (snapshot, mut rx) = threads.subscribe(&thread_id).await?;
    tauri::async_runtime::spawn(async move {
        for env in snapshot {
            if channel.send(env).is_err() {
                return;
            }
        }
        loop {
            match rx.recv().await {
                Ok(env) if env.thread_id == thread_id => {
                    if channel.send(env).is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                // Fell behind: send the running turn again (the view
                // replaces events by seq).
                Err(RecvError::Lagged(_)) => {
                    let Ok((again, fresh)) = threads.subscribe(&thread_id).await else {
                        return;
                    };
                    rx = fresh;
                    for env in again {
                        if channel.send(env).is_err() {
                            return;
                        }
                    }
                }
                Err(RecvError::Closed) => return,
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn thread_send(
    state: State<'_, AppState>,
    thread_id: ThreadId,
    text: String,
    attachment_ids: Option<Vec<AttachmentId>>,
    queue_if_busy: Option<bool>,
) -> Result<SendResult, IpcError> {
    if text.trim().is_empty() {
        return Err(IpcError::new(ErrorCode::Invalid, "Type a message first"));
    }
    state
        .threads
        .send(
            &thread_id,
            text,
            attachment_ids.unwrap_or_default(),
            queue_if_busy.unwrap_or(false),
        )
        .await
}

#[tauri::command]
pub async fn thread_stop(
    state: State<'_, AppState>,
    thread_id: ThreadId,
) -> Result<InterruptOutcome, IpcError> {
    state.threads.stop(&thread_id).await
}

#[tauri::command]
pub async fn thread_retry(
    state: State<'_, AppState>,
    thread_id: ThreadId,
) -> Result<SendResult, IpcError> {
    state.threads.retry(&thread_id).await
}
