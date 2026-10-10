//! Tasks (ARCHITECTURE §3.2 "Tasks"; F-TK-01, F-TK-03). The UI writes as
//! the human actor `me`; agents write through the MCP server.

use serde::{Deserialize, Serialize};
use tandem_core::{ErrorCode, IpcError};
use tandem_tasks::{
    Comment, HistoryEntry, Issue, IssueFilter, IssueSummary, Meta, Page, SaveIssue, TaskError,
    TaskStore, Writer,
};
use tauri::State;
use ts_rs::TS;

use crate::state::AppState;

/// An issue with its comments and history (UI-81).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskDetail {
    pub issue: Issue,
    pub comments: Vec<Comment>,
    pub history: Vec<HistoryEntry>,
}

pub fn task_err(e: TaskError) -> IpcError {
    let code = match &e {
        TaskError::RevisionConflict { .. } => ErrorCode::RevisionConflict,
        TaskError::NotFound(_) => ErrorCode::NotFound,
        TaskError::Invalid(_) => ErrorCode::Invalid,
        TaskError::ConnectionRevoked => ErrorCode::PermissionDenied,
        TaskError::Unavailable(_) => ErrorCode::Unavailable,
        TaskError::Io(_) | TaskError::Corrupt(_) => ErrorCode::Io,
    };
    IpcError::new(code, e.to_string())
}

fn tracker(state: &AppState) -> Result<TaskStore, IpcError> {
    state.tasks.tracker.clone().ok_or_else(|| {
        IpcError::new(
            ErrorCode::Unavailable,
            format!("Couldn't read tasks from {}.", state.data.tasks().display()),
        )
    })
}

/// Runs blocking tracker IO off the async runtime.
async fn blocking<T: Send + 'static>(
    state: &AppState,
    f: impl FnOnce(TaskStore) -> Result<T, TaskError> + Send + 'static,
) -> Result<T, IpcError> {
    let t = tracker(state)?;
    tokio::task::spawn_blocking(move || f(t))
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
        .map_err(task_err)
}

#[tauri::command]
pub async fn tasks_meta(state: State<'_, AppState>) -> Result<Meta, IpcError> {
    // Teammates are the actors issues can be assigned to.
    let teammates: Vec<(String, String)> = state
        .teammates
        .teammates()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|t| (t.id.as_str().to_string(), t.name))
        .collect();
    blocking(&state, move |t| {
        t.sync_teammate_actors(&teammates)?;
        t.meta()
    })
    .await
}

#[tauri::command]
pub async fn tasks_list(
    state: State<'_, AppState>,
    filter: IssueFilter,
    cursor: Option<String>,
) -> Result<Page<IssueSummary>, IpcError> {
    blocking(&state, move |t| t.list(&filter, cursor.as_deref(), None)).await
}

#[tauri::command]
pub async fn task_get(state: State<'_, AppState>, id: String) -> Result<TaskDetail, IpcError> {
    blocking(&state, move |t| {
        Ok(TaskDetail {
            issue: t.get(&id)?,
            comments: t.comments(&id)?,
            history: t.history(&id)?,
        })
    })
    .await
}

#[tauri::command]
pub async fn task_save(state: State<'_, AppState>, input: SaveIssue) -> Result<Issue, IpcError> {
    blocking(&state, move |t| t.save_issue(input, &Writer::human())).await
}

#[tauri::command]
pub async fn task_comment(
    state: State<'_, AppState>,
    id: String,
    body: String,
    parent_id: Option<String>,
    idempotency_key: Option<String>,
) -> Result<Comment, IpcError> {
    blocking(&state, move |t| {
        t.add_comment(
            &id,
            &body,
            parent_id.as_deref(),
            idempotency_key.as_deref(),
            &Writer::human(),
        )
    })
    .await
}

#[tauri::command]
pub async fn task_archive(
    state: State<'_, AppState>,
    id: String,
    expected_revision: u64,
) -> Result<Issue, IpcError> {
    blocking(&state, move |t| {
        t.set_archived(&id, true, Some(expected_revision), &Writer::human())
    })
    .await
}

#[tauri::command]
pub async fn task_restore(
    state: State<'_, AppState>,
    id: String,
    expected_revision: u64,
) -> Result<Issue, IpcError> {
    blocking(&state, move |t| {
        t.set_archived(&id, false, Some(expected_revision), &Writer::human())
    })
    .await
}

#[tauri::command]
pub async fn task_connections_list(
    state: State<'_, AppState>,
) -> Result<Vec<tandem_tasks::Connection>, IpcError> {
    blocking(&state, |t| t.connections()).await
}

/// Revokes an agent session's task access (F-TK-02 AC2).
#[tauri::command]
pub async fn task_connection_revoke(
    state: State<'_, AppState>,
    connection_id: String,
) -> Result<(), IpcError> {
    blocking(&state, move |t| t.revoke_connection(&connection_id)).await
}
