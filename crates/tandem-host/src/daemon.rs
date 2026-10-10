//! The session daemon (ARCHITECTURE §13.1 "Daemon"): it owns the host's
//! PTYs so they outlive an SSH connection. `tandem-host serve` (one per
//! connection) pipes the client's JSON-RPC to it over a Unix socket in
//! `~/.tandem-host/run/` (0700). Output is kept per PTY (1 MB) with byte
//! offsets, so a client that reconnects asks for `pty.attach{from_seq}` and
//! gets what it missed; the PTY and its PID are the same (F-RM-01 AC5).
//! With no client connected the daemon waits 10 minutes, then ends every
//! PTY and exits.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::protocol::{
    method, DirListing, EnginesDetect, ErrorKind, FsChanged, FsCreate, FsDirs, FsList, FsPath,
    FsRename, FsWatch, FsWatching, FsWrite, GitDiff, GitOp, GitRoot, Hello, HelloReply, Message,
    PtyAttach, PtyAttached, PtyEntry, PtyExit, PtyId, PtyOutput, PtyResize, PtySpawn, PtySpawned,
    PtyWrite, RpcError, StatusEvent, MAX_LINE, PROTOCOL, VERSION,
};
use crate::{detect, env, fsops, gitops};
use tandem_core::files::watch::{self, DirWatcher};

/// Output kept per PTY for re-attach.
pub const BUFFER: usize = 1024 * 1024;
/// How long PTYs outlive the last client.
pub const LINGER: Duration = Duration::from_secs(10 * 60);
/// Lines queued for a client before it counts as gone.
const CLIENT_QUEUE: usize = 4096;

/// `~/.tandem-host/run`, created 0700.
pub fn run_dir(home: &Path) -> PathBuf {
    home.join(".tandem-host").join("run")
}

/// This binary's identity: its version and the start of its SHA-256, so
/// a different build of the same version never talks to an older daemon.
pub fn build_id() -> &'static str {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(|| {
        let hash = std::env::current_exe()
            .ok()
            .and_then(|exe| crate::manifest::sha256_file(&exe).ok())
            .map(|h| h[..12].to_string())
            .unwrap_or_else(|| "unknown".into());
        format!("{VERSION}-{hash}")
    })
}

/// One socket per build, so an upgrade never talks to an old daemon.
pub fn socket_path(run: &Path) -> PathBuf {
    run.join(format!("daemon-{}.sock", build_id()))
}

pub fn status_socket_path(run: &Path) -> PathBuf {
    run.join(format!("status-{}.sock", build_id()))
}

pub fn ensure_run_dir(run: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(run)?;
    std::fs::set_permissions(run, std::fs::Permissions::from_mode(0o700))
}

fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

struct Pty {
    pid: u32,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    /// Output from offset `base` up to `base + buf.len()`.
    buf: Vec<u8>,
    base: u64,
    attached: Option<u64>,
    exit: Option<Option<i32>>,
}

impl Pty {
    fn end(&self) -> u64 {
        self.base + self.buf.len() as u64
    }

    fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
        if self.buf.len() > BUFFER {
            let cut = self.buf.len() - BUFFER;
            self.buf.drain(..cut);
            self.base += cut as u64;
        }
    }
}

/// Folders one connection may watch at once (inotify watches are a
/// per-user budget on the host).
const MAX_WATCHES: usize = 32;

struct State {
    ptys: HashMap<String, Pty>,
    clients: HashMap<u64, SyncSender<Vec<u8>>>,
    next_client: u64,
    idle_since: Option<Instant>,
    /// `fs.watch` subscriptions: id → (its connection, the watcher).
    watches: HashMap<String, (u64, DirWatcher)>,
    next_watch: u64,
}

impl State {
    fn send(&mut self, client: u64, msg: &Message) -> bool {
        let Some(tx) = self.clients.get(&client) else {
            return false;
        };
        match tx.try_send(msg.to_line()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                // A client that stopped reading is treated as gone.
                self.drop_client(client);
                false
            }
        }
    }

    fn drop_client(&mut self, client: u64) {
        // A connection's watches end with it; the desktop watches again
        // when it reconnects.
        let gone: Vec<String> = self
            .watches
            .iter()
            .filter(|(_, (c, _))| *c == client)
            .map(|(id, _)| id.clone())
            .collect();
        let stopped: Vec<DirWatcher> = gone
            .iter()
            .filter_map(|id| self.watches.remove(id).map(|(_, w)| w))
            .collect();
        drop_later(stopped);
        if self.clients.remove(&client).is_some() {
            for pty in self.ptys.values_mut() {
                if pty.attached == Some(client) {
                    pty.attached = None;
                }
            }
            if self.clients.is_empty() {
                self.idle_since = Some(Instant::now());
            }
        }
    }
}

/// Stops watchers off the state lock: a watcher's batch thread may be
/// waiting for that lock to send its notification.
fn drop_later(watchers: Vec<DirWatcher>) {
    if watchers.is_empty() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("unwatch".into())
        .spawn(move || drop(watchers));
}

#[derive(Clone)]
pub struct Daemon {
    state: Arc<Mutex<State>>,
    home: PathBuf,
    status_url: Option<String>,
    linger: Duration,
}

/// Parses `params` into `T`.
fn params<T: DeserializeOwned>(v: Option<Value>) -> Result<T, RpcError> {
    serde_json::from_value(v.unwrap_or(Value::Null))
        .map_err(|e| RpcError::new(ErrorKind::Protocol, format!("Bad parameters: {e}")))
}

fn to_value<T: serde::Serialize>(v: T) -> Result<Value, RpcError> {
    serde_json::to_value(v).map_err(|e| RpcError::new(ErrorKind::Protocol, e.to_string()))
}

impl Daemon {
    pub fn new(home: PathBuf, status_url: Option<String>, linger: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                ptys: HashMap::new(),
                clients: HashMap::new(),
                next_client: 1,
                idle_since: Some(Instant::now()),
                watches: HashMap::new(),
                next_watch: 1,
            })),
            home,
            status_url,
            linger,
        }
    }

    /// Serves `listener` until the linger runs out with no client.
    pub fn run(&self, listener: UnixListener) {
        let me = self.clone();
        let _ = std::thread::Builder::new()
            .name("accept".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    me.client(stream);
                }
            });
        loop {
            std::thread::sleep(Duration::from_millis(500));
            if self.expired() {
                self.kill_all();
                return;
            }
        }
    }

    fn expired(&self) -> bool {
        let s = self.state.lock();
        s.clients.is_empty() && s.idle_since.is_some_and(|t| t.elapsed() >= self.linger)
    }

    pub fn kill_all(&self) {
        let pids: Vec<u32> = self.state.lock().ptys.values().map(|p| p.pid).collect();
        for pid in pids {
            hangup(pid);
        }
    }

    /// Starts serving one connection (a `serve` pipe, or a test).
    pub fn client(&self, stream: UnixStream) {
        let (tx, rx) = sync_channel::<Vec<u8>>(CLIENT_QUEUE);
        let id = {
            let mut s = self.state.lock();
            let id = s.next_client;
            s.next_client += 1;
            s.clients.insert(id, tx);
            s.idle_since = None;
            id
        };
        let Ok(mut out) = stream.try_clone() else {
            self.state.lock().drop_client(id);
            return;
        };
        let _ = std::thread::Builder::new()
            .name("client-write".into())
            .spawn(move || {
                for line in rx {
                    if out.write_all(&line).is_err() {
                        let _ = out.shutdown(std::net::Shutdown::Both);
                        return;
                    }
                }
            });
        let me = self.clone();
        let _ = std::thread::Builder::new()
            .name("client-read".into())
            .spawn(move || {
                let mut reader = BufReader::new(stream);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    match (&mut reader)
                        .take(MAX_LINE as u64 + 1)
                        .read_until(b'\n', &mut line)
                    {
                        Ok(0) | Err(_) => break,
                        Ok(n) if n > MAX_LINE => break,
                        Ok(_) => me.handle_line(id, &line),
                    }
                }
                me.state.lock().drop_client(id);
            });
    }

    fn handle_line(&self, client: u64, line: &[u8]) {
        let msg: Message = match serde_json::from_slice(line) {
            Ok(m) => m,
            Err(_) => return,
        };
        let (Some(id), Some(name)) = (msg.id, msg.method.clone()) else {
            return;
        };
        let p = msg.params;
        // PTY calls are quick and keep their order; the rest may take a
        // while (git, detection) and run beside them.
        if name.starts_with("pty.") || name == method::HELLO {
            let out = self.dispatch(client, &name, p);
            self.state.lock().send(client, &Message::reply(id, out));
        } else {
            let me = self.clone();
            let _ = std::thread::Builder::new()
                .name("call".into())
                .spawn(move || {
                    let out = me.dispatch(client, &name, p);
                    me.state.lock().send(client, &Message::reply(id, out));
                });
        }
    }

    fn dispatch(&self, client: u64, name: &str, p: Option<Value>) -> Result<Value, RpcError> {
        match name {
            method::HELLO => {
                let hello: Hello = params(p)?;
                if hello.protocol != PROTOCOL {
                    return Err(RpcError::new(
                        ErrorKind::Unsupported,
                        format!(
                            "This host speaks protocol {PROTOCOL}, not {}",
                            hello.protocol
                        ),
                    ));
                }
                to_value(HelloReply {
                    protocol: PROTOCOL,
                    host_version: VERSION.into(),
                    os: std::env::consts::OS.into(),
                    arch: std::env::consts::ARCH.into(),
                    home: self.home.to_string_lossy().into_owned(),
                    daemon_pid: std::process::id(),
                })
            }
            method::PTY_SPAWN => to_value(self.spawn(client, params(p)?)?),
            method::PTY_WRITE => {
                let w: PtyWrite = params(p)?;
                let data = base64::engine::general_purpose::STANDARD
                    .decode(&w.data_b64)
                    .map_err(|_| RpcError::new(ErrorKind::Invalid, "That data isn't base64"))?;
                let mut s = self.state.lock();
                let pty = s.ptys.get_mut(&w.id).ok_or_else(no_pty)?;
                pty.writer
                    .write_all(&data)
                    .and_then(|_| pty.writer.flush())
                    .map_err(|e| RpcError::new(ErrorKind::Io, e.to_string()))?;
                Ok(Value::Null)
            }
            method::PTY_RESIZE => {
                let r: PtyResize = params(p)?;
                let s = self.state.lock();
                let pty = s.ptys.get(&r.id).ok_or_else(no_pty)?;
                pty.master
                    .resize(PtySize {
                        rows: r.rows.max(2),
                        cols: r.cols.max(2),
                        pixel_width: 0,
                        pixel_height: 0,
                    })
                    .map_err(|e| RpcError::new(ErrorKind::Io, e.to_string()))?;
                Ok(Value::Null)
            }
            method::PTY_KILL => {
                let k: PtyId = params(p)?;
                let pid = self
                    .state
                    .lock()
                    .ptys
                    .get(&k.id)
                    .map(|p| p.pid)
                    .ok_or_else(no_pty)?;
                kill_tree(pid);
                Ok(Value::Null)
            }
            method::PTY_ATTACH => to_value(self.attach(client, params(p)?)?),
            method::PTY_LIST => {
                let s = self.state.lock();
                let mut list: Vec<PtyEntry> = s
                    .ptys
                    .iter()
                    .map(|(id, p)| PtyEntry {
                        id: id.clone(),
                        pid: p.pid,
                        alive: p.exit.is_none(),
                        exit_code: p.exit.flatten(),
                    })
                    .collect();
                list.sort_by(|a, b| a.id.cmp(&b.id));
                to_value(list)
            }
            method::ENGINES_DETECT => {
                let d: EnginesDetect = params(p)?;
                to_value(detect::detect(&d.engines)?)
            }
            method::FS_LIST => to_value(fsops::list(&params::<FsList>(p)?)?),
            method::FS_READ => to_value(fsops::read(&params::<FsPath>(p)?)?),
            method::FS_WRITE => to_value(fsops::write(&params::<FsWrite>(p)?)?),
            method::FS_STAT => to_value(fsops::stat(&params::<FsPath>(p)?)?),
            method::FS_CREATE => to_value(fsops::create(&params::<FsCreate>(p)?)?),
            method::FS_RENAME => to_value(fsops::rename(&params::<FsRename>(p)?)?),
            method::FS_DELETE => to_value(fsops::delete(&params::<FsPath>(p)?)?),
            method::FS_DIRS => {
                let d: FsDirs = params(p)?;
                to_value::<DirListing>(fsops::dirs(&d.path, &self.home)?)
            }
            method::GIT_STATUS => {
                let g: GitRoot = params(p)?;
                to_value(gitops::status(Path::new(&g.root))?)
            }
            method::GIT_DIFF => {
                let g: GitDiff = params(p)?;
                to_value(gitops::diff(Path::new(&g.root), &g.path, g.staged)?)
            }
            method::GIT_OP => to_value(gitops::op(&params::<GitOp>(p)?)?),
            method::FS_WATCH => to_value(self.watch(client, params::<FsWatch>(p)?)?),
            method::FS_UNWATCH => {
                let w: FsWatching = params(p)?;
                let stopped = {
                    let mut s = self.state.lock();
                    match s.watches.get(&w.id) {
                        Some((c, _)) if *c == client => s.watches.remove(&w.id).map(|(_, w)| w),
                        _ => None,
                    }
                };
                drop_later(stopped.into_iter().collect());
                Ok(Value::Null)
            }
            other => Err(RpcError::new(
                ErrorKind::Protocol,
                format!("Unknown method {other}"),
            )),
        }
    }

    fn spawn(&self, client: u64, req: PtySpawn) -> Result<PtySpawned, RpcError> {
        if self
            .state
            .lock()
            .ptys
            .get(&req.id)
            .is_some_and(|p| p.exit.is_none())
        {
            return Err(RpcError::new(
                ErrorKind::Exists,
                "That terminal is already running",
            ));
        }
        let cwd = PathBuf::from(&req.cwd);
        if !cwd.is_dir() {
            return Err(RpcError::new(
                ErrorKind::NotFound,
                "The remote folder doesn't exist",
            ));
        }
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| s.starts_with('/'))
            .unwrap_or_else(|| "/bin/sh".into());
        let argv: Vec<String> = match (&req.argv, &req.command) {
            (Some(argv), _) if !argv.is_empty() => argv.clone(),
            (_, Some(cmd)) => vec![
                shell.clone(),
                "-lc".into(),
                format!("{cmd}\nexec {} -l", crate::quote(&shell)),
            ],
            _ => vec![shell.clone(), "-l".into()],
        };
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: req.rows.max(2),
                cols: req.cols.max(2),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| RpcError::new(ErrorKind::Io, e.to_string()))?;
        let mut cmd = CommandBuilder::new(&argv[0]);
        cmd.args(&argv[1..]);
        cmd.cwd(&cwd);
        cmd.env_clear();
        for (k, v) in env::compose(std::env::vars(), &req.env, self.status_url.as_deref()) {
            cmd.env(k, v);
        }
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| {
            RpcError::new(ErrorKind::Io, format!("Couldn't start {}: {e}", argv[0]))
        })?;
        drop(pair.slave);
        let pid = child.process_id().unwrap_or(0);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| RpcError::new(ErrorKind::Io, e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| RpcError::new(ErrorKind::Io, e.to_string()))?;
        self.state.lock().ptys.insert(
            req.id.clone(),
            Pty {
                pid,
                master: pair.master,
                writer,
                buf: Vec::new(),
                base: 0,
                attached: Some(client),
                exit: None,
            },
        );
        let state = self.state.clone();
        let id = req.id.clone();
        let _ = std::thread::Builder::new()
            .name("pty-read".into())
            .spawn(move || {
                let mut chunk = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let mut s = state.lock();
                            let Some(pty) = s.ptys.get_mut(&id) else {
                                break;
                            };
                            let seq = pty.end();
                            pty.push(&chunk[..n]);
                            if let Some(c) = pty.attached {
                                let note = Message::notification(
                                    method::PTY_OUTPUT,
                                    json!(PtyOutput {
                                        id: id.clone(),
                                        seq,
                                        data_b64: b64(&chunk[..n]),
                                    }),
                                );
                                s.send(c, &note);
                            }
                        }
                    }
                }
                let code = child.wait().ok().map(|s| s.exit_code() as i32);
                let mut s = state.lock();
                let target = s.ptys.get_mut(&id).map(|pty| {
                    pty.exit = Some(code);
                    pty.attached
                });
                if let Some(Some(c)) = target {
                    let note = Message::notification(
                        method::PTY_EXIT,
                        json!(PtyExit {
                            id: id.clone(),
                            code
                        }),
                    );
                    if s.send(c, &note) {
                        s.ptys.remove(&id);
                    }
                }
            });
        Ok(PtySpawned { pid })
    }

    fn attach(&self, client: u64, req: PtyAttach) -> Result<PtyAttached, RpcError> {
        let mut s = self.state.lock();
        let pty = s.ptys.get_mut(&req.id).ok_or_else(no_pty)?;
        pty.attached = Some(client);
        let from = req.from_seq.clamp(pty.base, pty.end());
        let replay = pty.buf[(from - pty.base) as usize..].to_vec();
        let (pid, exit) = (pty.pid, pty.exit);
        let reply = PtyAttached {
            pid,
            seq: from,
            alive: exit.is_none(),
        };
        // The replay is queued before the reply, so the client has it all
        // by the time attach returns; then the exit, if it came meanwhile.
        if !replay.is_empty() {
            let note = Message::notification(
                method::PTY_OUTPUT,
                json!(PtyOutput {
                    id: req.id.clone(),
                    seq: from,
                    data_b64: b64(&replay),
                }),
            );
            s.send(client, &note);
        }
        if let Some(code) = exit {
            let note = Message::notification(
                method::PTY_EXIT,
                json!(PtyExit {
                    id: req.id.clone(),
                    code
                }),
            );
            if s.send(client, &note) {
                s.ptys.remove(&req.id);
            }
        }
        Ok(reply)
    }

    /// `fs.watch` (D-1600): the same watcher as the desktop's own
    /// (`tandem_core::files::watch`, inotify per kept folder), its batches
    /// sent to this connection as `fs.changed`.
    fn watch(&self, client: u64, req: FsWatch) -> Result<FsWatching, RpcError> {
        let root = Path::new(&req.root)
            .canonicalize()
            .ok()
            .filter(|r| r.is_dir())
            .ok_or_else(|| RpcError::new(ErrorKind::NotFound, "The remote folder doesn't exist"))?;
        let id = {
            let mut s = self.state.lock();
            if s.watches.values().filter(|(c, _)| *c == client).count() >= MAX_WATCHES {
                return Err(RpcError::new(
                    ErrorKind::Unsupported,
                    "Too many folders are being watched on this host",
                ));
            }
            let id = format!("w{}", s.next_watch);
            s.next_watch += 1;
            id
        };
        let git = gitops::dirs(&root);
        let state = Arc::downgrade(&self.state);
        let note_id = id.clone();
        let watcher = watch::watch(
            &root,
            git,
            watch::WatchOptions::default(),
            Box::new(move |change| {
                let Some(state) = state.upgrade() else { return };
                let note = Message::notification(
                    method::FS_CHANGED,
                    json!(FsChanged {
                        id: note_id.clone(),
                        change,
                    }),
                );
                state.lock().send(client, &note);
            }),
        )
        .map_err(fsops::map_files)?;
        let mut s = self.state.lock();
        if !s.clients.contains_key(&client) {
            // The connection closed while the watcher started.
            drop(s);
            drop_later(vec![watcher]);
            return Err(RpcError::new(ErrorKind::NotFound, "The connection closed"));
        }
        s.watches.insert(id.clone(), (client, watcher));
        Ok(FsWatching { id })
    }

    /// Relays one hook report to the client attached to the pane it names.
    pub fn relay_status(&self, body: &[u8], signature: Option<String>) {
        let Ok(pane) = tandem_core::status_report::claimed_pane(body) else {
            return;
        };
        let mut s = self.state.lock();
        let Some(client) = s.ptys.get(&pane).and_then(|p| p.attached) else {
            return;
        };
        let note = Message::notification(
            method::STATUS_EVENT,
            json!(StatusEvent {
                body_b64: b64(body),
                signature,
            }),
        );
        s.send(client, &note);
    }

    /// The status endpoint hooks post to (`TANDEM_STATUS_URL`): HTTP/1.1
    /// `POST /v1/status` on a Unix socket, as the desktop's own.
    pub fn serve_status(&self, listener: UnixListener) {
        let me = self.clone();
        let _ = std::thread::Builder::new().name("status".into()).spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                if let Some((body, sig)) = read_report(&mut stream) {
                    me.relay_status(&body, sig);
                    let _ = stream.write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                } else {
                    let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                }
            }
        });
    }
}

fn no_pty() -> RpcError {
    RpcError::new(
        ErrorKind::NotFound,
        "That terminal isn't running on the host",
    )
}

/// The body and signature of a hook's `POST /v1/status`.
fn read_report(stream: &mut UnixStream) -> Option<(Vec<u8>, Option<String>)> {
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first).ok()?;
    if !first.starts_with("POST /v1/status ") {
        return None;
    }
    let mut len = 0usize;
    let mut sig = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (k, v) = line.split_once(':')?;
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        if k == "content-length" {
            len = v.parse().ok()?;
        } else if k == tandem_core::status_report::SIGNATURE_HEADER.to_ascii_lowercase() {
            sig = Some(v.to_string());
        }
    }
    if len > tandem_core::status_report::MAX_REPORT_BYTES {
        return None;
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).ok()?;
    Some((body, sig))
}

fn hangup(pid: u32) {
    if pid == 0 {
        return;
    }
    // SAFETY: kill(2) on a process group this daemon started.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGHUP);
        libc::kill(pid as i32, libc::SIGHUP);
    }
}

/// SIGHUP to the group, then SIGKILL 2 s later.
fn kill_tree(pid: u32) {
    hangup(pid);
    let _ = std::thread::Builder::new()
        .name("kill".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            // SAFETY: as above; harmless when the group is gone.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        });
}

/// `tandem-host daemon`: binds the sockets (or leaves when a daemon of this
/// version already runs) and serves until idle.
pub fn main(home: PathBuf) -> std::io::Result<()> {
    let run = run_dir(&home);
    ensure_run_dir(&run)?;
    let sock = socket_path(&run);
    if UnixStream::connect(&sock).is_ok() {
        return Ok(());
    }
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock)?;
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600))?;
    let status = status_socket_path(&run);
    let _ = std::fs::remove_file(&status);
    let status_listener = UnixListener::bind(&status).ok();
    let status_url = status_listener
        .as_ref()
        .map(|_| format!("unix://{}", status.display()));
    let linger = std::env::var("TANDEM_HOST_LINGER_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(LINGER);
    let daemon = Daemon::new(home, status_url, linger);
    if let Some(l) = status_listener {
        daemon.serve_status(l);
    }
    daemon.run(listener);
    let _ = std::fs::remove_file(&sock);
    let _ = std::fs::remove_file(&status);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;

    struct Client {
        out: UnixStream,
        lines: BufReader<UnixStream>,
        next: u64,
        notes: Vec<Message>,
    }

    impl Client {
        fn connect(d: &Daemon) -> Self {
            let (a, b) = UnixStream::pair().expect("pair");
            d.client(b);
            a.set_read_timeout(Some(Duration::from_secs(10)))
                .expect("timeout");
            Self {
                out: a.try_clone().expect("clone"),
                lines: BufReader::new(a),
                next: 1,
                notes: Vec::new(),
            }
        }

        fn call(&mut self, m: &str, p: Value) -> Result<Value, RpcError> {
            let id = self.next;
            self.next += 1;
            self.out
                .write_all(&Message::request(id, m, p).to_line())
                .expect("send");
            loop {
                let msg = self.read();
                if msg.id == Some(id) {
                    return match msg.error {
                        Some(e) => Err(e),
                        None => Ok(msg.result.unwrap_or(Value::Null)),
                    };
                }
                self.notes.push(msg);
            }
        }

        fn read(&mut self) -> Message {
            let mut line = String::new();
            self.lines.read_line(&mut line).expect("line");
            serde_json::from_str(&line).expect("json")
        }

        /// Output of `id` seen so far, waiting until it contains `want`.
        fn output_until(&mut self, id: &str, want: &str) -> (String, u64) {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let mut text = String::new();
                let mut end = 0;
                for n in &self.notes {
                    if n.method.as_deref() == Some(method::PTY_OUTPUT) {
                        let o: PtyOutput =
                            serde_json::from_value(n.params.clone().expect("p")).expect("o");
                        if o.id == id {
                            let d = base64::engine::general_purpose::STANDARD
                                .decode(o.data_b64)
                                .expect("b64");
                            text.push_str(&String::from_utf8_lossy(&d));
                            end = o.seq + d.len() as u64;
                        }
                    }
                }
                if text.contains(want) {
                    return (text, end);
                }
                assert!(Instant::now() < deadline, "no {want:?} in {text:?}");
                let m = self.read();
                self.notes.push(m);
            }
        }
    }

    fn daemon() -> (tempfile::TempDir, Daemon) {
        let dir = tempfile::tempdir().expect("tmp");
        let d = Daemon::new(dir.path().to_path_buf(), None, Duration::from_millis(300));
        (dir, d)
    }

    fn spawn_sh(c: &mut Client, id: &str, cwd: &Path, script: &str) -> u32 {
        let v = c
            .call(
                method::PTY_SPAWN,
                json!({"id": id, "argv": ["/bin/sh", "-c", script], "cwd": cwd, "cols": 80, "rows": 24,
                       "env": {"TANDEM_PANE_ID": id, "CLAUDE_CONFIG_DIR": "/x/.tandem/providers/claude/a"}}),
            )
            .expect("spawn");
        v["pid"].as_u64().expect("pid") as u32
    }

    #[test]
    fn f_rm_01_1_spawn_and_see_output() {
        let (dir, d) = daemon();
        let mut c = Client::connect(&d);
        let hello = c
            .call(
                method::HELLO,
                json!({"protocol": PROTOCOL, "client_version": VERSION}),
            )
            .expect("hello");
        assert_eq!(hello["protocol"], PROTOCOL);
        spawn_sh(&mut c, "p1", dir.path(), "echo hi; env");
        let (text, _) = c.output_until("p1", "hi");
        assert!(text.contains("hi"));
        let (text, _) = c.output_until("p1", "TANDEM_PANE_ID=p1");
        // F-RM-01 AC4: the provider home never reaches the remote spawn.
        assert!(!text.contains(".tandem/providers"), "{text}");
        assert!(!text.contains("CLAUDE_CONFIG_DIR"), "{text}");
        let bad = c.call(
            method::HELLO,
            json!({"protocol": 99, "client_version": "x"}),
        );
        assert_eq!(bad.expect_err("mismatch").kind, ErrorKind::Unsupported);
    }

    #[test]
    fn f_rm_01_5_reattach_gets_the_same_pid_and_the_missed_output() {
        let (dir, d) = daemon();
        let mut c = Client::connect(&d);
        let pid = spawn_sh(
            &mut c,
            "p2",
            dir.path(),
            "echo one; read x; echo two-$x; sleep 30",
        );
        let (_, end) = c.output_until("p2", "one");
        // The connection drops; the PTY keeps running.
        drop(c);
        std::thread::sleep(Duration::from_millis(100));
        let mut c2 = Client::connect(&d);
        // Input while nobody watched is fine once re-attached.
        let att = c2
            .call(method::PTY_ATTACH, json!({"id": "p2", "from_seq": end}))
            .expect("attach");
        assert_eq!(att["pid"].as_u64(), Some(pid as u64));
        assert_eq!(att["alive"], true);
        c2.call(
            method::PTY_WRITE,
            json!({"id": "p2", "data_b64": b64(b"x\n")}),
        )
        .expect("write");
        let (text, _) = c2.output_until("p2", "two-x");
        assert!(
            !text.contains("one"),
            "the replay starts where the client left off: {text:?}"
        );
        let list = c2.call(method::PTY_LIST, json!({})).expect("list");
        assert_eq!(list[0]["pid"].as_u64(), Some(pid as u64));
        c2.call(method::PTY_KILL, json!({"id": "p2"}))
            .expect("kill");
    }

    #[test]
    fn f_rm_01_ptys_end_when_the_linger_runs_out() {
        let (dir, d) = daemon();
        let mut c = Client::connect(&d);
        let pid = spawn_sh(&mut c, "p3", dir.path(), "sleep 30");
        drop(c);
        // `run` returns once idle for the linger (300 ms here).
        let l = UnixListener::bind(dir.path().join("t.sock")).expect("bind");
        d.run(l);
        std::thread::sleep(Duration::from_millis(200));
        // SAFETY: probing a pid with signal 0.
        let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
        assert!(!alive, "the PTY outlived the daemon's linger");
    }

    #[test]
    fn f_rm_01_status_relay_reaches_the_panes_client() {
        let (dir, d) = daemon();
        let mut c = Client::connect(&d);
        spawn_sh(&mut c, "p4", dir.path(), "sleep 5");
        let body = br#"{"v":1,"pane_id":"p4","event":"working","ts":1,"payload":{}}"#;
        d.relay_status(body, Some("sig".into()));
        let note = c.read();
        assert_eq!(note.method.as_deref(), Some(method::STATUS_EVENT));
        let ev: StatusEvent = serde_json::from_value(note.params.expect("p")).expect("ev");
        assert_eq!(ev.signature.as_deref(), Some("sig"));
        // Unknown panes go nowhere.
        d.relay_status(br#"{"pane_id":"nobody"}"#, None);
        c.call(method::PTY_KILL, json!({"id": "p4"})).expect("kill");
    }

    #[test]
    fn f_rm_01_3_files_through_the_daemon_are_contained() {
        let (dir, d) = daemon();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).expect("mkdir");
        let mut c = Client::connect(&d);
        let err = c
            .call(
                method::FS_READ,
                json!({"root": root, "path": "../../etc/passwd"}),
            )
            .expect_err("refused");
        assert_eq!(err.kind, ErrorKind::PathEscape);
    }

    #[test]
    fn f_rm_01_a_watched_folder_reports_changes_to_its_connection() {
        let (dir, d) = daemon();
        let root = dir.path().join("project");
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        let mut c = Client::connect(&d);
        let w: FsWatching = serde_json::from_value(
            c.call(method::FS_WATCH, json!({"root": root}))
                .expect("watch"),
        )
        .expect("id");
        std::fs::write(root.join("src/new.rs"), "fn main() {}").expect("write");
        let deadline = Instant::now() + Duration::from_secs(10);
        let changed = loop {
            assert!(Instant::now() < deadline, "no fs.changed");
            let m = c.read();
            if m.method.as_deref() == Some(method::FS_CHANGED) {
                let ch: FsChanged = serde_json::from_value(m.params.expect("p")).expect("ch");
                if ch.change.paths.iter().any(|p| p == "src/new.rs") {
                    break ch;
                }
            }
        };
        assert_eq!(changed.id, w.id);
        // Another connection can't stop it; its own can.
        let mut other = Client::connect(&d);
        other
            .call(method::FS_UNWATCH, json!({"id": w.id}))
            .expect("no-op");
        assert_eq!(d.state.lock().watches.len(), 1);
        c.call(method::FS_UNWATCH, json!({"id": w.id}))
            .expect("unwatch");
        assert!(d.state.lock().watches.is_empty());
        // A connection's watches end when it closes.
        c.call(method::FS_WATCH, json!({"root": root}))
            .expect("again");
        assert_eq!(d.state.lock().watches.len(), 1);
        drop(c);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !d.state.lock().watches.is_empty() {
            assert!(
                Instant::now() < deadline,
                "the closed connection's watch stayed"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        // A folder that isn't there is refused.
        let err = other
            .call(method::FS_WATCH, json!({"root": dir.path().join("nope")}))
            .expect_err("missing");
        assert_eq!(err.kind, ErrorKind::NotFound);
    }
}
