//! The app side of a routine run (F-RT-02): teammates, engines, sign-in,
//! the plan, the run's chat and notifications. Built from the services
//! themselves, so the GUI and `--run-routines` share it.

use std::sync::Arc;

use async_trait::async_trait;
use tandem_core::access::ThreadMode;
use tandem_core::entitlement::Limits;
use tandem_core::ids::{RoutineId, TeammateId, ThreadId};
use tandem_core::source::SourceRef;
use tandem_core::{ErrorCode, IpcError};
use tandem_store::repo::notifications::NotificationKind;
use tandem_store::repo::routines::{self as repo, Routine};
use tandem_store::repo::teammates::Teammate;
use tandem_store::repo::threads::{ChatExtra, NewThread, ThreadKind, ThreadState};
use tandem_store::Store;

use crate::services::routines::RoutineHost;

/// How a routine's run is named to the gateway (F-PL-08):
/// `plugins::parse_origin` reads it back for Held actions.
fn routine_origin(id: &RoutineId) -> String {
    format!("routine:{}", id.as_str())
}

pub struct AppRoutineHost {
    pub store: Store,
    pub teammates: Arc<crate::services::teammates::TeammatesService>,
    pub terminals: Arc<crate::services::terminal::TerminalService>,
    pub entitlements: Arc<crate::services::entitlement::EntitlementService>,
    pub threads: Arc<crate::services::threads::ThreadService>,
    pub notifications: Arc<crate::services::notifications::NotificationService>,
    /// Files what the run holds for review under the routine (F-PL-08).
    pub gateway: Arc<crate::services::gateway::GatewayService>,
}

#[async_trait]
impl RoutineHost for AppRoutineHost {
    async fn teammate(&self, id: &TeammateId) -> Option<Teammate> {
        self.teammates.get(id).await.ok()
    }

    fn chat_engine_ready(&self, engine_id: &str) -> bool {
        let engines = &self.terminals.engines;
        engines
            .get(engine_id)
            .is_some_and(|s| crate::services::threads::has_chat_adapter(&s))
            && engines.installed(engine_id).is_some()
    }

    fn limits(&self) -> Limits {
        self.entitlements.limits()
    }

    async fn start_chat(
        &self,
        teammate: &Teammate,
        routine: &Routine,
    ) -> Result<ThreadId, IpcError> {
        let cwd = crate::services::teammates::TeammatesService::place_for(teammate, None)?;
        std::fs::create_dir_all(&cwd).map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        let chat = self
            .threads
            .create_chat(
                NewThread {
                    id: ThreadId::new(),
                    kind: ThreadKind::TeammateChat,
                    workspace_id: None,
                    cwd,
                    engine_id: teammate.engine_id.clone(),
                    account_id: teammate.preferred_account_id.clone(),
                    model: teammate.model.clone(),
                    effort: teammate.effort.clone(),
                    // Full only when this routine was given it explicitly;
                    // the session's access is resolved again at launch
                    // (`Origin::Routine`, rule 4).
                    access: routine.access,
                    mode: ThreadMode::Act,
                    attended: false,
                },
                ChatExtra {
                    teammate_id: Some(teammate.id.clone()),
                    ..Default::default()
                },
            )
            .await?;
        // Named after the routine, for good: auto-title leaves it alone.
        repo::set_chat_routine(&self.store, &chat.id, &routine.id, &routine.name)
            .await
            .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        self.teammates.publish().await;
        // Before the first message starts the session: plugin writes it
        // holds say which routine they came from (F-PL-08).
        self.gateway
            .set_unattended_origin(&chat.id, &routine_origin(&routine.id));
        self.threads
            .send(&chat.id, routine.instruction.clone(), Vec::new(), false)
            .await?;
        Ok(chat.id)
    }

    async fn chat_state(&self, chat: &ThreadId) -> Option<ThreadState> {
        tandem_store::repo::threads::get(&self.store, chat)
            .await
            .ok()
            .flatten()
            .map(|t| t.state)
    }

    async fn notify(&self, kind: NotificationKind, title: String, body: String, source: SourceRef) {
        self.notifications
            .raise_routine(kind, title, body, source)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::plugins::parse_origin;
    use tandem_store::repo::held_drafts::OriginKind;

    #[test]
    fn f_pl_08_a_routine_run_files_held_drafts_under_its_routine() {
        let id = RoutineId::new();
        assert_eq!(
            parse_origin(&routine_origin(&id)),
            Some((OriginKind::Routine, id.as_str().to_string(), None))
        );
    }
}
