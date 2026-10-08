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
use tandem_core::approvals::{
    ApprovalBroker, ApprovalKind, ApprovalOutcome, ApprovalRequest, ApprovalScope,
};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::clock::Clock;
use tandem_core::ids::{AttachmentId, ThreadId, TurnId};
use tandem_core::source::SourceRef;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::chat::{
    AdapterEvent, ChatAdapter, ChatSession, ChatStart, InterruptOutcome, TurnInput,
};
use tandem_engines::events::{
    ApprovalDecisionView, ErrorAction, ErrorStage, ThreadEvent, ThreadEventEnvelope, ToolStatus,
    TurnEndReason,
};
use tandem_gateway::{ApprovalAnswer, ApprovalAsker, Caller, ToolApproval};
use tandem_store::repo::threads::{
    self as repo, MessageRow, NewThread, ThreadKind, ThreadRecord, ThreadState, TurnRecord,
    TurnState,
};
use tandem_store::Store;
use tokio::sync::{broadcast, mpsc, Mutex};
use ts_rs::TS;

/// How often an open text block is saved as a partial row.
const FLUSH_EVERY: Duration = Duration::from_secs(2);
/// After an acknowledged stop, how long the engine has to end the turn.
const ACKED_END: Duration = Duration::from_secs(3);
/// Live envelopes buffered per subscriber before it must resync.
const LIVE_BUFFER: usize = 4096;

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
}

pub struct ThreadService {
    store: Store,
    bus: EventBus,
    clock: Arc<dyn Clock>,
    approvals: Arc<ApprovalBroker>,
    launcher: Arc<dyn ChatLauncher>,
    runtimes: StdMutex<HashMap<String, Arc<Mutex<Runtime>>>>,
    live: broadcast::Sender<ThreadEventEnvelope>,
}

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

/// The approval card's kind and one-line summary for a tool use
/// ("Run `npm test`").
fn describe(ask: &ToolApproval, cwd: &std::path::Path) -> (ApprovalKind, String) {
    let what = tandem_engines::chat::claude::arg_summary(&ask.tool_name, &ask.input, cwd);
    match ask.tool_name.as_str() {
        "Bash" => (ApprovalKind::Command, format!("Run `{what}`")),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            (ApprovalKind::FileWrite, format!("Edit `{what}`"))
        }
        "WebFetch" => (ApprovalKind::Network, format!("Fetch `{what}`")),
        "WebSearch" => (
            ApprovalKind::Network,
            format!("Search the web for `{what}`"),
        ),
        t if t.starts_with("mcp__") => (ApprovalKind::Mcp, format!("Use `{what}`")),
        _ => (ApprovalKind::Other, format!("Use `{what}`")),
    }
}

/// The gateway's approvals tool, answered by the thread that asked.
pub struct GatewayAsker {
    threads: std::sync::Weak<ThreadService>,
}

#[async_trait]
impl ApprovalAsker for GatewayAsker {
    async fn ask(&self, caller: &Caller, ask: ToolApproval) -> ApprovalAnswer {
        match (caller, self.threads.upgrade()) {
            (Caller::Thread { thread_id }, Some(threads)) => {
                threads.ask_approval(thread_id, ask).await
            }
            _ => ApprovalAnswer::Deny { message: None },
        }
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
        })
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

    async fn publish_threads(&self) {
        if let Ok(threads) = repo::list(&self.store, ThreadKind::Thread, true).await {
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
        let thread = repo::insert(&self.store, new).await.map_err(store_err)?;
        tracing::info!(code = "threads.created", engine = %thread.engine_id, "thread created");
        self.publish_threads().await;
        Ok(thread)
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
        let rt = self.runtime(id);
        let rt = rt.lock().await;
        Ok(ThreadDetail {
            thread,
            turns,
            running: rt.turn.as_ref().map(|r| r.id.clone()),
            queued: rt.queued.as_ref().map(|q| q.text.clone()),
            draft,
        })
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
    async fn emit(&self, thread: &ThreadId, run: &mut Running, event: ThreadEvent) {
        let seq = run.take_seq();
        self.store_event(thread, &run.id, seq, &event, None).await;
        self.broadcast(self.envelope(thread, &run.id, seq, event));
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

    /// Saves the open block as a partial row (every 2 s while streaming).
    async fn flush(&self, thread: &ThreadId) {
        let rt = self.runtime(thread);
        let mut rt = rt.lock().await;
        let Some(run) = rt.turn.as_mut() else {
            return;
        };
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
    async fn end_turn(
        &self,
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
        tracing::info!(code = "threads.turn_ended", reason = ?reason, "turn ended");
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
        let thread = self.thread(id).await?;
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
        };
        self.emit(
            id,
            &mut run,
            ThreadEvent::User {
                text: text.clone(),
                attachments: attachments.clone(),
            },
        )
        .await;
        guard.turn = Some(run);
        self.set_state(id, ThreadState::Working).await;

        let session = match guard.session.clone() {
            Some(s) => s,
            None => match self.start_session(&thread, &mut guard).await {
                Ok(s) => s,
                Err(e) => {
                    self.fail_turn(
                        id,
                        &mut guard,
                        "spawn_failed",
                        &e.message,
                        ErrorStage::Spawn,
                    )
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
        let input = TurnInput {
            turn_id: turn_id.clone(),
            text,
            images: Vec::new(),
            files: Vec::new(),
        };
        if let Err(e) = session.send_turn(input).await {
            let mut guard = rt.lock().await;
            if guard.turn.as_ref().map(|r| &r.id) == Some(&turn_id) {
                self.fail_turn(
                    id,
                    &mut guard,
                    "send_failed",
                    &e.to_string(),
                    ErrorStage::Unknown,
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
        let (tx, rx) = mpsc::channel(256);
        let session: Arc<dyn ChatSession> = Arc::from(
            adapter
                .start(start, tx)
                .await
                .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?,
        );
        rt.generation += 1;
        rt.session = Some(session.clone());
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
                action: Some(ErrorAction::Retry),
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
            AdapterEvent::Exited { code, stderr_tail } => {
                if generation != guard.generation || guard.session.is_none() {
                    return;
                }
                guard.session = None;
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
            AdapterEvent::Event { turn_id, event } => {
                // F-TH-03 AC3: a finished turn's events change nothing.
                let Some(run) = guard.turn.as_mut().filter(|r| r.id == turn_id) else {
                    return;
                };
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
                        self.emit(thread, run, other).await;
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

    /// A tool use the engine asks about (F-TH-04 AC4, F-TH-11): an approval
    /// card in the running turn and the Dashboard, until someone answers or
    /// it expires (a deny).
    pub async fn ask_approval(&self, id: &ThreadId, ask: ToolApproval) -> ApprovalAnswer {
        let Ok(thread) = self.thread(id).await else {
            return ApprovalAnswer::Deny { message: None };
        };
        let (kind, summary) = describe(&ask, std::path::Path::new(&thread.cwd));
        let detail = serde_json::to_string_pretty(&ask.input).unwrap_or_default();
        let detail =
            tandem_core::redact::redact(&tandem_engines::events::truncate_output(&detail).0);
        let rt = self.runtime(id);
        let (view, ticket, turn) = {
            let mut guard = rt.lock().await;
            if guard.turn.is_none() {
                // No turn to show it in: nobody could answer.
                return ApprovalAnswer::Deny { message: None };
            }
            // Waiting before the card shows anywhere (F-TH-11).
            self.set_state(id, ThreadState::Waiting).await;
            let Some(run) = guard.turn.as_mut() else {
                return ApprovalAnswer::Deny { message: None };
            };
            let (view, ticket) = self.approvals.request(ApprovalRequest {
                source: SourceRef::Thread {
                    thread_id: id.as_str().to_string(),
                },
                kind,
                summary,
                detail,
                options: vec![ApprovalScope::Once, ApprovalScope::Thread],
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
        let (decision, answer) = match outcome {
            ApprovalOutcome::Allowed { scope, .. } => {
                (ApprovalDecisionView::Allow { scope }, ApprovalAnswer::Allow)
            }
            ApprovalOutcome::Denied { reason, .. } => (
                ApprovalDecisionView::Deny {
                    reason: reason.clone(),
                },
                ApprovalAnswer::Deny { message: reason },
            ),
            ApprovalOutcome::Expired => (
                ApprovalDecisionView::Expired,
                ApprovalAnswer::Deny {
                    message: Some("Nobody answered within 30 minutes".into()),
                },
            ),
            ApprovalOutcome::Cancelled => (
                ApprovalDecisionView::Deny { reason: None },
                ApprovalAnswer::Deny { message: None },
            ),
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
        answer
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

/// The app's launcher: the thread's engine and account, resolved like a
/// pane's (provider home, sanitized login env), run by that engine's adapter,
/// with the gateway (started on first use) as its only MCP server.
pub struct AppLauncher {
    terminals: Arc<crate::services::terminal::TerminalService>,
    adapters: HashMap<String, Arc<dyn ChatAdapter>>,
    /// `$DATA/mcp`: one 0600 config per thread session.
    mcp_dir: std::path::PathBuf,
    gateway: tokio::sync::OnceCell<tandem_gateway::GatewayHandle>,
    threads: std::sync::OnceLock<std::sync::Weak<ThreadService>>,
    /// The live lease per thread; a new session revokes the last one.
    leases: StdMutex<HashMap<String, tandem_core::ids::LeaseId>>,
}

impl AppLauncher {
    pub fn new(
        terminals: Arc<crate::services::terminal::TerminalService>,
        adapters: Vec<Arc<dyn ChatAdapter>>,
        mcp_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            terminals,
            adapters: adapters
                .into_iter()
                .map(|a| (a.engine_id().to_string(), a))
                .collect(),
            mcp_dir,
            gateway: tokio::sync::OnceCell::new(),
            threads: std::sync::OnceLock::new(),
            leases: StdMutex::default(),
        }
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
        vec![Arc::new(tandem_engines::chat::claude::ClaudeAdapter::new(
            claude_limits,
        ))]
    }

    /// The threads the gateway's approvals tool reports to.
    pub fn bind(&self, threads: &Arc<ThreadService>) {
        let _ = self.threads.set(Arc::downgrade(threads));
    }

    async fn gateway(&self) -> Result<&tandem_gateway::GatewayHandle, IpcError> {
        self.gateway
            .get_or_try_init(|| async {
                let threads = self.threads.get().cloned().unwrap_or_default();
                tandem_gateway::Gateway::start(tandem_gateway::GatewayDeps {
                    approvals: Arc::new(GatewayAsker { threads }),
                })
                .await
                .map_err(|e| {
                    IpcError::new(ErrorCode::Unavailable, e.to_string()).with_stage("gateway")
                })
            })
            .await
    }

    /// A lease for the thread's new session, in a config only the user can
    /// read (`$DATA/mcp/<thread>.json`, 0600).
    async fn mcp_config(&self, thread: &ThreadRecord) -> Result<std::path::PathBuf, IpcError> {
        let gateway = self.gateway().await?;
        let lease = gateway.prepare(tandem_gateway::SessionDescriptor {
            caller: Caller::Thread {
                thread_id: thread.id.clone(),
            },
            attended: if thread.attended {
                tandem_gateway::Attended::Interactive
            } else {
                tandem_gateway::Attended::Unattended {
                    origin: "thread".into(),
                }
            },
            access: thread.access,
            builtins: tandem_gateway::BuiltinSet::for_thread(),
        });
        let previous = match self.leases.lock() {
            Ok(mut l) => l.insert(thread.id.as_str().to_string(), lease.id.clone()),
            Err(_) => None,
        };
        if let Some(old) = previous {
            gateway.revoke(&old);
        }
        let config = serde_json::json!({"mcpServers": {"tandem": {
            "type": "http",
            "url": gateway.url,
            "headers": {"Authorization": format!("Bearer {}", lease.token)}
        }}});
        std::fs::create_dir_all(&self.mcp_dir)
            .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        let path = self.mcp_dir.join(format!("{}.json", thread.id.as_str()));
        tandem_core::atomic::atomic_write(&path, config.to_string().as_bytes(), Some(0o600))
            .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        Ok(path)
    }
}

#[async_trait]
impl ChatLauncher for AppLauncher {
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
                access: thread.access,
                mode: thread.mode,
                attended: thread.attended,
                resume_session: thread.engine_session_id.clone(),
                mcp_config: Some(self.mcp_config(thread).await?),
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tandem_core::access::{Access, ThreadMode};
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
    }

    struct ScriptedSession {
        script: Script,
        on_stop: OnStop,
        events: mpsc::Sender<AdapterEvent>,
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
            Ok(Box::new(ScriptedSession {
                script: self.script.clone(),
                on_stop: self.on_stop,
                events,
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
            _id: &ApprovalId,
            _d: ApprovalDecision,
        ) -> Result<(), AdapterError> {
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
        let detail = f.svc.get(&f.thread.id).await.expect("detail");
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
        for _ in 0..300 {
            if turn_states(&f).await.len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            turn_states(&f).await,
            vec![TurnState::Completed, TurnState::Running]
        );
        let users: Vec<String> = stored(&f)
            .await
            .into_iter()
            .filter_map(|e| match e {
                ThreadEvent::User { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(users, vec!["first".to_string(), "next".to_string()]);
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
        assert_eq!(
            f.svc.get(id).await.expect("detail").thread.state,
            ThreadState::Failed
        );

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
        let gateway = tandem_gateway::Gateway::start(tandem_gateway::GatewayDeps {
            approvals: Arc::new(GatewayAsker {
                threads: Arc::downgrade(&f.svc),
            }),
        })
        .await
        .expect("gateway");
        let lease = gateway.prepare(tandem_gateway::SessionDescriptor {
            caller: Caller::Thread {
                thread_id: id.clone(),
            },
            attended: tandem_gateway::Attended::Interactive,
            access: tandem_core::access::Access::Edits,
            builtins: tandem_gateway::BuiltinSet::for_thread(),
        });
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
}
