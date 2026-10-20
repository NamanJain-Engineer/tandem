//! Code intelligence (F-CD-23): definitions, references, hover and
//! diagnostics from the language servers the user already has on the login
//! PATH. One server per folder and language, started on first use, stopped
//! after [`IDLE`] without a request; at most [`MAX_SERVERS`] run at once.
//! Servers speak LSP's JSON-RPC over stdio (`Content-Length` framing).
//! Without a server, definitions fall back to the file's own tree-sitter
//! definitions (F-WT-10). Nothing leaves the machine.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, Notify};
use ts_rs::TS;

use tandem_core::{ErrorCode, IpcError};
use tandem_pty::env::{resolve_on_path, LoginEnv};

/// A server stops after this long without a request.
pub const IDLE: Duration = Duration::from_secs(10 * 60);
/// At most this many run at once; the one used longest ago stops first.
pub const MAX_SERVERS: usize = 4;
/// One request's limit.
pub const REQUEST_LIMIT: Duration = Duration::from_secs(10);
/// `initialize` may take longer: servers index on start.
const INIT_LIMIT: Duration = Duration::from_secs(30);
/// How long `lsp_sync` waits for the server's diagnostics.
const DIAGNOSTICS_WAIT: Duration = Duration::from_millis(1500);
/// A server that failed this often within [`RETRY_WINDOW`] isn't started again.
const MAX_FAILURES: usize = 3;
const RETRY_WINDOW: Duration = Duration::from_secs(10 * 60);
const HOVER_CHARS: usize = 4000;

/// A language server Vibezeo knows how to start.
#[derive(Debug, PartialEq, Eq)]
pub struct ServerSpec {
    /// The language's name for people ("Python").
    pub language: &'static str,
    /// The server's name for people ("pyright").
    pub server: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub exts: &'static [&'static str],
}

pub const SERVERS: &[ServerSpec] = &[
    ServerSpec {
        language: "Rust",
        server: "rust-analyzer",
        program: "rust-analyzer",
        args: &[],
        exts: &["rs"],
    },
    ServerSpec {
        language: "TypeScript",
        server: "typescript-language-server",
        program: "typescript-language-server",
        args: &["--stdio"],
        exts: &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"],
    },
    ServerSpec {
        language: "Python",
        server: "pyright",
        program: "pyright-langserver",
        args: &["--stdio"],
        exts: &["py"],
    },
    ServerSpec {
        language: "Go",
        server: "gopls",
        program: "gopls",
        args: &[],
        exts: &["go"],
    },
];

fn ext(path: &str) -> &str {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
}

pub fn spec_for(path: &str) -> Option<&'static ServerSpec> {
    let e = ext(path);
    SERVERS.iter().find(|s| s.exts.contains(&e))
}

/// LSP's `languageId` for a file.
fn language_id(path: &str) -> &'static str {
    match ext(path) {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "py" => "python",
        "go" => "go",
        _ => "plaintext",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LspState {
    /// No server for the language here (or none known for it).
    None,
    Starting,
    Ready,
    /// It failed to start or stopped; `message` says why.
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LspDiagnostic {
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LspLocation {
    /// Relative to the folder when `in_folder`, else absolute.
    pub path: String,
    pub in_folder: bool,
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
    /// The line's text, trimmed, at most 200 characters.
    pub preview: String,
}

/// `lsp_sync`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LspDocument {
    /// The language's name, when Vibezeo knows one for the file.
    pub language: Option<String>,
    /// The server's name.
    pub server: Option<String>,
    pub state: LspState,
    pub message: Option<String>,
    pub diagnostics: Vec<LspDiagnostic>,
}

// ---- The client -------------------------------------------------------------

type Reader = Box<dyn AsyncRead + Send + Unpin>;
type Writer = Box<dyn AsyncWrite + Send + Unpin>;

/// One running server.
pub struct Client {
    writer: tokio::sync::Mutex<Writer>,
    pending: Mutex<HashMap<i64, oneshot::Sender<Result<Value, String>>>>,
    next: AtomicI64,
    alive: AtomicBool,
    /// Published diagnostics by URI, and a count per URI of publishes.
    diags: Mutex<HashMap<String, (u64, Vec<LspDiagnostic>)>>,
    published: Notify,
    /// The text last sent per URI, with its version.
    docs: tokio::sync::Mutex<HashMap<String, (i32, String)>>,
    child: Mutex<Option<tokio::process::Child>>,
    /// Why it stopped, once it has.
    ended: Mutex<Option<String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

async fn read_frame<R: AsyncRead + Unpin>(r: &mut BufReader<R>) -> Option<Value> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).await.ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok();
        }
    }
    let len = len?;
    // A frame over 64 MiB isn't a reply anyone wants.
    if len > 64 * 1024 * 1024 {
        return None;
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await.ok()?;
    serde_json::from_slice(&body).ok()
}

fn severity(n: Option<u64>) -> Severity {
    match n {
        Some(2) => Severity::Warning,
        Some(3) => Severity::Info,
        Some(4) => Severity::Hint,
        _ => Severity::Error,
    }
}

fn u32_of(v: &Value) -> u32 {
    v.as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0)
}

fn diagnostic(v: &Value) -> LspDiagnostic {
    let r = &v["range"];
    LspDiagnostic {
        line: u32_of(&r["start"]["line"]),
        character: u32_of(&r["start"]["character"]),
        end_line: u32_of(&r["end"]["line"]),
        end_character: u32_of(&r["end"]["character"]),
        severity: severity(v["severity"].as_u64()),
        message: v["message"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(2000)
            .collect(),
    }
}

impl Client {
    pub fn start(
        reader: Reader,
        writer: Writer,
        child: Option<tokio::process::Child>,
    ) -> Arc<Self> {
        let me = Arc::new(Self {
            writer: tokio::sync::Mutex::new(writer),
            pending: Mutex::default(),
            next: AtomicI64::new(1),
            alive: AtomicBool::new(true),
            diags: Mutex::default(),
            published: Notify::new(),
            docs: tokio::sync::Mutex::default(),
            child: Mutex::new(child),
            ended: Mutex::new(None),
        });
        let reading = me.clone();
        tokio::spawn(async move { reading.read_loop(reader).await });
        me
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn ended(&self) -> Option<String> {
        lock(&self.ended).clone()
    }

    async fn read_loop(self: Arc<Self>, reader: Reader) {
        let mut r = BufReader::new(reader);
        while let Some(msg) = read_frame(&mut r).await {
            let id = msg.get("id").cloned();
            let method = msg.get("method").and_then(Value::as_str);
            match (id, method) {
                // A reply to us.
                (Some(id), None) => {
                    let Some(id) = id.as_i64() else { continue };
                    if let Some(tx) = lock(&self.pending).remove(&id) {
                        let out = match msg.get("error") {
                            Some(e) => Err(e["message"].as_str().unwrap_or("error").to_string()),
                            None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        let _ = tx.send(out);
                    }
                }
                // The server asks us something: answer so it doesn't wait.
                (Some(id), Some(m)) => {
                    let result = if m == "workspace/configuration" {
                        let n = msg["params"]["items"].as_array().map_or(0, Vec::len);
                        Value::Array(vec![Value::Null; n])
                    } else {
                        Value::Null
                    };
                    let _ = self
                        .send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
                        .await;
                }
                (None, Some("textDocument/publishDiagnostics")) => {
                    let uri = msg["params"]["uri"].as_str().unwrap_or("").to_string();
                    let list: Vec<LspDiagnostic> = msg["params"]["diagnostics"]
                        .as_array()
                        .map(|a| a.iter().take(1000).map(diagnostic).collect())
                        .unwrap_or_default();
                    {
                        let mut d = lock(&self.diags);
                        let e = d.entry(uri).or_insert((0, Vec::new()));
                        e.0 += 1;
                        e.1 = list;
                    }
                    self.published.notify_waiters();
                }
                _ => {}
            }
        }
        self.alive.store(false, Ordering::SeqCst);
        lock(&self.ended).get_or_insert_with(|| "it exited".to_string());
        for (_, tx) in lock(&self.pending).drain() {
            let _ = tx.send(Err("the server stopped".into()));
        }
        self.published.notify_waiters();
    }

    async fn send(&self, v: &Value) -> Result<(), String> {
        let body = serde_json::to_vec(v).map_err(|e| e.to_string())?;
        let mut w = self.writer.lock().await;
        w.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        w.write_all(&body).await.map_err(|e| e.to_string())?;
        w.flush().await.map_err(|e| e.to_string())
    }

    pub async fn request(
        &self,
        method: &str,
        params: Value,
        limit: Duration,
    ) -> Result<Value, String> {
        if !self.alive() {
            return Err("the server stopped".into());
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(id, tx);
        if let Err(e) = self
            .send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await
        {
            lock(&self.pending).remove(&id);
            return Err(e);
        }
        match tokio::time::timeout(limit, rx).await {
            Ok(Ok(out)) => out,
            Ok(Err(_)) => Err("the server stopped".into()),
            Err(_) => {
                lock(&self.pending).remove(&id);
                Err("timeout".into())
            }
        }
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await
    }

    async fn initialize(&self, root: &Path) -> Result<(), String> {
        let uri = file_uri(root);
        let params = json!({
            "processId": std::process::id(),
            "rootUri": uri,
            "workspaceFolders": [{ "uri": uri, "name": root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() }],
            "capabilities": {
                "textDocument": {
                    "synchronization": { "didSave": false },
                    "hover": { "contentFormat": ["plaintext", "markdown"] },
                    "definition": { "linkSupport": false },
                    "references": {},
                    "publishDiagnostics": { "relatedInformation": false },
                },
                "workspace": { "configuration": true, "workspaceFolders": true },
            },
        });
        self.request("initialize", params, INIT_LIMIT).await?;
        self.notify("initialized", json!({})).await
    }

    /// Sends the editor's text: open on first sight, then full changes.
    /// Sends the text if the server hasn't it yet; true when it was sent.
    async fn sync(&self, uri: &str, path: &str, text: &str) -> Result<bool, String> {
        let mut docs = self.docs.lock().await;
        match docs.get_mut(uri) {
            None => {
                self.notify(
                    "textDocument/didOpen",
                    json!({ "textDocument": { "uri": uri, "languageId": language_id(path), "version": 1, "text": text } }),
                )
                .await?;
                docs.insert(uri.to_string(), (1, text.to_string()));
            }
            Some((version, last)) if last != text => {
                *version += 1;
                *last = text.to_string();
                let v = *version;
                self.notify(
                    "textDocument/didChange",
                    json!({ "textDocument": { "uri": uri, "version": v }, "contentChanges": [{ "text": text }] }),
                )
                .await?;
            }
            Some(_) => return Ok(false),
        }
        Ok(true)
    }

    fn publishes(&self, uri: &str) -> u64 {
        lock(&self.diags).get(uri).map_or(0, |e| e.0)
    }

    fn diagnostics(&self, uri: &str) -> Vec<LspDiagnostic> {
        lock(&self.diags)
            .get(uri)
            .map(|e| e.1.clone())
            .unwrap_or_default()
    }

    /// Asks the server to shut down, then ends it.
    pub async fn stop(&self) {
        if self.alive() {
            let _ = self
                .request("shutdown", Value::Null, Duration::from_secs(2))
                .await;
            let _ = self.notify("exit", Value::Null).await;
        }
        let child = lock(&self.child).take();
        if let Some(mut child) = child {
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
            let _ = child.start_kill();
        }
        self.alive.store(false, Ordering::SeqCst);
    }
}

pub fn file_uri(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = if s.starts_with('/') {
        s
    } else {
        format!("/{s}")
    };
    let mut out = String::from("file://");
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut bytes = Vec::new();
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(n) = u8::from_str_radix(std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?, 16) {
                bytes.push(n);
                i += 3;
                continue;
            }
        }
        bytes.push(b[i]);
        i += 1;
    }
    let s = String::from_utf8(bytes).ok()?;
    // `file:///C:/x` on Windows.
    let s = match s.strip_prefix('/') {
        Some(rest) if rest.len() > 1 && rest.as_bytes()[1] == b':' => rest.to_string(),
        _ => s,
    };
    Some(PathBuf::from(s))
}

// ---- Starting servers -------------------------------------------------------

/// A started server's pipes.
pub struct Spawned {
    pub reader: Reader,
    pub writer: Writer,
    pub child: Option<tokio::process::Child>,
}

/// Starts servers (the real one runs the program on the login PATH; tests
/// serve the fake in-process).
pub trait Spawner: Send + Sync {
    fn available(&self, spec: &ServerSpec, login: &LoginEnv) -> bool;
    fn spawn(&self, spec: &ServerSpec, root: &Path, login: &LoginEnv) -> Result<Spawned, String>;
}

pub struct LoginSpawner;

impl LoginSpawner {
    fn program(spec: &ServerSpec, login: &LoginEnv) -> Option<PathBuf> {
        let path = login.vars.get("PATH").map(String::as_str).unwrap_or("");
        let pathext = login.vars.get("PATHEXT").map(String::as_str);
        resolve_on_path(spec.program, path, pathext)
    }
}

impl Spawner for LoginSpawner {
    fn available(&self, spec: &ServerSpec, login: &LoginEnv) -> bool {
        Self::program(spec, login).is_some()
    }

    fn spawn(&self, spec: &ServerSpec, root: &Path, login: &LoginEnv) -> Result<Spawned, String> {
        let program = Self::program(spec, login).ok_or("it isn't installed")?;
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(spec.args)
            .current_dir(root)
            .env_clear()
            .envs(&login.vars)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console flashes up.
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let reader = child.stdout.take().ok_or("no stdout")?;
        let writer = child.stdin.take().ok_or("no stdin")?;
        Ok(Spawned {
            reader: Box::new(reader),
            writer: Box::new(writer),
            child: Some(child),
        })
    }
}

struct Running {
    client: Arc<Client>,
    last: Mutex<Instant>,
}

type Key = (PathBuf, &'static str);

/// The servers this app runs.
pub struct Lsp {
    spawner: Box<dyn Spawner>,
    running: tokio::sync::Mutex<HashMap<Key, Arc<Running>>>,
    /// One start at a time per folder and server; others don't wait.
    gates: Mutex<HashMap<Key, Arc<tokio::sync::Mutex<()>>>>,
    failures: Mutex<HashMap<Key, (Vec<Instant>, String)>>,
    /// Starts so far (tests watch it).
    pub starts: AtomicU64,
}

static LSP: LazyLock<Lsp> = LazyLock::new(|| Lsp::new(Box::new(LoginSpawner)));

/// The app's servers.
pub fn global() -> &'static Lsp {
    &LSP
}

/// Why there's no client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoServer {
    pub state: LspState,
    pub message: String,
}

impl Lsp {
    pub fn new(spawner: Box<dyn Spawner>) -> Self {
        Self {
            spawner,
            running: tokio::sync::Mutex::default(),
            gates: Mutex::default(),
            failures: Mutex::default(),
            starts: AtomicU64::new(0),
        }
    }

    fn failed(&self, key: &Key, why: String) {
        let mut f = lock(&self.failures);
        let e = f.entry(key.clone()).or_insert((Vec::new(), String::new()));
        let now = Instant::now();
        e.0.retain(|t| now.duration_since(*t) < RETRY_WINDOW);
        e.0.push(now);
        e.1 = why;
    }

    /// The running server for the folder and language, started if needed.
    pub async fn client(
        &self,
        root: &Path,
        spec: &'static ServerSpec,
        login: &LoginEnv,
    ) -> Result<Arc<Client>, NoServer> {
        let key: Key = (root.to_path_buf(), spec.program);
        let gate = lock(&self.gates).entry(key.clone()).or_default().clone();
        let _starting = gate.lock().await;
        {
            let mut running = self.running.lock().await;
            if let Some(r) = running.get(&key) {
                if r.client.alive() {
                    *lock(&r.last) = Instant::now();
                    return Ok(r.client.clone());
                }
                let why = r.client.ended().unwrap_or_else(|| "it exited".into());
                running.remove(&key);
                self.failed(&key, why);
            }
        }
        if !self.spawner.available(spec, login) {
            return Err(NoServer {
                state: LspState::None,
                message: format!("No language server for {}", spec.language),
            });
        }
        if let Some((times, why)) = lock(&self.failures).get(&key) {
            let now = Instant::now();
            let recent = times
                .iter()
                .filter(|t| now.duration_since(**t) < RETRY_WINDOW)
                .count();
            if recent >= MAX_FAILURES {
                return Err(NoServer {
                    state: LspState::Failed,
                    message: format!("{} stopped: {why}", spec.server),
                });
            }
        }
        // Spawning and initializing run without the map's lock, so another
        // folder or language isn't held up by a slow start.
        let spawned = match self.spawner.spawn(spec, root, login) {
            Ok(s) => s,
            Err(why) => {
                self.failed(&key, why.clone());
                return Err(NoServer {
                    state: LspState::Failed,
                    message: format!("{} stopped: {why}", spec.server),
                });
            }
        };
        self.starts.fetch_add(1, Ordering::SeqCst);
        let client = Client::start(spawned.reader, spawned.writer, spawned.child);
        if let Err(why) = client.initialize(root).await {
            client.stop().await;
            self.failed(&key, why.clone());
            return Err(NoServer {
                state: LspState::Failed,
                message: format!("{} stopped: {why}", spec.server),
            });
        }
        tracing::info!(
            code = "lsp.started",
            server = spec.server,
            "language server started"
        );
        let evicted = {
            let mut running = self.running.lock().await;
            // The one used longest ago makes room.
            let mut evicted = Vec::new();
            while running.len() >= MAX_SERVERS {
                let oldest = running
                    .iter()
                    .min_by_key(|(_, r)| *lock(&r.last))
                    .map(|(k, _)| k.clone());
                match oldest.and_then(|k| running.remove(&k)) {
                    Some(r) => evicted.push(r),
                    None => break,
                }
            }
            running.insert(
                key,
                Arc::new(Running {
                    client: client.clone(),
                    last: Mutex::new(Instant::now()),
                }),
            );
            evicted
        };
        for r in evicted {
            r.client.stop().await;
        }
        Ok(client)
    }

    /// Stops the servers for `root` and the folders inside it (the folder
    /// was removed).
    pub async fn forget(&self, root: &Path) -> usize {
        let gone: Vec<Arc<Running>> = {
            let mut running = self.running.lock().await;
            let keys: Vec<Key> = running
                .keys()
                .filter(|(r, _)| r.starts_with(root))
                .cloned()
                .collect();
            keys.iter().filter_map(|k| running.remove(k)).collect()
        };
        for r in &gone {
            r.client.stop().await;
        }
        gone.len()
    }

    /// Stops servers idle for `idle` or longer.
    pub async fn reap(&self, idle: Duration) -> usize {
        let mut running = self.running.lock().await;
        let now = Instant::now();
        let stale: Vec<Key> = running
            .iter()
            .filter(|(_, r)| now.duration_since(*lock(&r.last)) >= idle || !r.client.alive())
            .map(|(k, _)| k.clone())
            .collect();
        for k in &stale {
            if let Some(r) = running.remove(k) {
                r.client.stop().await;
            }
        }
        stale.len()
    }

    /// Every server stops (the app quits).
    pub async fn shutdown(&self) {
        let all: Vec<Arc<Running>> = self.running.lock().await.drain().map(|(_, r)| r).collect();
        for r in all {
            r.client.stop().await;
        }
    }

    /// How many run now.
    pub async fn count(&self) -> usize {
        self.running.lock().await.len()
    }
}

/// Stops idle servers every minute, for as long as the app runs.
pub async fn run_reaper() {
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let n = global().reap(IDLE).await;
        if n > 0 {
            tracing::info!(
                code = "lsp.idle_stopped",
                servers = n,
                "idle language servers stopped"
            );
        }
    }
}

// ---- The requests -----------------------------------------------------------

/// A path relative to the folder, inside it, or an error.
pub fn inside(root: &Path, rel: &str) -> Result<PathBuf, IpcError> {
    let p = Path::new(rel);
    let ok = !rel.is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !ok {
        return Err(
            IpcError::new(ErrorCode::Invalid, "That file isn't in this folder").with_stage("lsp"),
        );
    }
    Ok(root.join(p))
}

fn no_server_doc(spec: Option<&ServerSpec>, why: Option<NoServer>) -> LspDocument {
    LspDocument {
        language: spec.map(|s| s.language.to_string()),
        server: spec.map(|s| s.server.to_string()),
        state: why.as_ref().map_or(LspState::None, |w| w.state),
        message: Some(match (spec, why) {
            (_, Some(w)) => w.message,
            (Some(s), None) => format!("No language server for {}", s.language),
            (None, None) => "No language server for this file".to_string(),
        }),
        diagnostics: Vec::new(),
    }
}

impl Lsp {
    /// `lsp_sync`: the editor's text to the server; its diagnostics.
    pub async fn sync(
        &self,
        root: &Path,
        rel: &str,
        text: &str,
        login: &LoginEnv,
    ) -> Result<LspDocument, IpcError> {
        let full = inside(root, rel)?;
        let Some(spec) = spec_for(rel) else {
            return Ok(no_server_doc(None, None));
        };
        let client = match self.client(root, spec, login).await {
            Ok(c) => c,
            Err(why) => return Ok(no_server_doc(Some(spec), Some(why))),
        };
        let uri = file_uri(&full);
        let before = client.publishes(&uri);
        let sent = match client.sync(&uri, rel, text).await {
            Ok(sent) => sent,
            Err(why) => {
                return Ok(no_server_doc(
                    Some(spec),
                    Some(NoServer {
                        state: LspState::Failed,
                        message: format!("{} stopped: {why}", spec.server),
                    }),
                ))
            }
        };
        // The server's diagnostics for new text, if they come quickly.
        if sent {
            let wait = async {
                while client.publishes(&uri) == before && client.alive() {
                    client.published.notified().await;
                }
            };
            let _ = tokio::time::timeout(DIAGNOSTICS_WAIT, wait).await;
        }
        Ok(LspDocument {
            language: Some(spec.language.to_string()),
            server: Some(spec.server.to_string()),
            state: LspState::Ready,
            message: None,
            diagnostics: client.diagnostics(&uri),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn ask(
        &self,
        method: &str,
        root: &Path,
        rel: &str,
        text: &str,
        line: u32,
        character: u32,
        login: &LoginEnv,
        extra: Value,
    ) -> Result<Option<Value>, IpcError> {
        let full = inside(root, rel)?;
        let Some(spec) = spec_for(rel) else {
            return Ok(None);
        };
        let Ok(client) = self.client(root, spec, login).await else {
            return Ok(None);
        };
        let uri = file_uri(&full);
        client.sync(&uri, rel, text).await.map_err(|e| {
            IpcError::new(
                ErrorCode::Unavailable,
                format!("{} stopped: {e}", spec.server),
            )
        })?;
        let mut params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
        });
        if let (Some(p), Some(e)) = (params.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        match client.request(method, params, REQUEST_LIMIT).await {
            Ok(v) => Ok(Some(v)),
            Err(e) if e == "timeout" => Err(IpcError::new(
                ErrorCode::Unavailable,
                format!("{} didn't answer", spec.server),
            )
            .with_stage("lsp")),
            Err(e) => Err(
                IpcError::new(ErrorCode::Unavailable, format!("{}: {e}", spec.server))
                    .with_stage("lsp"),
            ),
        }
    }

    /// `lsp_definition`.
    #[allow(clippy::too_many_arguments)]
    pub async fn definition(
        &self,
        root: &Path,
        rel: &str,
        text: &str,
        line: u32,
        character: u32,
        login: &LoginEnv,
    ) -> Result<Vec<LspLocation>, IpcError> {
        match self
            .ask(
                "textDocument/definition",
                root,
                rel,
                text,
                line,
                character,
                login,
                json!({}),
            )
            .await?
        {
            Some(v) => Ok(locations(root, &v, Some((rel, text)))),
            None => Ok(fallback_definition(rel, text, line, character)),
        }
    }

    /// `lsp_references`.
    #[allow(clippy::too_many_arguments)]
    pub async fn references(
        &self,
        root: &Path,
        rel: &str,
        text: &str,
        line: u32,
        character: u32,
        login: &LoginEnv,
    ) -> Result<Vec<LspLocation>, IpcError> {
        Ok(self
            .ask(
                "textDocument/references",
                root,
                rel,
                text,
                line,
                character,
                login,
                json!({ "context": { "includeDeclaration": true } }),
            )
            .await?
            .map(|v| locations(root, &v, Some((rel, text))))
            .unwrap_or_default())
    }

    /// `lsp_hover`: the hover's plain text.
    #[allow(clippy::too_many_arguments)]
    pub async fn hover(
        &self,
        root: &Path,
        rel: &str,
        text: &str,
        line: u32,
        character: u32,
        login: &LoginEnv,
    ) -> Result<Option<String>, IpcError> {
        let Some(v) = self
            .ask(
                "textDocument/hover",
                root,
                rel,
                text,
                line,
                character,
                login,
                json!({}),
            )
            .await?
        else {
            return Ok(None);
        };
        let text = hover_text(&v["contents"]);
        let text = text.trim();
        Ok((!text.is_empty()).then(|| text.chars().take(HOVER_CHARS).collect()))
    }
}

/// Hover contents: a string, `{value}`, or a list of either.
fn hover_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(hover_text).collect::<Vec<_>>().join("\n\n"),
        Value::Object(o) => o
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

fn preview(root: &Path, path: &Path, line: u32, open: Option<(&str, &str)>) -> String {
    let text = match open {
        Some((rel, text)) if root.join(rel) == path => Some(text.to_string()),
        _ => std::fs::read_to_string(path).ok(),
    };
    text.and_then(|t| {
        t.lines()
            .nth(line as usize)
            .map(|l| l.trim().chars().take(200).collect())
    })
    .unwrap_or_default()
}

/// A `Location`, `Location[]` or `LocationLink[]` answer.
fn locations(root: &Path, v: &Value, open: Option<(&str, &str)>) -> Vec<LspLocation> {
    let items: Vec<&Value> = match v {
        Value::Array(a) => a.iter().collect(),
        Value::Null => Vec::new(),
        one => vec![one],
    };
    items
        .into_iter()
        .take(1000)
        .filter_map(|item| {
            let uri = item
                .get("uri")
                .or_else(|| item.get("targetUri"))?
                .as_str()?;
            let range = item
                .get("range")
                .or_else(|| item.get("targetSelectionRange"))?;
            let path = uri_path(uri)?;
            let line = u32_of(&range["start"]["line"]);
            let (rel, in_folder) = match path.strip_prefix(root) {
                Ok(rel) => (rel.to_string_lossy().replace('\\', "/"), true),
                Err(_) => (path.to_string_lossy().into_owned(), false),
            };
            Some(LspLocation {
                preview: preview(root, &path, line, open),
                path: rel,
                in_folder,
                line,
                character: u32_of(&range["start"]["character"]),
                end_line: u32_of(&range["end"]["line"]),
                end_character: u32_of(&range["end"]["character"]),
            })
        })
        .collect()
}

/// The word at a 0-based line and UTF-16 character.
pub fn word_at(text: &str, line: u32, character: u32) -> Option<String> {
    let l = text.lines().nth(line as usize)?;
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    // The char index at that UTF-16 offset.
    let mut units = 0u32;
    let chars: Vec<char> = l.chars().collect();
    let mut at = chars.len();
    for (i, c) in chars.iter().enumerate() {
        if units >= character {
            at = i;
            break;
        }
        units += c.len_utf16() as u32;
    }
    let mut start = at;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    (end > start).then(|| chars[start..end].iter().collect())
}

/// Without a server: definitions of the same name in the same file, from
/// the tree-sitter index (TypeScript, Rust, Python).
pub fn fallback_definition(rel: &str, text: &str, line: u32, character: u32) -> Vec<LspLocation> {
    let (Some(lang), Some(name)) = (
        tandem_git::symbols::Lang::of(rel),
        word_at(text, line, character),
    ) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    tandem_git::symbols::definitions(lang, text)
        .into_iter()
        .filter(|d| d.name == name)
        .map(|d| {
            let l = d.line.saturating_sub(1);
            let src = lines.get(l as usize).copied().unwrap_or("");
            let col = src
                .find(&name)
                .map(|b| src[..b].encode_utf16().count() as u32)
                .unwrap_or(0);
            LspLocation {
                path: rel.to_string(),
                in_folder: true,
                line: l,
                character: col,
                end_line: l,
                end_character: col + name.encode_utf16().count() as u32,
                preview: src.trim().chars().take(200).collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serves `tandem_engines::fake_lsp` in-process for every spec.
    struct FakeSpawner {
        installed: bool,
    }

    impl Spawner for FakeSpawner {
        fn available(&self, _: &ServerSpec, _: &LoginEnv) -> bool {
            self.installed
        }
        fn spawn(&self, _: &ServerSpec, _: &Path, _: &LoginEnv) -> Result<Spawned, String> {
            let (client_side, server_side) = tokio::io::duplex(1 << 20);
            let (sr, sw) = tokio::io::split(server_side);
            tokio::spawn(tandem_engines::fake_lsp::serve(sr, sw));
            let (cr, cw) = tokio::io::split(client_side);
            Ok(Spawned {
                reader: Box::new(cr),
                writer: Box::new(cw),
                child: None,
            })
        }
    }

    fn login() -> LoginEnv {
        LoginEnv {
            vars: Default::default(),
            shell: "/bin/sh".into(),
            degraded: false,
        }
    }

    fn folder() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = std::fs::canonicalize(tmp.path()).expect("canonical");
        std::fs::write(root.join("util.py"), "def helper():\n    return 1\n").expect("write");
        (tmp, root)
    }

    const MAIN: &str = "from util import helper\n\nvalue = helper()\nERROR here\n";

    #[tokio::test]
    async fn f_cd_23_1_definition_in_another_file_and_hover() {
        let (_t, root) = folder();
        let lsp = Lsp::new(Box::new(FakeSpawner { installed: true }));
        // `helper` on line 2 (0-based), at character 9.
        let defs = lsp
            .definition(&root, "main.py", MAIN, 2, 9, &login())
            .await
            .expect("definition");
        let util: Vec<&LspLocation> = defs.iter().filter(|d| d.path == "util.py").collect();
        assert_eq!(util.len(), 1, "{defs:?}");
        assert_eq!((util[0].line, util[0].character), (0, 4));
        assert!(util[0].in_folder);
        assert_eq!(util[0].preview, "def helper():");
        let hover = lsp
            .hover(&root, "main.py", MAIN, 2, 9, &login())
            .await
            .expect("hover");
        assert_eq!(hover.as_deref(), Some("fake hover: helper"));
        assert_eq!(
            lsp.starts.load(Ordering::SeqCst),
            1,
            "one server for the folder"
        );
        lsp.shutdown().await;
    }

    #[tokio::test]
    async fn f_cd_23_2_references_across_the_folder() {
        let (_t, root) = folder();
        let lsp = Lsp::new(Box::new(FakeSpawner { installed: true }));
        let refs = lsp
            .references(&root, "main.py", MAIN, 2, 9, &login())
            .await
            .expect("references");
        let mut at: Vec<(String, u32)> = refs.iter().map(|r| (r.path.clone(), r.line)).collect();
        at.sort();
        assert_eq!(
            at,
            [
                ("main.py".to_string(), 0),
                ("main.py".to_string(), 2),
                ("util.py".to_string(), 0)
            ]
        );
        lsp.shutdown().await;
    }

    #[tokio::test]
    async fn f_cd_23_3_diagnostics_follow_the_editors_text() {
        let (_t, root) = folder();
        let lsp = Lsp::new(Box::new(FakeSpawner { installed: true }));
        let doc = lsp
            .sync(&root, "main.py", MAIN, &login())
            .await
            .expect("sync");
        assert_eq!(doc.state, LspState::Ready);
        assert_eq!(doc.server.as_deref(), Some("pyright"));
        assert_eq!(doc.diagnostics.len(), 1);
        assert_eq!(doc.diagnostics[0].line, 3);
        assert_eq!(doc.diagnostics[0].severity, Severity::Error);
        // The unsaved fix clears it.
        let fixed = MAIN.replace("ERROR here", "ok = 1");
        let doc = lsp
            .sync(&root, "main.py", &fixed, &login())
            .await
            .expect("sync");
        assert!(doc.diagnostics.is_empty());
        lsp.shutdown().await;
    }

    #[tokio::test]
    async fn f_cd_23_4_without_a_server_the_files_own_definitions_still_work() {
        let (_t, root) = folder();
        let lsp = Lsp::new(Box::new(FakeSpawner { installed: false }));
        let doc = lsp
            .sync(&root, "src/lib.rs", "fn a() {}\n", &login())
            .await
            .expect("sync");
        assert_eq!(doc.state, LspState::None);
        assert_eq!(doc.message.as_deref(), Some("No language server for Rust"));
        let text = "fn helper() -> u32 { 1 }\n\nfn main() { helper(); }\n";
        let defs = lsp
            .definition(&root, "src/lib.rs", text, 2, 13, &login())
            .await
            .expect("definition");
        assert_eq!(defs.len(), 1);
        assert_eq!((defs[0].line, defs[0].character), (0, 3));
        assert_eq!(lsp.starts.load(Ordering::SeqCst), 0);
        // A file Vibezeo knows no server for.
        let doc = lsp
            .sync(&root, "notes.md", "x", &login())
            .await
            .expect("sync");
        assert_eq!(
            doc.message.as_deref(),
            Some("No language server for this file")
        );
    }

    #[tokio::test]
    async fn f_cd_23_5_idle_servers_stop_and_start_again_on_the_next_request() {
        let (_t, root) = folder();
        let lsp = Lsp::new(Box::new(FakeSpawner { installed: true }));
        lsp.sync(&root, "main.py", MAIN, &login())
            .await
            .expect("sync");
        assert_eq!(lsp.count().await, 1);
        assert_eq!(lsp.reap(Duration::from_secs(3600)).await, 0, "not idle yet");
        assert_eq!(lsp.reap(Duration::ZERO).await, 1);
        assert_eq!(lsp.count().await, 0);
        lsp.sync(&root, "main.py", MAIN, &login())
            .await
            .expect("sync");
        assert_eq!(lsp.starts.load(Ordering::SeqCst), 2);
        // Removing the folder stops its server.
        assert_eq!(lsp.forget(&root).await, 1);
        assert_eq!(lsp.count().await, 0);
        lsp.shutdown().await;
    }

    #[tokio::test]
    async fn f_cd_23_at_most_four_servers_run_and_paths_stay_in_the_folder() {
        let lsp = Lsp::new(Box::new(FakeSpawner { installed: true }));
        let mut dirs = Vec::new();
        for _ in 0..5 {
            let (t, root) = folder();
            lsp.sync(&root, "main.py", MAIN, &login())
                .await
                .expect("sync");
            dirs.push(t);
        }
        assert_eq!(lsp.count().await, MAX_SERVERS);
        let root = std::fs::canonicalize(dirs[0].path()).expect("canonical");
        for bad in ["../x.py", "/etc/passwd.py", ""] {
            let e = lsp.sync(&root, bad, "", &login()).await.expect_err(bad);
            assert_eq!(e.code, ErrorCode::Invalid, "{bad}");
        }
        lsp.shutdown().await;
    }

    #[test]
    fn f_cd_23_uris_and_words() {
        let p = Path::new("/code/my shop/a.py");
        let uri = file_uri(p);
        assert_eq!(uri, "file:///code/my%20shop/a.py");
        assert_eq!(uri_path(&uri).as_deref(), Some(p));
        assert_eq!(word_at("let héllo_x = 1", 0, 6).as_deref(), Some("héllo_x"));
        assert_eq!(word_at("a  b", 0, 2), None);
        assert_eq!(hover_text(&json!([{ "value": "a" }, "b"])), "a\n\nb");
    }
}
