//! PTY sessions for terminal panes (ARCHITECTURE §2.4, §5): spawn without
//! waiting for output, one reader thread per pane, 16 ms batches sent as binary
//! frames over a per-pane sink, ack-based backpressure, a bounded scrollback
//! ring, answers to terminal queries, and process-group kill.

pub mod env;
pub mod frame;
pub mod orphans;
pub mod quote;
pub mod responder;
pub mod ring;
pub mod side;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::RwLock;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use frame::{FrameKind, PtyFrame};
pub use responder::{QueryResponder, TermColors};
pub use ring::ScrollbackRing;
pub use side::{PtyState, SideChannel};

use orphans::ChildRegistry;

/// Flush a batch after this long since its first byte (§5.5).
pub const BATCH_WINDOW: Duration = Duration::from_millis(16);
pub const BATCH_MAX: usize = 256 * 1024;
/// Stop reading when this many sent bytes are unacknowledged; resume below LOW.
pub const ACK_HIGH: u64 = 4 * 1024 * 1024;
pub const ACK_LOW: u64 = 1024 * 1024;
pub const READ_BUF: usize = 64 * 1024;
/// `pty:state` goes out at most this often per pane (ARCHITECTURE §3.3).
pub const STATE_DEBOUNCE: Duration = Duration::from_millis(100);

/// Receives `pty:state` changes (the app forwards them to the webview).
pub type StateSink = Arc<dyn Fn(PtyState) + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    #[error("no terminal with that id")]
    NotFound,
    #[error("couldn't start the terminal: {0}")]
    Spawn(String),
    #[error("terminal io: {0}")]
    Io(String),
}

impl From<PtyError> for tandem_core::IpcError {
    fn from(err: PtyError) -> Self {
        let code = match err {
            PtyError::NotFound => tandem_core::ErrorCode::NotFound,
            PtyError::Spawn(_) => tandem_core::ErrorCode::Unavailable,
            PtyError::Io(_) => tandem_core::ErrorCode::Io,
        };
        tandem_core::IpcError::new(code, err.to_string()).with_stage("pty")
    }
}

/// The receiving side of a sink went away (webview reloaded or closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkClosed;

/// Where a pane's frames go (a Tauri channel in the app, a Vec in tests).
pub trait OutputSink: Send + Sync {
    /// Err means the receiver is gone; the sink is dropped and output keeps
    /// going to the ring only.
    fn send(&self, frame: PtyFrame) -> Result<(), SinkClosed>;
}

#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub pane_id: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
    pub scrollback_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PtyInfo {
    pub pid: u32,
    pub started_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<String>,
}

struct FlowControl {
    sent: AtomicU64,
    acked: AtomicU64,
    gate: Mutex<()>,
    resume: Condvar,
}

impl FlowControl {
    fn unacked(&self) -> u64 {
        self.sent
            .load(Ordering::Acquire)
            .saturating_sub(self.acked.load(Ordering::Acquire))
    }
}

struct Session {
    pid: u32,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    sink: Arc<RwLock<Option<Arc<dyn OutputSink>>>>,
    ring: Arc<Mutex<ScrollbackRing>>,
    flow: Arc<FlowControl>,
    exited: Arc<Mutex<Option<ExitInfo>>>,
    seq: Arc<AtomicU64>,
    side: Mutex<SideChannel>,
    /// A `pty:state` change not sent yet (debounced).
    state_dirty: AtomicBool,
}

/// All live PTYs. Cheap to clone.
#[derive(Clone, Default)]
pub struct PtyManager {
    sessions: Arc<DashMap<String, Arc<Session>>>,
    colors: Arc<RwLock<TermColors>>,
    registry: Option<Arc<ChildRegistry>>,
    on_state: Option<StateSink>,
}

enum Msg {
    Data(Vec<u8>),
    Eof,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl PtyManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records every child in `run/children.json` for orphan detection.
    pub fn with_registry(mut self, registry: Arc<ChildRegistry>) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Where `pty:state` changes go.
    pub fn with_state_sink(mut self, sink: StateSink) -> Self {
        self.on_state = Some(sink);
        self
    }

    /// Colours answered to OSC 10/11/12 queries (F-SH-05 AC2).
    pub fn set_theme_colors(&self, colors: TermColors) {
        *self.colors.write() = colors;
    }

    pub fn theme_colors(&self) -> TermColors {
        self.colors.read().clone()
    }

    pub fn live_count(&self) -> usize {
        self.sessions
            .iter()
            .filter(|s| s.exited.lock().map(|e| e.is_none()).unwrap_or(false))
            .count()
    }

    /// Starts the process and returns at once; output streams to `sink`.
    pub fn spawn(
        &self,
        spec: SpawnSpec,
        sink: Option<Arc<dyn OutputSink>>,
    ) -> Result<PtyInfo, PtyError> {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: spec.rows.max(2),
                cols: spec.cols.max(2),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Spawn(e.to_string()))?;
        let mut cmd = CommandBuilder::new(&spec.program);
        cmd.args(&spec.args);
        cmd.cwd(&spec.cwd);
        cmd.env_clear();
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let child = pty
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::Spawn(e.to_string()))?;
        drop(pty.slave);
        let pid = child.process_id().unwrap_or(0);
        let reader = pty
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Spawn(e.to_string()))?;
        let writer = pty
            .master
            .take_writer()
            .map_err(|e| PtyError::Spawn(e.to_string()))?;

        let session = Arc::new(Session {
            pid,
            master: Mutex::new(pty.master),
            writer: Mutex::new(writer),
            sink: Arc::new(RwLock::new(sink)),
            ring: Arc::new(Mutex::new(ScrollbackRing::new(spec.scrollback_lines))),
            flow: Arc::new(FlowControl {
                sent: AtomicU64::new(0),
                acked: AtomicU64::new(0),
                gate: Mutex::new(()),
                resume: Condvar::new(),
            }),
            exited: Arc::new(Mutex::new(None)),
            seq: Arc::new(AtomicU64::new(0)),
            side: Mutex::new(SideChannel::new(&spec.pane_id, spec.rows, spec.cols)),
            state_dirty: AtomicBool::new(false),
        });
        self.sessions.insert(spec.pane_id.clone(), session.clone());
        let started_at = now_ms();
        if let Some(reg) = &self.registry {
            let exe = spec
                .program
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            reg.add(
                &spec.pane_id,
                pid,
                &exe,
                &spec.cwd.to_string_lossy(),
                started_at,
            );
        }

        let (tx, rx) = mpsc::sync_channel::<Msg>(256);
        self.start_reader(&spec.pane_id, session.clone(), reader, tx);
        let child_done = Arc::new((Mutex::new(false), Condvar::new()));
        let registered = self.registry.clone().map(|r| (r, spec.pane_id.clone()));
        Self::start_waiter(session.clone(), child, child_done.clone(), registered);
        Self::start_batcher(session, rx, child_done, self.on_state.clone());
        Ok(PtyInfo { pid, started_at })
    }

    fn start_reader(
        &self,
        pane: &str,
        session: Arc<Session>,
        mut reader: Box<dyn Read + Send>,
        tx: mpsc::SyncSender<Msg>,
    ) {
        let colors = self.colors.clone();
        let name = format!("pty-read-{pane}");
        let spawned = std::thread::Builder::new().name(name).spawn(move || {
            let mut buf = vec![0u8; READ_BUF];
            loop {
                // Backpressure only while a UI is attached (§5.5).
                if session.sink.read().is_some() && session.flow.unacked() > ACK_HIGH {
                    let guard = session.flow.gate.lock().unwrap_or_else(|e| e.into_inner());
                    let _unused = session
                        .flow
                        .resume
                        .wait_timeout_while(guard, Duration::from_millis(250), |_| {
                            session.flow.unacked() > ACK_LOW
                        })
                        .unwrap_or_else(|e| e.into_inner());
                    continue;
                }
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(Msg::Eof);
                        return;
                    }
                    Ok(n) => {
                        let chunk = &buf[..n];
                        // Queries are answered here, for visible and hidden
                        // panes alike, within the same read (§5.7).
                        let replies = match session.side.lock() {
                            Ok(mut side) => {
                                let replies = side.feed(chunk, &colors.read());
                                if side.state() != side_published(&side) {
                                    session.state_dirty.store(true, Ordering::Release);
                                }
                                replies
                            }
                            Err(_) => Vec::new(),
                        };
                        if !replies.is_empty() {
                            if let Ok(mut w) = session.writer.lock() {
                                let _ = w.write_all(&replies);
                                let _ = w.flush();
                            }
                        }
                        if let Ok(mut ring) = session.ring.lock() {
                            ring.push(chunk);
                        }
                        if tx.send(Msg::Data(chunk.to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        if let Err(err) = spawned {
            tracing::error!(error = %err, "couldn't start pty reader");
        }
    }

    fn start_waiter(
        session: Arc<Session>,
        mut child: Box<dyn Child + Send + Sync>,
        done: Arc<(Mutex<bool>, Condvar)>,
        registered: Option<(Arc<ChildRegistry>, String)>,
    ) {
        let _ = std::thread::Builder::new()
            .name("pty-wait".into())
            .spawn(move || {
                let status = child.wait();
                let info = match status {
                    Ok(s) => ExitInfo {
                        code: Some(s.exit_code() as i32),
                        signal: s.signal().map(str::to_string),
                    },
                    Err(_) => ExitInfo {
                        code: None,
                        signal: None,
                    },
                };
                if let Ok(mut e) = session.exited.lock() {
                    *e = Some(info);
                }
                if let Some((reg, pane)) = &registered {
                    reg.remove(pane);
                }
                let (lock, cv) = &*done;
                if let Ok(mut d) = lock.lock() {
                    *d = true;
                }
                cv.notify_all();
            });
    }

    fn start_batcher(
        session: Arc<Session>,
        rx: mpsc::Receiver<Msg>,
        child_done: Arc<(Mutex<bool>, Condvar)>,
        on_state: Option<StateSink>,
    ) {
        let _ = std::thread::Builder::new()
            .name("pty-batch".into())
            .spawn(move || {
                let send = |frame: PtyFrame| {
                    let len = frame.payload.len() as u64;
                    let mut gone = false;
                    if let Some(sink) = session.sink.read().as_ref() {
                        gone = sink.send(frame).is_err();
                        if !gone && len > 0 {
                            session.flow.sent.fetch_add(len, Ordering::AcqRel);
                        }
                    }
                    if gone {
                        *session.sink.write() = None;
                    }
                };
                // `pty:state` at most every 100 ms, the last change always sent.
                let mut last_state = Instant::now() - STATE_DEBOUNCE;
                let flush_state = |last: &mut Instant| {
                    let Some(sink) = &on_state else { return };
                    if !session.state_dirty.swap(false, Ordering::AcqRel) {
                        return;
                    }
                    let change = session.side.lock().ok().and_then(|mut s| s.take_change());
                    if let Some(state) = change {
                        sink(state);
                        *last = Instant::now();
                    }
                };
                let mut eof = false;
                while !eof {
                    let dirty = on_state.is_some() && session.state_dirty.load(Ordering::Acquire);
                    let first = if dirty {
                        let wait = STATE_DEBOUNCE.saturating_sub(last_state.elapsed());
                        match rx.recv_timeout(wait) {
                            Ok(m) => Ok(m),
                            Err(RecvTimeoutError::Timeout) => {
                                flush_state(&mut last_state);
                                continue;
                            }
                            Err(RecvTimeoutError::Disconnected) => Err(()),
                        }
                    } else {
                        rx.recv().map_err(|_| ())
                    };
                    let mut batch = match first {
                        Ok(Msg::Data(d)) => d,
                        Ok(Msg::Eof) | Err(_) => break,
                    };
                    let deadline = Instant::now() + BATCH_WINDOW;
                    while batch.len() < BATCH_MAX {
                        let left = deadline.saturating_duration_since(Instant::now());
                        if left.is_zero() {
                            break;
                        }
                        match rx.recv_timeout(left) {
                            Ok(Msg::Data(d)) => batch.extend_from_slice(&d),
                            Ok(Msg::Eof) | Err(RecvTimeoutError::Disconnected) => {
                                eof = true;
                                break;
                            }
                            Err(RecvTimeoutError::Timeout) => break,
                        }
                    }
                    let seq = session.seq.fetch_add(1, Ordering::AcqRel);
                    send(PtyFrame::data(seq, batch));
                    if last_state.elapsed() >= STATE_DEBOUNCE {
                        flush_state(&mut last_state);
                    }
                }
                flush_state(&mut last_state);
                // The exit frame always follows the last data frame.
                let (lock, cv) = &*child_done;
                let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                let _unused = cv
                    .wait_timeout_while(guard, Duration::from_secs(5), |done| !*done)
                    .unwrap_or_else(|e| e.into_inner());
                let info = session
                    .exited
                    .lock()
                    .ok()
                    .and_then(|e| e.clone())
                    .unwrap_or(ExitInfo {
                        code: None,
                        signal: None,
                    });
                let seq = session.seq.fetch_add(1, Ordering::AcqRel);
                send(PtyFrame::exit(seq, &info));
            });
    }

    fn get(&self, pane: &str) -> Result<Arc<Session>, PtyError> {
        self.sessions
            .get(pane)
            .map(|s| s.clone())
            .ok_or(PtyError::NotFound)
    }

    /// Replaces the sink (webview reload) and returns the ring contents to replay.
    pub fn attach(&self, pane: &str, sink: Arc<dyn OutputSink>) -> Result<Vec<u8>, PtyError> {
        let session = self.get(pane)?;
        let replay = session
            .ring
            .lock()
            .map(|r| r.contents())
            .unwrap_or_default();
        session
            .flow
            .acked
            .store(session.flow.sent.load(Ordering::Acquire), Ordering::Release);
        *session.sink.write() = Some(sink);
        Ok(replay)
    }

    pub fn write(&self, pane: &str, bytes: &[u8]) -> Result<(), PtyError> {
        let session = self.get(pane)?;
        let mut w = session
            .writer
            .lock()
            .map_err(|_| PtyError::Io("writer poisoned".into()))?;
        w.write_all(bytes)
            .map_err(|e| PtyError::Io(e.to_string()))?;
        w.flush().map_err(|e| PtyError::Io(e.to_string()))
    }

    pub fn ack(&self, pane: &str, bytes: u64) {
        if let Ok(session) = self.get(pane) {
            session.flow.acked.fetch_add(bytes, Ordering::AcqRel);
            session.flow.resume.notify_all();
        }
    }

    pub fn resize(&self, pane: &str, cols: u16, rows: u16) -> Result<(), PtyError> {
        let session = self.get(pane)?;
        if let Ok(mut side) = session.side.lock() {
            side.resize(rows, cols);
        }
        let master = session
            .master
            .lock()
            .map_err(|_| PtyError::Io("master poisoned".into()))?;
        master
            .resize(PtySize {
                rows: rows.max(2),
                cols: cols.max(2),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Io(e.to_string()))
    }

    pub fn pid(&self, pane: &str) -> Option<u32> {
        self.sessions.get(pane).map(|s| s.pid)
    }

    /// True while the pane's process is running.
    pub fn is_live(&self, pane: &str) -> bool {
        self.sessions
            .get(pane)
            .map(|s| s.exited.lock().map(|e| e.is_none()).unwrap_or(false))
            .unwrap_or(false)
    }

    /// Sends a one-line notice frame (e.g. "Folder not found — opened at
    /// workspace root") in sequence with the pane's output.
    pub fn notice(&self, pane: &str, message: &str) -> Result<(), PtyError> {
        let session = self.get(pane)?;
        let seq = session.seq.fetch_add(1, Ordering::AcqRel);
        if let Some(sink) = session.sink.read().as_ref() {
            let _ = sink.send(PtyFrame::notice(seq, message));
        }
        Ok(())
    }

    /// Modes and title as of now (attach, tests).
    pub fn state(&self, pane: &str) -> Option<PtyState> {
        let session = self.sessions.get(pane)?;
        let state = session.side.lock().ok().map(|s| s.state());
        state
    }

    /// Plain text of the pane's visible screen.
    pub fn screen_text(&self, pane: &str) -> Option<String> {
        let session = self.sessions.get(pane)?;
        let text = session.side.lock().ok().map(|s| s.screen_text());
        text
    }

    /// The last `lines` lines of the ring (raw bytes): the quit snapshot.
    pub fn tail(&self, pane: &str, lines: usize) -> Result<Vec<u8>, PtyError> {
        Ok(self
            .get(pane)?
            .ring
            .lock()
            .map(|r| r.tail(lines))
            .unwrap_or_default())
    }

    /// Panes with a session, live or exited.
    pub fn panes(&self) -> Vec<String> {
        self.sessions.iter().map(|e| e.key().clone()).collect()
    }

    pub fn exit_info(&self, pane: &str) -> Option<ExitInfo> {
        self.sessions
            .get(pane)
            .and_then(|s| s.exited.lock().ok().and_then(|e| e.clone()))
    }

    /// Raw ring contents (ANSI included). Callers redact before display or upload.
    pub fn scrollback(&self, pane: &str) -> Result<Vec<u8>, PtyError> {
        Ok(self
            .get(pane)?
            .ring
            .lock()
            .map(|r| r.contents())
            .unwrap_or_default())
    }

    /// Ends the process tree: SIGHUP, then SIGTERM, then SIGKILL, 2 s apart (§5.3).
    pub fn kill(&self, pane: &str) -> Result<ExitInfo, PtyError> {
        let session = self.get(pane)?;
        kill_tree(session.pid, &session.exited, Duration::from_secs(2));
        self.sessions.remove(pane);
        Ok(session
            .exited
            .lock()
            .ok()
            .and_then(|e| e.clone())
            .unwrap_or(ExitInfo {
                code: None,
                signal: Some("SIGKILL".into()),
            }))
    }

    pub fn kill_all(&self) {
        let panes: Vec<String> = self.sessions.iter().map(|e| e.key().clone()).collect();
        for pane in panes {
            let _ = self.kill(&pane);
        }
    }
}

/// The state last handed to `pty:state`, so the reader marks real changes only.
fn side_published(side: &SideChannel) -> PtyState {
    side.published()
}

fn exited(state: &Mutex<Option<ExitInfo>>) -> bool {
    state.lock().map(|e| e.is_some()).unwrap_or(true)
}

#[cfg(unix)]
fn kill_tree(pid: u32, state: &Mutex<Option<ExitInfo>>, step: Duration) {
    if pid == 0 {
        return;
    }
    // Collected first: once the leader dies its children are reparented.
    let escaped = descendants(pid);
    let group = -(pid as i32);
    for sig in [libc::SIGHUP, libc::SIGTERM, libc::SIGKILL] {
        if exited(state) {
            break;
        }
        // SAFETY: plain kill(2) on our own child's process group.
        unsafe {
            libc::kill(group, sig);
        }
        let until = Instant::now() + step;
        while Instant::now() < until {
            if exited(state) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    // Anything that left the group with setsid (daemonized `sleep &`).
    for child in escaped {
        // SAFETY: kill(2) on a process that was our descendant a moment ago.
        unsafe {
            libc::kill(child as i32, libc::SIGKILL);
        }
    }
}

/// All descendants of `root` (Linux reads /proc; other platforms return none
/// and rely on the process group).
pub fn descendants(root: u32) -> Vec<u32> {
    #[cfg(target_os = "linux")]
    {
        let mut parents: Vec<(u32, u32)> = Vec::new();
        if let Ok(dir) = std::fs::read_dir("/proc") {
            for entry in dir.flatten() {
                let Some(pid) = entry
                    .file_name()
                    .to_str()
                    .and_then(|n| n.parse::<u32>().ok())
                else {
                    continue;
                };
                let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
                    continue;
                };
                // Field 4 follows the parenthesised command name.
                let Some(rest) = stat.rfind(')').map(|i| &stat[i + 1..]) else {
                    continue;
                };
                if let Some(ppid) = rest.split_whitespace().nth(1).and_then(|p| p.parse().ok()) {
                    parents.push((pid, ppid));
                }
            }
        }
        let mut out = Vec::new();
        let mut frontier = vec![root];
        while let Some(p) = frontier.pop() {
            for (child, _) in parents.iter().filter(|(_, pp)| *pp == p) {
                if !out.contains(child) {
                    out.push(*child);
                    frontier.push(*child);
                }
            }
        }
        out
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = root;
        Vec::new()
    }
}

#[cfg(not(unix))]
fn kill_tree(_pid: u32, _state: &Mutex<Option<ExitInfo>>, _step: Duration) {
    // Windows: the Job Object with KILL_ON_JOB_CLOSE ends the tree (M3).
}

/// Used by tests to wait for a condition on collected frames.
pub fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + timeout;
    while Instant::now() < until {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    cond()
}

/// A sink that keeps frames in memory (tests, headless use).
#[derive(Default)]
pub struct VecSink {
    pub frames: Mutex<Vec<PtyFrame>>,
    pub closed: AtomicBool,
}

impl VecSink {
    pub fn data(&self) -> Vec<u8> {
        self.frames
            .lock()
            .map(|f| {
                f.iter()
                    .filter(|f| f.kind == FrameKind::Data)
                    .flat_map(|f| f.payload.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn has_exit(&self) -> bool {
        self.frames
            .lock()
            .map(|f| f.iter().any(|f| f.kind == FrameKind::Exit))
            .unwrap_or(false)
    }
}

impl OutputSink for VecSink {
    fn send(&self, frame: PtyFrame) -> Result<(), SinkClosed> {
        if self.closed.load(Ordering::Acquire) {
            return Err(SinkClosed);
        }
        self.frames.lock().map_err(|_| SinkClosed)?.push(frame);
        Ok(())
    }
}
