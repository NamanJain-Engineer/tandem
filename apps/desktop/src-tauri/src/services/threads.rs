//! Threads (F-TH-01, F-TH-03, ARCHITECTURE §6.8–6.9): turns run through a
//! chat adapter; their events are numbered, stored and streamed.
//!
//! - One turn at a time per thread, plus one queued follow-up.
//! - Events carry their turn id: a finished turn's stragglers are dropped.
//! - Stop asks the adapter to interrupt (the engine's own interrupt, then the
//!   process tree by 3.5 s); a stopped turn ends `cancelled`, never failed.
//! - An engine that exits mid-turn ends it with an error offering Retry.
//! - Streaming text is shown live and kept as one row per block: a partial
//!   row every 2 s (a crash keeps it), then the final text in its place.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tandem_core::access::{Access, ThreadMode};
use tandem_core::allow_rules::{self, AllowRule, ApprovalSubject};
use tandem_core::approvals::{
    ApprovalBroker, ApprovalKind, ApprovalOutcome, ApprovalRequest, ApprovalScope,
};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::clock::Clock;
use tandem_core::ids::{AttachmentId, ThreadId, TurnId};
use tandem_core::source::SourceRef;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::chat::{
    AdapterEvent, ChatAdapter, ChatSession, ChatStart, InterruptOutcome, ModelOffer, TurnInput,
};
use tandem_engines::events::{
    ApprovalDecisionView, ErrorAction, ErrorStage, ThreadEvent, ThreadEventEnvelope, ToolStatus,
    TurnEndReason,
};
use tandem_gateway::{ApprovalAnswer, Caller, ToolApproval};
use tandem_store::repo::attachments::{self as attachments_repo, Attachment, AttachmentKind};
use tandem_store::repo::threads::ThreadPatch;
use tandem_store::repo::threads::{
    self as repo, MessageRow, NewThread, ThreadKind, ThreadRecord, ThreadState, TurnRecord,
    TurnState,
};
use tandem_store::Store;
use tokio::sync::{broadcast, mpsc, Mutex};

use crate::services::thread_changes;
use crate::services::thread_files::{self, PathEntry};
use crate::services::thread_summary;
use crate::services::thread_titles::{self, LocalOnly, Titler};
use ts_rs::TS;

/// How often an open text block is saved as a partial row.
const FLUSH_EVERY: Duration = Duration::from_secs(2);
/// After an acknowledged stop, how long the engine has to end the turn.
const ACKED_END: Duration = Duration::from_secs(3);
/// Live envelopes buffered per subscriber before it must resync.
const LIVE_BUFFER: usize = 4096;
/// A working turn with no output this long gets a note (F-TH-15).
const SILENT_AFTER_MS: i64 = 10 * 60 * 1000;
/// Deleted threads' recovery copies are kept this long (F-TH-01).
pub const TRASH_KEEP_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// `thread_send` / `thread_retry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SendResult {
    pub turn_id: Option<TurnId>,
    pub queued: bool,
}

/// `thread_get`. Plan revisions, attachments and pending approvals join with
/// their features (M7 tasks 6, 8 and 11).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ThreadDetail {
    pub thread: ThreadRecord,
    pub turns: Vec<TurnRecord>,
    /// The running turn, if any.
    pub running: Option<TurnId>,
    /// The follow-up waiting for the running turn.
    pub queued: Option<String>,
    /// The composer draft (`Draft` JSON).
    pub draft: String,
    /// The draft's attachments (not sent yet).
    pub attachments: Vec<Attachment>,
    /// The plans written in Plan mode, v1 first (F-TH-10).
    pub plan_revisions: Vec<repo::PlanRevision>,
}

/// `thread_events`: oldest first; `next_before_id` pages further back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ThreadEventPage {
    pub items: Vec<ThreadEventEnvelope>,
    pub next_before_id: Option<i64>,
}

/// `threads:changed`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ThreadsChanged {
    pub threads: Vec<ThreadRecord>,
}

/// Makes a session's start request: the engine's adapter, executable, cwd
/// and account env. The app's launcher resolves engines and accounts; tests
/// script one.
#[async_trait]
pub trait ChatLauncher: Send + Sync {
    async fn prepare(
        &self,
        thread: &ThreadRecord,
    ) -> Result<(Arc<dyn ChatAdapter>, ChatStart), IpcError>;

    /// The model to start with: the saved one while it's still offered, else
    /// the engine's default and the note saying so (F-TH-08 AC2).
    fn model_for(&self, thread: &ThreadRecord) -> (Option<String>, Option<String>) {
        (thread.model.clone(), None)
    }

    /// The models an engine reported (ACP agents), for the picker.
    fn models_reported(&self, _engine_id: &str, _models: &[ModelOffer]) {}

    /// A thread's engine session ended (its process exited).
    fn session_ended(&self, _thread_id: &ThreadId) {}
}

/// The key a thread's engine session has in per-session registries.
pub fn session_key(thread: &ThreadId) -> String {
    format!("th:{}", thread.as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Assistant,
    Reasoning,
}

/// Streaming text not yet final.
struct Block {
    kind: BlockKind,
    seq: u64,
    text: String,
    /// Its partial row, once flushed.
    row: Option<i64>,
    dirty: bool,
}

impl Block {
    fn event(&self) -> ThreadEvent {
        match self.kind {
            BlockKind::Assistant => ThreadEvent::Assistant {
                markdown: self.text.clone(),
            },
            BlockKind::Reasoning => ThreadEvent::Reasoning {
                text: self.text.clone(),
                duration_ms: None,
            },
        }
    }
}

struct Running {
    id: TurnId,
    next_seq: u64,
    open: Option<Block>,
    stopping: bool,
    blocked: u32,
    /// The first turn's message, which names the thread when it ends.
    title_from: Option<String>,
    /// When the engine last said anything (`clock.now_ms()`).
    last_output: i64,
    silence_noted: bool,
    /// Whether an error this turn reported was the engine's own failure
    /// (F-EN-08: only those count against its version).
    engine_fault: bool,
}

impl Running {
    fn take_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }
}

struct Queued {
    text: String,
    attachments: Vec<AttachmentId>,
}

#[derive(Default)]
struct Runtime {
    session: Option<Arc<dyn ChatSession>>,
    /// Bumped per session, so a dead session's last events are ignored.
    generation: u64,
    turn: Option<Running>,
    queued: Option<Queued>,
    /// The engine process went away on its own; the next session says
    /// "Reconnecting…" and resumes the engine's session (F-TH-05 AC2).
    lost: bool,
    /// Access, mode or model changed: the next turn starts a fresh session
    /// (resuming the engine's conversation) with the new settings.
    stale: bool,
    /// After an engine switch: the next turn opens with this divider and
    /// seeds the new engine with a summary (F-TH-16).
    pending_switch: Option<String>,
    /// The new engine hasn't been told the conversation so far yet: kept
    /// until a turn's input reaches it (F-TH-16).
    seed_pending: bool,
}

pub struct ThreadService {
    store: Store,
    bus: EventBus,
    clock: Arc<dyn Clock>,
    approvals: Arc<ApprovalBroker>,
    launcher: Arc<dyn ChatLauncher>,
    runtimes: StdMutex<HashMap<String, Arc<Mutex<Runtime>>>>,
    live: broadcast::Sender<ThreadEventEnvelope>,
    /// Names threads (F-TH-15); local titles only until the app sets one.
    titler: std::sync::OnceLock<Arc<dyn Titler>>,
    /// `$DATA`: attachments and deleted threads' recovery copies.
    data: std::sync::OnceLock<tandem_core::DataDir>,
    /// Tells the status bus each thread's state (Dashboard, D-6).
    status_sink: std::sync::OnceLock<ThreadStatusSink>,
    /// Saves the folder before each turn (F-EX-01); unset in tests.
    checkpoints: std::sync::OnceLock<Arc<crate::services::checkpoints::CheckpointService>>,
    /// Tells the teammates service that chats changed.
    chats_sink: std::sync::OnceLock<Arc<dyn Fn() + Send + Sync>>,
    /// Hears every teammate chat turn that ended (learning, F-AG-08).
    turn_sink: std::sync::OnceLock<TurnSink>,
    /// The plan's limits right now; unset (tests) runs every engine.
    plan_limits: std::sync::OnceLock<PlanLimits>,
    /// Threads in SSH workspaces (F-RM-01).
    remote: std::sync::OnceLock<Arc<crate::services::remote::RemoteService>>,
    /// Hears how every turn ended, by engine (last good version, F-EN-08).
    health_sink: std::sync::OnceLock<HealthSink>,
}

/// A thread turn of an engine ended, and why (F-EN-08).
/// What a turn's end says about the engine's version (F-EN-08 AC3): a
/// completed turn, or a turn that ended because the engine itself broke
/// ([`ErrorStage::engine_fault`]). Any other error, a stop or a usage limit
/// says nothing about the version.
pub fn version_health(reason: TurnEndReason, engine_fault: bool) -> Option<TurnEndReason> {
    match reason {
        TurnEndReason::Completed => Some(TurnEndReason::Completed),
        TurnEndReason::Error if engine_fault => Some(TurnEndReason::Error),
        TurnEndReason::Error | TurnEndReason::Cancelled | TurnEndReason::Limit => None,
    }
}

pub type HealthSink = Arc<dyn Fn(&str, TurnEndReason) + Send + Sync>;

/// A teammate chat's turn ended: the chat, the turn and why.
pub type TurnSink = Arc<dyn Fn(ThreadRecord, TurnId, TurnEndReason) + Send + Sync>;

/// Reads the plan's current limits (the entitlement service in the app).
pub type PlanLimits = Arc<dyn Fn() -> tandem_core::entitlement::Limits + Send + Sync>;

/// Receives every thread whenever the list changes; an archived thread
/// leaves the Dashboard.
pub type ThreadStatusSink = Arc<dyn Fn(&ThreadRecord) + Send + Sync>;

fn store_err(e: tandem_store::StoreError) -> IpcError {
    IpcError::new(ErrorCode::Internal, e.to_string())
}

fn not_found() -> IpcError {
    IpcError::new(ErrorCode::NotFound, "This thread doesn't exist")
}

fn turn_state(reason: TurnEndReason) -> TurnState {
    match reason {
        TurnEndReason::Completed => TurnState::Completed,
        TurnEndReason::Cancelled => TurnState::Cancelled,
        TurnEndReason::Limit => TurnState::Limit,
        TurnEndReason::Error => TurnState::Error,
    }
}

fn thread_state(reason: TurnEndReason) -> ThreadState {
    match reason {
        TurnEndReason::Completed => ThreadState::Done,
        TurnEndReason::Cancelled | TurnEndReason::Limit => ThreadState::Idle,
        TurnEndReason::Error => ThreadState::Failed,
    }
}

/// Owner-only access to a directory (recovery copies).
fn restrict_dir(dir: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// A turn's message with its attachments: images inline, files and folders
/// as paths (relative to the cwd when they're inside it).
fn turn_input(
    turn: &TurnId,
    text: String,
    carried: &[Attachment],
    cwd: &std::path::Path,
) -> TurnInput {
    let mut images = Vec::new();
    let mut files = Vec::new();
    for a in carried {
        let path = std::path::PathBuf::from(&a.path);
        match a.kind {
            AttachmentKind::Image => images.push(path),
            AttachmentKind::File | AttachmentKind::Folder | AttachmentKind::ToolOutput => {
                let shown = path
                    .strip_prefix(cwd)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| a.path.clone());
                files.push(shown);
            }
        }
    }
    TurnInput {
        turn_id: turn.clone(),
        text,
        images,
        files,
    }
}

/// A streaming piece: its block kind and text.
fn delta_parts(event: &ThreadEvent) -> Option<(BlockKind, &str)> {
    match event {
        ThreadEvent::AssistantDelta { text } => Some((BlockKind::Assistant, text)),
        ThreadEvent::ReasoningDelta { text } => Some((BlockKind::Reasoning, text)),
        _ => None,
    }
}

/// A stored row back as an envelope (rows that don't parse are skipped).
fn envelope(row: &MessageRow) -> Option<ThreadEventEnvelope> {
    let event = serde_json::from_str(&row.payload_json).ok()?;
    Some(ThreadEventEnvelope {
        thread_id: row.thread_id.clone(),
        turn_id: row.turn_id.clone(),
        seq: row.seq,
        ts: row.created_at,
        event,
    })
}

impl ThreadService {
    pub fn new(
        store: Store,
        bus: EventBus,
        clock: Arc<dyn Clock>,
        approvals: Arc<ApprovalBroker>,
        launcher: Arc<dyn ChatLauncher>,
    ) -> Arc<Self> {
        let (live, _) = broadcast::channel(LIVE_BUFFER);
        Arc::new(Self {
            store,
            bus,
            clock,
            approvals,
            launcher,
            runtimes: StdMutex::default(),
            live,
            titler: std::sync::OnceLock::new(),
            data: std::sync::OnceLock::new(),
            status_sink: std::sync::OnceLock::new(),
            checkpoints: std::sync::OnceLock::new(),
            chats_sink: std::sync::OnceLock::new(),
            turn_sink: std::sync::OnceLock::new(),
            health_sink: std::sync::OnceLock::new(),
            plan_limits: std::sync::OnceLock::new(),
            remote: std::sync::OnceLock::new(),
        })
    }

    pub fn set_checkpoints(&self, c: Arc<crate::services::checkpoints::CheckpointService>) {
        let _ = self.checkpoints.set(c);
    }

    /// The app's titler and data folder (tests may leave them unset).
    pub fn configure(&self, titler: Arc<dyn Titler>, data: tandem_core::DataDir) {
        let _ = self.titler.set(titler);
        let _ = self.data.set(data);
    }

    /// Publishes every thread (at launch: the Dashboard shows them).
    pub async fn announce(&self) {
        self.publish_threads().await;
    }

    /// Where the plan's limits come from (the app's entitlement service).
    pub fn set_plan_limits(&self, limits: PlanLimits) {
        let _ = self.plan_limits.set(limits);
    }

    /// Free threads run only the plan's chat engines (PRODUCT_SPEC §3.2,
    /// D-127). Checked when a thread is made, switched or starts a turn, so
    /// a thread made on Pro stays readable after a lapse but can't run.
    /// Teammate chats follow their own limits (M10).
    fn check_plan_engine(&self, kind: ThreadKind, engine_id: &str) -> Result<(), IpcError> {
        let allowed = kind != ThreadKind::Thread
            || self
                .plan_limits
                .get()
                .is_none_or(|limits| limits().allows_chat_engine(engine_id));
        if allowed {
            Ok(())
        } else {
            Err(IpcError::new(
                ErrorCode::ForbiddenPlan,
                tandem_core::entitlement::CHAT_ENGINE_ON_PRO,
            ))
        }
    }

    pub fn set_status_sink(&self, sink: ThreadStatusSink) {
        let _ = self.status_sink.set(sink);
    }

    pub fn set_remote(&self, remote: Arc<crate::services::remote::RemoteService>) {
        let _ = self.remote.set(remote);
    }

    /// A thread whose folder is on an SSH host: the host's service and the
    /// folder there.
    async fn remote_root(
        &self,
        thread: &ThreadRecord,
    ) -> Option<(
        Arc<crate::services::remote::RemoteService>,
        crate::services::remote_files::RemoteRoot,
    )> {
        let ws = tandem_store::repo::workspaces::get(&self.store, thread.workspace_id.as_ref()?)
            .await
            .ok()?;
        if ws.kind != "ssh" {
            return None;
        }
        Some((
            self.remote.get()?.clone(),
            crate::services::remote_files::RemoteRoot {
                host: tandem_core::ids::HostId::from_db(ws.host_id),
                root: thread.cwd.clone(),
            },
        ))
    }

    /// The thread runs this computer's engine: no workspace, or a local one.
    async fn runs_locally(&self, thread: &ThreadRecord) -> bool {
        match &thread.workspace_id {
            None => true,
            Some(ws) => tandem_store::repo::workspaces::get(&self.store, ws)
                .await
                .map(|w| w.kind == "local")
                .unwrap_or(false),
        }
    }

    /// Where this machine reads a thread's folder: a WSL thread's Linux
    /// folder is `\\wsl.localhost\<distro>\…` from Windows (F-RM-02).
    async fn local_cwd(&self, thread: &ThreadRecord) -> String {
        let ws = match &thread.workspace_id {
            Some(ws) => tandem_store::repo::workspaces::get(&self.store, ws)
                .await
                .ok(),
            None => None,
        };
        match ws {
            Some(ws) if ws.kind == "wsl" => ws
                .wsl_distro
                .as_deref()
                .and_then(|d| tandem_core::wslpath::from_wsl_path(d, &thread.cwd))
                .unwrap_or_else(|| thread.cwd.clone()),
            _ => thread.cwd.clone(),
        }
    }

    /// A remote thread's attachments go to the host first (F-RM-01
    /// "Conversation attachments are stored under the remote workspace"):
    /// each local file is copied to `<root>/.tandem/attachments/<thread>/`
    /// and the engine gets that path. Paths already on the host stay; a
    /// local folder can't be sent and is left out.
    async fn carry_to_host(
        &self,
        thread: &ThreadRecord,
        carried: Vec<Attachment>,
    ) -> Result<Vec<Attachment>, IpcError> {
        let Some((remote, root)) = self.remote_root(thread).await else {
            return Ok(carried);
        };
        let mut out = Vec::with_capacity(carried.len());
        for mut a in carried {
            if a.path
                .starts_with(&format!("{}/", root.root.trim_end_matches('/')))
            {
                out.push(a);
                continue;
            }
            let local = std::path::PathBuf::from(&a.path);
            if !matches!(
                a.kind,
                AttachmentKind::Image | AttachmentKind::File | AttachmentKind::ToolOutput
            ) || !local.is_file()
            {
                tracing::info!(
                    code = "threads.remote_attachment_skipped",
                    "a local folder can't go to a remote thread"
                );
                continue;
            }
            let bytes = tokio::fs::read(&local).await.map_err(|e| {
                IpcError::new(ErrorCode::Io, format!("Couldn't read {}: {e}", a.name))
            })?;
            a.path = remote
                .files
                .upload(&remote, &root, thread.id.as_str(), &a.name, &bytes)
                .await?;
            // The engine reads it on the host by path, images too.
            a.kind = AttachmentKind::File;
            out.push(a);
        }
        Ok(out)
    }

    fn data_dir(&self) -> Result<&tandem_core::DataDir, IpcError> {
        self.data
            .get()
            .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "Attachments aren't available"))
    }

    fn runtime(&self, id: &ThreadId) -> Arc<Mutex<Runtime>> {
        let mut map = match self.runtimes.lock() {
            Ok(m) => m,
            Err(poisoned) => poisoned.into_inner(),
        };
        map.entry(id.as_str().to_string()).or_default().clone()
    }

    async fn thread(&self, id: &ThreadId) -> Result<ThreadRecord, IpcError> {
        repo::get(&self.store, id)
            .await
            .map_err(store_err)?
            .ok_or_else(not_found)
    }

    /// Publishes `threads:changed` after a change made outside this
    /// service (the launch settle, F-RT-06).
    pub async fn republish(&self) {
        self.publish_threads().await;
    }

    async fn publish_threads(&self) {
        if let Some(sink) = self.chats_sink.get() {
            sink();
        }
        if let Ok(threads) = repo::list(&self.store, ThreadKind::Thread, true).await {
            if let Some(sink) = self.status_sink.get() {
                for t in &threads {
                    sink(t);
                }
            }
            self.bus.publish(BusEvent::new(
                Topic::Threads,
                "threads:changed",
                ThreadsChanged { threads },
            ));
        }
    }

    async fn set_state(&self, id: &ThreadId, state: ThreadState) {
        if let Err(e) = repo::set_state(&self.store, id, state).await {
            tracing::warn!(code = "threads.state_failed", error = %e, "couldn't save a thread's state");
        }
        self.publish_threads().await;
    }

    pub async fn create(&self, new: NewThread) -> Result<ThreadRecord, IpcError> {
        self.check_plan_engine(new.kind, &new.engine_id)?;
        let thread = repo::insert(&self.store, new).await.map_err(store_err)?;
        tracing::info!(code = "threads.created", engine = %thread.engine_id, "thread created");
        self.publish_threads().await;
        Ok(thread)
    }

    /// A teammate chat (F-AG-03): after its teammate's other chats.
    pub async fn create_chat(
        &self,
        new: NewThread,
        extra: repo::ChatExtra,
    ) -> Result<ThreadRecord, IpcError> {
        let thread = repo::insert_chat(&self.store, new, extra)
            .await
            .map_err(store_err)?;
        tracing::info!(code = "threads.chat_created", engine = %thread.engine_id, "teammate chat created");
        self.publish_threads().await;
        Ok(thread)
    }

    /// Hears teammate chats' finished turns.
    pub fn set_health_sink(&self, sink: HealthSink) {
        let _ = self.health_sink.set(sink);
    }

    pub fn set_turn_sink(&self, sink: TurnSink) {
        let _ = self.turn_sink.set(sink);
    }

    /// Teammate chats changed state: the Agent views hear about it.
    pub fn set_chats_sink(&self, sink: Arc<dyn Fn() + Send + Sync>) {
        let _ = self.chats_sink.set(sink);
    }

    pub async fn list(
        &self,
        kind: ThreadKind,
        include_archived: bool,
    ) -> Result<Vec<ThreadRecord>, IpcError> {
        repo::list(&self.store, kind, include_archived)
            .await
            .map_err(store_err)
    }

    pub async fn get(&self, id: &ThreadId) -> Result<ThreadDetail, IpcError> {
        let thread = self.thread(id).await?;
        let turns = repo::turns(&self.store, id).await.map_err(store_err)?;
        let draft = repo::draft(&self.store, id).await.map_err(store_err)?;
        let attachments = attachments_repo::unsent(&self.store, id)
            .await
            .map_err(store_err)?;
        // Opening a thread reads it (F-TH-01 AC2).
        let mut thread = thread;
        if thread.unread {
            if let Err(e) = repo::set_unread(&self.store, id, false).await {
                tracing::warn!(code = "threads.unread_failed", error = %e, "couldn't mark a thread read");
            }
            thread.unread = false;
            self.publish_threads().await;
        }
        let plan_revisions = repo::plan_revisions(&self.store, id)
            .await
            .map_err(store_err)?;
        let rt = self.runtime(id);
        let rt = rt.lock().await;
        Ok(ThreadDetail {
            thread,
            turns,
            running: rt.turn.as_ref().map(|r| r.id.clone()),
            queued: rt.queued.as_ref().map(|q| q.text.clone()),
            draft,
            attachments,
            plan_revisions,
        })
    }

    /// Asks for an AI title in the background; the local one stays if
    /// there's none (F-TH-15, D-118).
    fn ai_title(self: &Arc<Self>, thread: &ThreadId, first: String) {
        let titler: Arc<dyn Titler> = match self.titler.get() {
            Some(t) => t.clone(),
            None => Arc::new(LocalOnly),
        };
        let svc = self.clone();
        let thread = thread.clone();
        tauri::async_runtime::spawn(async move {
            let Some(title) = titler.title(&first).await else {
                return;
            };
            match repo::set_auto_title(&svc.store, &thread, title, "auto").await {
                Ok(true) => svc.publish_threads().await,
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(code = "threads.title_failed", error = %e, "couldn't name a thread")
                }
            }
        });
    }

    /// `thread_update` (F-TH-01, F-TH-09). New access, mode or model take
    /// effect on the next turn, in a fresh session.
    pub async fn update(
        &self,
        id: &ThreadId,
        patch: ThreadPatch,
    ) -> Result<ThreadRecord, IpcError> {
        if patch.title.as_deref().is_some_and(|t| t.trim().is_empty()) {
            return Err(IpcError::new(ErrorCode::Invalid, "A thread needs a name"));
        }
        let patch = ThreadPatch {
            title: patch
                .title
                .map(|t| t.trim().chars().take(thread_titles::MAX_TITLE).collect()),
            ..patch
        };
        let before = self.thread(id).await?;
        let restart = patch.access.is_some_and(|a| a != before.access)
            || patch.mode.is_some_and(|m| m != before.mode)
            || patch.model.as_ref().is_some_and(|m| *m != before.model)
            || patch.effort.as_ref().is_some_and(|e| *e != before.effort);
        let thread = repo::update(&self.store, id, patch)
            .await
            .map_err(store_err)?
            .ok_or_else(not_found)?;
        if restart {
            self.runtime(id).lock().await.stale = true;
        }
        self.publish_threads().await;
        Ok(thread)
    }

    /// The next turn starts a fresh session (resuming the engine's
    /// conversation), so new grants or built-ins reach its lease.
    pub async fn refresh_session(&self, id: &ThreadId) {
        self.runtime(id).lock().await.stale = true;
    }

    /// Ends the engine session between turns; the next turn starts anew.
    async fn retire_session(&self, id: &ThreadId) -> Result<(), IpcError> {
        let rt = self.runtime(id);
        let mut guard = rt.lock().await;
        if guard.turn.is_some() {
            return Err(IpcError::new(
                ErrorCode::Conflict,
                "Wait for this turn to finish, or stop it",
            ));
        }
        guard.stale = false;
        guard.lost = false;
        if let Some(old) = guard.session.take() {
            guard.generation += 1;
            tauri::async_runtime::spawn(async move {
                let _ = old.shutdown().await;
            });
        }
        Ok(())
    }

    /// `thread_switch_engine` (F-TH-16, between turns): the new engine starts
    /// a fresh session, seeded with a summary of the conversation at its
    /// first turn, which opens with `divider`; this thread's allow rules
    /// don't carry over.
    pub async fn switch_engine(
        &self,
        id: &ThreadId,
        engine_id: &str,
        account_id: Option<&tandem_core::ids::AccountId>,
        divider: String,
    ) -> Result<ThreadRecord, IpcError> {
        let thread = self.thread(id).await?;
        if thread.engine_id == engine_id && thread.account_id.as_ref() == account_id {
            return Ok(thread);
        }
        self.check_plan_engine(thread.kind, engine_id)?;
        self.retire_session(id).await?;
        repo::set_engine(&self.store, id, engine_id, account_id)
            .await
            .map_err(store_err)?;
        let has_turns = !repo::turns(&self.store, id)
            .await
            .map_err(store_err)?
            .is_empty();
        if has_turns {
            let rt = self.runtime(id);
            let mut guard = rt.lock().await;
            guard.pending_switch = Some(divider);
            guard.seed_pending = true;
        }
        let thread = self.thread(id).await?;
        self.publish_threads().await;
        Ok(thread)
    }

    /// `thread_move_to_folder` (F-TH-02): the thread now works in `cwd`;
    /// its draft and attachments stay.
    pub async fn move_to_folder(
        &self,
        id: &ThreadId,
        workspace_id: &tandem_core::ids::WorkspaceId,
        cwd: &str,
    ) -> Result<ThreadRecord, IpcError> {
        self.thread(id).await?;
        self.retire_session(id).await?;
        repo::set_folder(&self.store, id, workspace_id, cwd)
            .await
            .map_err(store_err)?;
        let thread = self.thread(id).await?;
        self.publish_threads().await;
        Ok(thread)
    }

    /// `thread_archive`: an archived thread leaves the rail; a running turn
    /// stops first.
    pub async fn archive(self: &Arc<Self>, id: &ThreadId, archived: bool) -> Result<(), IpcError> {
        self.thread(id).await?;
        if archived {
            self.stop(id).await?;
        }
        repo::set_archived(&self.store, id, archived)
            .await
            .map_err(store_err)?;
        self.publish_threads().await;
        Ok(())
    }

    /// `thread_delete` (archived threads only): a recovery copy of the thread
    /// and its attachments is kept for 7 days, readable only by the user.
    pub async fn delete(self: &Arc<Self>, id: &ThreadId) -> Result<(), IpcError> {
        let thread = self.thread(id).await?;
        if !thread.archived {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Archive the thread before deleting it",
            ));
        }
        let data = self.data_dir()?.clone();
        let turns = repo::turns(&self.store, id).await.map_err(store_err)?;
        let messages = repo::all_messages(&self.store, id)
            .await
            .map_err(store_err)?;
        let copy = serde_json::json!({
            "thread": thread,
            "turns": turns,
            "events": messages.iter().filter_map(envelope).collect::<Vec<_>>(),
        });
        let dir =
            data.trash()
                .join("threads")
                .join(format!("{}-{}", id.as_str(), self.clock.now_ms()));
        let io = |e: std::io::Error| {
            IpcError::new(ErrorCode::Io, format!("Couldn't keep a recovery copy: {e}"))
        };
        std::fs::create_dir_all(&dir).map_err(io)?;
        restrict_dir(&dir);
        tandem_core::atomic::atomic_write(
            &dir.join("thread.json"),
            copy.to_string().as_bytes(),
            Some(0o600),
        )
        .map_err(io)?;
        let own = data.attachments().join(id.as_str());
        if own.is_dir() {
            std::fs::rename(&own, dir.join("attachments")).map_err(io)?;
        }
        // Its Changes snapshot isn't needed any more.
        let _ = std::fs::remove_file(data.snapshots().join(format!("{}.json", id.as_str())));
        // Nor its briefing (teammate chats).
        let _ = std::fs::remove_file(data.briefings().join(format!("{}.md", id.as_str())));
        // The engine session ends with the thread.
        let rt = self.runtime(id);
        if let Some(session) = rt.lock().await.session.take() {
            let _ = session.shutdown().await;
        }
        if let Ok(mut map) = self.runtimes.lock() {
            map.remove(id.as_str());
        }
        if let Some(sink) = self.status_sink.get() {
            // Archived: its card goes.
            sink(&thread);
        }
        repo::delete(&self.store, id).await.map_err(store_err)?;
        tracing::info!(
            code = "threads.deleted",
            "thread deleted; recovery copy kept for 7 days"
        );
        self.publish_threads().await;
        Ok(())
    }

    /// Removes deleted threads' recovery copies older than 7 days.
    pub fn sweep_trash(&self) -> usize {
        let Some(data) = self.data.get() else {
            return 0;
        };
        let Ok(entries) = std::fs::read_dir(data.trash().join("threads")) else {
            return 0;
        };
        let now = self.clock.now_ms();
        let mut removed = 0;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let deleted_at = name.rsplit('-').next().and_then(|t| t.parse::<i64>().ok());
            if deleted_at.is_some_and(|t| now - t > TRASH_KEEP_MS)
                && std::fs::remove_dir_all(entry.path()).is_ok()
            {
                removed += 1;
            }
        }
        removed
    }

    /// `thread_unqueue`: takes back the follow-up waiting for the running
    /// turn; its text goes back to the composer.
    pub async fn unqueue(&self, id: &ThreadId) -> Result<Option<String>, IpcError> {
        self.thread(id).await?;
        let rt = self.runtime(id);
        let queued = rt.lock().await.queued.take();
        Ok(queued.map(|q| q.text))
    }

    /// `thread_save_draft` (F-TH-07): `Draft {text, attachmentIds, queued?}`.
    pub async fn save_draft(
        &self,
        id: &ThreadId,
        draft: serde_json::Value,
    ) -> Result<(), IpcError> {
        if !draft.is_object() || draft.get("text").is_some_and(|t| !t.is_string()) {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "A draft is {text, attachmentIds}",
            ));
        }
        repo::set_draft(&self.store, id, draft.to_string())
            .await
            .map_err(store_err)
    }

    /// `thread_attach_paths`: dropped or picked files and folders.
    pub async fn attach_paths(
        &self,
        id: &ThreadId,
        paths: Vec<String>,
    ) -> Result<Vec<Attachment>, IpcError> {
        let pending = attachments_repo::unsent(&self.store, id)
            .await
            .map_err(store_err)?
            .len();
        if pending + paths.len() > thread_files::MAX_ATTACHMENTS {
            return Err(IpcError::new(
                ErrorCode::LimitReached,
                "A message can carry up to 20 attachments",
            ));
        }
        let thread = self.thread(id).await?;
        let remote = self.remote_root(&thread).await;
        let mut out = Vec::new();
        for p in paths {
            let a = match &remote {
                // A file from the host's own Files tab: it's already there.
                Some((svc, root))
                    if p.starts_with(&format!("{}/", root.root.trim_end_matches('/'))) =>
                {
                    let stat: tandem_host::protocol::FsStat = svc
                        .call(
                            &root.host,
                            tandem_host::protocol::method::FS_STAT,
                            serde_json::json!({"root": root.root, "path": p}),
                        )
                        .await?;
                    thread_files::on_host(
                        id,
                        &p,
                        stat.kind == tandem_host::protocol::EntryKind::Dir,
                        stat.size,
                    )
                }
                _ => thread_files::for_path(id, std::path::Path::new(&p))?,
            };
            out.push(
                attachments_repo::insert(&self.store, a)
                    .await
                    .map_err(store_err)?,
            );
        }
        Ok(out)
    }

    /// `thread_attach_clipboard_image`: the pixels, saved as a PNG chip.
    pub async fn attach_png(
        &self,
        id: &ThreadId,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<Attachment, IpcError> {
        self.thread(id).await?;
        let dir = self.data_dir()?.attachments().join(id.as_str());
        let thread = id.clone();
        let a = tokio::task::spawn_blocking(move || {
            thread_files::save_png(&dir, &thread, width, height, &rgba)
        })
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))??;
        attachments_repo::insert(&self.store, a)
            .await
            .map_err(store_err)
    }

    /// `attachment_remove`: drops a chip, and the copy if Vibezeo made it.
    pub async fn remove_attachment(&self, attachment: &AttachmentId) -> Result<(), IpcError> {
        let removed = attachments_repo::remove(&self.store, attachment)
            .await
            .map_err(store_err)?;
        if let (Some(a), Ok(data)) = (removed, self.data_dir()) {
            let path = std::path::PathBuf::from(&a.path);
            if path.starts_with(data.attachments()) {
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(())
    }

    /// `thread_mention_search` (F-TH-07 AC6): paths in the thread's folder.
    pub async fn mention_search(
        &self,
        id: &ThreadId,
        query: String,
    ) -> Result<Vec<PathEntry>, IpcError> {
        let thread = self.thread(id).await?;
        let root = std::path::PathBuf::from(self.local_cwd(&thread).await);
        tokio::task::spawn_blocking(move || thread_files::mention_search(&root, &query))
            .await
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))
    }

    /// A page of stored events, oldest first.
    pub async fn events(
        &self,
        id: &ThreadId,
        before_id: Option<i64>,
        limit: u32,
    ) -> Result<ThreadEventPage, IpcError> {
        let limit = limit.clamp(1, 500);
        let rows = repo::page(&self.store, id, before_id, limit)
            .await
            .map_err(store_err)?;
        let next_before_id = match rows.first() {
            Some(first) if rows.len() == limit as usize => Some(first.id),
            _ => None,
        };
        Ok(ThreadEventPage {
            items: rows.iter().filter_map(envelope).collect(),
            next_before_id,
        })
    }

    /// The running turn so far (stored events, then the open block's text),
    /// and the live stream from that point: taken together,
    /// so nothing is missed or doubled (ARCHITECTURE §3.3).
    pub async fn subscribe(
        &self,
        id: &ThreadId,
    ) -> Result<
        (
            Vec<ThreadEventEnvelope>,
            broadcast::Receiver<ThreadEventEnvelope>,
        ),
        IpcError,
    > {
        self.thread(id).await?;
        let rt = self.runtime(id);
        let rt = rt.lock().await;
        let rx = self.live.subscribe();
        let Some(run) = rt.turn.as_ref() else {
            return Ok((Vec::new(), rx));
        };
        let partial = run.open.as_ref().and_then(|b| b.row);
        let rows = repo::turn_messages(&self.store, &run.id)
            .await
            .map_err(store_err)?;
        let mut snapshot: Vec<ThreadEventEnvelope> = rows
            .iter()
            .filter(|r| Some(r.id) != partial)
            .filter_map(envelope)
            .collect();
        // The open block as its text so far: the view replaces by seq and
        // appends the live deltas that follow (a resync never doubles text).
        if let Some(block) = &run.open {
            snapshot.push(self.envelope(id, &run.id, block.seq, block.event()));
        }
        Ok((snapshot, rx))
    }

    fn envelope(
        &self,
        thread: &ThreadId,
        turn: &TurnId,
        seq: u64,
        event: ThreadEvent,
    ) -> ThreadEventEnvelope {
        ThreadEventEnvelope {
            thread_id: thread.clone(),
            turn_id: turn.clone(),
            seq,
            ts: self.clock.now_ms(),
            event,
        }
    }

    fn broadcast(&self, env: ThreadEventEnvelope) {
        // No subscriber is fine.
        let _ = self.live.send(env);
    }

    /// Stores and streams one event of the running turn.
    async fn emit(&self, thread: &ThreadId, run: &mut Running, event: ThreadEvent) -> Option<i64> {
        if let ThreadEvent::Error { stage, code, .. } = &event {
            run.engine_fault |= stage.engine_fault(code);
        }
        let seq = run.take_seq();
        let row = self.store_event(thread, &run.id, seq, &event, None).await;
        self.broadcast(self.envelope(thread, &run.id, seq, event));
        row
    }

    /// Inserts an event (or rewrites `row`); returns the row id.
    async fn store_event(
        &self,
        thread: &ThreadId,
        turn: &TurnId,
        seq: u64,
        event: &ThreadEvent,
        row: Option<i64>,
    ) -> Option<i64> {
        let payload = match serde_json::to_string(event) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(code = "threads.encode_failed", error = %e, "couldn't encode an event");
                return row;
            }
        };
        let done = match row {
            Some(id) => repo::replace(&self.store, id, event.tag(), payload)
                .await
                .map(|_| id),
            None => repo::append(&self.store, thread, turn, seq, event.tag(), payload).await,
        };
        match done {
            Ok(id) => Some(id),
            Err(e) => {
                tracing::warn!(code = "threads.store_failed", error = %e, "couldn't store an event");
                row
            }
        }
    }

    /// Ends the open block: `last` (the adapter's own final text) or the
    /// streamed text, in the block's place.
    async fn close_block(&self, thread: &ThreadId, run: &mut Running, last: Option<ThreadEvent>) {
        let Some(block) = run.open.take() else {
            return;
        };
        let event = last.unwrap_or_else(|| block.event());
        self.store_event(thread, &run.id, block.seq, &event, block.row)
            .await;
        self.broadcast(self.envelope(thread, &run.id, block.seq, event));
    }

    /// Saves the open block as a partial row (every 2 s while streaming),
    /// and notes a turn that has said nothing for 10 minutes (F-TH-15).
    async fn flush(&self, thread: &ThreadId) {
        let rt = self.runtime(thread);
        let mut rt = rt.lock().await;
        let Some(run) = rt.turn.as_mut() else {
            return;
        };
        if !run.silence_noted && self.clock.now_ms() - run.last_output >= SILENT_AFTER_MS {
            run.silence_noted = true;
            self.emit(
                thread,
                run,
                ThreadEvent::Note {
                    text: "No output for 10 minutes".into(),
                },
            )
            .await;
        }
        let turn = run.id.clone();
        let Some(block) = run.open.as_mut().filter(|b| b.dirty) else {
            return;
        };
        let event = block.event();
        block.row = self
            .store_event(thread, &turn, block.seq, &event, block.row)
            .await;
        block.dirty = false;
    }

    /// Ends the running turn; returns the queued follow-up to send, if the
    /// turn wasn't stopped.
    /// Records where the Changes panel starts, before the thread's first
    /// turn runs (F-TH-13): the folder's commit, or a snapshot manifest.
    async fn ensure_start(&self, thread: &ThreadRecord) {
        // A host's folder isn't read from here: no start point (D-411).
        if self.remote_root(thread).await.is_some() {
            return;
        }
        if !matches!(
            repo::start_point(&self.store, &thread.id).await,
            Ok((None, None))
        ) {
            return;
        }
        // Only a folder without git needs somewhere to keep its snapshot.
        let snapshot = self.data.get().map(|data| {
            data.snapshots()
                .join(format!("{}.json", thread.id.as_str()))
        });
        let cwd = self.local_cwd(thread).await;
        let start = match thread_changes::capture(cwd.into(), snapshot).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(code = "threads.start_failed", error = %e.message, "couldn't record where the thread starts");
                return;
            }
        };
        let (commit, snapshot) = match start {
            thread_changes::Start::Commit(c) => (Some(c), None),
            thread_changes::Start::Snapshot(p) => (None, Some(p.display().to_string())),
        };
        if let Err(e) = repo::set_start_point(&self.store, &thread.id, commit, snapshot).await {
            tracing::warn!(code = "threads.start_failed", error = %e, "couldn't record where the thread starts");
        }
    }

    /// `thread_file_read`: a text file in the thread's folder (Files tab).
    pub async fn read_file(
        &self,
        id: &ThreadId,
        path: String,
    ) -> Result<thread_files::FileText, IpcError> {
        let thread = self.thread(id).await?;
        if let Some((remote, root)) = self.remote_root(&thread).await {
            if tandem_core::contain::is_private_file(std::path::Path::new(&path)) {
                return Err(IpcError::new(ErrorCode::PathEscape, "that file is private"));
            }
            let f = remote.files.read(&remote, &root, path).await?;
            return thread_files::remote_text(f);
        }
        let root = std::path::PathBuf::from(self.local_cwd(&thread).await);
        tokio::task::spawn_blocking(move || thread_files::read_text(&root, &path))
            .await
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
    }

    /// `thread_changes`: what changed in the thread's folder since its first
    /// turn (F-TH-13).
    pub async fn changes(&self, id: &ThreadId) -> Result<thread_changes::ThreadChanges, IpcError> {
        let thread = self.thread(id).await?;
        if self.remote_root(&thread).await.is_some() {
            return Err(IpcError::new(
                ErrorCode::Unavailable,
                "The Changes panel follows local folders. This thread's folder is on a remote host: its Files tab shows git changes there.",
            )
            .with_stage("remote"));
        }
        let start = match repo::start_point(&self.store, id)
            .await
            .map_err(store_err)?
        {
            (Some(commit), _) => Some(thread_changes::Start::Commit(commit)),
            (None, Some(path)) => Some(thread_changes::Start::Snapshot(path.into())),
            (None, None) => None,
        };
        thread_changes::changes(self.local_cwd(&thread).await.into(), start).await
    }

    /// In Plan mode a completed turn's last reply is the thread's next plan
    /// version (F-TH-10). Returns that version.
    async fn save_plan(&self, thread: &ThreadId, turn: &TurnId) -> Option<u32> {
        let record = self.thread(thread).await.ok()?;
        if record.mode != ThreadMode::Plan {
            return None;
        }
        let rows = repo::turn_messages(&self.store, turn).await.ok()?;
        let text =
            rows.iter().rev().find_map(|row| {
                match serde_json::from_str::<ThreadEvent>(&row.payload_json).ok()? {
                    ThreadEvent::Assistant { markdown } if !markdown.trim().is_empty() => {
                        Some(markdown)
                    }
                    _ => None,
                }
            })?;
        match repo::add_plan_revision(&self.store, thread, turn, text).await {
            Ok(plan) => Some(plan.version),
            Err(e) => {
                tracing::warn!(code = "threads.plan_failed", error = %e, "couldn't save a plan version");
                None
            }
        }
    }

    /// Implement on a plan version: the thread goes to Act (at `access` when
    /// given) and that version's text is sent. An unsent draft stays in the
    /// composer (D-121).
    pub async fn implement_plan(
        self: &Arc<Self>,
        id: &ThreadId,
        version: u32,
        access: Option<Access>,
    ) -> Result<SendResult, IpcError> {
        let thread = self.thread(id).await?;
        self.check_plan_engine(thread.kind, &thread.engine_id)?;
        let plan = repo::plan_revisions(&self.store, id)
            .await
            .map_err(store_err)?
            .into_iter()
            .find(|p| p.version == version)
            .ok_or_else(|| {
                IpcError::new(
                    ErrorCode::NotFound,
                    format!("There's no plan version {version}"),
                )
            })?;
        if self.runtime(id).lock().await.turn.is_some() {
            return Err(IpcError::new(
                ErrorCode::Conflict,
                "Working — send when it's done, or stop it",
            ));
        }
        self.update(
            id,
            ThreadPatch {
                mode: Some(ThreadMode::Act),
                access,
                ..ThreadPatch::default()
            },
        )
        .await?;
        self.send(
            id,
            format!(
                "Implement plan version {version} exactly as written:\n\n{}",
                plan.text
            ),
            Vec::new(),
            false,
        )
        .await
    }

    async fn end_turn(
        self: &Arc<Self>,
        thread: &ThreadId,
        rt: &mut Runtime,
        reason: TurnEndReason,
    ) -> Option<Queued> {
        let mut run = rt.turn.take()?;
        // Nothing waits on an answer once the turn is over.
        self.approvals.cancel_source(&SourceRef::Thread {
            thread_id: thread.as_str().to_string(),
        });
        self.close_block(thread, &mut run, None).await;
        // A stop that the engine reports as an error is still a stop.
        let reason = match reason {
            TurnEndReason::Error if run.stopping => TurnEndReason::Cancelled,
            r => r,
        };
        let blocked = run.blocked;
        let engine_fault = run.engine_fault;
        if reason == TurnEndReason::Completed {
            if let Some(version) = self.save_plan(thread, &run.id).await {
                self.emit(thread, &mut run, ThreadEvent::PlanRevision { version })
                    .await;
            }
        }
        self.emit(
            thread,
            &mut run,
            ThreadEvent::TurnEnd {
                reason,
                blocked_actions: blocked,
            },
        )
        .await;
        if let Err(e) = repo::end_turn(&self.store, &run.id, turn_state(reason), blocked).await {
            tracing::warn!(code = "threads.turn_end_failed", error = %e, "couldn't end a turn");
        }
        // A teammate chat's prose becomes searchable (F-AG-11).
        if let Ok(Some(record)) = repo::get(&self.store, thread).await {
            if let Some(sink) = self.health_sink.get() {
                if let Some(health) = version_health(reason, engine_fault) {
                    // A thread on an SSH host or in WSL runs that host's
                    // engine, not this computer's (F-EN-08).
                    if self.runs_locally(&record).await {
                        sink(&record.engine_id, health);
                    }
                }
            }
            if let Some(teammate) = &record.teammate_id {
                if let Err(e) =
                    tandem_store::repo::search::index_turn(&self.store, thread, &run.id, teammate)
                        .await
                {
                    tracing::warn!(code = "threads.index_failed", error = %e, "couldn't index a turn");
                }
                if let Some(sink) = self.turn_sink.get() {
                    sink(record.clone(), run.id.clone(), reason);
                }
            }
        }
        tracing::info!(code = "threads.turn_ended", reason = ?reason, "turn ended");
        // Unread until the thread is opened again (F-TH-01 AC2).
        if let Err(e) = repo::set_unread(&self.store, thread, true).await {
            tracing::warn!(code = "threads.unread_failed", error = %e, "couldn't mark a thread unread");
        }
        if let (TurnEndReason::Completed, Some(first)) = (reason, run.title_from.take()) {
            self.ai_title(thread, first);
        }
        self.set_state(thread, thread_state(reason)).await;
        let queued = rt.queued.take();
        if reason == TurnEndReason::Cancelled {
            // Nothing runs after a stop: the follow-up goes back to the draft.
            if let Some(q) = queued {
                let draft = serde_json::json!({ "text": q.text, "attachmentIds": q.attachments });
                if let Err(e) = repo::set_draft(&self.store, thread, draft.to_string()).await {
                    tracing::warn!(code = "threads.draft_failed", error = %e, "couldn't keep the queued message");
                }
            }
            return None;
        }
        queued
    }

    /// Sends the follow-up after the turn that held it ended.
    fn send_queued(self: &Arc<Self>, thread: &ThreadId, queued: Option<Queued>) {
        let Some(q) = queued else {
            return;
        };
        let svc = self.clone();
        let thread = thread.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = svc.send(&thread, q.text, q.attachments, false).await {
                tracing::warn!(code = "threads.queued_failed", error = %e.message, "couldn't send the queued message");
            }
        });
    }

    /// `thread_send`: starts a turn, or queues one follow-up while a turn
    /// runs.
    pub async fn send(
        self: &Arc<Self>,
        id: &ThreadId,
        text: String,
        attachments: Vec<AttachmentId>,
        queue_if_busy: bool,
    ) -> Result<SendResult, IpcError> {
        self.send_with(id, text, attachments, queue_if_busy, None)
            .await
    }

    /// Delivers teammate mail (F-AG-12): a turn whose message is `text`,
    /// followed by the mail's row (shown in the message's place).
    pub async fn send_mail(
        self: &Arc<Self>,
        id: &ThreadId,
        text: String,
        row: ThreadEvent,
    ) -> Result<SendResult, IpcError> {
        self.send_with(id, text, Vec::new(), false, Some(row)).await
    }

    /// A mail row in a chat (F-AG-12): in its running turn, or in a turn of
    /// its own (the row, then its end) when none runs.
    pub async fn note_mail(&self, id: &ThreadId, row: ThreadEvent) -> Result<(), IpcError> {
        let rt = self.runtime(id);
        let mut guard = rt.lock().await;
        if let Some(run) = guard.turn.as_mut() {
            self.emit(id, run, row).await;
            return Ok(());
        }
        let thread = self.thread(id).await?;
        let turn_id = TurnId::new();
        repo::start_turn(
            &self.store,
            id,
            &turn_id,
            &thread.engine_id,
            thread.account_id.as_ref(),
        )
        .await
        .map_err(store_err)?;
        let mut run = Running {
            id: turn_id.clone(),
            next_seq: 0,
            open: None,
            stopping: false,
            blocked: 0,
            title_from: None,
            last_output: self.clock.now_ms(),
            silence_noted: false,
            engine_fault: false,
        };
        self.emit(id, &mut run, row).await;
        self.emit(
            id,
            &mut run,
            ThreadEvent::TurnEnd {
                reason: TurnEndReason::Completed,
                blocked_actions: 0,
            },
        )
        .await;
        repo::end_turn(&self.store, &turn_id, TurnState::Completed, 0)
            .await
            .map_err(store_err)?;
        Ok(())
    }

    async fn send_with(
        self: &Arc<Self>,
        id: &ThreadId,
        text: String,
        attachments: Vec<AttachmentId>,
        queue_if_busy: bool,
        mail_row: Option<ThreadEvent>,
    ) -> Result<SendResult, IpcError> {
        let thread = self.thread(id).await?;
        self.check_plan_engine(thread.kind, &thread.engine_id)?;
        // What the message carries, checked before anything starts (F-TH-07).
        let carried = attachments_repo::get_many(&self.store, id, &attachments)
            .await
            .map_err(store_err)?;
        thread_files::check_limits(&carried)?;
        let rt = self.runtime(id);
        let mut guard = rt.lock().await;
        if guard.turn.is_some() {
            if !queue_if_busy {
                return Err(IpcError::new(
                    ErrorCode::Conflict,
                    "Working — send when it's done, or stop it",
                ));
            }
            if guard.queued.is_some() {
                return Err(IpcError::new(
                    ErrorCode::Conflict,
                    "A message is already queued",
                ));
            }
            guard.queued = Some(Queued { text, attachments });
            return Ok(SendResult {
                turn_id: None,
                queued: true,
            });
        }

        if guard.stale {
            // New access, mode or model: a fresh session picks them up.
            guard.stale = false;
            if let Some(old) = guard.session.take() {
                guard.generation += 1;
                tauri::async_runtime::spawn(async move {
                    let _ = old.shutdown().await;
                });
            }
        }

        // After a switch the new engine hears what was said before (F-TH-16).
        let seed = if guard.seed_pending {
            let rows = repo::all_messages(&self.store, id)
                .await
                .map_err(store_err)?;
            let events: Vec<ThreadEvent> =
                rows.iter().filter_map(envelope).map(|e| e.event).collect();
            thread_summary::summary(&events)
        } else {
            None
        };

        let turn_id = TurnId::new();
        let record = repo::start_turn(
            &self.store,
            id,
            &turn_id,
            &thread.engine_id,
            thread.account_id.as_ref(),
        )
        .await
        .map_err(store_err)?;
        // The first message names the thread at once; an AI title may
        // follow when the turn ends (F-TH-15).
        let mut title_from = None;
        if record.n == 1
            && repo::title_source(&self.store, id)
                .await
                .is_ok_and(|s| s == "none")
        {
            let local = thread_titles::fallback(&text);
            if let Err(e) = repo::set_auto_title(&self.store, id, local, "fallback").await {
                tracing::warn!(code = "threads.title_failed", error = %e, "couldn't name a thread");
            }
            title_from = Some(text.clone());
        }
        let mut run = Running {
            id: turn_id.clone(),
            next_seq: 0,
            open: None,
            stopping: false,
            blocked: 0,
            title_from,
            last_output: self.clock.now_ms(),
            silence_noted: false,
            engine_fault: false,
        };
        let row = self
            .emit(
                id,
                &mut run,
                ThreadEvent::User {
                    text: text.clone(),
                    attachments: attachments.clone(),
                },
            )
            .await;
        if let Some(row) = mail_row {
            self.emit(id, &mut run, row).await;
        }
        // A turn starts with its message; the transcript shows the divider
        // above it.
        if let Some(text) = guard.pending_switch.take() {
            self.emit(id, &mut run, ThreadEvent::Divider { text }).await;
        }
        if let (Some(row), false) = (row, carried.is_empty()) {
            let ids: Vec<AttachmentId> = carried.iter().map(|a| a.id.clone()).collect();
            if let Err(e) = attachments_repo::mark_sent(&self.store, &ids, row).await {
                tracing::warn!(code = "threads.attachments_failed", error = %e, "couldn't tie attachments to their message");
            }
        }
        guard.turn = Some(run);
        self.set_state(id, ThreadState::Working).await;

        if guard.session.is_none() && guard.lost {
            if let Some(run) = guard.turn.as_mut() {
                self.emit(
                    id,
                    run,
                    ThreadEvent::Note {
                        text: "Reconnecting…".into(),
                    },
                )
                .await;
            }
        }
        let session = match guard.session.clone() {
            Some(s) => s,
            None => match self.start_session(&thread, &mut guard).await {
                Ok(s) => s,
                Err(e) => {
                    // An engine version Thread mode can't speak: its terminal
                    // still works. A sign-in is its own stage (F-TH-06).
                    let (code, stage, action) = match e.code {
                        ErrorCode::EngineUnsupported => {
                            ("spawn_failed", ErrorStage::Spawn, ErrorAction::OpenTerminal)
                        }
                        ErrorCode::Unauthenticated => {
                            ("auth_required", ErrorStage::Auth, ErrorAction::SignIn)
                        }
                        _ => ("spawn_failed", ErrorStage::Spawn, ErrorAction::Retry),
                    };
                    self.fail_turn(id, &mut guard, code, &e.message, stage, action)
                        .await;
                    return Ok(SendResult {
                        turn_id: Some(turn_id),
                        queued: false,
                    });
                }
            },
        };
        drop(guard);

        // Outside the lock: a slow stdin never holds up the event stream.
        self.ensure_start(&thread).await;
        // F-EX-01: the folder as it was before the engine hears the message.
        if let (Some(cps), Some(ws)) = (self.checkpoints.get(), thread.workspace_id.as_ref()) {
            cps.before_turn(
                crate::services::checkpoints::Request {
                    folder: std::path::PathBuf::from(&thread.cwd),
                    workspace_id: Some(ws.as_str().to_string()),
                    thread_id: Some(id.as_str().to_string()),
                    turn_id: Some(turn_id.as_str().to_string()),
                    label: repo::get(&self.store, id)
                        .await
                        .ok()
                        .flatten()
                        .map(|t| t.title)
                        .unwrap_or_default(),
                    ..Default::default()
                },
                tandem_store::repo::checkpoints::CheckpointReason::Turn,
            )
            .await;
        }
        let seeded = seed.is_some();
        let text = match seed {
            Some(summary) => thread_summary::seeded(&summary, &text),
            None => text,
        };
        let carried = match self.carry_to_host(&thread, carried).await {
            Ok(c) => c,
            Err(e) => {
                let mut guard = rt.lock().await;
                self.fail_turn(
                    id,
                    &mut guard,
                    "spawn_failed",
                    &e.message,
                    ErrorStage::Spawn,
                    ErrorAction::Retry,
                )
                .await;
                return Ok(SendResult {
                    turn_id: Some(turn_id),
                    queued: false,
                });
            }
        };
        let input = turn_input(&turn_id, text, &carried, std::path::Path::new(&thread.cwd));
        let sent = session.send_turn(input).await;
        if seeded && sent.is_ok() {
            rt.lock().await.seed_pending = false;
        }
        if let Err(e) = sent {
            let mut guard = rt.lock().await;
            if guard.turn.as_ref().map(|r| &r.id) == Some(&turn_id) {
                self.fail_turn(
                    id,
                    &mut guard,
                    "send_failed",
                    &e.to_string(),
                    ErrorStage::Unknown,
                    ErrorAction::Retry,
                )
                .await;
            }
        }
        Ok(SendResult {
            turn_id: Some(turn_id),
            queued: false,
        })
    }

    async fn start_session(
        self: &Arc<Self>,
        thread: &ThreadRecord,
        rt: &mut Runtime,
    ) -> Result<Arc<dyn ChatSession>, IpcError> {
        let (adapter, mut start) = self.launcher.prepare(thread).await?;
        if start.resume_session.is_none() {
            start.resume_session = thread.engine_session_id.clone();
        }
        let (model, notice) = self.launcher.model_for(thread);
        start.model = model;
        if let (Some(text), Some(run)) = (notice, rt.turn.as_mut()) {
            self.emit(&thread.id, run, ThreadEvent::Note { text }).await;
        }
        let (tx, rx) = mpsc::channel(256);
        let session: Arc<dyn ChatSession> =
            Arc::from(adapter.start(start, tx).await.map_err(|e| {
                let code = match e {
                    tandem_engines::chat::AdapterError::Unsupported(_) => {
                        ErrorCode::EngineUnsupported
                    }
                    tandem_engines::chat::AdapterError::Auth(_) => ErrorCode::Unauthenticated,
                    _ => ErrorCode::Internal,
                };
                IpcError::new(code, e.to_string())
            })?);
        rt.generation += 1;
        rt.session = Some(session.clone());
        rt.lost = false;
        let svc = self.clone();
        let id = thread.id.clone();
        let generation = rt.generation;
        tauri::async_runtime::spawn(async move { svc.pump(id, generation, rx).await });
        Ok(session)
    }

    /// The error card (with Retry) and the end of the turn.
    async fn fail_turn(
        self: &Arc<Self>,
        thread: &ThreadId,
        rt: &mut Runtime,
        code: &str,
        message: &str,
        stage: ErrorStage,
        action: ErrorAction,
    ) {
        let Some(run) = rt.turn.as_mut() else {
            return;
        };
        self.close_block(thread, run, None).await;
        self.emit(
            thread,
            run,
            ThreadEvent::Error {
                code: code.to_string(),
                message: message.to_string(),
                stage,
                hint: None,
                action: Some(action),
            },
        )
        .await;
        let queued = self.end_turn(thread, rt, TurnEndReason::Error).await;
        self.send_queued(thread, queued);
    }

    /// One session's events, until it ends.
    async fn pump(
        self: Arc<Self>,
        thread: ThreadId,
        generation: u64,
        mut rx: mpsc::Receiver<AdapterEvent>,
    ) {
        let mut tick = tokio::time::interval(FLUSH_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        tick.tick().await;
        loop {
            tokio::select! {
                ev = rx.recv() => match ev {
                    Some(ev) => self.handle(&thread, generation, ev).await,
                    None => {
                        let gone = AdapterEvent::Exited { code: None, stderr_tail: String::new() };
                        self.handle(&thread, generation, gone).await;
                        break;
                    }
                },
                _ = tick.tick() => self.flush(&thread).await,
            }
        }
    }

    async fn handle(self: &Arc<Self>, thread: &ThreadId, generation: u64, ev: AdapterEvent) {
        let rt = self.runtime(thread);
        let mut guard = rt.lock().await;
        match ev {
            AdapterEvent::SessionId(sid) => {
                if generation == guard.generation {
                    if let Err(e) = repo::set_engine_session(&self.store, thread, Some(&sid)).await
                    {
                        tracing::warn!(code = "threads.session_failed", error = %e, "couldn't save the engine session");
                    }
                }
            }
            AdapterEvent::Models {
                available,
                withdrawn,
            } => {
                let Ok(record) = self.thread(thread).await else {
                    return;
                };
                self.launcher.models_reported(&record.engine_id, &available);
                // The session already fell back to Automatic (F-TH-06 AC2).
                if withdrawn.is_some() && withdrawn == record.model {
                    let patch = ThreadPatch {
                        model: Some(None),
                        ..ThreadPatch::default()
                    };
                    if let Err(e) = repo::update(&self.store, thread, patch).await {
                        tracing::warn!(code = "threads.model_failed", error = %e, "couldn't clear a withdrawn model");
                    }
                    self.publish_threads().await;
                }
            }
            AdapterEvent::Exited { code, stderr_tail } => {
                if generation != guard.generation || guard.session.is_none() {
                    return;
                }
                guard.session = None;
                guard.lost = true;
                self.launcher.session_ended(thread);
                tracing::info!(code = "threads.engine_exited", exit = ?code, "engine process ended");
                let stopping = guard.turn.as_ref().map(|r| r.stopping);
                match stopping {
                    None => {}
                    Some(true) => {
                        let queued = self
                            .end_turn(thread, &mut guard, TurnEndReason::Cancelled)
                            .await;
                        self.send_queued(thread, queued);
                    }
                    Some(false) => {
                        let message = match code {
                            Some(c) => format!("The engine stopped unexpectedly (exit {c})."),
                            None => "The engine stopped unexpectedly.".to_string(),
                        };
                        let hint = stderr_tail
                            .lines()
                            .rev()
                            .find(|l| !l.trim().is_empty())
                            .map(|l| l.chars().take(300).collect::<String>());
                        if let Some(run) = guard.turn.as_mut() {
                            self.close_block(thread, run, None).await;
                            self.emit(
                                thread,
                                run,
                                ThreadEvent::Error {
                                    code: "engine_exited".into(),
                                    message,
                                    stage: ErrorStage::Unknown,
                                    hint,
                                    action: Some(ErrorAction::Retry),
                                },
                            )
                            .await;
                        }
                        let queued = self
                            .end_turn(thread, &mut guard, TurnEndReason::Error)
                            .await;
                        self.send_queued(thread, queued);
                    }
                }
            }
            AdapterEvent::Approval {
                turn_id,
                id: asked,
                kind,
                summary,
                detail,
                subject,
            } => {
                let Some(session) = guard
                    .session
                    .clone()
                    .filter(|_| generation == guard.generation)
                else {
                    return;
                };
                let current = guard.turn.as_ref().is_some_and(|r| r.id == turn_id);
                drop(guard);
                let svc = self.clone();
                let thread = thread.clone();
                // The card waits for the user; the event stream doesn't.
                tauri::async_runtime::spawn(async move {
                    let outcome = if current {
                        svc.ask_card(&thread, kind, summary, detail, subject).await
                    } else {
                        ApprovalOutcome::Cancelled
                    };
                    let decision = match outcome {
                        ApprovalOutcome::Allowed { scope, .. } => {
                            tandem_core::approvals::ApprovalDecision::Allow { scope }
                        }
                        ApprovalOutcome::Denied { reason, .. } => {
                            tandem_core::approvals::ApprovalDecision::Deny { reason }
                        }
                        ApprovalOutcome::Expired | ApprovalOutcome::Cancelled => {
                            tandem_core::approvals::ApprovalDecision::Deny { reason: None }
                        }
                    };
                    if let Err(e) = session.resolve_approval(&asked, decision).await {
                        tracing::warn!(code = "threads.approval_reply_failed", error = %e, "couldn't answer the engine");
                    }
                });
            }
            AdapterEvent::Event { turn_id, event } => {
                // F-TH-03 AC3: a finished turn's events change nothing.
                let Some(run) = guard.turn.as_mut().filter(|r| r.id == turn_id) else {
                    return;
                };
                run.last_output = self.clock.now_ms();
                run.silence_noted = false;
                if let Some((kind, text)) = delta_parts(&event) {
                    if text.is_empty() {
                        return;
                    }
                    if run.open.as_ref().map(|b| b.kind) != Some(kind) {
                        self.close_block(thread, run, None).await;
                        let seq = run.take_seq();
                        run.open = Some(Block {
                            kind,
                            seq,
                            text: String::new(),
                            row: None,
                            dirty: false,
                        });
                    }
                    if let Some(block) = run.open.as_mut() {
                        block.text.push_str(text);
                        block.dirty = true;
                        let env = self.envelope(thread, &turn_id, block.seq, event);
                        self.broadcast(env);
                    }
                    return;
                }
                match event {
                    ThreadEvent::Assistant { .. }
                        if run.open.as_ref().map(|b| b.kind) == Some(BlockKind::Assistant) =>
                    {
                        self.close_block(thread, run, Some(event)).await;
                    }
                    ThreadEvent::Reasoning { .. }
                        if run.open.as_ref().map(|b| b.kind) == Some(BlockKind::Reasoning) =>
                    {
                        self.close_block(thread, run, Some(event)).await;
                    }
                    ThreadEvent::TurnEnd { reason, .. } => {
                        let queued = self.end_turn(thread, &mut guard, reason).await;
                        self.send_queued(thread, queued);
                    }
                    other => {
                        self.close_block(thread, run, None).await;
                        if let ThreadEvent::Tool {
                            status: ToolStatus::Denied,
                            ..
                        } = other
                        {
                            run.blocked += 1;
                        }
                        // After a sign-in, the next turn starts a new session
                        // with the new credentials (F-TH-06).
                        let signed_out = matches!(
                            other,
                            ThreadEvent::Error {
                                stage: ErrorStage::Auth,
                                ..
                            }
                        );
                        self.emit(thread, run, other).await;
                        if signed_out {
                            guard.stale = true;
                        }
                    }
                }
            }
        }
    }

    /// `thread_stop` (F-TH-03 AC2).
    pub async fn stop(self: &Arc<Self>, id: &ThreadId) -> Result<InterruptOutcome, IpcError> {
        let rt = self.runtime(id);
        let (session, turn) = {
            let mut guard = rt.lock().await;
            let session = guard.session.clone();
            let Some(run) = guard.turn.as_mut() else {
                return Ok(InterruptOutcome {
                    acked: false,
                    killed: false,
                });
            };
            run.stopping = true;
            (session, run.id.clone())
        };
        tracing::info!(code = "threads.stop", "stopping a turn");
        // Outside the lock: the adapter may wait up to 3.5 s.
        let outcome = match &session {
            Some(s) => s.interrupt(&turn).await.unwrap_or(InterruptOutcome {
                acked: false,
                killed: true,
            }),
            None => InterruptOutcome {
                acked: false,
                killed: false,
            },
        };
        let mut guard = rt.lock().await;
        let current = guard.turn.as_ref().map(|r| &r.id) == Some(&turn);
        if outcome.killed {
            // The session is gone; its last events are no longer read.
            guard.session = None;
            guard.generation += 1;
        }
        if current && (outcome.killed || session.is_none()) {
            let queued = self
                .end_turn(id, &mut guard, TurnEndReason::Cancelled)
                .await;
            self.send_queued(id, queued);
        } else if current {
            // Acknowledged: the engine ends the turn; if it doesn't, we do.
            let svc = self.clone();
            let id = id.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(ACKED_END).await;
                let rt = svc.runtime(&id);
                let mut guard = rt.lock().await;
                if guard.turn.as_ref().map(|r| &r.id) == Some(&turn) {
                    let queued = svc
                        .end_turn(&id, &mut guard, TurnEndReason::Cancelled)
                        .await;
                    svc.send_queued(&id, queued);
                }
            });
        }
        Ok(outcome)
    }

    /// A tool use Claude asks about through the gateway (F-TH-04 AC4).
    pub async fn ask_approval(&self, id: &ThreadId, ask: ToolApproval) -> ApprovalAnswer {
        let Ok(thread) = self.thread(id).await else {
            return ApprovalAnswer::Deny { message: None };
        };
        let (kind, summary, detail, subject) = tandem_engines::chat::claude::approval_card(
            &ask.tool_name,
            &ask.input,
            std::path::Path::new(&thread.cwd),
        );
        match self.ask_card(id, kind, summary, detail, subject).await {
            ApprovalOutcome::Allowed { .. } => ApprovalAnswer::Allow,
            ApprovalOutcome::Denied { reason, .. } => ApprovalAnswer::Deny { message: reason },
            ApprovalOutcome::Expired => ApprovalAnswer::Deny {
                message: Some("Nobody answered within 30 minutes".into()),
            },
            ApprovalOutcome::Cancelled => ApprovalAnswer::Deny { message: None },
        }
    }

    /// The approval card (F-TH-11): Waiting first, then the card in the
    /// running turn and on the Dashboard, until someone answers or it
    /// expires; the answer is recorded in the turn. No running turn: nobody
    /// could answer, so it's cancelled.
    async fn ask_card(
        &self,
        id: &ThreadId,
        kind: ApprovalKind,
        summary: String,
        detail: String,
        subject: ApprovalSubject,
    ) -> ApprovalOutcome {
        // "Allow for this thread" answers without a card, for a user who is
        // there; an unattended session always waits (§6.10).
        let attended = self.thread(id).await.map(|t| t.attended).unwrap_or(false);
        let rules = self.allow_rules(id).await;
        if attended && allow_rules::allowed(&rules, &subject) {
            tracing::info!(code = "threads.approval_by_rule", kind = ?kind, "allowed by a thread rule");
            return ApprovalOutcome::Allowed {
                scope: ApprovalScope::Thread,
                via: tandem_core::approvals::ApprovalVia::Rule,
            };
        }
        let outcome = self
            .ask_user(
                id,
                kind,
                summary,
                detail,
                vec![ApprovalScope::Once, ApprovalScope::Thread],
            )
            .await;
        if let ApprovalOutcome::Allowed {
            scope: ApprovalScope::Thread,
            ..
        } = outcome
        {
            if let Some(rule) = AllowRule::for_subject(&subject) {
                let rules = allow_rules::with(rules, rule);
                if let Ok(json) = serde_json::to_string(&rules) {
                    if let Err(e) = repo::set_allow_rules(&self.store, id, json).await {
                        tracing::warn!(code = "threads.rule_failed", error = %e, "couldn't save the thread's allow rule");
                    }
                }
            }
        }
        outcome
    }

    async fn allow_rules(&self, id: &ThreadId) -> Vec<AllowRule> {
        repo::allow_rules(&self.store, id)
            .await
            .ok()
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default()
    }

    /// A plugin approval (F-PL-07): the card in the thread with the scopes
    /// the gateway offers ("Allow once", maybe "for this conversation").
    pub async fn ask_with_options(
        &self,
        id: &ThreadId,
        kind: ApprovalKind,
        summary: String,
        detail: String,
        options: Vec<ApprovalScope>,
    ) -> ApprovalOutcome {
        self.ask_user(id, kind, summary, detail, options).await
    }

    /// The card itself.
    async fn ask_user(
        &self,
        id: &ThreadId,
        kind: ApprovalKind,
        summary: String,
        detail: String,
        options: Vec<ApprovalScope>,
    ) -> ApprovalOutcome {
        let rt = self.runtime(id);
        let (view, ticket, turn) = {
            let mut guard = rt.lock().await;
            if guard.turn.is_none() {
                return ApprovalOutcome::Cancelled;
            }
            // Waiting before the card shows anywhere.
            self.set_state(id, ThreadState::Waiting).await;
            let Some(run) = guard.turn.as_mut() else {
                return ApprovalOutcome::Cancelled;
            };
            let (view, ticket) = self.approvals.request(ApprovalRequest {
                source: SourceRef::Thread {
                    thread_id: id.as_str().to_string(),
                },
                kind,
                summary,
                detail,
                options,
            });
            self.emit(
                id,
                run,
                ThreadEvent::ApprovalRequest {
                    id: view.id.clone(),
                    kind: view.kind,
                    summary: view.summary.clone(),
                    detail: view.detail.clone(),
                    options: view.options.clone(),
                    expires_at: view.expires_at,
                },
            )
            .await;
            (view, ticket, run.id.clone())
        };
        tracing::info!(code = "threads.approval_asked", kind = ?view.kind, "approval requested");
        let outcome = ticket.outcome().await;
        let decision = match &outcome {
            ApprovalOutcome::Allowed { scope, .. } => ApprovalDecisionView::Allow { scope: *scope },
            ApprovalOutcome::Denied { reason, .. } => ApprovalDecisionView::Deny {
                reason: reason.clone(),
            },
            ApprovalOutcome::Expired => ApprovalDecisionView::Expired,
            ApprovalOutcome::Cancelled => ApprovalDecisionView::Deny { reason: None },
        };
        let mut guard = rt.lock().await;
        if let Some(run) = guard.turn.as_mut().filter(|r| r.id == turn) {
            self.emit(
                id,
                run,
                ThreadEvent::ApprovalResolved {
                    id: view.id.clone(),
                    decision,
                },
            )
            .await;
            drop(guard);
            self.set_state(id, ThreadState::Working).await;
        }
        outcome
    }

    /// `thread_retry`: sends the last failed or interrupted turn's message
    /// again.
    pub async fn retry(self: &Arc<Self>, id: &ThreadId) -> Result<SendResult, IpcError> {
        let turns = repo::turns(&self.store, id).await.map_err(store_err)?;
        let last = turns
            .last()
            .filter(|t| matches!(t.state, TurnState::Error | TurnState::Interrupted))
            .ok_or_else(|| IpcError::new(ErrorCode::Invalid, "Nothing to retry"))?;
        let rows = repo::turn_messages(&self.store, &last.id)
            .await
            .map_err(store_err)?;
        let (text, attachments) = rows
            .iter()
            .filter_map(envelope)
            .find_map(|e| match e.event {
                ThreadEvent::User { text, attachments } => Some((text, attachments)),
                _ => None,
            })
            .ok_or_else(|| IpcError::new(ErrorCode::Invalid, "Nothing to retry"))?;
        self.send(id, text, attachments, false).await
    }
}

/// The one place a session's access is decided (F-TH-09): the thread's
/// level and mode for its origin, then held to the Team org's maximum
/// (F-BL-06). An interactive Full session is reported to the org's audit
/// log when the org turned it on (F-TL-10).
fn session_access(
    guard: Option<&dyn crate::services::audit::OrgGuard>,
    thread: &ThreadRecord,
    origin: tandem_core::access::Origin,
    engine_id: &str,
) -> tandem_core::access::Access {
    use tandem_core::access::Origin;
    let resolved = tandem_core::access::resolve(thread.access, thread.mode, origin);
    let policy = guard.and_then(|g| g.policy());
    let access = tandem_core::policy::clamp_access(policy.as_ref(), resolved);
    if access == Access::Full && matches!(origin, Origin::Interactive) {
        if let Some(g) = guard {
            g.audit(
                crate::services::audit::FULL_ACCESS_USED,
                &[("engine", engine_id), ("surface", "thread")],
            );
        }
    }
    access
}

/// Who started a thread's runs. Routines and tasks pass their own explicit
/// Full grant when they arrive (M13, M15); until then unattended runs grant
/// nothing.
fn origin_of(thread: &ThreadRecord) -> tandem_core::access::Origin {
    use tandem_core::access::Origin;
    match (thread.attended, thread.kind) {
        (true, _) => Origin::Interactive,
        (false, ThreadKind::TeammateChat) => Origin::Mail,
        (false, ThreadKind::Thread) => Origin::Routine { grants_full: false },
    }
}

/// The chat adapters Thread mode has (ARCHITECTURE §6.8); ACP arrives
/// with M7 task 10.
pub const CHAT_ADAPTERS: &[&str] = &["claude", "codex", "acp"];

/// An engine Thread mode can run.
pub fn has_chat_adapter(spec: &tandem_engines::registry::EngineSpec) -> bool {
    spec.chat_adapter
        .as_deref()
        .is_some_and(|a| CHAT_ADAPTERS.contains(&a))
}

/// The app's launcher: the thread's engine and account, resolved like a
/// pane's (provider home, sanitized login env), run by that engine's adapter,
/// with the gateway (started on first use) as its only MCP server.
pub struct AppLauncher {
    terminals: Arc<crate::services::terminal::TerminalService>,
    config: Arc<crate::services::config::ConfigService>,
    models: Arc<crate::services::models::RuntimeModels>,
    adapters: HashMap<String, Arc<dyn ChatAdapter>>,
    /// `$DATA/mcp`: one 0600 config per thread session.
    mcp_dir: std::path::PathBuf,
    gateway: Arc<crate::services::gateway::GatewayService>,
    store: Store,
    /// The live lease per thread; a new session revokes the last one.
    leases: StdMutex<HashMap<String, tandem_core::ids::LeaseId>>,
    /// Each remote thread session's MCP channels (D-1604), by thread.
    remote_mcp: StdMutex<HashMap<String, crate::services::remote_mcp::RemoteMcp>>,
    teammates: std::sync::OnceLock<std::sync::Weak<crate::services::teammates::TeammatesService>>,
    /// The tasks server sessions get (F-TK-02).
    tasks: std::sync::OnceLock<Arc<crate::services::tasks::TasksService>>,
    /// The Team org's policy and audit log (F-BL-06, F-TL-10).
    guard: std::sync::OnceLock<Arc<dyn crate::services::audit::OrgGuard>>,
}

impl AppLauncher {
    /// The SSH host a thread's folder is on, when it's a remote workspace.
    async fn remote_host(
        &self,
        thread: &ThreadRecord,
    ) -> Result<
        Option<(
            Arc<crate::services::remote::RemoteService>,
            tandem_core::ids::HostId,
        )>,
        IpcError,
    > {
        let Some(ws) = &thread.workspace_id else {
            return Ok(None);
        };
        let ws = tandem_store::repo::workspaces::get(self.terminals.store(), ws)
            .await
            .map_err(store_err)?;
        if ws.kind != "ssh" {
            return Ok(None);
        }
        let remote = self.terminals.remote().cloned().ok_or_else(|| {
            IpcError::new(
                ErrorCode::Unavailable,
                "Remote hosts aren't available in this build",
            )
        })?;
        Ok(Some((
            remote,
            tandem_core::ids::HostId::from_db(ws.host_id),
        )))
    }

    /// A thread on an SSH host (F-RM-01): the host's own engine, run
    /// through `tandem-host run` over the host's connection (D-403), with
    /// the host's own sign-in. Nothing local crosses: no provider home, no
    /// account, no gateway lease or port (D-408); plugins and task tools
    /// reach it as stdio servers relayed over the connection (D-1604).
    async fn prepare_remote(
        &self,
        thread: &ThreadRecord,
        engine: &tandem_engines::EngineSpec,
        adapter: Arc<dyn ChatAdapter>,
        remote: Arc<crate::services::remote::RemoteService>,
        host: tandem_core::ids::HostId,
    ) -> Result<(Arc<dyn ChatAdapter>, ChatStart), IpcError> {
        // The org's engine policy holds on hosts too (F-BL-06).
        self.terminals.engines.check_allowed(engine)?;
        // The connection first: its detection names the host's engines.
        remote.client(&host).await?;
        let mut report = remote.report(&host).await;
        if report.as_ref().and_then(|r| r.get(&engine.id)).is_none() {
            let list = self.terminals.engines.registry().all().to_vec();
            report = remote.detect(&host, &list).await.ok();
        }
        let found = report
            .as_ref()
            .and_then(|r| r.get(&engine.id))
            .cloned()
            .ok_or_else(|| {
                IpcError::new(
                    ErrorCode::EngineMissing,
                    format!("{} isn't installed on this host.", engine.label),
                )
            })?;
        let (ssh, target) = remote.transport(&host).await?;
        let access = session_access(
            self.guard.get().map(|g| g.as_ref()),
            thread,
            origin_of(thread),
            &engine.id,
        );
        // A teammate's briefing (F-AG-06): Codex and ACP agents get its text
        // from here; Claude reads a file, so a copy goes to the host over
        // stdin (never argv) and the relocated command names that copy.
        let briefing = self
            .briefing_file(thread, std::path::Path::new(&thread.cwd), access)
            .await?;
        let remote_briefing = match &briefing {
            Some(local) => Some(copy_briefing_to_host(&ssh, &target, &thread.id, local).await?),
            None => None,
        };
        let stdio_servers = self.remote_tools(thread, &remote, &host).await;
        let cwd = thread.cwd.clone();
        let local_env: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| {
                matches!(
                    k.as_str(),
                    "HOME"
                        | "USER"
                        | "LOGNAME"
                        | "PATH"
                        | "SSH_AUTH_SOCK"
                        | "LANG"
                        | "LC_ALL"
                        | "TMPDIR"
                        | "XDG_RUNTIME_DIR"
                )
            })
            .collect();
        let here = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let relocate = tandem_engines::chat::Relocate(Arc::new(
            move |spec: tandem_engines::chat::process::ProcessSpec| {
                let spec = swap_briefing_path(spec, remote_briefing.as_deref());
                let mut argv = vec![spec.program.to_string_lossy().into_owned()];
                argv.extend(spec.args.iter().map(|a| a.to_string_lossy().into_owned()));
                let env: std::collections::BTreeMap<String, String> =
                    spec.env.iter().cloned().collect();
                let (program, args) = ssh.run_args(&target, &cwd, &env, &argv);
                tandem_engines::chat::process::ProcessSpec {
                    program,
                    args: args.into_iter().map(std::ffi::OsString::from).collect(),
                    env: local_env.clone(),
                    cwd: here.clone(),
                    ..spec
                }
            },
        ));
        Ok((
            adapter,
            ChatStart {
                thread_id: thread.id.clone(),
                engine_id: engine.id.clone(),
                program: std::path::PathBuf::from(&found.path),
                cwd: std::path::PathBuf::from(&thread.cwd),
                env: vec![("TERM".into(), "dumb".into())],
                model: thread.model.clone(),
                effort: thread.effort.clone(),
                access,
                mode: thread.mode,
                attended: thread.attended,
                resume_session: thread.engine_session_id.clone(),
                mcp_config: None,
                mcp: None,
                // Relayed to this machine's gateway and tasks server (D-1604).
                stdio_servers,
                engine_version: found.version,
                relocate: Some(relocate),
                briefing,
            },
        ))
    }

    /// The WSL distro a thread's folder is in, when it's a WSL workspace.
    async fn wsl_host(
        &self,
        thread: &ThreadRecord,
    ) -> Result<Option<(tandem_core::ids::HostId, String)>, IpcError> {
        let Some(ws) = &thread.workspace_id else {
            return Ok(None);
        };
        let ws = tandem_store::repo::workspaces::get(self.terminals.store(), ws)
            .await
            .map_err(store_err)?;
        if ws.kind != "wsl" {
            return Ok(None);
        }
        let distro = ws.wsl_distro.ok_or_else(|| {
            IpcError::new(ErrorCode::Invalid, "This WSL workspace has no distribution")
        })?;
        Ok(Some((
            tandem_core::ids::HostId::from_db(ws.host_id),
            distro,
        )))
    }

    /// A thread in a WSL workspace (F-RM-02): the distro's own engine, run
    /// through `wsl.exe` with the distro's own sign-in. As on SSH hosts,
    /// nothing local crosses: no provider home, no account, no gateway.
    async fn prepare_wsl(
        &self,
        thread: &ThreadRecord,
        engine: &tandem_engines::EngineSpec,
        adapter: Arc<dyn ChatAdapter>,
        host: tandem_core::ids::HostId,
        distro: String,
    ) -> Result<(Arc<dyn ChatAdapter>, ChatStart), IpcError> {
        self.terminals.engines.check_allowed(engine)?;
        let wsl = self
            .terminals
            .wsl()
            .cloned()
            .filter(|_| crate::services::wsl::system().is_some())
            .ok_or_else(|| {
                IpcError::new(ErrorCode::Unavailable, "WSL folders open on Windows only")
                    .with_stage("wsl")
            })?;
        let remote = self.terminals.remote().cloned();
        let mut report = match &remote {
            Some(r) => r.report(&host).await,
            None => None,
        };
        if report.as_ref().and_then(|r| r.get(&engine.id)).is_none() {
            let list = self.terminals.engines.registry().all().to_vec();
            let fresh = wsl.detect_report(&host, &distro, &list).await?;
            if let Some(r) = &remote {
                r.publish_report(&host, &fresh).await?;
            }
            report = Some(fresh);
        }
        let found = report
            .as_ref()
            .and_then(|r| r.get(&engine.id))
            .cloned()
            .ok_or_else(|| {
                IpcError::new(
                    ErrorCode::EngineMissing,
                    format!("{} isn't installed in {distro}.", engine.label),
                )
            })?;
        let access = session_access(
            self.guard.get().map(|g| g.as_ref()),
            thread,
            origin_of(thread),
            &engine.id,
        );
        // Claude reads a teammate's briefing from a file: the distro reaches
        // this machine's copy at its /mnt path.
        let briefing = self
            .briefing_file(thread, std::path::Path::new(&thread.cwd), access)
            .await?;
        let wsl_briefing = match &briefing {
            Some(local) => Some(
                tandem_core::wslpath::to_wsl_path(&local.to_string_lossy()).ok_or_else(|| {
                    IpcError::new(ErrorCode::Io, "The briefing isn't reachable from WSL")
                        .with_stage("briefing")
                })?,
            ),
            None => None,
        };
        let cwd = thread.cwd.clone();
        let relocate = tandem_engines::chat::Relocate(Arc::new(
            move |spec: tandem_engines::chat::process::ProcessSpec| {
                let spec = swap_briefing_path(spec, wsl_briefing.as_deref());
                crate::services::wsl::thread_spec(&distro, &cwd, spec, std::env::vars())
            },
        ));
        Ok((
            adapter,
            ChatStart {
                thread_id: thread.id.clone(),
                engine_id: engine.id.clone(),
                program: std::path::PathBuf::from(&found.path),
                cwd: std::path::PathBuf::from(&thread.cwd),
                env: vec![("TERM".into(), "dumb".into())],
                model: thread.model.clone(),
                effort: thread.effort.clone(),
                access,
                mode: thread.mode,
                attended: thread.attended,
                resume_session: thread.engine_session_id.clone(),
                mcp_config: None,
                mcp: None,
                // No local stdio servers on a remote or WSL engine: the tasks
                // server runs on this machine (F-TK-02).
                stdio_servers: Vec::new(),
                engine_version: found.version,
                relocate: Some(relocate),
                briefing,
            },
        ))
    }

    pub fn new(
        terminals: Arc<crate::services::terminal::TerminalService>,
        config: Arc<crate::services::config::ConfigService>,
        models: Arc<crate::services::models::RuntimeModels>,
        adapters: Vec<Arc<dyn ChatAdapter>>,
        mcp_dir: std::path::PathBuf,
        gateway: Arc<crate::services::gateway::GatewayService>,
        store: Store,
    ) -> Self {
        Self {
            terminals,
            config,
            models,
            adapters: adapters
                .into_iter()
                .map(|a| (a.engine_id().to_string(), a))
                .collect(),
            mcp_dir,
            gateway,
            store,
            leases: StdMutex::default(),
            remote_mcp: StdMutex::default(),
            teammates: std::sync::OnceLock::new(),
            tasks: std::sync::OnceLock::new(),
            guard: std::sync::OnceLock::new(),
        }
    }

    /// Who started the thread's runs: a routine's chat runs with that
    /// routine's own Full grant, if any (F-RT-01, rule 4).
    async fn origin(&self, thread: &ThreadRecord) -> tandem_core::access::Origin {
        if !thread.attended {
            // A task run's session: the task's own Full grant, if any.
            if let Ok(Some(a)) =
                tandem_store::repo::task_attempts::for_thread(&self.store, &thread.id).await
            {
                return tandem_core::access::Origin::Task {
                    grants_full: a.access == Access::Full,
                };
            }
            if let Ok(Some(r)) =
                tandem_store::repo::routines::routine_of_chat(&self.store, &thread.id).await
            {
                return tandem_core::access::Origin::Routine {
                    grants_full: r.access == Access::Full,
                };
            }
        }
        origin_of(thread)
    }

    /// The tracker whose MCP server chat sessions get.
    pub fn bind_tasks(&self, tasks: &Arc<crate::services::tasks::TasksService>) {
        let _ = self.tasks.set(tasks.clone());
    }

    /// The tasks server for a thread's new session, with its own connection.
    async fn tasks_server(
        &self,
        thread: &ThreadRecord,
    ) -> Option<tandem_engines::chat::StdioServer> {
        let tasks = self.tasks.get()?;
        let teammate = self.teammate_of(thread).await;
        let (label, actor) = match &teammate {
            Some(t) => (t.name.clone(), t.id.as_str().to_string()),
            None => (
                if thread.title.is_empty() {
                    "Thread".to_string()
                } else {
                    thread.title.clone()
                },
                thread.id.as_str().to_string(),
            ),
        };
        tasks.chat_server(&session_key(&thread.id), &label, &actor)
    }

    /// The teammates whose chats this launcher starts.
    pub fn bind_teammates(&self, teammates: &Arc<crate::services::teammates::TeammatesService>) {
        let _ = self.teammates.set(Arc::downgrade(teammates));
    }

    /// A teammate chat's briefing, written for this session to
    /// `$DATA/briefings/<thread>.md` (0600; F-AG-06). Threads have none.
    async fn briefing_file(
        &self,
        thread: &ThreadRecord,
        cwd: &std::path::Path,
        access: tandem_core::access::Access,
    ) -> Result<Option<std::path::PathBuf>, IpcError> {
        let (Some(teammate), Some(service)) = (
            self.teammate_of(thread).await,
            self.teammates.get().and_then(std::sync::Weak::upgrade),
        ) else {
            return Ok(None);
        };
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let briefing = service
            .briefing(&teammate, cwd, access, !thread.attended, &date)
            .await;
        let dir = service.data().briefings();
        tandem_engines::briefing::write_file(&dir, thread.id.as_str(), &briefing)
            .map(Some)
            .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()).with_stage("briefing"))
    }

    /// A teammate chat's teammate, if the thread is one.
    async fn teammate_of(
        &self,
        thread: &ThreadRecord,
    ) -> Option<tandem_store::repo::teammates::Teammate> {
        let id = thread.teammate_id.as_ref()?;
        tandem_store::repo::teammates::get(&self.store, id)
            .await
            .ok()
            .flatten()
    }

    /// The org policy and audit log every session start consults.
    pub fn set_guard(&self, guard: Arc<dyn crate::services::audit::OrgGuard>) {
        let _ = self.guard.set(guard);
    }

    /// The engines Thread mode supports now.
    pub fn adapters(
        terminals: &crate::services::terminal::TerminalService,
    ) -> Vec<Arc<dyn ChatAdapter>> {
        let claude_limits = terminals
            .engines
            .get("claude")
            .map(|e| e.limit_patterns)
            .unwrap_or_default();
        let codex_limits = terminals
            .engines
            .get("codex")
            .map(|e| e.limit_patterns)
            .unwrap_or_default();
        let mut adapters: Vec<Arc<dyn ChatAdapter>> = vec![
            Arc::new(tandem_engines::chat::claude::ClaudeAdapter::new(
                claude_limits,
            )),
            Arc::new(tandem_engines::chat::codex::CodexAdapter::new(codex_limits)),
        ];
        // Every engine that speaks the Agent Client Protocol (F-TH-06).
        let registry = terminals.engines.registry();
        for engine in registry.all() {
            if let (Some("acp"), Some(cmd)) = (engine.chat_adapter.as_deref(), &engine.acp_cmd) {
                adapters.push(Arc::new(tandem_engines::chat::acp::AcpAdapter::new(
                    &engine.id,
                    &engine.label,
                    cmd,
                    engine.limit_patterns.clone(),
                )));
            }
        }
        adapters
    }

    /// The plugin connections a thread's session may use: its own grants
    /// and those given to every thread (F-PL-05).
    async fn thread_grants(&self, thread: &ThreadRecord) -> Vec<tandem_core::ids::ConnectionId> {
        use tandem_store::repo::plugins::{grants, GrantSubject};
        let mut out = grants(
            &self.store,
            &GrantSubject::Thread {
                thread_id: thread.id.as_str().to_string(),
            },
        )
        .await
        .unwrap_or_default();
        // Plus "all threads" and the thread's workspace (F-PL-05).
        let mut more = grants(&self.store, &GrantSubject::AllThreads)
            .await
            .unwrap_or_default();
        if let Some(ws) = &thread.workspace_id {
            more.extend(
                grants(
                    &self.store,
                    &GrantSubject::Workspace {
                        workspace_id: ws.as_str().to_string(),
                    },
                )
                .await
                .unwrap_or_default(),
            );
        }
        for c in more {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }

    /// A lease for the thread's new session, in a config only the user can
    /// read (`$DATA/mcp/<thread>.json`, 0600).
    async fn mcp_config(
        &self,
        thread: &ThreadRecord,
        stdio: &[tandem_engines::chat::StdioServer],
    ) -> Result<(std::path::PathBuf, tandem_engines::chat::McpEndpoint), IpcError> {
        let (gateway, lease) = self.lease(thread).await?;
        let mut config = serde_json::json!({"mcpServers": {"tandem": {
            "type": "http",
            "url": gateway.url,
            "headers": {"Authorization": format!("Bearer {}", lease.token)}
        }}});
        for s in stdio {
            config["mcpServers"][&s.name] = serde_json::json!({
                "type": "stdio",
                "command": s.command,
                "args": s.args,
            });
        }
        std::fs::create_dir_all(&self.mcp_dir)
            .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        let path = self.mcp_dir.join(format!("{}.json", thread.id.as_str()));
        tandem_core::atomic::atomic_write(&path, config.to_string().as_bytes(), Some(0o600))
            .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        Ok((
            path,
            tandem_engines::chat::McpEndpoint {
                url: gateway.url.clone(),
                token: lease.token,
            },
        ))
    }

    /// A gateway lease for the thread's new session (its previous one is
    /// revoked): its caller, access, built-ins and plugin grants.
    async fn lease(
        &self,
        thread: &ThreadRecord,
    ) -> Result<(tandem_gateway::GatewayHandle, tandem_gateway::Lease), IpcError> {
        let gateway = self.gateway.handle().await?;
        let teammate = self.teammate_of(thread).await;
        let caller = match &teammate {
            Some(t) => Caller::TeammateChat {
                teammate_id: t.id.clone(),
                thread_id: thread.id.clone(),
            },
            None => Caller::Thread {
                thread_id: thread.id.clone(),
            },
        };
        let mut session = tandem_gateway::SessionDescriptor::new(
            caller,
            if thread.attended {
                tandem_gateway::Attended::Interactive
            } else {
                // Who started the run (a routine, task, mail or Auto) names
                // it, so what it holds for review says where it came from.
                tandem_gateway::Attended::Unattended {
                    origin: self
                        .gateway
                        .unattended_origin(&thread.id)
                        .unwrap_or_else(|| "thread".into()),
                }
            },
            thread.access,
            // Claude asks through the gateway; Codex and ACP agents ask in-band.
            if thread.engine_id == "claude" {
                tandem_gateway::BuiltinSet::for_thread()
            } else {
                tandem_gateway::BuiltinSet::default()
            },
        );
        match &teammate {
            Some(t) => {
                // A teammate chat: memory, skills, search and (when it may
                // message) mail; the teammate's own plugin grants (F-PL-05).
                let approvals = session.builtins.approvals;
                session.builtins = tandem_gateway::BuiltinSet::for_teammate(
                    t.settings.may_message,
                    t.settings.may_schedule,
                );
                session.builtins.approvals = approvals;
                session.granted = tandem_store::repo::plugins::grants(
                    &self.store,
                    &tandem_store::repo::plugins::GrantSubject::Teammate {
                        teammate_id: t.id.as_str().to_string(),
                    },
                )
                .await
                .unwrap_or_default();
            }
            None => session.granted = self.thread_grants(thread).await,
        }
        if let Some(r) = &thread.conversation_ref {
            session.conversation_ref = r.clone();
        }
        let lease = gateway.prepare(session);
        let previous = match self.leases.lock() {
            Ok(mut l) => l.insert(thread.id.as_str().to_string(), lease.id.clone()),
            Err(_) => None,
        };
        if let Some(old) = previous {
            gateway.revoke(&old);
        }
        Ok((gateway.clone(), lease))
    }

    /// Plugins and task tools for a session on a host (D-1604): a channel
    /// for the gateway, bound here to the session's lease, and one for the
    /// tasks server, which runs here. The engine there gets two stdio
    /// servers, `tandem-host mcp --channel <id>`; nothing else crosses. A
    /// channel that can't open leaves the thread without those tools.
    async fn remote_tools(
        &self,
        thread: &ThreadRecord,
        remote: &Arc<crate::services::remote::RemoteService>,
        host: &tandem_core::ids::HostId,
    ) -> Vec<tandem_engines::chat::StdioServer> {
        use crate::services::remote_mcp::{GatewayBackend, ProcessBackend, RemoteMcp};
        let mut mcp = RemoteMcp::new(remote.clone());
        let mut servers = Vec::new();
        match self.lease(thread).await {
            Ok((gateway, lease)) => {
                match mcp
                    .add(host, "tandem", Arc::new(GatewayBackend { gateway, lease }))
                    .await
                {
                    Ok(s) => servers.push(s),
                    Err(e) => {
                        tracing::warn!(code = "remote.mcp_unavailable", error = %e.message, "no plugin tools for a remote thread")
                    }
                }
            }
            Err(e) => {
                tracing::info!(code = "remote.gateway_unavailable", error = %e.message, "no gateway for a remote thread")
            }
        }
        if let Some(server) = self.tasks_server(thread).await {
            let name = server.name.clone();
            match mcp
                .add(
                    host,
                    &name,
                    Arc::new(ProcessBackend {
                        server,
                        env: Vec::new(),
                    }),
                )
                .await
            {
                Ok(s) => servers.push(s),
                Err(e) => {
                    tracing::warn!(code = "remote.mcp_unavailable", error = %e.message, "no task tools for a remote thread")
                }
            }
        }
        // The thread's previous session's channels close here.
        if let Ok(mut m) = self.remote_mcp.lock() {
            m.insert(thread.id.as_str().to_string(), mcp);
        }
        servers
    }
}

#[async_trait]
impl ChatLauncher for AppLauncher {
    fn model_for(&self, thread: &ThreadRecord) -> (Option<String>, Option<String>) {
        // An ACP agent's models are known once it reports them; until then
        // the saved one goes to the agent, which says whether it's offered.
        let acp = self
            .terminals
            .engines
            .get(&thread.engine_id)
            .is_some_and(|e| e.chat_adapter.as_deref() == Some("acp"));
        if acp && self.models.get(&thread.engine_id).is_empty() {
            return (thread.model.clone(), None);
        }
        let catalog =
            crate::services::models::catalog_for(&thread.engine_id, &self.models, &self.config);
        crate::services::models::resolve(thread.model.as_deref(), &catalog)
    }

    fn session_ended(&self, thread_id: &ThreadId) {
        if let Some(tasks) = self.tasks.get() {
            tasks.end_session(&session_key(thread_id));
        }
        // A remote session's channels close: its servers on the host end.
        let ended = self
            .remote_mcp
            .lock()
            .ok()
            .and_then(|mut m| m.remove(thread_id.as_str()));
        drop(ended);
    }

    fn models_reported(&self, engine_id: &str, models: &[ModelOffer]) {
        let entries = models
            .iter()
            .map(|m| crate::services::models::ModelEntry {
                id: m.id.clone(),
                label: m.label.clone(),
                description: String::new(),
                is_default: false,
                effort_levels: Vec::new(),
                context_window: None,
                source: crate::services::models::ModelSource::Runtime,
            })
            .collect();
        self.models.set(engine_id, entries);
    }

    async fn prepare(
        &self,
        thread: &ThreadRecord,
    ) -> Result<(Arc<dyn ChatAdapter>, ChatStart), IpcError> {
        let engines = &self.terminals.engines;
        let engine = engines.get(&thread.engine_id).ok_or_else(|| {
            IpcError::new(
                ErrorCode::EngineMissing,
                "This engine isn't known to Vibezeo",
            )
        })?;
        let adapter = self.adapters.get(&engine.id).cloned().ok_or_else(|| {
            IpcError::new(
                ErrorCode::EngineUnsupported,
                format!(
                    "{} can't run in Thread mode yet. Use it in Code mode.",
                    engine.label
                ),
            )
        })?;
        if let Some((remote, host)) = self.remote_host(thread).await? {
            return self
                .prepare_remote(thread, &engine, adapter, remote, host)
                .await;
        }
        if let Some((host, distro)) = self.wsl_host(thread).await? {
            return self
                .prepare_wsl(thread, &engine, adapter, host, distro)
                .await;
        }
        let program = self
            .terminals
            .resolve_engine(&engine)
            .await
            .ok_or_else(|| {
                IpcError::new(
                    ErrorCode::EngineMissing,
                    format!("{} isn't installed on this machine.", engine.label),
                )
            })?;
        let cwd = std::path::PathBuf::from(&thread.cwd);
        let launch = engines
            .agent_launch(
                &engine,
                program.clone(),
                &cwd,
                None,
                thread.account_id.as_ref(),
            )
            .await?;
        // What a pane gets: the sanitized login env and the account's home.
        let login = self.terminals.login_env().await;
        let mut env: Vec<(String, String)> = login
            .vars
            .iter()
            .chain(launch.spec.env.iter())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        env.extend(crate::services::fake_engine_env());
        let stdio_servers: Vec<_> = self.tasks_server(thread).await.into_iter().collect();
        let (config, endpoint) = self.mcp_config(thread, &stdio_servers).await?;
        let origin = self.origin(thread).await;
        let access = session_access(
            self.guard.get().map(|g| g.as_ref()),
            thread,
            origin,
            &engine.id,
        );
        let briefing = self.briefing_file(thread, &cwd, access).await?;
        Ok((
            adapter,
            ChatStart {
                thread_id: thread.id.clone(),
                engine_id: engine.id.clone(),
                program,
                cwd,
                env,
                model: thread.model.clone(),
                effort: thread.effort.clone(),
                // The one place a session's access is decided (F-TH-09).
                access,
                mode: thread.mode,
                attended: thread.attended,
                resume_session: thread.engine_session_id.clone(),
                mcp_config: Some(config),
                mcp: Some(endpoint),
                engine_version: engines.report().and_then(|r| {
                    r.found
                        .into_iter()
                        .find(|f| f.engine_id == engine.id)
                        .and_then(|f| f.version)
                }),
                relocate: None,
                briefing,
                stdio_servers,
            },
        ))
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use tandem_core::access::Origin;
    use tandem_core::policy::OrgPolicy;

    struct Guard {
        policy: Option<OrgPolicy>,
        audits: StdMutex<Vec<(String, String)>>,
    }

    impl crate::services::audit::OrgGuard for Guard {
        fn policy(&self) -> Option<OrgPolicy> {
            self.policy.clone()
        }
        fn audit(&self, event: &str, meta: &[(&str, &str)]) {
            let engine = meta
                .iter()
                .find(|(k, _)| *k == "engine")
                .map(|(_, v)| v.to_string());
            self.audits
                .lock()
                .expect("lock")
                .push((event.to_string(), engine.unwrap_or_default()));
        }
    }

    fn thread(access: Access) -> ThreadRecord {
        ThreadRecord {
            id: ThreadId::new(),
            kind: ThreadKind::Thread,
            workspace_id: None,
            cwd: "/tmp".into(),
            engine_id: "claude".into(),
            account_id: None,
            model: None,
            effort: None,
            access,
            mode: ThreadMode::Act,
            title: String::new(),
            state: ThreadState::Idle,
            attended: true,
            engine_session_id: None,
            pinned: false,
            archived: false,
            unread: false,
            sort_key: "a".into(),
            created_at: 0,
            updated_at: 0,
            teammate_id: None,
            conversation_ref: None,
        }
    }

    fn guard(policy: Option<OrgPolicy>) -> Guard {
        Guard {
            policy,
            audits: StdMutex::default(),
        }
    }

    #[test]
    fn f_bl_06_1_max_access_edits_holds_a_full_thread_to_edits() {
        let g = guard(Some(OrgPolicy {
            max_access: Access::Edits,
            audit_enabled: true,
            ..OrgPolicy::default()
        }));
        let got = session_access(
            Some(&g),
            &thread(Access::Full),
            Origin::Interactive,
            "claude",
        );
        assert_eq!(got, Access::Edits);
        // Nothing ran with Full, so nothing is audited.
        assert!(g.audits.lock().expect("lock").is_empty());
    }

    #[test]
    fn f_tl_10_1_a_full_session_start_is_offered_to_the_audit_log() {
        let g = guard(Some(OrgPolicy::default()));
        let got = session_access(
            Some(&g),
            &thread(Access::Full),
            Origin::Interactive,
            "codex",
        );
        assert_eq!(got, Access::Full);
        assert_eq!(
            g.audits.lock().expect("lock").as_slice(),
            [("full_access_used".to_string(), "codex".to_string())]
        );
        // Edits sessions and unattended runs (never Full, D-4) aren't Full access.
        session_access(
            Some(&g),
            &thread(Access::Edits),
            Origin::Interactive,
            "codex",
        );
        session_access(
            Some(&g),
            &thread(Access::Full),
            Origin::Routine { grants_full: false },
            "codex",
        );
        assert_eq!(g.audits.lock().expect("lock").len(), 1);
        // Without a guard (no org) nothing changes.
        assert_eq!(
            session_access(None, &thread(Access::Full), Origin::Interactive, "codex"),
            Access::Full
        );
    }
}

/// Points Claude's `--append-system-prompt-file` at the briefing's copy where
/// the engine runs (an SSH host or a WSL distro). Other engines take the
/// briefing as text and have no such flag.
pub(crate) fn swap_briefing_path(
    mut spec: tandem_engines::chat::process::ProcessSpec,
    there: Option<&str>,
) -> tandem_engines::chat::process::ProcessSpec {
    let Some(there) = there else {
        return spec;
    };
    if let Some(i) = spec
        .args
        .iter()
        .position(|a| a == "--append-system-prompt-file")
    {
        if let Some(path) = spec.args.get_mut(i + 1) {
            *path = there.into();
        }
    }
    spec
}

/// Copies a teammate briefing to `~/.tandem-host/briefings/<thread>.md` on
/// the host (private to the user) through ssh's stdin, and returns its path
/// there.
async fn copy_briefing_to_host(
    ssh: &tandem_host::ssh::Ssh,
    target: &tandem_host::ssh::SshTarget,
    thread: &ThreadId,
    local: &std::path::Path,
) -> Result<String, IpcError> {
    let id = thread.as_str();
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(IpcError::new(ErrorCode::Invalid, "Unexpected thread id"));
    }
    let text = std::fs::read(local)
        .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()).with_stage("briefing"))?;
    let script = format!(
        "umask 077 && mkdir -p \"$HOME\"/.tandem-host/briefings && \
         cat > \"$HOME\"/.tandem-host/briefings/{id}.md && printf %s \"$HOME\""
    );
    let home = ssh
        .exec(target, &script, Some(&text))
        .await
        .map_err(crate::services::remote::ssh_error)?;
    Ok(format!(
        "{}/.tandem-host/briefings/{id}.md",
        home.trim_end()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tandem_core::approvals::ApprovalDecision;
    use tandem_core::clock::SystemClock;
    use tandem_core::ids::ApprovalId;
    use tandem_core::DataDir;
    use tandem_engines::chat::AdapterError;
    use tandem_engines::events::ToolVerb;

    /// What the scripted engine does with a turn.
    type Script = Arc<dyn Fn(TurnId, mpsc::Sender<AdapterEvent>) + Send + Sync>;
    /// A turn's id and event sender, kept to play more events later.
    type TurnSink = Arc<StdMutex<Option<(TurnId, mpsc::Sender<AdapterEvent>)>>>;

    #[derive(Clone, Copy, PartialEq)]
    enum OnStop {
        /// Acknowledges and ends the turn as an error (engines report an
        /// interrupted turn variously).
        Ack,
        /// Never answers: the process tree is killed.
        Ignore,
    }

    struct ScriptedAdapter {
        script: Script,
        on_stop: OnStop,
        starts: AtomicUsize,
        /// Each start's `resume_session`.
        resumed: StdMutex<Vec<Option<String>>>,
        /// Every in-band approval answer.
        answers: Arc<StdMutex<Vec<(ApprovalId, ApprovalDecision)>>>,
        /// Every turn's input, in order.
        inputs: Arc<StdMutex<Vec<TurnInput>>>,
    }

    struct ScriptedSession {
        script: Script,
        on_stop: OnStop,
        events: mpsc::Sender<AdapterEvent>,
        answers: Arc<StdMutex<Vec<(ApprovalId, ApprovalDecision)>>>,
        inputs: Arc<StdMutex<Vec<TurnInput>>>,
    }

    #[async_trait]
    impl ChatAdapter for ScriptedAdapter {
        fn engine_id(&self) -> &str {
            "scripted"
        }
        async fn start(
            &self,
            _req: ChatStart,
            events: mpsc::Sender<AdapterEvent>,
        ) -> Result<Box<dyn ChatSession>, AdapterError> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            if let Ok(mut r) = self.resumed.lock() {
                r.push(_req.resume_session.clone());
            }
            Ok(Box::new(ScriptedSession {
                script: self.script.clone(),
                on_stop: self.on_stop,
                events,
                answers: self.answers.clone(),
                inputs: self.inputs.clone(),
            }))
        }
    }

    #[async_trait]
    impl ChatSession for ScriptedSession {
        fn engine_session_id(&self) -> Option<String> {
            None
        }
        async fn send_turn(&self, turn: TurnInput) -> Result<TurnId, AdapterError> {
            (self.script)(turn.turn_id.clone(), self.events.clone());
            if let Ok(mut i) = self.inputs.lock() {
                i.push(turn.clone());
            }
            Ok(turn.turn_id)
        }
        async fn interrupt(&self, turn: &TurnId) -> Result<InterruptOutcome, AdapterError> {
            match self.on_stop {
                OnStop::Ack => {
                    let _ = self
                        .events
                        .send(AdapterEvent::Event {
                            turn_id: turn.clone(),
                            event: ThreadEvent::TurnEnd {
                                reason: TurnEndReason::Error,
                                blocked_actions: 0,
                            },
                        })
                        .await;
                    Ok(InterruptOutcome {
                        acked: true,
                        killed: false,
                    })
                }
                OnStop::Ignore => Ok(InterruptOutcome {
                    acked: false,
                    killed: true,
                }),
            }
        }
        async fn resolve_approval(
            &self,
            id: &ApprovalId,
            d: ApprovalDecision,
        ) -> Result<(), AdapterError> {
            if let Ok(mut a) = self.answers.lock() {
                a.push((id.clone(), d));
            }
            Ok(())
        }
        async fn set_model(
            &self,
            _m: Option<String>,
            _e: Option<String>,
        ) -> Result<(), AdapterError> {
            Ok(())
        }
        async fn shutdown(&self) -> Result<(), AdapterError> {
            Ok(())
        }
    }

    struct ScriptedLauncher(Arc<ScriptedAdapter>);

    #[async_trait]
    impl ChatLauncher for ScriptedLauncher {
        async fn prepare(
            &self,
            thread: &ThreadRecord,
        ) -> Result<(Arc<dyn ChatAdapter>, ChatStart), IpcError> {
            Ok((
                self.0.clone(),
                ChatStart {
                    thread_id: thread.id.clone(),
                    engine_id: thread.engine_id.clone(),
                    program: "/bin/true".into(),
                    cwd: thread.cwd.clone().into(),
                    env: Vec::new(),
                    model: None,
                    effort: None,
                    access: thread.access,
                    mode: thread.mode,
                    attended: true,
                    resume_session: None,
                    mcp_config: None,
                    mcp: None,
                    engine_version: None,
                    relocate: None,
                    briefing: None,
                    stdio_servers: Vec::new(),
                },
            ))
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
        svc: Arc<ThreadService>,
        adapter: Arc<ScriptedAdapter>,
        thread: ThreadRecord,
    }

    async fn fixture(on_stop: OnStop, script: Script) -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let store =
            Store::open(&DataDir::at(dir.path().to_path_buf()).expect("data")).expect("store");
        let adapter = Arc::new(ScriptedAdapter {
            script,
            on_stop,
            starts: AtomicUsize::new(0),
            resumed: StdMutex::default(),
            answers: Arc::default(),
            inputs: Arc::default(),
        });
        let svc = ThreadService::new(
            store.clone(),
            EventBus::new(),
            Arc::new(SystemClock),
            ApprovalBroker::new(Arc::new(SystemClock)),
            Arc::new(ScriptedLauncher(adapter.clone())),
        );
        let thread = svc
            .create(NewThread {
                id: ThreadId::new(),
                kind: ThreadKind::Thread,
                workspace_id: None,
                cwd: dir.path().display().to_string(),
                engine_id: "claude".into(),
                account_id: None,
                model: None,
                effort: None,
                access: Access::Edits,
                mode: ThreadMode::Act,
                attended: true,
            })
            .await
            .expect("thread");
        Fixture {
            _dir: dir,
            store,
            svc,
            adapter,
            thread,
        }
    }

    #[tokio::test]
    async fn f_ex_01_9_a_turns_checkpoint_has_the_files_from_before_the_engine_saw_the_message() {
        use tandem_store::repo::workspaces::{self, NewWorkspace};
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let folder = dir.path().join("work");
        std::fs::create_dir_all(&folder).expect("mkdir");
        std::fs::write(folder.join("a.txt"), "before").expect("write");
        let folder = std::fs::canonicalize(&folder).expect("canon");
        // The "engine" edits the file the moment it gets the turn.
        let edit = folder.join("a.txt");
        let script: Script = Arc::new(move |turn, tx| {
            std::fs::write(&edit, "after").expect("edit");
            play(tx, vec![ev(&turn, end(TurnEndReason::Completed))]);
        });
        let adapter = Arc::new(ScriptedAdapter {
            script,
            on_stop: OnStop::Ack,
            starts: AtomicUsize::new(0),
            resumed: StdMutex::default(),
            answers: Arc::default(),
            inputs: Arc::default(),
        });
        let svc = ThreadService::new(
            store.clone(),
            EventBus::new(),
            Arc::new(SystemClock),
            ApprovalBroker::new(Arc::new(SystemClock)),
            Arc::new(ScriptedLauncher(adapter)),
        );
        let terminals = crate::services::terminal::TerminalService::new(
            tandem_pty::PtyManager::new(),
            store.clone(),
        );
        terminals.set_login_env(tandem_pty::env::LoginEnv {
            vars: std::env::vars().collect(),
            shell: "/bin/sh".into(),
            degraded: false,
        });
        let cps = crate::services::checkpoints::CheckpointService::new(
            data,
            store.clone(),
            EventBus::new(),
            Arc::new(SystemClock),
            terminals,
        );
        svc.set_checkpoints(cps.clone());
        let (ws, _) = workspaces::add(
            &store,
            NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: folder.display().to_string(),
                name: "work".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("workspace");
        let thread = svc
            .create(NewThread {
                id: ThreadId::new(),
                kind: ThreadKind::Thread,
                workspace_id: Some(ws.id),
                cwd: folder.display().to_string(),
                engine_id: "claude".into(),
                account_id: None,
                model: None,
                effort: None,
                access: Access::Edits,
                mode: ThreadMode::Act,
                attended: true,
            })
            .await
            .expect("thread");
        svc.send(&thread.id, "change a".into(), vec![], false)
            .await
            .expect("send");
        assert_eq!(
            std::fs::read_to_string(folder.join("a.txt")).expect("read"),
            "after"
        );
        let tl = cps
            .timeline(&folder, Some(thread.id.as_str().to_string()))
            .await
            .expect("timeline");
        assert_eq!(tl.items.len(), 1);
        assert_eq!(tl.items[0].differs, 1);
        cps.restore(&tl.items[0].id, None).await.expect("restore");
        assert_eq!(
            std::fs::read_to_string(folder.join("a.txt")).expect("read"),
            "before"
        );
    }

    fn ev(turn: &TurnId, event: ThreadEvent) -> AdapterEvent {
        AdapterEvent::Event {
            turn_id: turn.clone(),
            event,
        }
    }

    fn delta(text: &str) -> ThreadEvent {
        ThreadEvent::AssistantDelta { text: text.into() }
    }

    fn end(reason: TurnEndReason) -> ThreadEvent {
        ThreadEvent::TurnEnd {
            reason,
            blocked_actions: 0,
        }
    }

    /// Sends `events` in order from a task, as an engine would.
    fn play(tx: mpsc::Sender<AdapterEvent>, events: Vec<AdapterEvent>) {
        tokio::spawn(async move {
            for e in events {
                let _ = tx.send(e).await;
            }
        });
    }

    /// A turn that streams nothing and never ends on its own.
    fn hang() -> Script {
        Arc::new(|_turn, _tx| {})
    }

    async fn turn_states(f: &Fixture) -> Vec<TurnState> {
        repo::turns(&f.store, &f.thread.id)
            .await
            .expect("turns")
            .iter()
            .map(|t| t.state)
            .collect()
    }

    async fn ended(f: &Fixture, n: usize) {
        for _ in 0..300 {
            let states = turn_states(f).await;
            if states.len() >= n && states[..n].iter().all(|s| *s != TurnState::Running) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("turn {n} never ended: {:?}", turn_states(f).await);
    }

    /// The user messages stored so far.
    async fn stored_users(f: &Fixture) -> Vec<String> {
        stored(f)
            .await
            .into_iter()
            .filter_map(|e| match e {
                ThreadEvent::User { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    async fn stored(f: &Fixture) -> Vec<ThreadEvent> {
        f.svc
            .events(&f.thread.id, None, 500)
            .await
            .expect("events")
            .items
            .into_iter()
            .map(|e| e.event)
            .collect()
    }

    #[tokio::test]
    async fn f_th_03_a_turn_streams_and_is_stored_with_its_text_compacted() {
        let script: Script = Arc::new(|turn, tx| {
            play(
                tx,
                vec![
                    ev(&turn, delta("Hel")),
                    ev(&turn, delta("lo")),
                    ev(
                        &turn,
                        ThreadEvent::tool("t1", ToolVerb::Bash, "npm test", ToolStatus::Running),
                    ),
                    ev(
                        &turn,
                        ThreadEvent::tool("t1", ToolVerb::Bash, "npm test", ToolStatus::Ok),
                    ),
                    ev(&turn, delta("Done.")),
                    ev(&turn, end(TurnEndReason::Completed)),
                ],
            )
        });
        let f = fixture(OnStop::Ack, script).await;
        let mut live = f.svc.live.subscribe();
        let sent = f
            .svc
            .send(&f.thread.id, "run the tests".into(), Vec::new(), false)
            .await
            .expect("send");
        assert!(!sent.queued);
        ended(&f, 1).await;

        assert_eq!(
            stored(&f).await,
            vec![
                ThreadEvent::User {
                    text: "run the tests".into(),
                    attachments: Vec::new()
                },
                ThreadEvent::Assistant {
                    markdown: "Hello".into()
                },
                ThreadEvent::tool("t1", ToolVerb::Bash, "npm test", ToolStatus::Running),
                ThreadEvent::tool("t1", ToolVerb::Bash, "npm test", ToolStatus::Ok),
                ThreadEvent::Assistant {
                    markdown: "Done.".into()
                },
                end(TurnEndReason::Completed),
            ]
        );
        // Live: deltas share their block's seq with the final text.
        let mut seen = Vec::new();
        while let Ok(env) = live.try_recv() {
            seen.push((env.seq, env.event.tag()));
        }
        assert_eq!(
            seen,
            vec![
                (0, "user"),
                (1, "assistant_delta"),
                (1, "assistant_delta"),
                (1, "assistant"),
                (2, "tool"),
                (3, "tool"),
                (4, "assistant_delta"),
                (4, "assistant"),
                (5, "turn_end"),
            ]
        );
        // As in f_th_03_4 below: the state is written just after the turn's end
        // is stored, so wait for it rather than read it once (it raced under
        // the full gate's load in chain5).
        let mut detail = f.svc.get(&f.thread.id).await.expect("detail");
        for _ in 0..100 {
            if detail.thread.state != ThreadState::Working {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            detail = f.svc.get(&f.thread.id).await.expect("detail");
        }
        assert_eq!(detail.thread.state, ThreadState::Done);
        assert_eq!(detail.running, None);
    }

    #[tokio::test]
    async fn f_th_03_3_events_of_a_finished_turn_change_nothing() {
        let sink: Arc<StdMutex<Option<mpsc::Sender<AdapterEvent>>>> = Arc::default();
        let keep = sink.clone();
        let script: Script = Arc::new(move |turn, tx| {
            if let Ok(mut s) = keep.lock() {
                *s = Some(tx.clone());
            }
            play(
                tx,
                vec![
                    ev(&turn, delta("first")),
                    ev(&turn, end(TurnEndReason::Completed)),
                ],
            );
        });
        let f = fixture(OnStop::Ack, script).await;
        let t1 = f
            .svc
            .send(&f.thread.id, "one".into(), Vec::new(), false)
            .await
            .expect("one");
        ended(&f, 1).await;
        let before = stored(&f).await;
        // Turn 1's straggler arrives after turn 1 ended.
        let tx = sink.lock().expect("sink").clone().expect("tx");
        tx.send(ev(t1.turn_id.as_ref().expect("t1"), delta("late")))
            .await
            .expect("late");
        tx.send(ev(
            t1.turn_id.as_ref().expect("t1"),
            end(TurnEndReason::Error),
        ))
        .await
        .expect("late end");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(stored(&f).await, before);
        assert_eq!(turn_states(&f).await, vec![TurnState::Completed]);
    }

    #[tokio::test]
    async fn f_th_03_one_turn_at_a_time_and_one_queued_follow_up() {
        let f = fixture(OnStop::Ack, hang()).await;
        let id = &f.thread.id;
        let t1 = f
            .svc
            .send(id, "first".into(), Vec::new(), false)
            .await
            .expect("first");
        let busy = f
            .svc
            .send(id, "now".into(), Vec::new(), false)
            .await
            .expect_err("busy");
        assert_eq!(busy.code, ErrorCode::Conflict);
        let q = f
            .svc
            .send(id, "next".into(), Vec::new(), true)
            .await
            .expect("queue");
        assert_eq!(
            q,
            SendResult {
                turn_id: None,
                queued: true
            }
        );
        let again = f
            .svc
            .send(id, "more".into(), Vec::new(), true)
            .await
            .expect_err("one queued");
        assert_eq!(again.message, "A message is already queued");
        assert_eq!(
            f.svc.get(id).await.expect("detail").queued.as_deref(),
            Some("next")
        );

        // Turn 1 ends: the queued message starts turn 2.
        let rt = f.svc.runtime(id);
        let generation = {
            let g = rt.lock().await;
            assert!(g.session.is_some());
            g.generation
        };
        f.svc
            .handle(
                id,
                generation,
                ev(
                    t1.turn_id.as_ref().expect("t1"),
                    end(TurnEndReason::Completed),
                ),
            )
            .await;
        // The follow-up's turn row and its stored message land a moment
        // apart: wait for both, then check both.
        for _ in 0..300 {
            if turn_states(&f).await.len() == 2 && stored_users(&f).await.len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            turn_states(&f).await,
            vec![TurnState::Completed, TurnState::Running]
        );
        assert_eq!(
            stored_users(&f).await,
            vec!["first".to_string(), "next".to_string()]
        );
        // The same session carried both turns.
        assert_eq!(f.adapter.starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn f_th_03_2_a_stop_ends_the_turn_cancelled_and_the_queued_message_goes_back_to_the_draft(
    ) {
        let f = fixture(OnStop::Ignore, hang()).await;
        let id = &f.thread.id;
        f.svc
            .send(id, "long job".into(), Vec::new(), false)
            .await
            .expect("send");
        f.svc
            .send(id, "after".into(), Vec::new(), true)
            .await
            .expect("queue");
        let out = f.svc.stop(id).await.expect("stop");
        assert_eq!(
            out,
            InterruptOutcome {
                acked: false,
                killed: true
            }
        );
        assert_eq!(turn_states(&f).await, vec![TurnState::Cancelled]);
        let detail = f.svc.get(id).await.expect("detail");
        assert_eq!(
            detail.thread.state,
            ThreadState::Idle,
            "cancelled is not failed"
        );
        assert_eq!(detail.queued, None);
        assert!(
            detail.draft.contains("\"after\""),
            "draft: {}",
            detail.draft
        );
        assert_eq!(
            stored(&f).await.last(),
            Some(&end(TurnEndReason::Cancelled))
        );
        // The killed session is replaced on the next send.
        f.svc
            .send(id, "again".into(), Vec::new(), false)
            .await
            .expect("again");
        assert_eq!(f.adapter.starts.load(Ordering::SeqCst), 2);
        // Stopping with nothing running does nothing.
        let f2 = fixture(OnStop::Ignore, hang()).await;
        assert_eq!(
            f2.svc.stop(&f2.thread.id).await.expect("idle stop"),
            InterruptOutcome {
                acked: false,
                killed: false
            }
        );
    }

    #[tokio::test]
    async fn f_th_03_an_acknowledged_stop_is_cancelled_even_when_the_engine_says_error() {
        let f = fixture(OnStop::Ack, hang()).await;
        let id = &f.thread.id;
        f.svc
            .send(id, "job".into(), Vec::new(), false)
            .await
            .expect("send");
        let out = f.svc.stop(id).await.expect("stop");
        assert!(out.acked);
        ended(&f, 1).await;
        assert_eq!(turn_states(&f).await, vec![TurnState::Cancelled]);
    }

    /// F-EN-08 AC3: a thread on an SSH host or in WSL runs that host's
    /// engine, so its turns never reach this computer's version book.
    #[tokio::test]
    async fn f_en_08_3_a_remote_threads_turns_never_reach_the_health_sink() {
        use tandem_store::repo::workspaces::{self, NewWorkspace};
        let script: Script = Arc::new(|turn, tx| {
            play(
                tx,
                vec![
                    ev(&turn, delta("partial answer")),
                    AdapterEvent::Exited {
                        code: Some(137),
                        stderr_tail: "segfault\n".into(),
                    },
                ],
            )
        });
        let f = fixture(OnStop::Ack, script).await;
        let seen: Arc<StdMutex<Vec<(String, TurnEndReason)>>> = Arc::default();
        let sink = seen.clone();
        f.svc.set_health_sink(Arc::new(move |engine, reason| {
            if let Ok(mut s) = sink.lock() {
                s.push((engine.to_string(), reason));
            }
        }));
        let ssh = tandem_store::repo::hosts::add_ssh(
            &f.store,
            tandem_store::repo::hosts::NewSshHost {
                label: "box".into(),
                target: "dev@box".into(),
                port: None,
                identity_file: None,
            },
        )
        .await
        .expect("host");
        let ssh_host = ssh.id.as_str().to_string();
        let mut threads = Vec::new();
        for (kind, host, distro) in [
            ("ssh", ssh_host.as_str(), None),
            ("wsl", "local", Some("Ubuntu")),
            ("local", "local", None),
        ] {
            let root = f._dir.path().join(kind);
            std::fs::create_dir_all(&root).expect("mkdir");
            let (ws, _) = workspaces::add(
                &f.store,
                NewWorkspace {
                    kind: kind.into(),
                    host_id: host.into(),
                    root: root.display().to_string(),
                    name: kind.into(),
                    wsl_distro: distro.map(str::to_string),
                },
                None,
            )
            .await
            .expect("workspace");
            let thread = f
                .svc
                .create(NewThread {
                    id: ThreadId::new(),
                    kind: ThreadKind::Thread,
                    workspace_id: Some(ws.id.clone()),
                    cwd: root.display().to_string(),
                    engine_id: "claude".into(),
                    account_id: None,
                    model: None,
                    effort: None,
                    access: Access::Edits,
                    mode: ThreadMode::Act,
                    attended: true,
                })
                .await
                .expect("thread");
            threads.push((kind, thread.id.clone()));
        }
        for (_, id) in &threads {
            f.svc
                .send(id, "do it".into(), Vec::new(), false)
                .await
                .expect("send");
        }
        // Every turn ends; only the local thread's reached the sink.
        for _ in 0..300 {
            let mut done = 0;
            for (_, id) in &threads {
                let turns = repo::turns(&f.store, id).await.expect("turns");
                if turns.iter().any(|t| t.state != TurnState::Running) {
                    done += 1;
                }
            }
            if done == threads.len() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        let seen = seen.lock().expect("lock").clone();
        assert_eq!(seen, vec![("claude".to_string(), TurnEndReason::Error)]);
    }

    #[tokio::test]
    async fn f_th_03_4_an_engine_that_exits_mid_turn_shows_an_error_with_retry() {
        let script: Script = Arc::new(|turn, tx| {
            play(
                tx,
                vec![
                    ev(&turn, delta("partial answer")),
                    AdapterEvent::Exited {
                        code: Some(137),
                        stderr_tail: "warming up\nsegfault in worker\n".into(),
                    },
                ],
            )
        });
        let f = fixture(OnStop::Ack, script).await;
        let id = &f.thread.id;
        f.svc
            .send(id, "do it".into(), Vec::new(), false)
            .await
            .expect("send");
        ended(&f, 1).await;
        let events = stored(&f).await;
        assert_eq!(
            events[1],
            ThreadEvent::Assistant {
                markdown: "partial answer".into()
            }
        );
        assert_eq!(
            events[2],
            ThreadEvent::Error {
                code: "engine_exited".into(),
                message: "The engine stopped unexpectedly (exit 137).".into(),
                stage: ErrorStage::Unknown,
                hint: Some("segfault in worker".into()),
                action: Some(ErrorAction::Retry),
            }
        );
        assert_eq!(events[3], end(TurnEndReason::Error));
        // The state is written just after the turn's end is stored: wait for
        // it rather than read it once (it raced under the full suite's load).
        let mut state = f.svc.get(id).await.expect("detail").thread.state;
        for _ in 0..100 {
            if state == ThreadState::Failed {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            state = f.svc.get(id).await.expect("detail").thread.state;
        }
        assert_eq!(state, ThreadState::Failed);

        // Retry sends the same message on a new session.
        let again = f.svc.retry(id).await.expect("retry");
        assert!(again.turn_id.is_some());
        ended(&f, 2).await;
        assert_eq!(f.adapter.starts.load(Ordering::SeqCst), 2);
        let users = stored(&f)
            .await
            .into_iter()
            .filter(|e| matches!(e, ThreadEvent::User { text, .. } if text == "do it"))
            .count();
        assert_eq!(users, 2);
        // The retry ran (and this engine failed again).
        assert_eq!(
            turn_states(&f).await,
            vec![TurnState::Error, TurnState::Error]
        );
    }

    #[tokio::test]
    async fn f_th_03_streaming_text_is_kept_every_2_s_and_a_late_subscriber_sees_it_all() {
        let sink: TurnSink = Arc::default();
        let keep = sink.clone();
        let script: Script = Arc::new(move |turn, tx| {
            if let Ok(mut s) = keep.lock() {
                *s = Some((turn.clone(), tx.clone()));
            }
            play(tx, vec![ev(&turn, delta("Thinking about "))]);
        });
        let f = fixture(OnStop::Ack, script).await;
        let id = &f.thread.id;
        f.svc
            .send(id, "q".into(), Vec::new(), false)
            .await
            .expect("send");
        tokio::time::sleep(Duration::from_millis(2_300)).await;
        // The partial row is stored (a crash keeps it).
        assert_eq!(
            stored(&f).await,
            vec![
                ThreadEvent::User {
                    text: "q".into(),
                    attachments: Vec::new()
                },
                ThreadEvent::Assistant {
                    markdown: "Thinking about ".into()
                },
            ]
        );
        let (turn, tx) = sink.lock().expect("sink").clone().expect("tx");
        tx.send(ev(&turn, delta("it."))).await.expect("delta");
        tokio::time::sleep(Duration::from_millis(50)).await;
        // A view mounted now gets the stored turn and the whole open block.
        let (snapshot, _rx) = f.svc.subscribe(id).await.expect("subscribe");
        let seen: Vec<(u64, ThreadEvent)> =
            snapshot.into_iter().map(|e| (e.seq, e.event)).collect();
        assert_eq!(
            seen,
            vec![
                (
                    0,
                    ThreadEvent::User {
                        text: "q".into(),
                        attachments: Vec::new()
                    }
                ),
                (
                    1,
                    ThreadEvent::Assistant {
                        markdown: "Thinking about it.".into()
                    }
                ),
            ]
        );
        tx.send(ev(&turn, end(TurnEndReason::Completed)))
            .await
            .expect("end");
        ended(&f, 1).await;
        let events = stored(&f).await;
        // The final text replaced the partial row: still one assistant row.
        assert_eq!(
            events,
            vec![
                ThreadEvent::User {
                    text: "q".into(),
                    attachments: Vec::new()
                },
                ThreadEvent::Assistant {
                    markdown: "Thinking about it.".into()
                },
                end(TurnEndReason::Completed),
            ]
        );
    }

    #[tokio::test]
    async fn f_th_04_4_an_approval_request_shows_in_the_turn_and_allow_answers_the_tool() {
        let f = fixture(OnStop::Ack, hang()).await;
        let id = f.thread.id.clone();
        f.svc
            .send(&id, "run the tests".into(), Vec::new(), false)
            .await
            .expect("send");
        let ask = ToolApproval {
            tool_name: "Bash".into(),
            input: serde_json::json!({"command": "npm test"}),
            tool_use_id: Some("toolu_1".into()),
        };
        let svc = f.svc.clone();
        let asking = {
            let id = id.clone();
            tokio::spawn(async move { svc.ask_approval(&id, ask).await })
        };
        // The card is in the turn and the thread waits for the user.
        let mut pending = Vec::new();
        for _ in 0..200 {
            pending = f.svc.approvals.pending();
            if !pending.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let view = pending.first().cloned().expect("pending approval");
        assert_eq!(view.kind, ApprovalKind::Command);
        assert_eq!(view.summary, "Run `npm test`");
        assert_eq!(
            f.svc.get(&id).await.expect("detail").thread.state,
            ThreadState::Waiting
        );
        assert!(stored(&f).await.iter().any(|e| matches!(e, ThreadEvent::ApprovalRequest { summary, .. } if summary == "Run `npm test`")));

        f.svc
            .approvals
            .resolve(
                &view.id,
                tandem_core::approvals::ApprovalDecision::Allow {
                    scope: ApprovalScope::Once,
                },
                tandem_core::approvals::ApprovalVia::Inline,
            )
            .expect("resolve");
        assert_eq!(asking.await.expect("join"), ApprovalAnswer::Allow);
        assert!(stored(&f).await.iter().any(|e| matches!(
            e,
            ThreadEvent::ApprovalResolved {
                decision: ApprovalDecisionView::Allow {
                    scope: ApprovalScope::Once
                },
                ..
            }
        )));
        assert_eq!(
            f.svc.get(&id).await.expect("detail").thread.state,
            ThreadState::Working
        );

        // A deny's reason goes back to the engine.
        let svc = f.svc.clone();
        let asking = {
            let id = id.clone();
            tokio::spawn(async move {
                svc.ask_approval(
                    &id,
                    ToolApproval {
                        tool_name: "Write".into(),
                        input: serde_json::json!({"file_path": "/etc/hosts"}),
                        tool_use_id: None,
                    },
                )
                .await
            })
        };
        let mut pending = Vec::new();
        for _ in 0..200 {
            pending = f.svc.approvals.pending();
            if !pending.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let view = pending.first().cloned().expect("pending");
        assert_eq!(view.kind, ApprovalKind::FileWrite);
        f.svc
            .approvals
            .resolve(
                &view.id,
                tandem_core::approvals::ApprovalDecision::Deny {
                    reason: Some("Not that file".into()),
                },
                tandem_core::approvals::ApprovalVia::Inline,
            )
            .expect("resolve");
        assert_eq!(
            asking.await.expect("join"),
            ApprovalAnswer::Deny {
                message: Some("Not that file".into())
            }
        );

        // A stopped turn cancels what it was waiting on.
        let svc = f.svc.clone();
        let asking = {
            let id = id.clone();
            tokio::spawn(async move {
                svc.ask_approval(
                    &id,
                    ToolApproval {
                        tool_name: "Bash".into(),
                        input: serde_json::json!({"command": "make"}),
                        tool_use_id: None,
                    },
                )
                .await
            })
        };
        for _ in 0..200 {
            if !f.svc.approvals.pending().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        f.svc.stop(&id).await.expect("stop");
        assert_eq!(
            asking.await.expect("join"),
            ApprovalAnswer::Deny { message: None }
        );
        assert!(f.svc.approvals.pending().is_empty());
    }

    /// A JSON-RPC call to the gateway (JSON body or SSE stream).
    async fn mcp(
        url: &str,
        token: &str,
        session: Option<&str>,
        body: serde_json::Value,
    ) -> (Option<String>, serde_json::Value) {
        let mut req = reqwest::Client::new()
            .post(url)
            .bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(s) = session {
            req = req
                .header("mcp-session-id", s)
                .header("mcp-protocol-version", "2025-03-26");
        }
        let resp = req.send().await.expect("send");
        let sid = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str(&text).ok().or_else(|| {
            text.lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .filter_map(|d| serde_json::from_str::<serde_json::Value>(d.trim()).ok())
                .find(|m| m.get("id").is_some())
        });
        (sid, msg.unwrap_or_default())
    }

    #[tokio::test]
    async fn f_th_04_4_the_gateway_tool_asks_the_thread_and_allow_returns_behavior_allow() {
        let f = fixture(OnStop::Ack, hang()).await;
        let id = f.thread.id.clone();
        let service = crate::services::gateway::GatewayService::new(
            f.store.clone(),
            f._dir.path().join("run"),
        );
        service.bind_threads(&f.svc);
        let gateway = service.handle().await.expect("gateway").clone();
        let lease = gateway.prepare(tandem_gateway::SessionDescriptor::new(
            Caller::Thread {
                thread_id: id.clone(),
            },
            tandem_gateway::Attended::Interactive,
            tandem_core::access::Access::Edits,
            tandem_gateway::BuiltinSet::for_thread(),
        ));
        f.svc
            .send(&id, "test it".into(), Vec::new(), false)
            .await
            .expect("send");
        let (sid, _) = mcp(
            &gateway.url,
            &lease.token,
            None,
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "claude-code", "version": "2"}
            }}),
        )
        .await;
        let sid = sid.expect("session");
        mcp(
            &gateway.url,
            &lease.token,
            Some(&sid),
            serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await;
        // Claude asks; the user allows from the card.
        let (url, token, s) = (gateway.url.clone(), lease.token.clone(), sid.clone());
        let call = tokio::spawn(async move {
            mcp(&url, &token, Some(&s), serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
                "name": "tandem_approvals__request",
                "arguments": {"tool_name": "Bash", "input": {"command": "npm test"}, "tool_use_id": "toolu_7"}
            }}))
            .await
        });
        let mut pending = Vec::new();
        for _ in 0..300 {
            pending = f.svc.approvals.pending();
            if !pending.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let view = pending.first().cloned().expect("a card");
        assert_eq!(
            view.source,
            SourceRef::Thread {
                thread_id: id.as_str().to_string()
            }
        );
        f.svc
            .approvals
            .resolve(
                &view.id,
                tandem_core::approvals::ApprovalDecision::Allow {
                    scope: ApprovalScope::Once,
                },
                tandem_core::approvals::ApprovalVia::Inline,
            )
            .expect("allow");
        let (_, reply) = call.await.expect("join");
        let text = reply
            .pointer("/result/content/0/text")
            .and_then(serde_json::Value::as_str)
            .expect("tool text");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(text).expect("json"),
            serde_json::json!({"behavior": "allow", "updatedInput": {"command": "npm test"}})
        );
        gateway.shutdown();
    }

    #[tokio::test]
    async fn f_th_05_an_in_band_approval_shows_the_card_and_answers_the_engine() {
        let script: Script = Arc::new(|turn, tx| {
            play(
                tx,
                vec![AdapterEvent::Approval {
                    turn_id: turn,
                    id: ApprovalId::from_db("ap_engine_1".into()),
                    kind: ApprovalKind::Command,
                    summary: "Run `npm ci`".into(),
                    detail: "npm ci".into(),
                    subject: ApprovalSubject::Command("npm ci".into()),
                }],
            )
        });
        let f = fixture(OnStop::Ack, script).await;
        f.svc
            .send(&f.thread.id, "install".into(), Vec::new(), false)
            .await
            .expect("send");
        let mut pending = Vec::new();
        for _ in 0..300 {
            pending = f.svc.approvals.pending();
            if !pending.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let view = pending.first().cloned().expect("card");
        assert_eq!(view.summary, "Run `npm ci`");
        f.svc
            .approvals
            .resolve(
                &view.id,
                ApprovalDecision::Allow {
                    scope: ApprovalScope::Thread,
                },
                tandem_core::approvals::ApprovalVia::Dashboard,
            )
            .expect("allow");
        for _ in 0..300 {
            if !f.adapter.answers.lock().expect("answers").is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            f.adapter.answers.lock().expect("answers").clone(),
            vec![(
                ApprovalId::from_db("ap_engine_1".into()),
                ApprovalDecision::Allow {
                    scope: ApprovalScope::Thread
                }
            )]
        );
    }

    #[tokio::test]
    async fn f_th_05_2_an_engine_that_died_while_idle_is_restarted_resumed_and_says_reconnecting() {
        let script: Script = Arc::new(|turn, tx| {
            play(
                tx,
                vec![
                    AdapterEvent::SessionId("thr_engine".into()),
                    ev(&turn, delta("first reply")),
                    ev(&turn, end(TurnEndReason::Completed)),
                ],
            )
        });
        let f = fixture(OnStop::Ack, script).await;
        let id = &f.thread.id;
        f.svc
            .send(id, "one".into(), Vec::new(), false)
            .await
            .expect("one");
        ended(&f, 1).await;
        // The engine process dies while the thread is idle.
        let generation = f.svc.runtime(id).lock().await.generation;
        f.svc
            .handle(
                id,
                generation,
                AdapterEvent::Exited {
                    code: Some(137),
                    stderr_tail: String::new(),
                },
            )
            .await;
        assert_eq!(
            turn_states(&f).await,
            vec![TurnState::Completed],
            "an idle exit fails nothing"
        );
        f.svc
            .send(id, "two".into(), Vec::new(), false)
            .await
            .expect("two");
        ended(&f, 2).await;
        assert_eq!(f.adapter.starts.load(Ordering::SeqCst), 2);
        assert_eq!(
            f.adapter.resumed.lock().expect("resumed").clone(),
            vec![None, Some("thr_engine".into())]
        );
        let events = stored(&f).await;
        assert!(events.contains(&ThreadEvent::Note {
            text: "Reconnecting…".into()
        }));
        // Earlier messages are still there.
        assert!(events.contains(&ThreadEvent::Assistant {
            markdown: "first reply".into()
        }));
    }

    #[tokio::test]
    async fn f_th_05_3_an_unsupported_engine_version_offers_the_terminal() {
        struct Old;
        #[async_trait]
        impl ChatLauncher for Old {
            async fn prepare(
                &self,
                _thread: &ThreadRecord,
            ) -> Result<(Arc<dyn ChatAdapter>, ChatStart), IpcError> {
                Err(IpcError::new(
                    ErrorCode::EngineUnsupported,
                    "Codex version 0.31.0 isn't supported yet — update Vibezeo or Codex",
                ))
            }
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let store =
            Store::open(&DataDir::at(dir.path().to_path_buf()).expect("data")).expect("store");
        let svc = ThreadService::new(
            store,
            EventBus::new(),
            Arc::new(SystemClock),
            ApprovalBroker::new(Arc::new(SystemClock)),
            Arc::new(Old),
        );
        let thread = svc
            .create(NewThread {
                id: ThreadId::new(),
                kind: ThreadKind::Thread,
                workspace_id: None,
                cwd: "/tmp".into(),
                engine_id: "codex".into(),
                account_id: None,
                model: None,
                effort: None,
                access: Access::Read,
                mode: ThreadMode::Act,
                attended: true,
            })
            .await
            .expect("thread");
        svc.send(&thread.id, "hi".into(), Vec::new(), false)
            .await
            .expect("send");
        let events = svc
            .events(&thread.id, None, 50)
            .await
            .expect("events")
            .items;
        assert!(matches!(
            &events[1].event,
            ThreadEvent::Error { action: Some(ErrorAction::OpenTerminal), stage: ErrorStage::Spawn, message, .. }
                if message.starts_with("Codex version 0.31.0 isn't supported yet")
        ));
    }

    #[tokio::test]
    async fn f_th_10_1_plan_turns_make_versions_and_implement_sends_the_chosen_one() {
        let n = Arc::new(AtomicUsize::new(0));
        let count = n.clone();
        let f = fixture(
            OnStop::Ack,
            Arc::new(move |turn, tx| {
                let i = count.fetch_add(1, Ordering::SeqCst) + 1;
                play(
                    tx,
                    vec![
                        ev(&turn, delta(&format!("1. Step of plan {i}"))),
                        ev(&turn, end(TurnEndReason::Completed)),
                    ],
                );
            }),
        )
        .await;
        let id = &f.thread.id;
        f.svc
            .update(
                id,
                ThreadPatch {
                    mode: Some(ThreadMode::Plan),
                    ..ThreadPatch::default()
                },
            )
            .await
            .expect("plan mode");
        for (k, text) in ["plan it", "plan it again"].into_iter().enumerate() {
            f.svc
                .send(id, text.into(), Vec::new(), false)
                .await
                .expect("send");
            ended(&f, k + 1).await;
        }
        let detail = f.svc.get(id).await.expect("detail");
        assert_eq!(
            detail
                .plan_revisions
                .iter()
                .map(|p| (p.version, p.text.as_str()))
                .collect::<Vec<_>>(),
            vec![(1, "1. Step of plan 1"), (2, "1. Step of plan 2")]
        );
        let events = stored(&f).await;
        assert!(events.contains(&ThreadEvent::PlanRevision { version: 1 }));
        assert!(events.contains(&ThreadEvent::PlanRevision { version: 2 }));

        f.svc.implement_plan(id, 1, None).await.expect("implement");
        ended(&f, 3).await;
        let sent = f.adapter.inputs.lock().expect("inputs")[2].text.clone();
        assert_eq!(
            sent,
            "Implement plan version 1 exactly as written:\n\n1. Step of plan 1"
        );
        assert!(!sent.contains("plan 2"));
        let detail = f.svc.get(id).await.expect("detail");
        assert_eq!(detail.thread.mode, ThreadMode::Act);
        // An Act turn writes no plan.
        assert_eq!(detail.plan_revisions.len(), 2);
        let missing = f.svc.implement_plan(id, 9, None).await;
        assert_eq!(missing.err().map(|e| e.code), Some(ErrorCode::NotFound));
    }

    #[tokio::test]
    async fn f_th_13_the_first_turn_records_the_start_and_changes_follow_it() {
        let f = fixture(
            OnStop::Ack,
            Arc::new(|turn, tx| play(tx, vec![ev(&turn, end(TurnEndReason::Completed))])),
        )
        .await;
        let cwd = std::path::PathBuf::from(&f.thread.cwd);
        let git = |args: &[&str]| {
            let empty = std::env::temp_dir().join("tandem-desktop-tests-empty-gitconfig");
            std::fs::write(&empty, "").expect("empty gitconfig");
            let out = std::process::Command::new("git")
                .current_dir(&cwd)
                .env("GIT_CONFIG_GLOBAL", &empty)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "T")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "T")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                .args(args)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {args:?}");
        };
        git(&["-c", "init.defaultBranch=main", "init", "-q"]);
        // The fixture's database lives in the same folder.
        std::fs::write(cwd.join(".gitignore"), "tandem.db*\n").expect("write");
        std::fs::write(cwd.join("a.txt"), "one\n").expect("write");
        git(&["add", "."]);
        git(&["commit", "-qm", "first"]);
        let id = &f.thread.id;
        let before = f.svc.changes(id).await.expect("changes");
        assert_eq!(before.source, thread_changes::ChangesSource::NotStarted);

        f.svc
            .send(id, "go".into(), Vec::new(), false)
            .await
            .expect("send");
        ended(&f, 1).await;
        std::fs::write(cwd.join("a.txt"), "two\n").expect("edit");
        git(&["commit", "-qam", "the agent committed"]);
        std::fs::write(cwd.join("b.txt"), "new\n").expect("add");
        // A later turn doesn't move the start.
        f.svc
            .send(id, "more".into(), Vec::new(), false)
            .await
            .expect("send");
        ended(&f, 2).await;
        let after = f.svc.changes(id).await.expect("changes");
        assert_eq!(after.source, thread_changes::ChangesSource::Git);
        assert_eq!(
            after
                .files
                .iter()
                .map(|d| d.path.as_str())
                .collect::<Vec<_>>(),
            vec!["a.txt", "b.txt"]
        );
    }

    #[tokio::test]
    async fn f_th_06_2_a_withdrawn_model_is_cleared_from_the_thread_with_the_note() {
        let f = fixture(
            OnStop::Ack,
            Arc::new(|turn, tx| {
                play(
                    tx,
                    vec![
                        AdapterEvent::Models {
                            available: vec![ModelOffer {
                                id: "grok-4".into(),
                                label: "Grok 4".into(),
                            }],
                            withdrawn: Some("grok-3".into()),
                        },
                        ev(
                            &turn,
                            ThreadEvent::Note {
                                text: "Model no longer offered by Grok".into(),
                            },
                        ),
                        ev(&turn, delta("Hi")),
                        ev(&turn, end(TurnEndReason::Completed)),
                    ],
                )
            }),
        )
        .await;
        let id = &f.thread.id;
        f.svc
            .update(
                id,
                ThreadPatch {
                    model: Some(Some("grok-3".into())),
                    ..ThreadPatch::default()
                },
            )
            .await
            .expect("model");
        f.svc
            .send(id, "hello".into(), Vec::new(), false)
            .await
            .expect("send");
        ended(&f, 1).await;
        assert_eq!(f.svc.thread(id).await.expect("thread").model, None);
        assert!(stored(&f).await.contains(&ThreadEvent::Note {
            text: "Model no longer offered by Grok".into()
        }));
    }

    #[tokio::test]
    async fn f_th_06_the_turn_after_a_sign_in_error_starts_a_new_session() {
        let calls = Arc::new(AtomicUsize::new(0));
        let n = calls.clone();
        let f = fixture(
            OnStop::Ack,
            Arc::new(move |turn, tx| {
                let first = n.fetch_add(1, Ordering::SeqCst) == 0;
                let events = if first {
                    vec![
                        ev(
                            &turn,
                            ThreadEvent::Error {
                                code: "auth_required".into(),
                                message: "Grok needs you to sign in.".into(),
                                stage: ErrorStage::Auth,
                                hint: None,
                                action: Some(ErrorAction::SignIn),
                            },
                        ),
                        ev(&turn, end(TurnEndReason::Error)),
                    ]
                } else {
                    vec![
                        ev(&turn, delta("Hi")),
                        ev(&turn, end(TurnEndReason::Completed)),
                    ]
                };
                play(tx, events);
            }),
        )
        .await;
        let id = &f.thread.id;
        f.svc
            .send(id, "one".into(), Vec::new(), false)
            .await
            .expect("one");
        ended(&f, 1).await;
        f.svc
            .send(id, "two".into(), Vec::new(), false)
            .await
            .expect("two");
        ended(&f, 2).await;
        // The new session reads the account's credentials again.
        assert_eq!(f.adapter.starts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn f_th_06_an_engine_that_needs_a_sign_in_offers_sign_in() {
        struct NeedsLogin;
        #[async_trait]
        impl ChatAdapter for NeedsLogin {
            fn engine_id(&self) -> &str {
                "grok"
            }
            async fn start(
                &self,
                _req: ChatStart,
                _events: mpsc::Sender<AdapterEvent>,
            ) -> Result<Box<dyn ChatSession>, AdapterError> {
                Err(AdapterError::Auth("Grok needs you to sign in.".into()))
            }
        }
        struct Launcher;
        #[async_trait]
        impl ChatLauncher for Launcher {
            async fn prepare(
                &self,
                thread: &ThreadRecord,
            ) -> Result<(Arc<dyn ChatAdapter>, ChatStart), IpcError> {
                Ok((
                    Arc::new(NeedsLogin),
                    ChatStart {
                        thread_id: thread.id.clone(),
                        engine_id: thread.engine_id.clone(),
                        program: "/bin/true".into(),
                        cwd: thread.cwd.clone().into(),
                        env: Vec::new(),
                        model: None,
                        effort: None,
                        access: thread.access,
                        mode: thread.mode,
                        attended: true,
                        resume_session: None,
                        mcp_config: None,
                        mcp: None,
                        engine_version: None,
                        relocate: None,
                        briefing: None,
                        stdio_servers: Vec::new(),
                    },
                ))
            }
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let store =
            Store::open(&DataDir::at(dir.path().to_path_buf()).expect("data")).expect("store");
        let svc = ThreadService::new(
            store,
            EventBus::new(),
            Arc::new(SystemClock),
            ApprovalBroker::new(Arc::new(SystemClock)),
            Arc::new(Launcher),
        );
        let thread = svc
            .create(NewThread {
                id: ThreadId::new(),
                kind: ThreadKind::Thread,
                workspace_id: None,
                cwd: "/tmp".into(),
                engine_id: "grok".into(),
                account_id: None,
                model: None,
                effort: None,
                access: Access::Edits,
                mode: ThreadMode::Act,
                attended: true,
            })
            .await
            .expect("thread");
        svc.send(&thread.id, "hi".into(), Vec::new(), false)
            .await
            .expect("send");
        let events = svc
            .events(&thread.id, None, 50)
            .await
            .expect("events")
            .items;
        assert!(matches!(
            &events[1].event,
            ThreadEvent::Error { action: Some(ErrorAction::SignIn), stage: ErrorStage::Auth, message, code, .. }
                if message == "Grok needs you to sign in." && code == "auth_required"
        ));
    }

    #[tokio::test]
    async fn f_th_09_3_an_approval_pending_31_min_is_denied_and_the_engine_told_so() {
        let mock = tandem_core::clock::MockClock::new(
            chrono::DateTime::parse_from_rfc3339("2026-10-08T10:00:00Z")
                .expect("start")
                .with_timezone(&chrono::Utc),
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let store =
            Store::open(&DataDir::at(dir.path().to_path_buf()).expect("data")).expect("store");
        let adapter = Arc::new(ScriptedAdapter {
            script: hang(),
            on_stop: OnStop::Ack,
            starts: AtomicUsize::new(0),
            resumed: StdMutex::default(),
            answers: Arc::default(),
            inputs: Arc::default(),
        });
        let svc = ThreadService::new(
            store,
            EventBus::new(),
            Arc::new(SystemClock),
            ApprovalBroker::new(Arc::new(mock.clone())),
            Arc::new(ScriptedLauncher(adapter)),
        );
        let thread = svc
            .create(NewThread {
                id: ThreadId::new(),
                kind: ThreadKind::Thread,
                workspace_id: None,
                cwd: "/tmp".into(),
                engine_id: "claude".into(),
                account_id: None,
                model: None,
                effort: None,
                access: Access::Edits,
                mode: ThreadMode::Act,
                attended: true,
            })
            .await
            .expect("thread");
        svc.send(&thread.id, "go".into(), Vec::new(), false)
            .await
            .expect("send");
        let asking = {
            let svc = svc.clone();
            let id = thread.id.clone();
            tokio::spawn(async move {
                svc.ask_approval(
                    &id,
                    ToolApproval {
                        tool_name: "Bash".into(),
                        input: serde_json::json!({"command": "make deploy"}),
                        tool_use_id: None,
                    },
                )
                .await
            })
        };
        for _ in 0..300 {
            if !svc.approvals.pending().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        mock.advance(Duration::from_secs(29 * 60));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(svc.approvals.pending().len(), 1, "still pending at 29 min");
        mock.advance(Duration::from_secs(2 * 60));
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), asking)
                .await
                .expect("answered")
                .expect("join"),
            ApprovalAnswer::Deny {
                message: Some("Nobody answered within 30 minutes".into())
            }
        );
        let events: Vec<ThreadEvent> = svc
            .events(&thread.id, None, 50)
            .await
            .expect("events")
            .items
            .into_iter()
            .map(|e| e.event)
            .collect();
        assert!(events.iter().any(|e| matches!(
            e,
            ThreadEvent::ApprovalResolved {
                decision: ApprovalDecisionView::Expired,
                ..
            }
        )));
    }

    /// Asks about `command` and answers the card with `decision` (if one
    /// shows); returns the answer and whether a card showed.
    async fn ask_command(
        f: &Fixture,
        command: &str,
        decision: Option<ApprovalDecision>,
    ) -> (ApprovalAnswer, bool) {
        let svc = f.svc.clone();
        let id = f.thread.id.clone();
        let command = command.to_string();
        let asking = tokio::spawn(async move {
            svc.ask_approval(
                &id,
                ToolApproval {
                    tool_name: "Bash".into(),
                    input: serde_json::json!({ "command": command }),
                    tool_use_id: None,
                },
            )
            .await
        });
        let mut carded = false;
        for _ in 0..50 {
            if asking.is_finished() {
                break;
            }
            if let Some(view) = f.svc.approvals.pending().first().cloned() {
                carded = true;
                f.svc
                    .approvals
                    .resolve(
                        &view.id,
                        decision.clone().expect("a card needs an answer"),
                        tandem_core::approvals::ApprovalVia::Inline,
                    )
                    .expect("resolve");
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        (asking.await.expect("join"), carded)
    }

    #[tokio::test]
    async fn f_th_11_2_allow_for_this_thread_on_npm_test_allows_npm_test_again_but_not_npm_install()
    {
        let f = fixture(OnStop::Ack, hang()).await;
        f.svc
            .send(&f.thread.id, "test".into(), Vec::new(), false)
            .await
            .expect("send");
        let thread_scope = ApprovalDecision::Allow {
            scope: ApprovalScope::Thread,
        };
        assert_eq!(
            ask_command(&f, "npm test", Some(thread_scope.clone())).await,
            (ApprovalAnswer::Allow, true)
        );
        // No card the second time.
        assert_eq!(
            ask_command(&f, "npm  test -- --watch", None).await,
            (ApprovalAnswer::Allow, false)
        );
        // A different command still asks.
        let deny = ApprovalDecision::Deny { reason: None };
        assert_eq!(
            ask_command(&f, "npm install", Some(deny.clone())).await,
            (ApprovalAnswer::Deny { message: None }, true)
        );
        assert_eq!(
            ask_command(&f, "npm test && rm -rf x", Some(deny)).await,
            (ApprovalAnswer::Deny { message: None }, true)
        );
        // Allow once makes no rule.
        let once = ApprovalDecision::Allow {
            scope: ApprovalScope::Once,
        };
        assert_eq!(
            ask_command(&f, "make", Some(once.clone())).await,
            (ApprovalAnswer::Allow, true)
        );
        assert_eq!(
            ask_command(&f, "make", Some(once)).await,
            (ApprovalAnswer::Allow, true)
        );
    }

    #[tokio::test]
    async fn f_th_03_a_launch_failure_is_an_error_card_not_a_hang() {
        struct NoEngine;
        #[async_trait]
        impl ChatLauncher for NoEngine {
            async fn prepare(
                &self,
                _thread: &ThreadRecord,
            ) -> Result<(Arc<dyn ChatAdapter>, ChatStart), IpcError> {
                Err(IpcError::new(
                    ErrorCode::NotFound,
                    "Claude Code isn't installed",
                ))
            }
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let store =
            Store::open(&DataDir::at(dir.path().to_path_buf()).expect("data")).expect("store");
        let svc = ThreadService::new(
            store,
            EventBus::new(),
            Arc::new(SystemClock),
            ApprovalBroker::new(Arc::new(SystemClock)),
            Arc::new(NoEngine),
        );
        let thread = svc
            .create(NewThread {
                id: ThreadId::new(),
                kind: ThreadKind::Thread,
                workspace_id: None,
                cwd: "/tmp".into(),
                engine_id: "claude".into(),
                account_id: None,
                model: None,
                effort: None,
                access: Access::Read,
                mode: ThreadMode::Act,
                attended: true,
            })
            .await
            .expect("thread");
        svc.send(&thread.id, "hi".into(), Vec::new(), false)
            .await
            .expect("send");
        let events: Vec<ThreadEvent> = svc
            .events(&thread.id, None, 50)
            .await
            .expect("events")
            .items
            .into_iter()
            .map(|e| e.event)
            .collect();
        assert!(matches!(
            &events[1],
            ThreadEvent::Error { stage: ErrorStage::Spawn, action: Some(ErrorAction::Retry), message, .. }
                if message == "Claude Code isn't installed"
        ));
        assert_eq!(events[2], end(TurnEndReason::Error));
    }

    fn reply(text: &'static str) -> Script {
        Arc::new(move |turn, tx| {
            play(
                tx,
                vec![
                    ev(&turn, delta(text)),
                    ev(&turn, end(TurnEndReason::Completed)),
                ],
            )
        })
    }

    struct FixedTitler(&'static str);

    #[async_trait]
    impl Titler for FixedTitler {
        async fn title(&self, _: &str) -> Option<String> {
            Some(self.0.to_string())
        }
    }

    async fn now(f: &Fixture) -> ThreadRecord {
        repo::get(&f.store, &f.thread.id)
            .await
            .expect("get")
            .expect("thread")
    }

    fn data(f: &Fixture) -> DataDir {
        DataDir::at(f._dir.path().join("data")).expect("data")
    }

    async fn send(f: &Fixture, text: &str, attachments: Vec<AttachmentId>) {
        f.svc
            .send(&f.thread.id, text.into(), attachments, false)
            .await
            .expect("send");
    }

    #[tokio::test]
    async fn f_th_15_the_first_message_names_the_thread_then_the_ai_title_and_a_rename_sticks() {
        let f = fixture(OnStop::Ack, reply("Hi")).await;
        f.svc
            .configure(Arc::new(FixedTitler("Login redirect fix")), data(&f));
        send(
            &f,
            "fix the login bug where the redirect loops forever",
            vec![],
        )
        .await;
        assert_eq!(
            now(&f).await.title,
            "Fix the login bug where the redirect loops"
        );
        ended(&f, 1).await;
        for _ in 0..300 {
            if now(&f).await.title == "Login redirect fix" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(now(&f).await.title, "Login redirect fix");
        f.svc
            .update(
                &f.thread.id,
                ThreadPatch {
                    title: Some("  Mine ".into()),
                    ..ThreadPatch::default()
                },
            )
            .await
            .expect("rename");
        send(&f, "and the tests", vec![]).await;
        ended(&f, 2).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(now(&f).await.title, "Mine");
        let blank = f
            .svc
            .update(
                &f.thread.id,
                ThreadPatch {
                    title: Some("  ".into()),
                    ..ThreadPatch::default()
                },
            )
            .await;
        assert_eq!(blank.map_err(|e| e.code), Err(ErrorCode::Invalid));
    }

    #[tokio::test]
    async fn d_118_without_the_opt_in_the_local_title_stays() {
        let f = fixture(OnStop::Ack, reply("Hi")).await;
        send(&f, "explain this repo", vec![]).await;
        ended(&f, 1).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(now(&f).await.title, "Explain this repo");
    }

    #[tokio::test]
    async fn f_th_01_2_a_finished_turn_is_unread_until_the_thread_is_opened() {
        let f = fixture(OnStop::Ack, reply("Hi")).await;
        send(&f, "hello", vec![]).await;
        ended(&f, 1).await;
        assert!(now(&f).await.unread);
        let detail = f.svc.get(&f.thread.id).await.expect("get");
        assert!(!detail.thread.unread);
        assert!(!now(&f).await.unread);
    }

    #[tokio::test]
    async fn f_th_07_attachments_go_with_the_message_and_leave_the_draft() {
        let f = fixture(OnStop::Ack, reply("Seen")).await;
        f.svc.configure(Arc::new(LocalOnly), data(&f));
        let file = f._dir.path().join("notes.txt");
        std::fs::write(&file, "x").expect("write");
        let files = f
            .svc
            .attach_paths(&f.thread.id, vec![file.display().to_string()])
            .await
            .expect("attach");
        let png = f
            .svc
            .attach_png(&f.thread.id, 1, 1, vec![0, 0, 0, 255])
            .await
            .expect("png");
        assert!(std::path::Path::new(&png.path).starts_with(data(&f).attachments()));
        let detail = f.svc.get(&f.thread.id).await.expect("get");
        assert_eq!(detail.attachments.len(), 2);
        let ids = vec![files[0].id.clone(), png.id.clone()];
        send(&f, "look", ids.clone()).await;
        ended(&f, 1).await;
        let input = f.adapter.inputs.lock().expect("inputs")[0].clone();
        assert_eq!(input.files, vec!["notes.txt".to_string()]);
        assert_eq!(input.images, vec![std::path::PathBuf::from(&png.path)]);
        assert!(f
            .svc
            .get(&f.thread.id)
            .await
            .expect("get")
            .attachments
            .is_empty());
        // 20 per message.
        let many = vec![file.display().to_string(); 21];
        assert_eq!(
            f.svc
                .attach_paths(&f.thread.id, many)
                .await
                .map(|_| ())
                .map_err(|e| e.code),
            Err(ErrorCode::LimitReached)
        );
        // Removing a pasted image deletes Vibezeo's copy.
        let again = f
            .svc
            .attach_png(&f.thread.id, 1, 1, vec![0, 0, 0, 255])
            .await
            .expect("png");
        f.svc.remove_attachment(&again.id).await.expect("remove");
        assert!(!std::path::Path::new(&again.path).exists());
        assert!(std::path::Path::new(&png.path).exists());
    }

    #[tokio::test]
    async fn f_th_09_new_access_starts_a_fresh_session_on_the_next_turn() {
        let f = fixture(OnStop::Ack, reply("ok")).await;
        send(&f, "one", vec![]).await;
        ended(&f, 1).await;
        let starts = || f.adapter.starts.load(Ordering::SeqCst);
        assert_eq!(starts(), 1);
        let same = ThreadPatch {
            access: Some(Access::Edits),
            pinned: Some(true),
            ..ThreadPatch::default()
        };
        f.svc.update(&f.thread.id, same).await.expect("update");
        send(&f, "two", vec![]).await;
        ended(&f, 2).await;
        assert_eq!(starts(), 1, "nothing that matters changed");
        let read = ThreadPatch {
            access: Some(Access::Read),
            ..ThreadPatch::default()
        };
        f.svc.update(&f.thread.id, read).await.expect("update");
        send(&f, "three", vec![]).await;
        ended(&f, 3).await;
        assert_eq!(starts(), 2);
    }

    #[tokio::test]
    async fn f_th_01_delete_needs_archive_and_keeps_a_private_recovery_copy() {
        let f = fixture(OnStop::Ack, reply("ok")).await;
        f.svc.configure(Arc::new(LocalOnly), data(&f));
        send(&f, "hello", vec![]).await;
        ended(&f, 1).await;
        assert_eq!(
            f.svc.delete(&f.thread.id).await.map_err(|e| e.code),
            Err(ErrorCode::Invalid)
        );
        f.svc.archive(&f.thread.id, true).await.expect("archive");
        assert!(f
            .svc
            .list(ThreadKind::Thread, false)
            .await
            .expect("rail")
            .is_empty());
        f.svc.delete(&f.thread.id).await.expect("delete");
        assert!(f.svc.get(&f.thread.id).await.is_err());
        let trash = data(&f).trash().join("threads");
        let copy = std::fs::read_dir(&trash)
            .expect("trash")
            .flatten()
            .next()
            .expect("a copy")
            .path();
        let json = std::fs::read_to_string(copy.join("thread.json")).expect("thread.json");
        assert!(json.contains("hello"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &std::path::Path| {
                std::fs::metadata(p).expect("meta").permissions().mode() & 0o777
            };
            assert_eq!(mode(&copy.join("thread.json")), 0o600);
            assert_eq!(mode(&copy), 0o700);
        }
        // Kept 7 days, then swept.
        assert_eq!(f.svc.sweep_trash(), 0);
        let old = trash.join(format!("{}-1000", f.thread.id.as_str()));
        std::fs::rename(&copy, &old).expect("age it");
        assert_eq!(f.svc.sweep_trash(), 1);
        assert!(!old.exists());
    }

    struct GoneModel(Arc<ScriptedAdapter>);

    #[async_trait]
    impl ChatLauncher for GoneModel {
        async fn prepare(
            &self,
            thread: &ThreadRecord,
        ) -> Result<(Arc<dyn ChatAdapter>, ChatStart), IpcError> {
            ScriptedLauncher(self.0.clone()).prepare(thread).await
        }
        fn model_for(&self, _: &ThreadRecord) -> (Option<String>, Option<String>) {
            (
                None,
                Some("The saved model gone-1 isn't available — using Default".into()),
            )
        }
    }

    #[tokio::test]
    async fn f_th_08_2_a_missing_saved_model_falls_back_with_the_notice() {
        let f = fixture(OnStop::Ack, reply("ok")).await;
        let svc = ThreadService::new(
            f.store.clone(),
            EventBus::new(),
            Arc::new(SystemClock),
            ApprovalBroker::new(Arc::new(SystemClock)),
            Arc::new(GoneModel(f.adapter.clone())),
        );
        svc.send(&f.thread.id, "hi".into(), vec![], false)
            .await
            .expect("send");
        ended(&f, 1).await;
        let events = stored(&f).await;
        assert!(events.contains(&ThreadEvent::Note {
            text: "The saved model gone-1 isn't available — using Default".into()
        }));
    }

    #[tokio::test]
    async fn f_th_15_a_silent_turn_gets_a_note_after_10_minutes() {
        let clock = tandem_core::clock::MockClock::new(
            chrono::DateTime::parse_from_rfc3339("2026-10-08T10:00:00Z")
                .expect("start")
                .with_timezone(&chrono::Utc),
        );
        let f = fixture(OnStop::Ack, hang()).await;
        let svc = ThreadService::new(
            f.store.clone(),
            EventBus::new(),
            Arc::new(clock.clone()),
            ApprovalBroker::new(Arc::new(SystemClock)),
            Arc::new(ScriptedLauncher(f.adapter.clone())),
        );
        svc.send(&f.thread.id, "wait".into(), vec![], false)
            .await
            .expect("send");
        clock.advance(Duration::from_secs(11 * 60));
        let note = ThreadEvent::Note {
            text: "No output for 10 minutes".into(),
        };
        for _ in 0..400 {
            if stored(&f).await.contains(&note) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(stored(&f).await.iter().filter(|e| **e == note).count(), 1);
        svc.stop(&f.thread.id).await.expect("stop");
    }

    #[tokio::test]
    async fn f_ag_12_1_mail_rows_go_in_a_turn_of_their_own_and_a_delivery_opens_with_its_message() {
        let f = fixture(OnStop::Ack, reply("on it")).await;
        let row = |inbound| ThreadEvent::MailRow {
            mail_id: tandem_core::ids::MailId::new(),
            from: "Ada".into(),
            to: "Bea".into(),
            preview: "Check CI".into(),
            inbound,
        };
        // No turn runs: the row is a turn of its own, already ended.
        let sent = row(false);
        f.svc
            .note_mail(&f.thread.id, sent.clone())
            .await
            .expect("note");
        assert_eq!(turn_states(&f).await, vec![TurnState::Completed]);
        assert_eq!(stored(&f).await[0], sent);
        // A delivery: the message, then the row; the engine hears the message.
        let got = row(true);
        f.svc
            .send_mail(&f.thread.id, "From Ada:\nCheck CI".into(), got.clone())
            .await
            .expect("deliver");
        ended(&f, 2).await;
        let turn = repo::turns(&f.store, &f.thread.id).await.expect("turns")[1].clone();
        let rows: Vec<ThreadEvent> = repo::turn_messages(&f.store, &turn.id)
            .await
            .expect("rows")
            .iter()
            .filter_map(envelope)
            .map(|e| e.event)
            .collect();
        assert!(
            matches!(&rows[0], ThreadEvent::User { text, .. } if text == "From Ada:\nCheck CI")
        );
        assert_eq!(rows[1], got);
        let inputs = f.adapter.inputs.lock().expect("inputs").clone();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].text, "From Ada:\nCheck CI");
    }

    #[tokio::test]
    async fn f_th_16_1_switching_engines_seeds_a_summary_shows_a_divider_and_clears_rules() {
        let f = fixture(OnStop::Ack, reply("ok")).await;
        for (n, text) in ["one", "two", "three"].into_iter().enumerate() {
            send(&f, text, vec![]).await;
            ended(&f, n + 1).await;
        }
        repo::set_allow_rules(
            &f.store,
            &f.thread.id,
            r#"[{"kind":"tool","name":"Edit"}]"#.into(),
        )
        .await
        .expect("rules");
        let divider = "Continued on Codex (Default) with a summary";
        let thread = f
            .svc
            .switch_engine(&f.thread.id, "codex", None, divider.into())
            .await
            .expect("switch");
        assert_eq!(thread.engine_id, "codex");
        assert_eq!(
            repo::allow_rules(&f.store, &f.thread.id)
                .await
                .expect("rules"),
            "[]"
        );
        send(&f, "four", vec![]).await;
        ended(&f, 4).await;
        // The new engine's first input carries the summary, then the message.
        let inputs: Vec<String> = f
            .adapter
            .inputs
            .lock()
            .expect("inputs")
            .iter()
            .map(|i| i.text.clone())
            .collect();
        let first = &inputs[3];
        assert!(first.contains("<previous_conversation>"), "{first}");
        for said in ["User: one", "User: two", "User: three", "Assistant: ok"] {
            assert!(first.contains(said), "{said} missing from {first}");
        }
        assert!(
            first.ends_with("</previous_conversation>\n\nfour"),
            "{first}"
        );
        // The turn opens with the message, then the divider; not the summary.
        let turn = repo::turns(&f.store, &f.thread.id).await.expect("turns")[3].clone();
        assert_eq!(turn.engine_id, "codex");
        let rows: Vec<ThreadEvent> = repo::turn_messages(&f.store, &turn.id)
            .await
            .expect("rows")
            .iter()
            .filter_map(envelope)
            .map(|e| e.event)
            .collect();
        assert_eq!(
            rows[0],
            ThreadEvent::User {
                text: "four".into(),
                attachments: vec![]
            }
        );
        assert_eq!(
            rows[1],
            ThreadEvent::Divider {
                text: divider.into()
            }
        );
        // Only once: the next turn is plain.
        send(&f, "five", vec![]).await;
        ended(&f, 5).await;
        assert_eq!(f.adapter.inputs.lock().expect("inputs")[4].text, "five");
        assert_eq!(f.adapter.starts.load(Ordering::SeqCst), 2);
        // Not mid-turn.
        let busy = fixture(OnStop::Ack, hang()).await;
        send(&busy, "go", vec![]).await;
        assert_eq!(
            busy.svc
                .switch_engine(&busy.thread.id, "codex", None, String::new())
                .await
                .map(|_| ())
                .map_err(|e| e.code),
            Err(ErrorCode::Conflict)
        );
        busy.svc.stop(&busy.thread.id).await.expect("stop");
    }

    #[tokio::test]
    async fn f_th_07_a_queued_follow_up_can_be_taken_back() {
        let f = fixture(OnStop::Ack, hang()).await;
        send(&f, "first", vec![]).await;
        let queued = f
            .svc
            .send(&f.thread.id, "second".into(), vec![], true)
            .await
            .expect("queue");
        assert!(queued.queued);
        assert_eq!(
            f.svc
                .get(&f.thread.id)
                .await
                .expect("get")
                .queued
                .as_deref(),
            Some("second")
        );
        assert_eq!(
            f.svc
                .unqueue(&f.thread.id)
                .await
                .expect("unqueue")
                .as_deref(),
            Some("second")
        );
        assert_eq!(f.svc.unqueue(&f.thread.id).await.expect("unqueue"), None);
        f.svc.stop(&f.thread.id).await.expect("stop");
    }

    /// A plan switch the tests can flip: Pro (every engine) until `free`.
    fn plan_switch(svc: &ThreadService) -> Arc<std::sync::atomic::AtomicBool> {
        let free = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let on_free = free.clone();
        svc.set_plan_limits(Arc::new(move || {
            let mut limits = tandem_core::entitlement::Limits::free();
            if !on_free.load(Ordering::SeqCst) {
                limits.chat_engines = None;
            }
            limits
        }));
        free
    }

    fn new_thread(f: &Fixture, engine: &str, kind: ThreadKind) -> NewThread {
        NewThread {
            id: ThreadId::new(),
            kind,
            workspace_id: None,
            cwd: f.thread.cwd.clone(),
            engine_id: engine.into(),
            account_id: None,
            model: None,
            effort: None,
            access: Access::Edits,
            mode: ThreadMode::Act,
            attended: true,
        }
    }

    #[tokio::test]
    async fn f_th_06_free_plan_starts_threads_only_on_claude_code_and_codex() {
        let f = fixture(OnStop::Ack, hang()).await;
        plan_switch(&f.svc).store(true, Ordering::SeqCst);
        for engine in ["claude", "codex"] {
            let made = f
                .svc
                .create(new_thread(&f, engine, ThreadKind::Thread))
                .await;
            assert!(made.is_ok(), "{engine}: {made:?}");
        }
        for engine in ["cursor", "grok", "opencode"] {
            let err = f
                .svc
                .create(new_thread(&f, engine, ThreadKind::Thread))
                .await
                .expect_err(engine);
            assert_eq!(err.code, ErrorCode::ForbiddenPlan, "{engine}");
            assert_eq!(err.message, tandem_core::entitlement::CHAT_ENGINE_ON_PRO);
        }
    }

    #[tokio::test]
    async fn f_th_06_pro_plan_starts_threads_on_every_chat_engine() {
        let f = fixture(OnStop::Ack, hang()).await;
        plan_switch(&f.svc);
        for engine in ["claude", "codex", "cursor", "grok", "opencode"] {
            let made = f
                .svc
                .create(new_thread(&f, engine, ThreadKind::Thread))
                .await;
            assert!(made.is_ok(), "{engine}: {made:?}");
        }
    }

    #[tokio::test]
    async fn f_th_06_free_plan_refuses_switching_a_thread_to_an_acp_engine() {
        let f = fixture(OnStop::Ack, hang()).await;
        plan_switch(&f.svc).store(true, Ordering::SeqCst);
        let err = f
            .svc
            .switch_engine(&f.thread.id, "cursor", None, "divider".into())
            .await
            .expect_err("cursor on Free");
        assert_eq!(err.code, ErrorCode::ForbiddenPlan);
        let kept = f.svc.thread(&f.thread.id).await.expect("thread");
        assert_eq!(kept.engine_id, "claude");
        let codex = f
            .svc
            .switch_engine(&f.thread.id, "codex", None, "divider".into())
            .await
            .expect("codex on Free");
        assert_eq!(codex.engine_id, "codex");
    }

    #[tokio::test]
    async fn f_th_06_after_a_lapse_an_acp_thread_keeps_its_history_but_starts_no_turn() {
        let f = fixture(OnStop::Ack, hang()).await;
        let free = plan_switch(&f.svc);
        let acp = f
            .svc
            .create(new_thread(&f, "grok", ThreadKind::Thread))
            .await
            .expect("grok on Pro");
        free.store(true, Ordering::SeqCst);
        let err = f
            .svc
            .send(&acp.id, "hello".into(), Vec::new(), false)
            .await
            .expect_err("grok on Free");
        assert_eq!(err.code, ErrorCode::ForbiddenPlan);
        let err = f
            .svc
            .send(&acp.id, "hello".into(), Vec::new(), true)
            .await
            .expect_err("queued on Free");
        assert_eq!(err.code, ErrorCode::ForbiddenPlan);
        assert_eq!(f.adapter.starts.load(Ordering::SeqCst), 0);
        assert!(repo::turns(&f.store, &acp.id)
            .await
            .expect("turns")
            .is_empty());
        // Still readable, and it can move to an engine Free runs.
        assert!(f.svc.get(&acp.id).await.is_ok());
        assert!(f.svc.events(&acp.id, None, 50).await.is_ok());
        f.svc
            .switch_engine(&acp.id, "claude", None, "divider".into())
            .await
            .expect("back to Claude Code");
        f.svc
            .send(&acp.id, "hello".into(), Vec::new(), false)
            .await
            .expect("Claude Code on Free");
        f.svc.stop(&acp.id).await.expect("stop");
    }

    #[tokio::test]
    async fn f_th_06_the_thread_limit_leaves_teammate_chats_to_their_own_limits() {
        let f = fixture(OnStop::Ack, hang()).await;
        plan_switch(&f.svc).store(true, Ordering::SeqCst);
        let made = f
            .svc
            .create(new_thread(&f, "cursor", ThreadKind::TeammateChat))
            .await;
        assert!(made.is_ok(), "{made:?}");
    }
}
