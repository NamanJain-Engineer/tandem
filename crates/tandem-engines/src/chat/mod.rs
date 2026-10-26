//! Chat adapters (F-TH-03..06, ARCHITECTURE §2.5, §6.8): the one interface
//! the threads service drives, whatever the engine. Adapters are the only
//! engine-specific code; they turn an engine's output into
//! [`ThreadEvent`](crate::events::ThreadEvent)s.

pub mod acp;
pub mod claude;
pub mod codex;
pub mod process;

use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tandem_core::access::{Access, ThreadMode};
use tandem_core::allow_rules::ApprovalSubject;
use tandem_core::approvals::{ApprovalDecision, ApprovalKind};
use tandem_core::ids::{ApprovalId, ThreadId, TurnId};
use tokio::sync::mpsc;
use ts_rs::TS;

use crate::events::ThreadEvent;

/// Runs a session's engine somewhere else: turns the spec the adapter
/// built into the one that starts it there (an SSH host's `tandem-host
/// run`, F-RM-01, D-403). The adapter's stdio handling stays as it is.
#[derive(Clone)]
pub struct Relocate(
    pub std::sync::Arc<dyn Fn(process::ProcessSpec) -> process::ProcessSpec + Send + Sync>,
);

impl std::fmt::Debug for Relocate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Relocate")
    }
}

impl Relocate {
    pub fn apply(r: Option<&Relocate>, spec: process::ProcessSpec) -> process::ProcessSpec {
        match r {
            Some(r) => (r.0)(spec),
            None => spec,
        }
    }
}

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
    /// The gateway and this session's lease, for engines that take MCP
    /// servers as flags (Codex).
    pub mcp: Option<McpEndpoint>,
    /// The engine version detection found, for the adapter's version gate.
    pub engine_version: Option<String>,
    /// Where the engine runs when it isn't this machine.
    pub relocate: Option<Relocate>,
    /// A teammate chat's briefing (`$DATA/briefings/<thread>.md`, F-AG-06):
    /// Claude reads it with `--append-system-prompt-file`, Codex gets it as
    /// developer instructions and ACP agents as the start of the first
    /// prompt. Never argv.
    pub briefing: Option<PathBuf>,
    /// Local stdio MCP servers besides the gateway: the tasks server
    /// (F-TK-02), one connection per session.
    pub stdio_servers: Vec<StdioServer>,
    /// A session with every tool off, no MCP server, briefing or resume:
    /// Lead answering a plan run's question (F-TK-08, D-2838). An adapter
    /// that can't hold its engine to that refuses to start
    /// ([`ChatAdapter::answers_without_tools`]).
    pub tools_off: bool,
}

/// A stdio MCP server an engine starts itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StdioServer {
    /// The name the engine knows it by (`tandem_tasks`).
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    /// Env vars the engine must pass through to the server (Codex starts
    /// servers with a minimal env).
    pub env_vars: Vec<String>,
}

impl ChatStart {
    /// The briefing's text, if the session has one that can be read.
    pub fn briefing_text(&self) -> Option<String> {
        self.briefing
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .filter(|t| !t.trim().is_empty())
    }
}

/// The gateway URL and a session's lease token.
#[derive(Clone, PartialEq, Eq)]
pub struct McpEndpoint {
    pub url: String,
    pub token: String,
}

impl std::fmt::Debug for McpEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpEndpoint")
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .finish()
    }
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
    /// The engine asks in-band whether a tool may run (Codex, ACP): the
    /// threads service shows the card and answers with
    /// [`ChatSession::resolve_approval`] under this `id`.
    Approval {
        turn_id: TurnId,
        id: ApprovalId,
        kind: ApprovalKind,
        summary: String,
        detail: String,
        /// What an "Allow for this thread" rule matches on.
        subject: ApprovalSubject,
    },
    /// The models the engine offers (ACP agents report theirs), for the
    /// picker. `withdrawn` is the thread's model when it's no longer among
    /// them: the thread falls back to Automatic.
    Models {
        available: Vec<ModelOffer>,
        withdrawn: Option<String>,
    },
    /// The engine process ended. `stderr_tail` is for the error card only
    /// and is never logged.
    Exited {
        code: Option<i32>,
        stderr_tail: String,
    },
}

/// A model an engine offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOffer {
    pub id: String,
    pub label: String,
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
    /// The engine needs the user to sign in (shown with Sign in).
    #[error("{0}")]
    Auth(String),
}

#[async_trait]
pub trait ChatAdapter: Send + Sync {
    fn engine_id(&self) -> &str;
    /// Whether a [`ChatStart::tools_off`] session can hold this engine, at
    /// `version` (`--version`'s), to no tool at all (F-TK-08): only Claude
    /// Code from [`claude::TOOLS_OFF_MIN`]. Codex's read-only sandbox still
    /// runs commands that read the disk, and ACP agents bring their own
    /// tools.
    fn answers_without_tools(&self, _version: Option<&str>) -> bool {
        false
    }
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
