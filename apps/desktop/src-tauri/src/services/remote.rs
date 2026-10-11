//! SSH hosts and their connections (F-RM-01, ARCHITECTURE §13.1).
//!
//! One connection per host: the system `ssh` master (`tandem_host::ssh`),
//! `tandem-host serve` on it, and a JSON-RPC client. Remote panes are
//! sessions in the app's own [`PtyManager`] whose bytes come from the host
//! (`spawn_remote`), so scrollback, marks, status heuristics and the view
//! work as for local panes. When the connection drops every remote pane
//! says "Reconnecting…", the service reconnects (the host keeps its PTYs
//! for 10 minutes) and re-attaches each PTY from the last byte it showed.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::HostId;
use tandem_core::{ErrorCode, IpcError};
use tandem_host::client::{ClientError, HostClient};
use tandem_host::protocol::{
    method, DirListing, ErrorKind, Found, FsChanged, Hello, HelloReply, Message, PtyAttached,
    PtyExit, PtyOutput, PtySpawned, RpcError, StatusEvent, PROTOCOL, VERSION,
};
use tandem_host::ssh::{Askpass, Category, Ssh, SshError, SshTarget};
use tandem_pty::{OutputSink, PtyInfo, PtyManager, RemoteFeed, RemotePty, SpawnSpec};
use tandem_store::repo::hosts::{self, Host};
use tandem_store::Store;
use tokio::sync::{mpsc, oneshot};
use ts_rs::TS;

use crate::services::status::StatusService;

pub const RECONNECTING: &str = "Reconnecting…";
pub const RECONNECTED: &str = "Reconnected";
pub const HOST_GONE: &str = "The connection to the host is down. This terminal has ended.";
/// How long a reconnect keeps trying: as long as the host keeps the PTYs.
pub const RECONNECT_FOR: Duration = Duration::from_secs(10 * 60);
/// A prompt nobody answers gives up after this.
const PROMPT_WAIT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HostState {
    Connected,
    Connecting,
    Reconnecting,
    Down,
}

/// `ssh_hosts_list` rows: the stored host and its connection now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HostView {
    #[serde(flatten)]
    #[ts(flatten)]
    pub host: Host,
    pub state: HostState,
}

/// What `remote:ssh` asks the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum SshPrompt {
    /// An unknown host's key, for approval before the first connection.
    Hostkey {
        host: String,
        fingerprint: String,
        key_type: String,
    },
    /// ssh asked for a password or a key's passphrase.
    Password { label: String },
}

/// `remote:ssh`: a connection's progress (stages `resolving`, `hostkey`,
/// `connecting`, `installing`, `starting`, `connected`, `failed`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SshProgress {
    pub op_id: String,
    pub host_id: HostId,
    pub stage: String,
    pub prompt: Option<SshPrompt>,
    pub error: Option<IpcError>,
}

/// `remote:host`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HostEvent {
    pub host_id: HostId,
    pub state: HostState,
}

enum Reply {
    HostKey(bool),
    Secret(Option<String>),
}

/// The words for a transport error, with the category as the stage.
pub fn ssh_error(e: SshError) -> IpcError {
    let stage = serde_json::to_value(e.category)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "unreachable".into());
    IpcError::new(ErrorCode::Unavailable, e.message).with_stage(stage)
}

pub fn rpc_error(e: ClientError) -> IpcError {
    match e {
        ClientError::Rpc(RpcError { kind, message, .. }) => {
            let code = match kind {
                ErrorKind::PathEscape => ErrorCode::PathEscape,
                ErrorKind::NotFound => ErrorCode::NotFound,
                ErrorKind::Invalid | ErrorKind::Protocol => ErrorCode::Invalid,
                ErrorKind::Conflict | ErrorKind::Exists => ErrorCode::Conflict,
                ErrorKind::ReadOnly => ErrorCode::PermissionDenied,
                ErrorKind::Unsupported => ErrorCode::Unavailable,
                ErrorKind::Io => ErrorCode::Io,
            };
            IpcError::new(code, message).with_stage("remote")
        }
        ClientError::Timeout => {
            IpcError::new(ErrorCode::Timeout, "The host didn't answer in time").with_stage("remote")
        }
        ClientError::Disconnected => IpcError::new(
            ErrorCode::Unavailable,
            "The connection to the host is down. Vibezeo is reconnecting.",
        )
        .with_stage("unreachable"),
        ClientError::Decode(m) => IpcError::new(ErrorCode::Internal, m).with_stage("remote"),
    }
}

/// The SSH target a host row describes.
pub fn target_of(host: &Host) -> Result<SshTarget, IpcError> {
    let raw = host
        .ssh_target
        .as_deref()
        .ok_or_else(|| IpcError::new(ErrorCode::Invalid, "This host has no address"))?;
    let mut t = SshTarget::parse(raw)
        .ok_or_else(|| IpcError::new(ErrorCode::Invalid, "That isn't an SSH host"))?;
    if host.ssh_port.is_some() {
        t.port = host.ssh_port;
    }
    t.identity_file = host.identity_file.clone().filter(|s| !s.is_empty());
    Ok(t)
}

/// One remote pane's link to its PTY.
#[derive(Default)]
struct PaneLink {
    feed: Option<RemoteFeed>,
    /// Output that came before the pane's session existed.
    early: Vec<u8>,
    early_exit: Option<Option<i32>>,
    /// The next byte offset this pane expects.
    next_seq: u64,
}

struct HostConn {
    host_id: HostId,
    label: String,
    client: RwLock<Option<Arc<HostClient>>>,
    child: Mutex<Option<tokio::process::Child>>,
    panes: Mutex<HashMap<String, PaneLink>>,
    state: Mutex<HostState>,
    hello: Mutex<Option<HelloReply>>,
    /// The user disconnected: don't reconnect.
    closing: AtomicBool,
    /// One connect or reconnect at a time.
    connecting: tokio::sync::Mutex<()>,
}

impl HostConn {
    fn client(&self) -> Option<Arc<HostClient>> {
        self.client
            .read()
            .ok()
            .and_then(|c| c.clone())
            .filter(|c| !c.is_closed())
    }

    fn state(&self) -> HostState {
        self.state.lock().map(|s| *s).unwrap_or(HostState::Down)
    }
}

/// A remote PTY as `tandem_pty` drives it.
struct RemoteBackend {
    conn: Weak<HostConn>,
    id: String,
}

impl RemotePty for RemoteBackend {
    fn write(&self, data: &[u8]) -> std::io::Result<()> {
        let client = self
            .conn
            .upgrade()
            .and_then(|c| c.client())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "reconnecting"))?;
        client
            .send(
                method::PTY_WRITE,
                json!({"id": self.id, "data_b64": base64::engine::general_purpose::STANDARD.encode(data)}),
            )
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotConnected, "reconnecting"))
    }

    fn resize(&self, cols: u16, rows: u16) {
        if let Some(c) = self.conn.upgrade().and_then(|c| c.client()) {
            let _ = c.send(
                method::PTY_RESIZE,
                json!({"id": self.id, "cols": cols, "rows": rows}),
            );
        }
    }

    fn kill(&self) {
        let Some(conn) = self.conn.upgrade() else {
            return;
        };
        match conn.client() {
            Some(c) => {
                let _ = c.send(method::PTY_KILL, json!({"id": self.id}));
            }
            // Nothing to reach: the pane ends here.
            None => {
                let feed = conn
                    .panes
                    .lock()
                    .ok()
                    .and_then(|mut p| p.remove(&self.id))
                    .and_then(|l| l.feed);
                if let Some(f) = feed {
                    f.exit(None);
                }
            }
        }
    }
}

/// Where the bundled `tandem-host` binaries are: `TANDEM_HOST_BIN_DIR` in
/// debug builds, else the app's resources (`resources/tandem-host`), and
/// the source tree's resources for a debug build run from `target/`.
pub fn bin_dir(resource_dir: Option<PathBuf>) -> PathBuf {
    if cfg!(debug_assertions) {
        if let Some(dir) = std::env::var_os("TANDEM_HOST_BIN_DIR") {
            return PathBuf::from(dir);
        }
    }
    if let Some(dir) = resource_dir
        .map(|d| d.join("host-binaries"))
        .filter(|d| d.is_dir())
    {
        return dir;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/tandem-host")
}

pub struct RemoteService {
    /// Files and git on hosts, with their cached statuses.
    pub files: crate::services::remote_files::RemoteFiles,
    store: Store,
    bus: EventBus,
    ssh: Ssh,
    bin_dir: RwLock<PathBuf>,
    pty: PtyManager,
    status: Arc<StatusService>,
    conns: Mutex<HashMap<String, Arc<HostConn>>>,
    prompts: Mutex<HashMap<String, oneshot::Sender<Reply>>>,
    askpass: std::sync::OnceLock<Arc<crate::services::askpass::AskpassServer>>,
    detections: Mutex<HashMap<String, tandem_engines::detect::DetectionReport>>,
    /// The engine registry, for detection after each connection.
    engines: std::sync::OnceLock<Arc<crate::services::engines::EngineService>>,
}

impl RemoteService {
    pub fn new(
        store: Store,
        bus: EventBus,
        data: &tandem_core::DataDir,
        bin_dir: PathBuf,
        pty: PtyManager,
        status: Arc<StatusService>,
    ) -> Arc<Self> {
        let mut ssh = Ssh::new(data.run(), data.root().join("ssh").join("known_hosts"));
        if let Some(p) = std::env::var_os("TANDEM_SSH_BIN").filter(|_| cfg!(debug_assertions)) {
            ssh.ssh = PathBuf::from(p);
        }
        // An ssh config used instead of the user's own (debug builds; E2E-M14-9).
        if let Some(p) = std::env::var_os("TANDEM_SSH_CONFIG").filter(|_| cfg!(debug_assertions)) {
            ssh.config = Some(PathBuf::from(p));
        }
        Arc::new(Self {
            files: Default::default(),
            store,
            bus,
            ssh,
            bin_dir: RwLock::new(bin_dir),
            pty,
            status,
            conns: Mutex::default(),
            prompts: Mutex::default(),
            askpass: std::sync::OnceLock::new(),
            detections: Mutex::default(),
            engines: std::sync::OnceLock::new(),
        })
    }

    /// Password prompts go through the askpass helper (the app's own exe).
    pub fn set_askpass(&self, server: Arc<crate::services::askpass::AskpassServer>) {
        let _ = self.askpass.set(server);
    }

    pub fn set_engines(&self, engines: Arc<crate::services::engines::EngineService>) {
        let _ = self.engines.set(engines);
    }

    /// The bundled binaries, once the app knows its resource folder.
    pub fn set_bin_dir(&self, dir: PathBuf) {
        if let Ok(mut d) = self.bin_dir.write() {
            *d = dir;
        }
    }

    fn bin_dir(&self) -> PathBuf {
        self.bin_dir.read().map(|d| d.clone()).unwrap_or_default()
    }

    pub fn ssh(&self) -> &Ssh {
        &self.ssh
    }

    fn emit_progress(
        &self,
        op: &str,
        host: &HostId,
        stage: &str,
        prompt: Option<SshPrompt>,
        error: Option<IpcError>,
    ) {
        self.bus.publish(BusEvent::new(
            Topic::Remote,
            "remote:ssh",
            SshProgress {
                op_id: op.into(),
                host_id: host.clone(),
                stage: stage.into(),
                prompt,
                error,
            },
        ));
    }

    fn set_state(&self, conn: &HostConn, state: HostState) {
        if let Ok(mut s) = conn.state.lock() {
            if *s == state {
                return;
            }
            *s = state;
        }
        self.bus.publish(BusEvent::new(
            Topic::Remote,
            "remote:host",
            HostEvent {
                host_id: conn.host_id.clone(),
                state,
            },
        ));
    }

    pub fn state(&self, host: &HostId) -> HostState {
        self.conns
            .lock()
            .ok()
            .and_then(|c| c.get(host.as_str()).cloned())
            .map(|c| c.state())
            .unwrap_or(HostState::Down)
    }

    pub async fn list(&self) -> Result<Vec<HostView>, IpcError> {
        Ok(hosts::list(&self.store)
            .await?
            .into_iter()
            .map(|h| HostView {
                state: self.state(&h.id),
                host: h,
            })
            .collect())
    }

    // ── prompts ─────────────────────────────────────────────────────

    async fn ask(&self, op: &str, host: &HostId, prompt: SshPrompt) -> Option<Reply> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut p) = self.prompts.lock() {
            p.insert(op.to_string(), tx);
        }
        let stage = match prompt {
            SshPrompt::Hostkey { .. } => "hostkey",
            SshPrompt::Password { .. } => "password",
        };
        self.emit_progress(op, host, stage, Some(prompt), None);
        let reply = tokio::time::timeout(PROMPT_WAIT, rx)
            .await
            .ok()
            .and_then(Result::ok);
        if let Ok(mut p) = self.prompts.lock() {
            p.remove(op);
        }
        reply
    }

    /// `ssh_hostkey_decide`.
    pub fn decide_host_key(&self, op: &str, accept: bool) -> bool {
        self.prompts
            .lock()
            .ok()
            .and_then(|mut p| p.remove(op))
            .map(|tx| tx.send(Reply::HostKey(accept)).is_ok())
            .unwrap_or(false)
    }

    /// `ssh_askpass_reply`.
    pub fn reply_secret(&self, op: &str, secret: Option<String>) -> bool {
        self.prompts
            .lock()
            .ok()
            .and_then(|mut p| p.remove(op))
            .map(|tx| tx.send(Reply::Secret(secret)).is_ok())
            .unwrap_or(false)
    }

    /// The askpass helper asked for `label` during op `op`.
    pub async fn askpass(&self, op: &str, host: &HostId, label: String) -> Option<String> {
        match self.ask(op, host, SshPrompt::Password { label }).await {
            Some(Reply::Secret(s)) => s,
            _ => None,
        }
    }

    fn ssh_for(self: &Arc<Self>, op: &str, host: &HostId) -> Ssh {
        let mut ssh = self.ssh.clone();
        if let Some(server) = self.askpass.get() {
            if let Some((program, env)) = server.register(op, host.clone(), Arc::downgrade(self)) {
                ssh.askpass = Some(Askpass { program, env });
            }
        }
        ssh
    }

    fn conn(&self, host: &Host) -> Arc<HostConn> {
        let mut conns = self.conns.lock().unwrap_or_else(|e| e.into_inner());
        conns
            .entry(host.id.as_str().to_string())
            .or_insert_with(|| {
                Arc::new(HostConn {
                    host_id: host.id.clone(),
                    label: host.label.clone(),
                    client: RwLock::new(None),
                    child: Mutex::new(None),
                    panes: Mutex::default(),
                    state: Mutex::new(HostState::Down),
                    hello: Mutex::new(None),
                    closing: AtomicBool::new(false),
                    connecting: tokio::sync::Mutex::new(()),
                })
            })
            .clone()
    }

    // ── connecting ──────────────────────────────────────────────────

    /// `ssh_connect`: starts the connection and reports through
    /// `remote:ssh`; returns the op id at once.
    pub fn connect(self: &Arc<Self>, host_id: HostId) -> String {
        let op = format!("op_{}", ulid::Ulid::new().to_string().to_lowercase());
        let me = self.clone();
        let op_id = op.clone();
        tokio::spawn(async move {
            match me.connect_now(&host_id, &op_id).await {
                Ok(()) => {
                    me.emit_progress(&op_id, &host_id, "connected", None, None);
                    // What the + menu offers on this host (F-EN-01 per host).
                    if let Some(engines) = me.engines.get().cloned() {
                        let list = engines.registry().all().to_vec();
                        if let Err(e) = me.detect(&host_id, &list).await {
                            tracing::info!(code = "remote.detect_failed", error = %e.message, "couldn't detect the host's engines");
                        }
                    }
                }
                Err(e) => me.emit_progress(&op_id, &host_id, "failed", None, Some(e)),
            }
        });
        op
    }

    /// Connects (asking about an unknown host key through `op`), installs
    /// the helper and opens the channel. Already connected: nothing to do.
    pub async fn connect_now(self: &Arc<Self>, host_id: &HostId, op: &str) -> Result<(), IpcError> {
        let host = hosts::get(&self.store, host_id).await?;
        let conn = self.conn(&host);
        let _one = conn.connecting.lock().await;
        if conn.client().is_some() {
            return Ok(());
        }
        conn.closing.store(false, Ordering::SeqCst);
        self.set_state(&conn, HostState::Connecting);
        let out = self.open(&host, &conn, op, true).await;
        if let Err(e) = &out {
            self.set_state(&conn, HostState::Down);
            tracing::info!(
                code = "remote.connect_failed",
                stage = e.stage.as_deref().unwrap_or(""),
                "couldn't connect to a host"
            );
        }
        out
    }

    async fn open(
        self: &Arc<Self>,
        host: &Host,
        conn: &Arc<HostConn>,
        op: &str,
        interactive: bool,
    ) -> Result<(), IpcError> {
        let target = target_of(host)?;
        let ssh = self.ssh_for(op, &host.id);
        self.emit_progress(op, &host.id, "resolving", None, None);
        let resolved = ssh.resolve(&target).await.map_err(ssh_error)?;
        if ssh.known(&resolved).is_empty() {
            if !interactive {
                return Err(ssh_error(SshError::new(
                    Category::HostKey,
                    format!(
                        "{} has no approved host key. Connect again to check it.",
                        target.label()
                    ),
                )));
            }
            let keys = ssh.host_keys(&target, &resolved).await.map_err(ssh_error)?;
            let key = keys[0].clone();
            let prompt = SshPrompt::Hostkey {
                host: target.label(),
                fingerprint: key.fingerprint.clone(),
                key_type: key.key_type.clone(),
            };
            match self.ask(op, &host.id, prompt).await {
                Some(Reply::HostKey(true)) => {}
                _ => {
                    return Err(IpcError::new(
                        ErrorCode::Cancelled,
                        "You didn't trust the host's key, so Vibezeo didn't connect",
                    )
                    .with_stage("host_key"))
                }
            }
            ssh.trust(&resolved, &key)
                .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
            hosts::set_key(&self.store, &host.id, Some(key.fingerprint)).await?;
        }
        self.emit_progress(op, &host.id, "connecting", None, None);
        ssh.master_up(&target).await.map_err(ssh_error)?;
        self.emit_progress(op, &host.id, "installing", None, None);
        let installed = ssh
            .install(&target, &self.bin_dir())
            .await
            .map_err(ssh_error)?;
        hosts::set_installed(&self.store, &host.id, PROTOCOL, &installed.platform).await?;
        self.emit_progress(op, &host.id, "starting", None, None);
        let mut child = ssh
            .serve(&target)
            .map_err(|e| ssh_error(SshError::new(Category::Unreachable, e.to_string())))?;
        let (Some(out), Some(input)) = (child.stdout.take(), child.stdin.take()) else {
            return Err(IpcError::new(ErrorCode::Internal, "ssh gave no pipes"));
        };
        let (client, notes) = HostClient::start(out, input);
        let hello: HelloReply = client
            .call_within(
                method::HELLO,
                Hello {
                    protocol: PROTOCOL,
                    client_version: VERSION.into(),
                },
                Duration::from_secs(20),
            )
            .await
            .map_err(|e| {
                ssh_error(SshError::new(
                    Category::InstallFailed,
                    format!("The helper on {} didn't start: {e}", target.label()),
                ))
            })?;
        if let Ok(mut h) = conn.hello.lock() {
            *h = Some(hello);
        }
        if let Ok(mut c) = conn.child.lock() {
            *c = Some(child);
        }
        if let Ok(mut c) = conn.client.write() {
            *c = Some(client.clone());
        }
        self.pump(conn.clone(), notes);
        self.reattach(conn, &client).await;
        self.files.rewatch(&conn.host_id, &client).await;
        self.warm_git(conn.host_id.clone());
        self.set_state(conn, HostState::Connected);
        self.watch(conn.clone(), client);
        Ok(())
    }

    /// Reads the git status of this host's folders once it's connected, so
    /// the rail names their branches and offers worktrees (D-1602); then
    /// says so (`remote:git`).
    fn warm_git(self: &Arc<Self>, host: HostId) {
        let me = self.clone();
        tokio::spawn(async move {
            let Ok(rows) = tandem_store::repo::workspaces::list(&me.store).await else {
                return;
            };
            for ws in rows
                .iter()
                .filter(|w| w.kind == "ssh" && w.host_id == host.as_str())
            {
                let r = crate::services::remote_files::RemoteRoot {
                    host: host.clone(),
                    root: ws.root.clone(),
                };
                let _ = me.files.status(&me, &r).await;
            }
            me.bus.publish(BusEvent::new(
                Topic::Remote,
                "remote:git",
                serde_json::json!({"hostId": host}),
            ));
        });
    }

    /// Routes the host's notifications to panes and the status service.
    fn pump(self: &Arc<Self>, conn: Arc<HostConn>, mut notes: mpsc::UnboundedReceiver<Message>) {
        let me = self.clone();
        tokio::spawn(async move {
            while let Some(n) = notes.recv().await {
                let Some(params) = n.params else { continue };
                match n.method.as_deref() {
                    Some(method::PTY_OUTPUT) => {
                        if let Ok(o) = serde_json::from_value::<PtyOutput>(params) {
                            if let Ok(data) =
                                base64::engine::general_purpose::STANDARD.decode(&o.data_b64)
                            {
                                deliver(&conn, &o.id, o.seq, data);
                            }
                        }
                    }
                    Some(method::PTY_EXIT) => {
                        if let Ok(e) = serde_json::from_value::<PtyExit>(params) {
                            finish(&conn, &e.id, e.code);
                        }
                    }
                    Some(method::FS_CHANGED) => {
                        if let Ok(c) = serde_json::from_value::<FsChanged>(params) {
                            me.files.changed(&conn.host_id, &c.id, &c.change);
                        }
                    }
                    Some(method::STATUS_EVENT) => {
                        if let Ok(ev) = serde_json::from_value::<StatusEvent>(params) {
                            if let Ok(body) =
                                base64::engine::general_purpose::STANDARD.decode(&ev.body_b64)
                            {
                                me.status.relayed(&body, ev.signature.as_deref());
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
    }

    /// Re-attaches every pane this host had, from the byte each one
    /// expects next; a PTY the host no longer has ends its pane.
    async fn reattach(&self, conn: &Arc<HostConn>, client: &Arc<HostClient>) {
        let wanted: Vec<(String, u64)> = conn
            .panes
            .lock()
            .map(|p| {
                p.iter()
                    .filter(|(_, l)| l.feed.is_some())
                    .map(|(id, l)| (id.clone(), l.next_seq))
                    .collect()
            })
            .unwrap_or_default();
        for (id, from) in wanted {
            let res: Result<PtyAttached, ClientError> = client
                .call(method::PTY_ATTACH, json!({"id": id, "from_seq": from}))
                .await;
            match res {
                Ok(_) => {
                    let _ = self.pty.notice(&id, RECONNECTED);
                }
                Err(_) => {
                    let _ = self.pty.notice(&id, HOST_GONE);
                    finish(conn, &id, None);
                }
            }
        }
    }

    /// Waits for the connection to drop, then reconnects unless the user
    /// disconnected.
    fn watch(self: &Arc<Self>, conn: Arc<HostConn>, client: Arc<HostClient>) {
        let me = self.clone();
        tokio::spawn(async move {
            client.closed().await;
            if let Ok(mut c) = conn.client.write() {
                if c.as_ref().is_some_and(|x| Arc::ptr_eq(x, &client)) {
                    *c = None;
                }
            }
            if conn.closing.load(Ordering::SeqCst) {
                me.set_state(&conn, HostState::Down);
                return;
            }
            me.reconnect(conn).await;
        });
    }

    async fn reconnect(self: &Arc<Self>, conn: Arc<HostConn>) {
        self.set_state(&conn, HostState::Reconnecting);
        let panes: Vec<String> = conn
            .panes
            .lock()
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default();
        for id in &panes {
            let _ = self.pty.notice(id, RECONNECTING);
        }
        tracing::info!(
            code = "remote.reconnecting",
            panes = panes.len(),
            "the host connection dropped"
        );
        let started = Instant::now();
        let mut delay = Duration::from_millis(500);
        loop {
            if conn.closing.load(Ordering::SeqCst) {
                return;
            }
            let attempt = async {
                let host = hosts::get(&self.store, &conn.host_id).await?;
                let _one = conn.connecting.lock().await;
                if conn.client().is_some() {
                    return Ok(());
                }
                let op = format!("op_{}", ulid::Ulid::new().to_string().to_lowercase());
                self.open(&host, &conn, &op, false).await
            };
            match attempt.await {
                Ok(()) => return,
                Err(e)
                    if e.stage.as_deref() == Some("host_key")
                        || started.elapsed() > RECONNECT_FOR =>
                {
                    tracing::warn!(
                        code = "remote.reconnect_failed",
                        stage = e.stage.as_deref().unwrap_or(""),
                        "gave up reconnecting"
                    );
                    self.set_state(&conn, HostState::Down);
                    let ids: Vec<String> = conn
                        .panes
                        .lock()
                        .map(|p| p.keys().cloned().collect())
                        .unwrap_or_default();
                    for id in ids {
                        let _ = self.pty.notice(&id, HOST_GONE);
                        finish(&conn, &id, None);
                    }
                    return;
                }
                Err(_) => {
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(15));
                }
            }
        }
    }

    /// `ssh_disconnect`: ends this host's panes and its connection.
    pub async fn disconnect(&self, host_id: &HostId) -> Result<(), IpcError> {
        let host = hosts::get(&self.store, host_id).await?;
        let Some(conn) = self
            .conns
            .lock()
            .ok()
            .and_then(|c| c.get(host_id.as_str()).cloned())
        else {
            return Ok(());
        };
        conn.closing.store(true, Ordering::SeqCst);
        let ids: Vec<String> = conn
            .panes
            .lock()
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default();
        for id in ids {
            let _ = self.pty.kill(&id);
        }
        if let Some(mut child) = conn.child.lock().ok().and_then(|mut c| c.take()) {
            let _ = child.start_kill();
        }
        if let Ok(t) = target_of(&host) {
            self.ssh.master_exit(&t).await;
        }
        if let Ok(mut c) = conn.client.write() {
            *c = None;
        }
        self.set_state(&conn, HostState::Down);
        Ok(())
    }

    /// The connected client for a host, connecting first when needed
    /// (with no prompts: a host never approved says to connect).
    pub async fn client(self: &Arc<Self>, host_id: &HostId) -> Result<Arc<HostClient>, IpcError> {
        let host = hosts::get(&self.store, host_id).await?;
        let conn = self.conn(&host);
        if let Some(c) = conn.client() {
            return Ok(c);
        }
        if conn.state() == HostState::Reconnecting {
            return Err(rpc_error(ClientError::Disconnected));
        }
        let _one = conn.connecting.lock().await;
        if let Some(c) = conn.client() {
            return Ok(c);
        }
        let op = format!("op_{}", ulid::Ulid::new().to_string().to_lowercase());
        self.set_state(&conn, HostState::Connecting);
        match self.open(&host, &conn, &op, false).await {
            Ok(()) => conn
                .client()
                .ok_or_else(|| rpc_error(ClientError::Disconnected)),
            Err(e) => {
                self.set_state(&conn, HostState::Down);
                Err(e)
            }
        }
    }

    pub async fn call<T: serde::de::DeserializeOwned>(
        self: &Arc<Self>,
        host_id: &HostId,
        method: &str,
        params: Value,
    ) -> Result<T, IpcError> {
        self.client(host_id)
            .await?
            .call(method, params)
            .await
            .map_err(rpc_error)
    }

    /// `ssh_list_dir`: the folder picker.
    pub async fn list_dir(
        self: &Arc<Self>,
        host_id: &HostId,
        path: &str,
    ) -> Result<DirListing, IpcError> {
        self.call(host_id, method::FS_DIRS, json!({"path": path}))
            .await
    }

    /// Forgets the pinned key (after the user confirmed "Trust new key…"),
    /// so the next connection shows the new fingerprint for approval.
    pub async fn reset_key(&self, host_id: &HostId) -> Result<(), IpcError> {
        let host = hosts::get(&self.store, host_id).await?;
        let target = target_of(&host)?;
        let resolved = self.ssh.resolve(&target).await.map_err(ssh_error)?;
        self.ssh
            .forget(&resolved)
            .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
        hosts::set_key(&self.store, host_id, None).await?;
        Ok(())
    }

    // ── panes ───────────────────────────────────────────────────────

    /// Starts a remote PTY and its local session. `command` runs first in
    /// the login shell (agents); the env is filtered to the allowlist.
    #[allow(clippy::too_many_arguments)]
    pub async fn spawn_pane(
        self: &Arc<Self>,
        host_id: &HostId,
        spec: SpawnSpec,
        cwd: &str,
        command: Option<String>,
        sink: Option<Arc<dyn OutputSink>>,
    ) -> Result<PtyInfo, IpcError> {
        let client = self.client(host_id).await?;
        let host = hosts::get(&self.store, host_id).await?;
        let conn = self.conn(&host);
        let id = spec.pane_id.clone();
        if let Ok(mut p) = conn.panes.lock() {
            p.insert(id.clone(), PaneLink::default());
        }
        let env = tandem_host::env::allowed(&spec.env);
        let spawn = |cwd: String| {
            let client = client.clone();
            let (id, env, command) = (id.clone(), env.clone(), command.clone());
            let (cols, rows) = (spec.cols, spec.rows);
            async move {
                client
                    .call::<_, PtySpawned>(
                        method::PTY_SPAWN,
                        json!({"id": id, "command": command, "cwd": cwd, "env": env, "cols": cols, "rows": rows}),
                    )
                    .await
            }
        };
        let asked = Instant::now();
        let spawned = spawn(cwd.to_string()).await.map_err(rpc_error);
        let spawn_ms = asked.elapsed().as_millis() as u64;
        let spawned = match spawned {
            Ok(s) => s,
            Err(e) => {
                if let Ok(mut p) = conn.panes.lock() {
                    p.remove(&id);
                }
                return Err(if e.code == ErrorCode::NotFound {
                    IpcError::new(
                        ErrorCode::NotFound,
                        format!("That folder doesn't exist on {}.", conn.label),
                    )
                    .with_stage("folder_missing")
                } else {
                    e
                });
            }
        };
        let backend = Arc::new(RemoteBackend {
            conn: Arc::downgrade(&conn),
            id: id.clone(),
        });
        let (info, feed) = self
            .pty
            .spawn_remote(spec, spawned.pid, spawn_ms, backend, sink)?;
        if let Ok(mut p) = conn.panes.lock() {
            let link = p.entry(id.clone()).or_default();
            if !link.early.is_empty() {
                feed.data(std::mem::take(&mut link.early));
            }
            if let Some(code) = link.early_exit.take() {
                feed.exit(code);
            } else {
                link.feed = Some(feed);
            }
        }
        Ok(info)
    }

    /// The remote pane's host, when it is one.
    pub fn host_of_pane(&self, pane: &str) -> Option<HostId> {
        self.conns
            .lock()
            .ok()?
            .values()
            .find(|c| {
                c.panes
                    .lock()
                    .map(|p| p.contains_key(pane))
                    .unwrap_or(false)
            })
            .map(|c| c.host_id.clone())
    }

    // ── engines ─────────────────────────────────────────────────────

    /// Detection on the host (F-EN-01 per host): saved per host and sent
    /// as `engines:detected`.
    pub async fn detect(
        self: &Arc<Self>,
        host_id: &HostId,
        engines: &[tandem_engines::EngineSpec],
    ) -> Result<tandem_engines::detect::DetectionReport, IpcError> {
        let wanted: Vec<Value> = engines
            .iter()
            .map(|e| json!({"id": e.id, "commands": e.commands, "version_args": e.version_cmd}))
            .collect();
        let found: Vec<Found> = self
            .call(host_id, method::ENGINES_DETECT, json!({"engines": wanted}))
            .await?;
        let report = report_from(
            host_id,
            "$SHELL -lc (on the host)",
            engines,
            found
                .into_iter()
                .map(|f| (f.id, f.path, f.version))
                .collect(),
        );
        self.publish_report(host_id, &report).await?;
        Ok(report)
    }

    /// Saves a host's (or WSL distro's) detection and sends it as
    /// `engines:detected`.
    pub async fn publish_report(
        &self,
        host_id: &HostId,
        report: &tandem_engines::detect::DetectionReport,
    ) -> Result<(), IpcError> {
        let installs = report
            .found
            .iter()
            .map(|f| tandem_store::repo::accounts::InstallRow {
                engine_id: f.engine_id.clone(),
                path: f.path.clone(),
                version: f.version.clone(),
            })
            .collect();
        if let Ok(json) = serde_json::to_string(report) {
            tandem_store::repo::accounts::save_detection(
                &self.store,
                host_id.as_str(),
                installs,
                json,
            )
            .await?;
        }
        if let Ok(mut d) = self.detections.lock() {
            d.insert(host_id.as_str().to_string(), report.clone());
        }
        self.bus.publish(BusEvent::new(
            Topic::Engines,
            "engines:detected",
            json!({"hostId": host_id, "report": report}),
        ));
        Ok(())
    }

    /// The host's last detection report (memory, then the saved one).
    pub async fn report(
        &self,
        host_id: &HostId,
    ) -> Option<tandem_engines::detect::DetectionReport> {
        if let Some(r) = self.detections.lock().ok()?.get(host_id.as_str()).cloned() {
            return Some(r);
        }
        let json = tandem_store::repo::accounts::last_detection(&self.store, host_id.as_str())
            .await
            .ok()
            .flatten()?;
        serde_json::from_str(&json).ok()
    }

    /// The engines a thread or agent on this host runs through: the
    /// `tandem-host run` command line over the host's master connection
    /// (D-403).
    pub async fn run_command(
        &self,
        host_id: &HostId,
        cwd: &str,
        env: &BTreeMap<String, String>,
        argv: &[String],
    ) -> Result<(PathBuf, Vec<String>), IpcError> {
        let host = hosts::get(&self.store, host_id).await?;
        let target = target_of(&host)?;
        Ok(self.ssh.run_args(&target, cwd, env, argv))
    }

    /// What the app calls a host.
    pub async fn label(&self, host_id: &HostId) -> String {
        hosts::get(&self.store, host_id)
            .await
            .map(|h| h.label)
            .unwrap_or_else(|_| host_id.as_str().to_string())
    }

    /// The transport and target for a host (thread engines run over it).
    pub async fn transport(&self, host_id: &HostId) -> Result<(Ssh, SshTarget), IpcError> {
        let host = hosts::get(&self.store, host_id).await?;
        Ok((self.ssh.clone(), target_of(&host)?))
    }

    /// Quit: remote panes end with the app, like local ones.
    pub fn shutdown(&self) {
        let conns: Vec<Arc<HostConn>> = self
            .conns
            .lock()
            .map(|c| c.values().cloned().collect())
            .unwrap_or_default();
        for c in conns {
            c.closing.store(true, Ordering::SeqCst);
            if let Some(mut child) = c.child.lock().ok().and_then(|mut x| x.take()) {
                let _ = child.start_kill();
            }
        }
    }
}

/// A detection report for a host or WSL distro from what its login shell
/// found: `(engine id, path, version line)`. The registry's pattern picks
/// the version out of the line, as local detection does.
pub fn report_from(
    host_id: &HostId,
    shell: &str,
    engines: &[tandem_engines::EngineSpec],
    found: Vec<(String, String, Option<String>)>,
) -> tandem_engines::detect::DetectionReport {
    use tandem_engines::detect::{DetectionReport, FoundEngine, FoundSource};
    DetectionReport {
        host: host_id.as_str().to_string(),
        shells_asked: vec![shell.to_string()],
        folders_checked: Vec::new(),
        missing: engines
            .iter()
            .filter(|e| !found.iter().any(|f| f.0 == e.id))
            .map(|e| e.id.clone())
            .collect(),
        found: found
            .into_iter()
            .map(|(id, path, line)| {
                let version = line.and_then(|line| {
                    let pattern = engines
                        .iter()
                        .find(|e| e.id == id)
                        .map(|e| e.version_regex.as_str())?;
                    regex::Regex::new(pattern)
                        .ok()?
                        .captures(&line)
                        .and_then(|c| c.get(1).or_else(|| c.get(0)))
                        .map(|m| m.as_str().to_string())
                });
                FoundEngine {
                    engine_id: id,
                    path,
                    version,
                    source: FoundSource::Shell,
                }
            })
            .collect(),
        fixes: Vec::new(),
        slow_shell: None,
    }
}

/// Hands output to a pane: only bytes past what it already showed.
fn deliver(conn: &HostConn, id: &str, seq: u64, data: Vec<u8>) {
    let Ok(mut panes) = conn.panes.lock() else {
        return;
    };
    let Some(link) = panes.get_mut(id) else {
        return;
    };
    let end = seq + data.len() as u64;
    if end <= link.next_seq {
        return;
    }
    let skip = link.next_seq.saturating_sub(seq) as usize;
    let fresh = data[skip..].to_vec();
    link.next_seq = end;
    match &link.feed {
        Some(f) => f.data(fresh),
        None => link.early.extend_from_slice(&fresh),
    }
}

fn finish(conn: &HostConn, id: &str, code: Option<i32>) {
    let Ok(mut panes) = conn.panes.lock() else {
        return;
    };
    match panes.get_mut(id) {
        Some(link) if link.feed.is_some() => {
            if let Some(f) = panes.remove(id).and_then(|l| l.feed) {
                f.exit(code);
            }
        }
        Some(link) => link.early_exit = Some(code),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::files::watch::FsChange;

    fn link_conn() -> HostConn {
        HostConn {
            host_id: HostId::local(),
            label: "h".into(),
            client: RwLock::new(None),
            child: Mutex::new(None),
            panes: Mutex::default(),
            state: Mutex::new(HostState::Down),
            hello: Mutex::new(None),
            closing: AtomicBool::new(false),
            connecting: tokio::sync::Mutex::new(()),
        }
    }

    #[test]
    fn f_rm_01_5_replayed_output_never_shows_twice() {
        let conn = link_conn();
        conn.panes
            .lock()
            .expect("p")
            .insert("p".into(), PaneLink::default());
        deliver(&conn, "p", 0, b"hello ".to_vec());
        // A re-attach replays from where the pane was; overlap is dropped.
        deliver(&conn, "p", 3, b"lo world".to_vec());
        deliver(&conn, "p", 0, b"hel".to_vec());
        let p = conn.panes.lock().expect("p");
        let link = p.get("p").expect("link");
        assert_eq!(String::from_utf8_lossy(&link.early), "hello world");
        assert_eq!(link.next_seq, 11);
    }

    #[test]
    fn f_rm_01_errors_say_what_needs_attention() {
        let e = ssh_error(SshError::new(
            Category::HostKey,
            "The host key for h changed. Vibezeo won't connect until you verify it.",
        ));
        assert_eq!(e.code, ErrorCode::Unavailable);
        assert_eq!(e.stage.as_deref(), Some("host_key"));
        let e = rpc_error(ClientError::Rpc(RpcError::new(
            ErrorKind::PathEscape,
            "outside",
        )));
        assert_eq!(e.code, ErrorCode::PathEscape);
        let t = target_of(&Host {
            id: HostId::local(),
            kind: "ssh".into(),
            label: "f".into(),
            ssh_target: Some("tandem@127.0.0.1".into()),
            ssh_port: Some(2222),
            identity_file: Some("/k".into()),
            wsl_distro: None,
            host_key_fp: None,
            host_protocol: None,
            arch: None,
            workspaces: 0,
            created_at: 0,
        })
        .expect("target");
        assert_eq!(
            (t.user.as_deref(), t.target.as_str(), t.port),
            (Some("tandem"), "127.0.0.1", Some(2222))
        );
        assert_eq!(t.identity_file.as_deref(), Some("/k"));
    }

    /// Against the `sshd` compose service (`scripts/gen-ssh-fixture.sh`,
    /// `docker compose … --profile remote up -d sshd`, `scripts/build-tandem-host.sh`):
    /// `cargo test -p tandem-desktop --lib f_rm_01_app -- --include-ignored`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs the sshd compose service (profile remote)"]
    async fn f_rm_01_app_connect_terminal_and_reattach_after_a_drop() {
        use tandem_pty::VecSink;
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let dir = tempfile::tempdir().expect("tmp");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let bus = EventBus::new();
        let pty = PtyManager::new();
        let status = StatusService::new(bus.clone(), Arc::new(tandem_core::clock::SystemClock));
        let svc = RemoteService::new(
            store.clone(),
            bus.clone(),
            &data,
            repo.join("apps/desktop/src-tauri/resources/tandem-host"),
            pty.clone(),
            status,
        );
        let host = hosts::add_ssh(
            &store,
            hosts::NewSshHost {
                label: "fixture".into(),
                target: "tandem@127.0.0.1".into(),
                port: Some(2222),
                identity_file: Some(
                    repo.join("infra/ssh/id_ed25519")
                        .to_string_lossy()
                        .into_owned(),
                ),
            },
        )
        .await
        .expect("host");
        // The user approves the fingerprint the prompt shows.
        let mut events = bus.subscribe(Topic::Remote);
        let approver = svc.clone();
        let saw = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen = saw.clone();
        tokio::spawn(async move {
            while let Ok(ev) = events.recv().await {
                if ev.name == "remote:ssh" {
                    if let Ok(p) = serde_json::from_value::<SshProgress>(ev.payload) {
                        seen.lock().expect("seen").push(p.stage.clone());
                        if let Some(SshPrompt::Hostkey { fingerprint, .. }) = p.prompt {
                            assert!(fingerprint.starts_with("SHA256:"));
                            approver.decide_host_key(&p.op_id, true);
                        }
                    }
                }
            }
        });
        svc.connect_now(&host.id, "op_test")
            .await
            .expect("connected");
        assert!(saw.lock().expect("saw").contains(&"hostkey".to_string()));
        assert_eq!(svc.state(&host.id), HostState::Connected);
        let pinned = hosts::get(&store, &host.id).await.expect("host");
        assert!(pinned.host_key_fp.is_some_and(|f| f.starts_with("SHA256:")));

        let sink = Arc::new(VecSink::default());
        let pane = format!("pn_{}", ulid::Ulid::new());
        let spec = SpawnSpec {
            pane_id: pane.clone(),
            program: PathBuf::from("ssh"),
            args: Vec::new(),
            cwd: PathBuf::from("/home/tandem/project"),
            env: BTreeMap::from([
                ("TERM".to_string(), "xterm-256color".to_string()),
                (
                    "CLAUDE_CONFIG_DIR".to_string(),
                    "/me/.tandem/providers/claude/a".to_string(),
                ),
            ]),
            cols: 100,
            rows: 30,
            scrollback_lines: 1000,
            mark_token: None,
        };
        let info = svc
            .spawn_pane(
                &host.id,
                spec,
                "/home/tandem/project",
                None,
                Some(sink.clone() as Arc<dyn OutputSink>),
            )
            .await
            .expect("pane");
        let text = || String::from_utf8_lossy(&sink.data()).into_owned();
        let notices = || {
            sink.frames
                .lock()
                .map(|f| {
                    f.iter()
                        .filter(|f| f.kind == tandem_pty::FrameKind::Notice)
                        .map(|f| String::from_utf8_lossy(&f.payload).into_owned())
                        .collect::<Vec<_>>()
                        .join("|")
                })
                .unwrap_or_default()
        };
        pty.write(&pane, b"echo hi-$$; env | grep -c providers\n")
            .expect("write");
        let want = format!("hi-{}", info.pid);
        assert!(
            tandem_pty::wait_until(Duration::from_secs(20), || text().contains(&want)),
            "{}",
            text()
        );
        assert!(
            tandem_pty::wait_until(Duration::from_secs(5), || text().contains("\r\n0\r\n")),
            "{}",
            text()
        );

        // The host's folder is watched by the helper (D-1600): a file
        // made on the host reaches the subscriber without any action here.
        let batches = Arc::new(Mutex::new(Vec::<FsChange>::new()));
        let got = batches.clone();
        let root = crate::services::remote_files::RemoteRoot {
            host: host.id.clone(),
            root: "/home/tandem/project".into(),
        };
        svc.files
            .watch(
                &svc,
                &root,
                "fsw_test".into(),
                Arc::new(move |c: &FsChange| {
                    got.lock().expect("got").push(c.clone());
                    true
                }),
            )
            .await
            .expect("watch");
        let saw_path = |name: &str| {
            batches
                .lock()
                .expect("b")
                .iter()
                .any(|b| b.paths.iter().any(|p| p == name))
        };
        pty.write(&pane, b"echo w > watched-1.txt\n")
            .expect("write");
        assert!(
            tandem_pty::wait_until(Duration::from_secs(20), || saw_path("watched-1.txt")),
            "{:?}",
            batches.lock().expect("b")
        );

        // The connection drops (ssh -O exit): "Reconnecting…", then the
        // same remote PTY is back.
        let t = target_of(&hosts::get(&store, &host.id).await.expect("h")).expect("t");
        svc.ssh().master_exit(&t).await;
        assert!(
            tandem_pty::wait_until(Duration::from_secs(20), || svc.state(&host.id)
                == HostState::Connected
                && notices().contains(RECONNECTED)),
            "{:?} {}",
            svc.state(&host.id),
            notices()
        );
        assert!(notices().contains(RECONNECTING), "{}", notices());
        pty.write(&pane, b"echo again-$$\n").expect("write");
        let want = format!("again-{}", info.pid);
        assert!(
            tandem_pty::wait_until(Duration::from_secs(20), || text().contains(&want)),
            "{}",
            text()
        );
        assert_eq!(pty.pid(&pane), Some(info.pid));
        // The watch came back with the connection: "read everything" once,
        // then batches again.
        assert!(
            batches.lock().expect("b").iter().any(|b| b.overflow),
            "no read-everything batch after the reconnect"
        );
        pty.write(
            &pane,
            b"echo w > watched-2.txt; rm -f watched-1.txt watched-2.txt\n",
        )
        .expect("write");
        assert!(
            tandem_pty::wait_until(Duration::from_secs(20), || saw_path("watched-2.txt")),
            "{:?}",
            batches.lock().expect("b")
        );
        assert!(svc.files.unwatch(&svc, "fsw_test"));
        assert!(svc.files.watched().is_empty());
        pty.kill(&pane).expect("kill");
        svc.disconnect(&host.id).await.expect("disconnect");
    }
}
