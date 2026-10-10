//! Find work by meaning (ARCHITECTURE §3 "Find work", F-CN-05).

use async_trait::async_trait;
use tauri::{AppHandle, Manager, State};

use tandem_core::status::SessionKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::panes::{self, PaneRecord};
use tandem_store::repo::threads::{self, ThreadKind, ThreadRecord};

use crate::services::api::ResolveCandidate;
use crate::services::find_work::{
    self, FindHost, FindWorkResult, HostedAnswer, TeammateFacts, MAX_QUERY_CHARS, OUTPUT_LINES,
};
use crate::state::AppState;

/// The stored events read back for a thread's last message.
const MESSAGES_READ: u32 = 40;
/// How long the hosted ranking may take before the local one answers.
const HOSTED_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

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

    async fn teammates(&self) -> Vec<TeammateFacts> {
        let store = &self.state().store;
        let teammates = tandem_store::repo::teammates::list(store)
            .await
            .unwrap_or_default();
        let chats = threads::list(store, ThreadKind::TeammateChat, false)
            .await
            .unwrap_or_default();
        let mut out = Vec::new();
        for t in teammates {
            let latest = chats
                .iter()
                .filter(|c| c.teammate_id.as_ref() == Some(&t.id))
                .max_by_key(|c| c.updated_at);
            let last_message = match latest {
                Some(chat) => self.last_message(chat).await,
                None => None,
            };
            out.push(TeammateFacts {
                id: t.id.as_str().to_string(),
                name: t.name,
                brief: t.brief,
                last_message,
            });
        }
        out
    }

    /// Only with the Privacy opt-in on (D-1302); any failure, the local
    /// ranking answers.
    async fn hosted_rank(
        &self,
        query: &str,
        candidates: &[ResolveCandidate],
    ) -> Option<HostedAnswer> {
        let state = self.state();
        let on = tandem_store::repo::settings::get(
            &state.store,
            tandem_core::settings::SettingKey::FindWorkHosted,
        )
        .await
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
        if !on {
            return None;
        }
        if !state.endpoints.hosted {
            return Some(Err("this build has no hosted service".into()));
        }
        if !state.auth.is_signed_in() {
            return Some(Err("signed out".into()));
        }
        let api = state.api.clone();
        let asked = state.auth.with_token(|token| {
            let (api, query, candidates) = (api.clone(), query.to_string(), candidates.to_vec());
            async move { api.voice_resolve(&token, &query, &candidates).await }
        });
        Some(match tokio::time::timeout(HOSTED_WAIT, asked).await {
            Err(_) => Err("timed out".into()),
            Ok(Err(e)) => Err(e.to_string()),
            Ok(Ok(matches)) => Ok(matches.into_iter().map(|m| (m.id, m.score)).collect()),
        })
    }
}

/// Ranks the open panes, threads and teammates for `query`: on this
/// machine, or with the hosted ranking when the user turned it on.
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
