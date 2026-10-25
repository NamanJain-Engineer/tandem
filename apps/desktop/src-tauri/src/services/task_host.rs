//! The app side of task execution (F-TK-04): lanes, sessions, the plan,
//! notifications.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tandem_core::access::ThreadMode;
use tandem_core::entitlement::Limits;
use tandem_core::ids::{AccountId, AttemptId, TeammateId, ThreadId, WorkspaceId};
use tandem_core::source::SourceRef;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::events::ThreadEvent;
use tandem_store::repo::notifications::NotificationKind;
use tandem_store::repo::threads::{ChatExtra, NewThread, ThreadKind, ThreadState};
use tandem_store::Store;

use crate::services::tasks_exec::{ExecHost, SessionStart};

/// How a task run is named to the gateway (F-PL-08):
/// `plugins::parse_origin` reads it back for Held actions.
fn task_origin(issue_id: &str, attempt: &AttemptId) -> String {
    format!("task:{issue_id}:{}", attempt.as_str())
}

pub struct AppExecHost {
    pub store: Store,
    pub data: tandem_core::DataDir,
    pub entitlements: Arc<crate::services::entitlement::EntitlementService>,
    pub terminals: Arc<crate::services::terminal::TerminalService>,
    pub teammates: Arc<crate::services::teammates::TeammatesService>,
    pub threads: Arc<crate::services::threads::ThreadService>,
    pub notifications: Arc<crate::services::notifications::NotificationService>,
    /// Files what a run holds for review under its task (F-PL-08).
    pub gateway: Arc<crate::services::gateway::GatewayService>,
}

impl AppExecHost {
    async fn git(&self) -> Result<tandem_git::Git, IpcError> {
        let login = self.terminals.login_env().await;
        crate::services::lanes::git_for(&self.data, &login)
    }
}

#[async_trait]
impl ExecHost for AppExecHost {
    fn limits(&self) -> Limits {
        self.entitlements.limits()
    }

    async fn workspace_root(&self, ws: &WorkspaceId) -> Result<PathBuf, IpcError> {
        let w = tandem_store::repo::workspaces::get(&self.store, ws).await?;
        Ok(PathBuf::from(w.root))
    }

    async fn workspace_kind(&self, ws: &WorkspaceId) -> Result<String, IpcError> {
        Ok(tandem_store::repo::workspaces::get(&self.store, ws)
            .await?
            .kind)
    }

    async fn create_lane(
        &self,
        ws: &WorkspaceId,
        slug: &str,
    ) -> Result<(WorkspaceId, PathBuf), IpcError> {
        let git = self.git().await?;
        let lane = crate::services::lanes::create(&self.store, &git, ws, slug).await?;
        Ok((lane.id, PathBuf::from(lane.root)))
    }

    async fn remove_lane(&self, lane: &WorkspaceId) -> Result<(), IpcError> {
        let git = self.git().await?;
        // Without force: a lane with anything in it stays (never deleted).
        crate::services::lanes::remove(&self.store, &self.terminals.pty, &git, lane, None).await
    }

    async fn lane_diff(&self, lane: &WorkspaceId) -> String {
        crate::services::lanes::diff(&self.store, lane)
            .await
            .map(|files| {
                files
                    .into_iter()
                    .map(|f| f.patch)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    }

    async fn teammate_engine(&self, teammate: &TeammateId) -> Option<(String, Option<AccountId>)> {
        let t = self.teammates.get(teammate).await.ok()?;
        Some((t.engine_id, t.preferred_account_id))
    }

    async fn start_session(&self, s: SessionStart) -> Result<ThreadId, IpcError> {
        let teammate = match &s.teammate_id {
            Some(id) if !s.review => Some(self.teammates.get(id).await?),
            _ => None,
        };
        let new = NewThread {
            id: ThreadId::new(),
            kind: if teammate.is_some() {
                ThreadKind::TeammateChat
            } else {
                ThreadKind::Thread
            },
            workspace_id: Some(s.workspace_id.clone()),
            cwd: s.cwd.to_string_lossy().into_owned(),
            engine_id: s.engine_id.clone(),
            account_id: s.account_id.clone().or_else(|| {
                teammate
                    .as_ref()
                    .and_then(|t| t.preferred_account_id.clone())
            }),
            model: teammate.as_ref().and_then(|t| t.model.clone()),
            effort: teammate.as_ref().and_then(|t| t.effort.clone()),
            access: s.access,
            mode: ThreadMode::Act,
            attended: false,
        };
        let thread = match &teammate {
            Some(t) => {
                self.threads
                    .create_chat(
                        new,
                        ChatExtra {
                            teammate_id: Some(t.id.clone()),
                            ..Default::default()
                        },
                    )
                    .await?
            }
            None => self.threads.create(new).await?,
        };
        // Before the first message starts the session: plugin writes it
        // holds say which task they came from (F-PL-08).
        self.gateway
            .set_unattended_origin(&thread.id, &task_origin(&s.issue_id, &s.attempt));
        if !s.review {
            tandem_store::repo::task_attempts::update(
                &self.store,
                &s.attempt,
                tandem_store::repo::task_attempts::AttemptPatch {
                    thread_id: Some(thread.id.clone()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        }
        let title = if s.review {
            format!("Review {}", s.issue_id)
        } else {
            s.issue_id.clone()
        };
        let _ = self
            .threads
            .update(
                &thread.id,
                tandem_store::repo::threads::ThreadPatch {
                    title: Some(title),
                    ..Default::default()
                },
            )
            .await;
        self.threads
            .send(&thread.id, s.text, Vec::new(), false)
            .await?;
        Ok(thread.id)
    }

    async fn send(&self, thread: &ThreadId, text: String) -> Result<(), IpcError> {
        self.threads
            .send(thread, text, Vec::new(), false)
            .await
            .map(|_| ())
    }

    async fn thread_state(&self, thread: &ThreadId) -> Option<ThreadState> {
        tandem_store::repo::threads::get(&self.store, thread)
            .await
            .ok()
            .flatten()
            .map(|t| t.state)
    }

    async fn last_reply(&self, thread: &ThreadId) -> Option<String> {
        let rows = tandem_store::repo::threads::all_messages(&self.store, thread)
            .await
            .ok()?;
        rows.iter().rev().find_map(|r| {
            match serde_json::from_str::<ThreadEvent>(&r.payload_json).ok()? {
                ThreadEvent::Assistant { markdown } => Some(markdown),
                _ => None,
            }
        })
    }

    async fn stop(&self, thread: &ThreadId) {
        let _ = self.threads.stop(thread).await;
    }

    fn other_chat_engine(&self, not: &str) -> Option<String> {
        let engines = &self.terminals.engines;
        engines
            .registry()
            .all()
            .iter()
            .filter(|e| e.id != not)
            .filter(|e| crate::services::threads::has_chat_adapter(e))
            .find(|e| engines.installed(&e.id).is_some())
            .map(|e| e.id.clone())
    }

    async fn notify(&self, title: String, body: String, issue_id: &str) {
        self.notifications
            .raise_routine(
                NotificationKind::TaskReview,
                title,
                body,
                SourceRef::Task {
                    issue_id: issue_id.to_string(),
                },
            )
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::plugins::parse_origin;
    use tandem_store::repo::held_drafts::OriginKind;

    #[test]
    fn f_pl_08_a_task_run_files_held_drafts_under_its_task_and_attempt() {
        let attempt = AttemptId::new();
        assert_eq!(
            parse_origin(&task_origin("ISS-4", &attempt)),
            Some((
                OriginKind::Task,
                "ISS-4".to_string(),
                Some(attempt.as_str().to_string())
            ))
        );
    }
}
