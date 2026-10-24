//! PTY sessions for terminal panes (ARCHITECTURE §2.4, §5): spawn without
//! waiting for output, one reader thread per pane, 16 ms batches sent as binary
//! frames over a per-pane sink, ack-based backpressure, a bounded scrollback
//! ring, answers to terminal queries, and process-group kill.

pub mod env;
pub mod frame;
pub mod governor;
pub mod integration;
#[cfg(windows)]
mod job;
pub mod marks;
pub mod mask;
pub mod orphans;
pub mod ports;
pub mod quote;
pub mod remote;
pub mod responder;
pub mod ring;
pub mod side;
pub mod suspend;
pub mod text;
pub mod typing;
pub mod winpath;

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
pub use marks::{Mark, Ran};
pub use remote::{RemoteFeed, RemotePty};
pub use responder::{QueryResponder, TermColors};
pub use ring::ScrollbackRing;
pub use side::{Fed, PtyState, SideChannel};

use orphans::ChildRegistry;

/// Flush a batch after this long since its first byte (§5.5).
pub const BATCH_WINDOW: Duration = Duration::from_millis(16);
pub const BATCH_MAX: usize = 256 * 1024;
/// The ring holds the scrollback plus a screen's rows, as xterm does, so a
/// parked view's base fits (§5.1, D-139).
pub const SCREEN_ROWS: usize = 256;
/// Stop reading when this many sent bytes are unacknowledged; resume below LOW.
pub const ACK_HIGH: u64 = 4 * 1024 * 1024;
pub const ACK_LOW: u64 = 1024 * 1024;
pub const READ_BUF: usize = 64 * 1024;
/// `pty:state` goes out at most this often per pane (ARCHITECTURE §3.3).
pub const STATE_DEBOUNCE: Duration = Duration::from_millis(100);

/// Receives `pty:state` changes (the app forwards them to the webview).
pub type StateSink = Arc<dyn Fn(PtyState) + Send + Sync>;
/// Told when an agent exits and its pane hands back to the shell: (pane, exit code).
pub type HandbackSink = Arc<dyn Fn(&str, i32) + Send + Sync>;
/// Called with each shell-integration mark (`pty:mark`, F-CD-10).
pub type MarkSink = Arc<dyn Fn(&str, &Mark) + Send + Sync>;
/// Called with each chunk's size as a pane's program writes (status
/// heuristics, F-EN-05). Runs on the pane's reader thread: keep it short.
pub type ActivitySink = Arc<dyn Fn(&str, usize) + Send + Sync>;
/// Called when a pane's reader recovered from a panic (`failed` false) or
/// gave up after 3 in 10 minutes (`failed` true): `(pane, restarts, failed)`.
/// Output keeps flowing either way; after giving up it's passed through raw.
pub type ReaderFailureSink = Arc<dyn Fn(&str, usize, bool) + Send + Sync>;

/// Tests: the next N feeds panic, to exercise the reader's recovery.
#[cfg(test)]
pub(crate) static PANIC_NEXT_FEEDS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

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
    /// This launch's token: shell-integration and handback marks must carry
    /// it (§5.8); `None` accepts no marks.
    pub mark_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PtyInfo {
    pub pid: u32,
    pub started_at: i64,
}

/// One live terminal's numbers, for Help › Diagnostics (§5.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaneMetrics {
    pub pane_id: String,
    /// How long opening the PTY and starting the program took.
    #[ts(type = "number")]
    pub spawn_ms: u64,
    /// Output sent to the UI that it hasn't acknowledged yet (bytes).
    #[ts(type = "number")]
    pub queued_bytes: u64,
    pub exited: bool,
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
    /// `None` for a remote PTY (F-RM-01): it resizes through `remote`.
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    /// A PTY on an SSH host: its PID isn't ours to signal.
    remote: Option<Arc<dyn RemotePty>>,
    writer: Mutex<Box<dyn Write + Send>>,
    sink: Arc<RwLock<Option<Arc<dyn OutputSink>>>>,
    ring: Arc<Mutex<ScrollbackRing>>,
    flow: Arc<FlowControl>,
    exited: Arc<Mutex<Option<ExitInfo>>>,
    seq: Arc<AtomicU64>,
    side: Mutex<SideChannel>,
    /// A `pty:state` change not sent yet (debounced).
    state_dirty: AtomicBool,
    /// How long the spawn took (Diagnostics).
    spawn_ms: AtomicU64,
    /// Windows: the kill-on-close Job Object holding the tree.
    #[cfg(windows)]
    job: Option<job::Job>,
    /// Secret values masked in this pane's output (F-TL-12).
    mask: Option<Arc<MaskSlot>>,
}

/// A pane's masker; `done` stops its flusher, and `wake` tells it a tail
/// is held (or the reader ended).
struct MaskSlot {
    masker: Mutex<mask::LiteralMasker>,
    done: AtomicBool,
    wake: std::sync::Condvar,
}

/// All live PTYs. Cheap to clone.
#[derive(Clone, Default)]
pub struct PtyManager {
    sessions: Arc<DashMap<String, Arc<Session>>>,
    colors: Arc<RwLock<TermColors>>,
    registry: Option<Arc<ChildRegistry>>,
    on_state: Option<StateSink>,
    on_handback: Option<HandbackSink>,
    on_mark: Option<MarkSink>,
    on_activity: Option<ActivitySink>,
    on_reader_failure: Option<ReaderFailureSink>,
    /// Values the pane's next spawn masks (F-TL-12).
    masks: Arc<DashMap<String, Vec<zeroize::Zeroizing<Vec<u8>>>>>,
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

    /// Records every child in this run's `run/children-<pid>.json` for orphan detection.
    pub fn with_registry(mut self, registry: Arc<ChildRegistry>) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Where agent handbacks go (Saved sessions, F-CD-03 AC3).
    pub fn with_handback_sink(mut self, sink: HandbackSink) -> Self {
        self.on_handback = Some(sink);
        self
    }

    /// Where shell-integration marks go (`pty:mark`).
    pub fn with_mark_sink(mut self, sink: MarkSink) -> Self {
        self.on_mark = Some(sink);
        self
    }

    /// Where output activity goes (status heuristics).
    pub fn with_activity_sink(mut self, sink: ActivitySink) -> Self {
        self.on_activity = Some(sink);
        self
    }

    /// Where reader recoveries and failures go (the watchdog, §5.2).
    pub fn with_reader_failure_sink(mut self, sink: ReaderFailureSink) -> Self {
        self.on_reader_failure = Some(sink);
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
        let spawn_started = Instant::now();
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: spec.rows.max(2),
                cols: spec.cols.max(2),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Spawn(e.to_string()))?;
        let cmd = if tandem_core::sandbox::in_flatpak() {
            // On the host, with exactly the pane's environment (D-2032).
            let args: Vec<std::ffi::OsString> = spec.args.iter().map(Into::into).collect();
            let argv = tandem_core::sandbox::flatpak_host_argv(
                spec.program.as_os_str(),
                &args,
                Some(spec.cwd.as_path()),
                Some(&spec.env.iter().collect::<Vec<_>>()),
            );
            let mut cmd = CommandBuilder::from_argv(argv);
            if let Some(home) = std::env::var_os("HOME") {
                cmd.cwd(home);
            }
            cmd
        } else {
            let mut cmd = CommandBuilder::new(&spec.program);
            cmd.args(&spec.args);
            cmd.cwd(&spec.cwd);
            cmd.env_clear();
            for (k, v) in &spec.env {
                cmd.env(k, v);
            }
            cmd
        };
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
            master: Mutex::new(Some(pty.master)),
            remote: None,
            writer: Mutex::new(writer),
            sink: Arc::new(RwLock::new(sink)),
            ring: Arc::new(Mutex::new(ScrollbackRing::new(
                spec.scrollback_lines + SCREEN_ROWS,
            ))),
            flow: Arc::new(FlowControl {
                sent: AtomicU64::new(0),
                acked: AtomicU64::new(0),
                gate: Mutex::new(()),
                resume: Condvar::new(),
            }),
            exited: Arc::new(Mutex::new(None)),
            seq: Arc::new(AtomicU64::new(0)),
            side: Mutex::new(SideChannel::with_token(
                &spec.pane_id,
                spec.rows,
                spec.cols,
                spec.mark_token.clone(),
            )),
            state_dirty: AtomicBool::new(false),
            spawn_ms: AtomicU64::new(0),
            mask: self.take_mask(&spec.pane_id),
            #[cfg(windows)]
            job: job::Job::for_process(pid),
        });
        session.spawn_ms.store(
            spawn_started.elapsed().as_millis() as u64,
            Ordering::Release,
        );
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

    /// A PTY that runs on an SSH host (F-RM-01): `pid` is the remote one,
    /// its output and exit arrive through the returned feed, and writes,
    /// resizes and kills go to `backend`. It's never in the child registry
    /// and never signalled locally. `spawn_ms` is how long the host took to
    /// start it, as Diagnostics shows it (D-823).
    pub fn spawn_remote(
        &self,
        spec: SpawnSpec,
        pid: u32,
        spawn_ms: u64,
        backend: Arc<dyn RemotePty>,
        sink: Option<Arc<dyn OutputSink>>,
    ) -> Result<(PtyInfo, RemoteFeed), PtyError> {
        let (feed, reader, child) = remote::channel(pid, backend.clone());
        let session = Arc::new(Session {
            pid,
            master: Mutex::new(None),
            remote: Some(backend.clone()),
            writer: Mutex::new(Box::new(remote::RemoteWriter(backend))),
            sink: Arc::new(RwLock::new(sink)),
            ring: Arc::new(Mutex::new(ScrollbackRing::new(
                spec.scrollback_lines + SCREEN_ROWS,
            ))),
            flow: Arc::new(FlowControl {
                sent: AtomicU64::new(0),
                acked: AtomicU64::new(0),
                gate: Mutex::new(()),
                resume: Condvar::new(),
            }),
            exited: Arc::new(Mutex::new(None)),
            seq: Arc::new(AtomicU64::new(0)),
            side: Mutex::new(SideChannel::with_token(
                &spec.pane_id,
                spec.rows,
                spec.cols,
                spec.mark_token.clone(),
            )),
            state_dirty: AtomicBool::new(false),
            spawn_ms: AtomicU64::new(spawn_ms),
            mask: self.take_mask(&spec.pane_id),
            #[cfg(windows)]
            job: None,
        });
        self.sessions.insert(spec.pane_id.clone(), session.clone());
        let (tx, rx) = mpsc::sync_channel::<Msg>(256);
        self.start_reader(&spec.pane_id, session.clone(), Box::new(reader), tx);
        let child_done = Arc::new((Mutex::new(false), Condvar::new()));
        Self::start_waiter(session.clone(), Box::new(child), child_done.clone(), None);
        Self::start_batcher(session, rx, child_done, self.on_state.clone());
        Ok((
            PtyInfo {
                pid,
                started_at: now_ms(),
            },
            feed,
        ))
    }

    /// True for a PTY on an SSH host.
    pub fn is_remote(&self, pane: &str) -> bool {
        self.sessions
            .get(pane)
            .map(|s| s.remote.is_some())
            .unwrap_or(false)
    }

    /// The pane's next spawn masks these values in its output (F-TL-12).
    pub fn mask_next_spawn(&self, pane: &str, values: Vec<zeroize::Zeroizing<Vec<u8>>>) {
        self.masks.insert(pane.to_string(), values);
    }

    fn take_mask(&self, pane: &str) -> Option<Arc<MaskSlot>> {
        let (_, values) = self.masks.remove(pane)?;
        let masker = mask::LiteralMasker::new(values.iter().map(|v| v.as_slice()))?;
        Some(Arc::new(MaskSlot {
            masker: Mutex::new(masker),
            done: AtomicBool::new(false),
            wake: std::sync::Condvar::new(),
        }))
    }

    /// Releases a masked pane's held tail after [`mask::HOLD`] of quiet,
    /// through the screen parser like any read, until the reader ends.
    fn start_flusher(
        pane: &str,
        session: Arc<Session>,
        slot: Arc<MaskSlot>,
        colors: Arc<RwLock<TermColors>>,
        tx: mpsc::SyncSender<Msg>,
    ) {
        let spawned = std::thread::Builder::new()
            .name(format!("pty-mask-{pane}"))
            .spawn(move || {
                let mut masker = slot.masker.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if slot.done.load(Ordering::Acquire) {
                        return;
                    }
                    // Sleeps until a read holds a tail, then until it's due;
                    // a read in between restarts the wait.
                    match masker.due_in() {
                        None => {
                            masker = slot.wake.wait(masker).unwrap_or_else(|e| e.into_inner());
                            continue;
                        }
                        Some(left) if !left.is_zero() => {
                            masker = slot
                                .wake
                                .wait_timeout(masker, left)
                                .unwrap_or_else(|e| e.into_inner())
                                .0;
                            continue;
                        }
                        Some(_) => {}
                    }
                    let held = masker.flush();
                    let output = match session.side.lock() {
                        Ok(mut side) => {
                            let fed = side.feed_raw(&held, &colors.read());
                            if side.state() != side_published(&side) {
                                session.state_dirty.store(true, Ordering::Release);
                            }
                            fed.output
                        }
                        Err(_) => held,
                    };
                    if !output.is_empty() && tx.send(Msg::Data(output)).is_err() {
                        return;
                    }
                }
            });
        if let Err(err) = spawned {
            tracing::error!(error = %err, "couldn't start the pty masker");
        }
    }

    fn start_reader(
        &self,
        pane: &str,
        session: Arc<Session>,
        mut reader: Box<dyn Read + Send>,
        tx: mpsc::SyncSender<Msg>,
    ) {
        let colors = self.colors.clone();
        let on_handback = self.on_handback.clone();
        let on_mark = self.on_mark.clone();
        let on_activity = self.on_activity.clone();
        let on_failure = self.on_reader_failure.clone();
        let pane_id = pane.to_string();
        if let Some(slot) = &session.mask {
            Self::start_flusher(
                pane,
                session.clone(),
                slot.clone(),
                colors.clone(),
                tx.clone(),
            );
        }
        let name = format!("pty-read-{pane}");
        let spawned = std::thread::Builder::new().name(name).spawn(move || {
            let mut buf = vec![0u8; READ_BUF];
            let mut budget = tandem_core::supervisor::RestartBudget::default();
            let mut gave_up = false;
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
                        if let Some(slot) = &session.mask {
                            // A held tail wasn't a whole value.
                            let mut masker = slot.masker.lock().unwrap_or_else(|e| e.into_inner());
                            let held = masker.flush();
                            slot.done.store(true, Ordering::Release);
                            slot.wake.notify_one();
                            if !held.is_empty() {
                                let _ = tx.send(Msg::Data(held));
                            }
                        }
                        let _ = tx.send(Msg::Eof);
                        return;
                    }
                    Ok(n) => {
                        // Secret values go before anything reads the output:
                        // the screen, the ring and every reader of the pane
                        // see the mask (F-TL-12). The lock keeps the flusher's
                        // releases in order with the reads.
                        let mut masker = session
                            .mask
                            .as_ref()
                            .map(|m| m.masker.lock().unwrap_or_else(|e| e.into_inner()));
                        let masked = masker.as_mut().map(|m| m.push(&buf[..n]));
                        if let Some(slot) = &session.mask {
                            slot.wake.notify_one();
                        }
                        let input: &[u8] = masked.as_deref().unwrap_or(&buf[..n]);
                        if input.is_empty() {
                            continue;
                        }
                        // Marks come out of the stream first; queries are
                        // answered here, for visible and hidden panes alike,
                        // within the same read (§5.7, §5.8).
                        let raw = || Fed {
                            output: input.to_vec(),
                            ..Fed::default()
                        };
                        let fed = if gave_up {
                            raw()
                        } else {
                            let fed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                match session.side.lock() {
                                    Ok(mut side) => {
                                        #[cfg(test)]
                                        if PANIC_NEXT_FEEDS
                                            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                                                n.checked_sub(1)
                                            })
                                            .is_ok()
                                        {
                                            panic!("test: a feed panicked");
                                        }
                                        let fed = side.feed_raw(input, &colors.read());
                                        if side.state() != side_published(&side) {
                                            session.state_dirty.store(true, Ordering::Release);
                                        }
                                        fed
                                    }
                                    Err(_) => raw(),
                                }
                            }));
                            match fed {
                                Ok(fed) => fed,
                                Err(_) => {
                                    // The watchdog (§5.2): a fresh parser and
                                    // on, at most 3 times per 10 minutes; then
                                    // the output is passed through raw.
                                    session.side.clear_poison();
                                    if let Ok(mut side) = session.side.lock() {
                                        side.recover();
                                    }
                                    gave_up = !budget.try_restart(now_ms());
                                    tracing::warn!(pane = %pane_id, gave_up, "pty reader recovered from a panic");
                                    if let Some(sink) = &on_failure {
                                        sink(&pane_id, budget.recent(), gave_up);
                                    }
                                    raw()
                                }
                            }
                        };
                        for mark in &fed.marks {
                            match (mark, &on_handback, &on_mark) {
                                (Mark::Handback(code), Some(sink), _) => sink(&pane_id, *code),
                                (Mark::Handback(_), None, _) => {}
                                (m, _, Some(sink)) => sink(&pane_id, m),
                                _ => {}
                            }
                        }
                        let Fed {
                            output: chunk,
                            replies,
                            ..
                        } = fed;
                        if !replies.is_empty() {
                            if let Ok(mut w) = session.writer.lock() {
                                let _ = w.write_all(&replies);
                                let _ = w.flush();
                            }
                        }
                        if chunk.is_empty() {
                            continue;
                        }
                        // The batcher puts it in the ring as it sends it (§5.1).
                        if let Some(sink) = &on_activity {
                            sink(&pane_id, chunk.len());
                        }
                        if tx.send(Msg::Data(chunk)).is_err() {
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
                    // The ring takes the batch as the sink gets it, under one
                    // lock, so an attach's replay and the frames after it
                    // neither repeat nor skip a byte (§5.1, D-139).
                    {
                        let mut ring = session.ring.lock().unwrap_or_else(|e| e.into_inner());
                        ring.push(&batch);
                        let seq = session.seq.fetch_add(1, Ordering::AcqRel);
                        send(PtyFrame::data(seq, batch));
                    }
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

    /// Replaces the sink (webview reload, a parked view shown again) and
    /// returns the ring contents to replay with the position they end at.
    pub fn attach(&self, pane: &str, sink: Arc<dyn OutputSink>) -> Result<Attached, PtyError> {
        let session = self.get(pane)?;
        let ring = session.ring.lock().unwrap_or_else(|e| e.into_inner());
        session
            .flow
            .acked
            .store(session.flow.sent.load(Ordering::Acquire), Ordering::Release);
        *session.sink.write() = Some(sink);
        Ok(Attached {
            replay: ring.contents(),
            end: ring.end(),
        })
    }

    /// A hidden view gave up its terminal (§5.1, D-139): `base` (one chunk
    /// per line, then the screen) stands for everything up to position `at`,
    /// which is what the view had parsed. The sink is dropped, so the program
    /// runs on without backpressure and its output waits in the ring for the
    /// next attach. False when the ring no longer holds the output after
    /// `at`; it then replays as it is.
    pub fn park(&self, pane: &str, at: u64, base: Vec<Vec<u8>>) -> Result<bool, PtyError> {
        let session = self.get(pane)?;
        let mut ring = session.ring.lock().unwrap_or_else(|e| e.into_inner());
        *session.sink.write() = None;
        session.flow.resume.notify_all();
        Ok(ring.rebase(at, base))
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
        if let Some(remote) = &session.remote {
            remote.resize(cols.max(2), rows.max(2));
            return Ok(());
        }
        let master = session
            .master
            .lock()
            .map_err(|_| PtyError::Io("master poisoned".into()))?;
        let Some(master) = master.as_ref() else {
            return Err(PtyError::NotFound);
        };
        master
            .resize(PtySize {
                rows: rows.max(2),
                cols: cols.max(2),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Io(e.to_string()))
    }

    /// The pane's foreground process group (the job the terminal is
    /// running, such as an engine started from its shell), on Unix.
    pub fn foreground_group(&self, pane: &str) -> Option<i32> {
        #[cfg(unix)]
        {
            let session = self.sessions.get(pane)?.clone();
            let master = session.master.lock().ok()?;
            master.as_ref()?.process_group_leader()
        }
        #[cfg(not(unix))]
        {
            let _ = pane;
            None
        }
    }

    pub fn pid(&self, pane: &str) -> Option<u32> {
        self.sessions.get(pane).map(|s| s.pid)
    }

    /// Pauses the pane's job in group `agent` for the runaway guard
    /// (F-AC-09), stopping the pane's own shell first (D-857).
    pub fn pause_job(
        &self,
        pane: &str,
        agent: i32,
        control: &dyn suspend::ProcessControl,
    ) -> Result<suspend::PausedJob, suspend::SuspendError> {
        // The pane's process leads its own session and group.
        let shell = self.pid(pane).and_then(|p| i32::try_from(p).ok());
        suspend::pause(control, agent, shell)
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
    /// The last finished command's plain-text output and exit code (F-CD-10 AC3).
    pub fn last_output(&self, pane: &str) -> Option<(String, Option<i32>)> {
        let session = self.sessions.get(pane)?.clone();
        let out = session.side.lock().ok().and_then(|s| s.last_output());
        out
    }

    /// The pane's shell is at its prompt (shell integration said so).
    pub fn at_prompt(&self, pane: &str) -> bool {
        let Some(session) = self.sessions.get(pane) else {
            return false;
        };
        let at = session.side.lock().map(|s| s.at_prompt()).unwrap_or(false);
        at
    }

    pub fn screen_text(&self, pane: &str) -> Option<String> {
        let session = self.sessions.get(pane)?;
        let text = session.side.lock().ok().map(|s| s.screen_text());
        text
    }

    /// The last `lines` lines of output as plain text, laid out at the
    /// pane's width (handoff briefs, F-CD-16).
    pub fn plain_tail(&self, pane: &str, lines: u16) -> Result<String, PtyError> {
        let session = self.get(pane)?;
        let cols = session.side.lock().map(|s| s.cols()).unwrap_or(120);
        let bytes = session
            .ring
            .lock()
            .map(|r| r.tail(lines as usize))
            .unwrap_or_default();
        Ok(text::render_plain(&bytes, lines, cols))
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
        if let Some(remote) = &session.remote {
            // The host ends the tree; its exit comes back through the feed.
            remote.kill();
            let until = Instant::now() + Duration::from_secs(5);
            while !exited(&session.exited) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
            self.sessions.remove(pane);
            return Ok(session
                .exited
                .lock()
                .ok()
                .and_then(|e| e.clone())
                .unwrap_or(ExitInfo {
                    code: None,
                    signal: Some("SIGHUP".into()),
                }));
        }
        #[cfg(windows)]
        if let Some(job) = &session.job {
            job.terminate();
        }
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

    /// Every live terminal's numbers (Help › Diagnostics).
    pub fn metrics(&self) -> Vec<PaneMetrics> {
        let mut out: Vec<PaneMetrics> = self
            .sessions
            .iter()
            .map(|e| PaneMetrics {
                pane_id: e.key().clone(),
                spawn_ms: e.value().spawn_ms.load(Ordering::Acquire),
                queued_bytes: e.value().flow.unacked(),
                exited: e
                    .value()
                    .exited
                    .lock()
                    .map(|x| x.is_some())
                    .unwrap_or(false),
            })
            .collect();
        out.sort_by(|a, b| a.pane_id.cmp(&b.pane_id));
        out
    }

    pub fn kill_all(&self) {
        let panes: Vec<String> = self.sessions.iter().map(|e| e.key().clone()).collect();
        for pane in panes {
            let _ = self.kill(&pane);
        }
    }

    /// App exit: stops every child within [`QUIT_DEADLINE`], then drops this
    /// run's registry file so the next launch has nothing to ask about.
    pub fn shutdown(&self) {
        self.shutdown_within(QUIT_DEADLINE);
    }

    /// Quit (ARCHITECTURE §1.4, F-SH-07): every tree gets SIGHUP, SIGTERM
    /// and SIGKILL 2 s apart, all panes in parallel; whatever is still alive
    /// at `deadline` is force-killed so quitting never hangs.
    pub fn shutdown_within(&self, deadline: Duration) {
        let until = Instant::now() + deadline;
        let sessions: Vec<Arc<Session>> = self.sessions.iter().map(|e| e.value().clone()).collect();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        for session in &sessions {
            if let Some(remote) = &session.remote {
                remote.kill();
                let _ = done_tx.send(());
                continue;
            }
            #[cfg(windows)]
            if let Some(job) = &session.job {
                job.terminate();
            }
            let (s, tx) = (session.clone(), done_tx.clone());
            let spawned = std::thread::Builder::new()
                .name("pty-quit".into())
                .spawn(move || {
                    kill_tree(s.pid, &s.exited, Duration::from_secs(2));
                    let _ = tx.send(());
                });
            if spawned.is_err() {
                force_kill(session.pid);
                let _ = done_tx.send(());
            }
        }
        drop(done_tx);
        let mut finished = 0;
        while finished < sessions.len() {
            let left = until.saturating_duration_since(Instant::now());
            match done_rx.recv_timeout(left) {
                Ok(()) => finished += 1,
                Err(_) => break,
            }
        }
        let forced = sessions
            .iter()
            .filter(|s| s.remote.is_none() && !exited(&s.exited))
            .count();
        if forced > 0 {
            tracing::warn!(
                code = "pty.quit_forced",
                count = forced,
                "processes outlived the quit deadline; force-killing"
            );
            for s in sessions
                .iter()
                .filter(|s| s.remote.is_none() && !exited(&s.exited))
            {
                force_kill(s.pid);
            }
        }
        self.sessions.clear();
        if let Some(reg) = &self.registry {
            reg.close();
        }
    }
}

/// How long quitting may take before every child tree is force-killed.
pub const QUIT_DEADLINE: Duration = Duration::from_secs(5);

/// SIGKILL to a tree at once: its group, the root and every descendant.
#[cfg(unix)]
fn force_kill(pid: u32) {
    if pid == 0 {
        return;
    }
    let escaped = descendants(pid);
    // SAFETY: kill(2) on our own child's group and its descendants.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
        libc::kill(pid as i32, libc::SIGKILL);
        for child in escaped {
            libc::kill(child as i32, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn force_kill(pid: u32) {
    if pid == 0 {
        return;
    }
    use orphans::ProcessTable;
    orphans::SystemTable::default().kill(pid);
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
    // A paused job (F-AC-09) continues first, so nothing is left stopped:
    // the pane's shell (D-857) and every descendant.
    // SAFETY: kill(2) on our own child.
    unsafe {
        libc::kill(pid as i32, libc::SIGCONT);
    }
    for child in &escaped {
        // SAFETY: kill(2) on our own descendant.
        unsafe {
            libc::kill(*child as i32, libc::SIGCONT);
        }
    }
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

/// Windows: the Job Object already ended the tree (see `kill`); this waits
/// for the exit and stops the root directly if there was no job.
#[cfg(not(unix))]
fn kill_tree(pid: u32, state: &Mutex<Option<ExitInfo>>, step: Duration) {
    if wait_until(step, || exited(state)) || pid == 0 {
        return;
    }
    let mut table = orphans::SystemTable::default();
    use orphans::ProcessTable;
    table.kill(pid);
    let _ = wait_until(step, || exited(state));
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

/// What `attach` hands back: the ring to replay and the position it ends at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    pub replay: Vec<u8>,
    pub end: u64,
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

#[cfg(all(test, unix))]
mod watchdog_tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;

    fn spec(pane: &str, script: &str) -> SpawnSpec {
        let mut env = BTreeMap::new();
        env.insert(
            "PATH".into(),
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        );
        SpawnSpec {
            pane_id: pane.into(),
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), script.into()],
            cwd: std::env::temp_dir(),
            env,
            cols: 80,
            rows: 24,
            scrollback_lines: 1000,
            mark_token: None,
        }
    }

    #[test]
    fn nfr_5_2_a_reader_that_panics_recovers_three_times_then_reports_and_keeps_the_output() {
        let reports: Arc<Mutex<Vec<(String, usize, bool)>>> = Arc::default();
        let seen = reports.clone();
        let mgr = PtyManager::new().with_reader_failure_sink(Arc::new(move |pane, n, failed| {
            if let Ok(mut r) = seen.lock() {
                r.push((pane.to_string(), n, failed));
            }
        }));
        let sink = Arc::new(VecSink::default());
        mgr.spawn(
            spec(
                "wd1",
                "for i in 1 2 3 4 5 6; do read line; echo out-$line; done",
            ),
            Some(sink.clone() as Arc<dyn OutputSink>),
        )
        .expect("spawn");
        let text = || String::from_utf8_lossy(&sink.data()).into_owned();
        let count = || reports.lock().map(|r| r.len()).unwrap_or(0);
        for i in 1..=4 {
            PANIC_NEXT_FEEDS.store(1, Ordering::SeqCst);
            mgr.write("wd1", format!("p{i}\r").as_bytes())
                .expect("write");
            assert!(
                wait_until(Duration::from_secs(5), || count() >= i),
                "panic {i} reported"
            );
            // The output still reaches the pane.
            assert!(
                wait_until(Duration::from_secs(5), || text()
                    .contains(&format!("out-p{i}"))),
                "out-p{i} shown: {}",
                text()
            );
        }
        let r = reports.lock().expect("reports").clone();
        assert_eq!(
            r,
            vec![
                ("wd1".to_string(), 1, false),
                ("wd1".to_string(), 2, false),
                ("wd1".to_string(), 3, false),
                ("wd1".to_string(), 3, true),
            ]
        );
        // After giving up the output is passed through raw: no more panics.
        mgr.write("wd1", b"after\r").expect("write");
        assert!(wait_until(Duration::from_secs(5), || text().contains("out-after")));
        assert_eq!(count(), 4);
        PANIC_NEXT_FEEDS.store(0, Ordering::SeqCst);
        let _ = mgr.kill("wd1");
    }
}

#[cfg(all(test, target_os = "linux"))]
mod pause_tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// The agent wrapper's shape (`env::agent_wrapper`): an interactive shell
    /// runs the agent as a job, then hands the pane back when it ends.
    fn wrapped(pane: &str, job: &str) -> SpawnSpec {
        let mut env = BTreeMap::new();
        env.insert(
            "PATH".into(),
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        );
        SpawnSpec {
            pane_id: pane.into(),
            program: PathBuf::from("/bin/sh"),
            args: vec!["-ic".into(), format!("{job}; echo HANDBACK-$?")],
            cwd: std::env::temp_dir(),
            env,
            cols: 80,
            rows: 24,
            scrollback_lines: 1000,
            mark_token: None,
        }
    }

    #[test]
    fn f_ac_09_8_a_paused_agent_keeps_its_pane_and_its_shell_never_hands_it_back() {
        let mgr = PtyManager::new();
        let sink = Arc::new(VecSink::default());
        mgr.spawn(
            wrapped("pj", "sleep 3"),
            Some(sink.clone() as Arc<dyn OutputSink>),
        )
        .expect("spawn");
        let text = || String::from_utf8_lossy(&sink.data()).into_owned();
        let shell = mgr.pid("pj").expect("pid") as i32;
        // The job holds the terminal, in a group of its own.
        assert!(wait_until(Duration::from_secs(5), || mgr
            .foreground_group("pj")
            .is_some_and(|g| g != shell)));
        let agent = mgr.foreground_group("pj").expect("group");
        let job = mgr
            .pause_job("pj", agent, &suspend::SignalControl)
            .expect("pause");
        assert_eq!(job.shell, Some(shell));
        std::thread::sleep(Duration::from_millis(800));
        // Stopping only the job, the shell took the terminal back and went on
        // (HANDBACK-147): the pane went to a new shell, the agent astray.
        assert!(!text().contains("HANDBACK"), "{}", text());
        assert_eq!(mgr.foreground_group("pj"), Some(agent));
        suspend::resume(&suspend::SignalControl, &job).expect("resume");
        assert_eq!(mgr.foreground_group("pj"), Some(agent));
        // The job ends as usual, and so does the shell after it.
        assert!(
            wait_until(Duration::from_secs(10), || text().contains("HANDBACK-0")),
            "{}",
            text()
        );
        let _ = mgr.kill("pj");
    }
}
