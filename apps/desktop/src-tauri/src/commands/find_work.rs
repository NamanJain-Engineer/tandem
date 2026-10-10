//! Find work by meaning (ARCHITECTURE §3 "Find work", F-CN-05).

use async_trait::async_trait;
use tauri::{AppHandle, Manager, State};

use tandem_core::status::SessionKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::panes::{self, PaneRecord};
use tandem_store::repo::threads::{self, ThreadKind, ThreadRecord};

use crate::services::find_work::{self, FindHost, FindWorkResult, MAX_QUERY_CHARS, OUTPUT_LINES};
use crate::state::AppState;

/// The stored events read back for a thread's last message.
const MESSAGES_READ: u32 = 40;

/// The running app as a search's host.
struct AppHost {
    app: AppHandle,
}

impl AppHost {
    fn state(&self) -> State<'_, AppState> {
        self.app.state::<AppState>()
    }
}

#[async_trait]
impl FindHost for AppHost {
    async fn panes(&self) -> Vec<PaneRecord> {
        match panes::list_all_open(&self.state().store).await {
            Ok(rows) => rows,
            Err(err) => {
                tracing::warn!(error = %err, "find work: panes unavailable");
                Vec::new()
            }
        }
    }

    fn output(&self, pane: &str) -> String {
        self.state()
            .terminals
            .pty
            .plain_tail(pane, OUTPUT_LINES as u16 * 2)
            .unwrap_or_default()
    }

    fn activity(&self, pane: &str) -> Option<String> {
        let record = self
            .state()
            .terminals
            .status
            .store
            .get(&SessionKey::pane(pane))?;
        record.preview.or(record.tool)
    }

    fn last_output(&self, pane: &str) -> Option<String> {
        self.state()
            .terminals
            .pty
            .last_output(pane)
            .map(|(text, _exit)| text)
    }

    fn live_title(&self, pane: &str) -> Option<String> {
        self.state().terminals.pty.state(pane).and_then(|s| s.title)
    }

    fn engine_label(&self, engine: &str) -> Option<String> {
        self.state()
            .terminals
            .engines
            .get(engine)
            .map(|e| e.label.clone())
    }

    async fn threads(&self) -> Vec<ThreadRecord> {
        match threads::list(&self.state().store, ThreadKind::Thread, false).await {
            Ok(rows) => rows,
            Err(err) => {
                tracing::warn!(error = %err, "find work: threads unavailable");
                Vec::new()
            }
        }
    }

    async fn last_message(&self, thread: &ThreadRecord) -> Option<String> {
        let rows = threads::page(&self.state().store, &thread.id, None, MESSAGES_READ)
            .await
            .ok()?;
        rows.iter()
            .rev()
            .find_map(|row| find_work::message_text(&row.kind, &row.payload_json))
    }
}

/// Ranks the open panes and threads for `query`, on this machine.
#[tauri::command]
pub async fn find_work(app: AppHandle, query: String) -> Result<FindWorkResult, IpcError> {
    if query.chars().count() > MAX_QUERY_CHARS {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Describe the work in at most 500 characters.",
        ));
    }
    let host = AppHost { app };
    Ok(find_work::find(&host, &query).await)
}
