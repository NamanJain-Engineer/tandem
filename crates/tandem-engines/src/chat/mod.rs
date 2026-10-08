//! Chat adapters (F-TH-03..06, ARCHITECTURE §2.5, §6.8): the one interface
//! the threads service drives, whatever the engine. Adapters are the only
//! engine-specific code; they turn an engine's output into
//! [`ThreadEvent`](crate::events::ThreadEvent)s.

pub mod claude;
pub mod process;

use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tandem_core::access::{Access, ThreadMode};
use tandem_core::approvals::ApprovalDecision;
use tandem_core::ids::{ApprovalId, ThreadId, TurnId};
use tokio::sync::mpsc;
use ts_rs::TS;

use crate::events::ThreadEvent;

/// Everything an adapter needs to start a session.
#[derive(Debug, Clone)]
pub struct ChatStart {
    pub thread_id: ThreadId,
    pub engine_id: String,
    /// The engine's resolved executable.
    pub program: PathBuf,
    pub cwd: PathBuf,
    /// The provider home and login env; nothing else of the app's.
    pub env: Vec<(String, String)>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub access: Access,
    pub mode: ThreadMode,
    /// `false` for routine, task, mail and Auto runs (F-TH-09 caps).
    pub attended: bool,
    /// The engine's own session, to resume after a restart or idle stop.
    pub resume_session: Option<String>,
    /// `$DATA/mcp/<session>.json` (F-PL-01), when the gateway is up.
    pub mcp_config: Option<PathBuf>,
}

/// One user turn.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnInput {
    pub turn_id: TurnId,
    pub text: String,
    /// Image attachments (sent inline).
    pub images: Vec<PathBuf>,
    /// File attachments, as paths relative to the cwd (`@path`).
    pub files: Vec<String>,
}

/// What a session reports to the threads service.
#[derive(Debug, Clone, PartialEq)]
pub enum AdapterEvent {
    /// An event of `turn_id`; the service drops it if that turn is over.
    Event { turn_id: TurnId, event: ThreadEvent },
    /// The engine's own session id (for `--resume` and friends).
    SessionId(String),
    /// The engine process ended. `stderr_tail` is for the error card only
    /// and is never logged.
    Exited {
        code: Option<i32>,
        stderr_tail: String,
    },
}

/// `thread_stop`'s answer: the engine acknowledged, or its process tree was
/// stopped (F-TH-03 AC2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InterruptOutcome {
    pub acked: bool,
    pub killed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("couldn't start the engine: {0}")]
    Spawn(std::io::Error),
    #[error("the engine isn't running")]
    Closed,
    #[error("the engine stopped reading its input")]
    StdinBlocked,
    #[error("the engine's output didn't make sense: {0}")]
    Protocol(String),
    #[error("{0}")]
    Unsupported(String),
}

#[async_trait]
pub trait ChatAdapter: Send + Sync {
    fn engine_id(&self) -> &str;
    async fn start(
        &self,
        req: ChatStart,
        events: mpsc::Sender<AdapterEvent>,
    ) -> Result<Box<dyn ChatSession>, AdapterError>;
}

#[async_trait]
pub trait ChatSession: Send + Sync {
    fn engine_session_id(&self) -> Option<String>;
    async fn send_turn(&self, turn: TurnInput) -> Result<TurnId, AdapterError>;
    /// The engine's interrupt; 3 s without an answer stops the process tree.
    async fn interrupt(&self, turn: &TurnId) -> Result<InterruptOutcome, AdapterError>;
    async fn resolve_approval(
        &self,
        id: &ApprovalId,
        decision: ApprovalDecision,
    ) -> Result<(), AdapterError>;
    async fn set_model(
        &self,
        model: Option<String>,
        effort: Option<String>,
    ) -> Result<(), AdapterError>;
    async fn shutdown(&self) -> Result<(), AdapterError>;
}
