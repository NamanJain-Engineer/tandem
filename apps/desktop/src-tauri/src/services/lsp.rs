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
use tandem_core::DataDir;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Notify};
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
    /// Arguments that must succeed for the program to count as installed:
    /// rustup's `rust-analyzer` proxy exists without the component and
    /// only fails when run.
    pub probe: Option<&'static [&'static str]>,
}

pub const SERVERS: &[ServerSpec] = &[
    ServerSpec {
        language: "Rust",
        server: "rust-analyzer",
        program: "rust-analyzer",
        args: &[],
        exts: &["rs"],
        probe: Some(&["--version"]),
    },
    ServerSpec {
        language: "TypeScript",
        server: "typescript-language-server",
        program: "typescript-language-server",
        args: &["--stdio"],
        exts: &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"],
        probe: None,
    },
    ServerSpec {
        language: "Python",
        server: "pyright",
        program: "pyright-langserver",
        args: &["--stdio"],
        exts: &["py"],
        probe: None,
    },
    ServerSpec {
        language: "Go",
        server: "gopls",
        program: "gopls",
        args: &[],
        exts: &["go"],
        probe: None,
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
    /// It's installed but waits for the repository's trust (D-2580).
    Untrusted,
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
    /// `untrusted` for a worktree whose agents run sandboxed: it's trusted
    /// on its own (D-2580).
    #[serde(default)]
    pub sandboxed: bool,
}

// ---- The client -------------------------------------------------------------

type Reader = Box<dyn AsyncRead + Send + Unpin>;
type Writer = Box<dyn AsyncWrite + Send + Unpin>;

/// Why a request got no answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    /// No answer within the request's limit; the request was cancelled.
    #[error("didn't answer")]
    Timeout,
    #[error("the server stopped")]
    Stopped,
    /// Its input is full: it isn't reading what we send.
    #[error("it isn't reading its input")]
    Stalled,
    /// The server answered with an error.
    #[error("{0}")]
    Server(String),
}

/// A message larger than this is skipped (a parsed `Value` takes many times
/// its text's size).
const MAX_FRAME: usize = 8 * 1024 * 1024;
const MAX_HEADER_LINE: usize = 1024;
const MAX_HEADERS: usize = 32;
/// Writing one message may take this long; past it the server is stuck.
const WRITE_LIMIT: Duration = Duration::from_secs(10);
/// Messages waiting to be written; a full queue means a stuck server.
const QUEUE: usize = 256;
/// The end of the server's stderr kept for "stopped: …".
const STDERR_TAIL: usize = 4096;
/// `stop` takes at most this long before the process tree is killed.
const STOP_LIMIT: Duration = Duration::from_secs(4);

/// One running server.
pub struct Client {
    /// Frames for the writer task (which owns the server's stdin).
    out: mpsc::Sender<Vec<u8>>,
    pending: Mutex<HashMap<i64, oneshot::Sender<Result<Value, ClientError>>>>,
    next: AtomicI64,
    alive: AtomicBool,
    /// Published diagnostics by file (parsed from the URI, so a server that
    /// re-encodes it still matches), and a count of publishes per file.
    diags: Mutex<HashMap<String, (u64, Vec<LspDiagnostic>)>>,
    published: Notify,
    /// The text last sent per URI, with its version.
    docs: Mutex<HashMap<String, (i32, String)>>,
    child: Mutex<Option<tokio::process::Child>>,
    on_stop: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// The process group (Unix) or process (Windows) to kill.
    tree: Option<u32>,
    stderr: Arc<Mutex<std::collections::VecDeque<u8>>>,
    /// Why it stopped, once it has.
    ended: Mutex<Option<String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One read from the server.
enum Frame {
    Message(Value),
    /// Unreadable or too large: skipped, the stream stays in step.
    Skip,
    /// The stream ended or broke.
    End,
}

/// `\uD800`…`\uDFFF` escapes replaced by `\uFFFD`: JavaScript servers send
/// lone surrogates, which serde_json refuses. Only for a message that
/// failed to parse.
fn without_surrogates(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        let esc = body.get(i..i + 6);
        let is_surrogate = esc.is_some_and(|e| {
            e[0] == b'\\'
                && (e[1] == b'u' || e[1] == b'U')
                && (e[2] == b'd' || e[2] == b'D')
                && matches!(e[3], b'8'..=b'9' | b'a'..=b'f' | b'A'..=b'F')
        });
        if is_surrogate {
            out.extend_from_slice(b"\\uFFFD");
            i += 6;
        } else {
            out.push(body[i]);
            i += 1;
        }
    }
    out
}

async fn read_frame<R: AsyncRead + Unpin>(r: &mut BufReader<R>) -> Frame {
    let mut len: Option<usize> = None;
    let mut headers = 0;
    loop {
        let mut line = Vec::new();
        let mut limited = AsyncReadExt::take(&mut *r, MAX_HEADER_LINE as u64 + 1);
        match limited.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => return Frame::End,
            Ok(_) => {}
        }
        // A header line that long, or that many, isn't LSP: give up.
        if !line.ends_with(b"\n") {
            return Frame::End;
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end();
        if text.is_empty() {
            break;
        }
        headers += 1;
        if headers > MAX_HEADERS {
            return Frame::End;
        }
        if let Some((name, value)) = text.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                len = value.trim().parse().ok();
            }
        }
    }
    let Some(len) = len else {
        return Frame::End;
    };
    if len > MAX_FRAME {
        let left = len as u64;
        let mut skip = AsyncReadExt::take(&mut *r, left);
        return match tokio::io::copy(&mut skip, &mut tokio::io::sink()).await {
            Ok(n) if n == left => Frame::Skip,
            _ => Frame::End,
        };
    }
    let mut body = vec![0u8; len];
    if r.read_exact(&mut body).await.is_err() {
        return Frame::End;
    }
    match serde_json::from_slice(&body) {
        Ok(v) => Frame::Message(v),
        Err(_) => serde_json::from_slice(&without_surrogates(&body))
            .map(Frame::Message)
            .unwrap_or(Frame::Skip),
    }
}

fn frame(v: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(v).unwrap_or_default();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(&body);
    out
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

/// The key a file's diagnostics are kept under: its path, so `file:///C:/x`
/// and `file:///c%3A/x` are one file.
fn diag_key(uri: &str) -> String {
    match uri_path(uri) {
        #[cfg(windows)]
        Some(p) => p.to_string_lossy().to_lowercase(),
        #[cfg(not(windows))]
        Some(p) => p.to_string_lossy().into_owned(),
        None => uri.to_string(),
    }
}

/// The last line of the server's stderr, for "stopped: …".
fn last_line(tail: &std::collections::VecDeque<u8>) -> Option<String> {
    let bytes: Vec<u8> = tail.iter().copied().collect();
    let text = String::from_utf8_lossy(&bytes);
    let line = text.lines().rev().map(str::trim).find(|l| !l.is_empty())?;
    Some(line.chars().take(200).collect())
}

/// Kills a server's whole tree: its process group on Unix, `taskkill /T`
/// on Windows (cargo check and tsserver are its children).
async fn kill_tree(id: u32) {
    // `kill -KILL -- -1` would signal every process the user owns.
    if id <= 1 {
        return;
    }
    let argv: Vec<String> = if cfg!(windows) {
        ["taskkill", "/T", "/F", "/PID", &id.to_string()]
            .map(String::from)
            .to_vec()
    } else {
        ["kill", "-KILL", "--", &format!("-{id}")]
            .map(String::from)
            .to_vec()
    };
    let mut cmd = tokio::process::Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: no console flashes up on every stop.
        cmd.creation_flags(0x0800_0000);
    }
    if let Ok(mut child) = cmd.spawn() {
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    }
}

impl Client {
    pub fn start(spawned: Spawned) -> Arc<Self> {
        let (out, mut queue) = mpsc::channel::<Vec<u8>>(QUEUE);
        let tail = Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let me = Arc::new(Self {
            out,
            pending: Mutex::default(),
            next: AtomicI64::new(1),
            alive: AtomicBool::new(true),
            diags: Mutex::default(),
            published: Notify::new(),
            docs: Mutex::default(),
            tree: spawned.tree,
            child: Mutex::new(spawned.child),
            on_stop: Mutex::new(spawned.on_stop),
            stderr: tail.clone(),
            ended: Mutex::new(None),
        });
        // The writer: one message at a time, each within WRITE_LIMIT.
        let weak = Arc::downgrade(&me);
        let mut writer = spawned.writer;
        tokio::spawn(async move {
            while let Some(bytes) = queue.recv().await {
                let wrote = tokio::time::timeout(WRITE_LIMIT, async {
                    writer.write_all(&bytes).await?;
                    writer.flush().await
                })
                .await;
                if !matches!(wrote, Ok(Ok(()))) {
                    if let Some(me) = weak.upgrade() {
                        me.end("it stopped reading its input".into());
                    }
                    break;
                }
            }
        });
        if let Some(mut err) = spawned.stderr {
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                while let Ok(n) = err.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let mut t = lock(&tail);
                    t.extend(&buf[..n]);
                    let over = t.len().saturating_sub(STDERR_TAIL);
                    t.drain(..over);
                }
            });
        }
        let reading = me.clone();
        tokio::spawn(async move { reading.read_loop(spawned.reader).await });
        me
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn ended(&self) -> Option<String> {
        lock(&self.ended).clone()
    }

    /// Marks it stopped (once) and fails what waits on it.
    fn end(&self, why: String) {
        self.alive.store(false, Ordering::SeqCst);
        lock(&self.ended).get_or_insert(why);
        for (_, tx) in lock(&self.pending).drain() {
            let _ = tx.send(Err(ClientError::Stopped));
        }
        self.published.notify_waiters();
    }

    /// Queues a message without waiting: a full queue is a stuck server.
    fn send(&self, v: &Value) -> Result<(), ClientError> {
        match self.out.try_send(frame(v)) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(ClientError::Stalled),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(ClientError::Stopped),
        }
    }

    async fn read_loop(self: Arc<Self>, reader: Reader) {
        let mut r = BufReader::new(reader);
        loop {
            let msg = match read_frame(&mut r).await {
                Frame::Message(m) => m,
                Frame::Skip => continue,
                Frame::End => break,
            };
            let id = msg.get("id").cloned();
            let method = msg.get("method").and_then(Value::as_str);
            match (id, method) {
                // A reply to us.
                (Some(id), None) => {
                    let Some(id) = id.as_i64() else { continue };
                    if let Some(tx) = lock(&self.pending).remove(&id) {
                        let out = match msg.get("error") {
                            Some(e) => Err(ClientError::Server(
                                e["message"].as_str().unwrap_or("error").to_string(),
                            )),
                            None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        let _ = tx.send(out);
                    }
                }
                // The server asks us something: answer without waiting on
                // the writer, so a busy input never stalls reading.
                (Some(id), Some(m)) => {
                    let result = if m == "workspace/configuration" {
                        let n = msg["params"]["items"].as_array().map_or(0, Vec::len);
                        Value::Array(vec![Value::Null; n])
                    } else {
                        Value::Null
                    };
                    let _ = self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
                }
                (None, Some("textDocument/publishDiagnostics")) => {
                    let uri = msg["params"]["uri"].as_str().unwrap_or("");
                    let list: Vec<LspDiagnostic> = msg["params"]["diagnostics"]
                        .as_array()
                        .map(|a| a.iter().take(1000).map(diagnostic).collect())
                        .unwrap_or_default();
                    {
                        let mut d = lock(&self.diags);
                        let e = d.entry(diag_key(uri)).or_insert((0, Vec::new()));
                        e.0 += 1;
                        e.1 = list;
                    }
                    self.published.notify_waiters();
                }
                _ => {}
            }
        }
        let why = match last_line(&lock(&self.stderr)) {
            Some(line) => format!("it exited ({line})"),
            None => "it exited".to_string(),
        };
        self.end(why);
    }

    pub async fn request(
        &self,
        method: &str,
        params: Value,
        limit: Duration,
    ) -> Result<Value, ClientError> {
        if !self.alive() {
            return Err(ClientError::Stopped);
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(id, tx);
        if let Err(e) =
            self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
        {
            lock(&self.pending).remove(&id);
            return Err(e);
        }
        match tokio::time::timeout(limit, rx).await {
            Ok(Ok(out)) => out,
            Ok(Err(_)) => Err(ClientError::Stopped),
            Err(_) => {
                lock(&self.pending).remove(&id);
                // The server may stop working on it.
                let _ = self.notify("$/cancelRequest", json!({ "id": id }));
                Err(ClientError::Timeout)
            }
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), ClientError> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    async fn initialize(&self, root: &Path, options: Value) -> Result<(), ClientError> {
        let uri = file_uri(root);
        let params = json!({
            "initializationOptions": options,
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
                "workspace": {
                    "configuration": true,
                    "workspaceFolders": true,
                    "didChangeWatchedFiles": { "dynamicRegistration": false },
                },
            },
        });
        self.request("initialize", params, INIT_LIMIT).await?;
        self.notify("initialized", json!({}))
    }

    /// Sends the text if the server hasn't it yet (open on first sight,
    /// then full changes); true when it was sent. Nothing waits while the
    /// document map is held.
    fn sync(&self, uri: &str, path: &str, text: &str) -> Result<bool, ClientError> {
        let mut docs = lock(&self.docs);
        match docs.get_mut(uri) {
            None => {
                self.notify(
                    "textDocument/didOpen",
                    json!({ "textDocument": { "uri": uri, "languageId": language_id(path), "version": 1, "text": text } }),
                )?;
                docs.insert(uri.to_string(), (1, text.to_string()));
            }
            Some((version, last)) if last != text => {
                let v = *version + 1;
                self.notify(
                    "textDocument/didChange",
                    json!({ "textDocument": { "uri": uri, "version": v }, "contentChanges": [{ "text": text }] }),
                )?;
                *version = v;
                *last = text.to_string();
            }
            Some(_) => return Ok(false),
        }
        Ok(true)
    }

    /// The editor closed the file: the server reads it from disk again.
    fn close(&self, uri: &str) {
        if lock(&self.docs).remove(uri).is_some() {
            let _ = self.notify(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": uri } }),
            );
        }
        lock(&self.diags).remove(&diag_key(uri));
    }

    /// Files changed on disk (an agent, git): `workspace/didChangeWatchedFiles`.
    fn changed(&self, uris: &[String]) {
        if uris.is_empty() {
            return;
        }
        let changes: Vec<Value> = uris
            .iter()
            .map(|u| json!({ "uri": u, "type": 2 }))
            .collect();
        let _ = self.notify(
            "workspace/didChangeWatchedFiles",
            json!({ "changes": changes }),
        );
    }

    fn publishes(&self, uri: &str) -> u64 {
        lock(&self.diags).get(&diag_key(uri)).map_or(0, |e| e.0)
    }

    fn diagnostics(&self, uri: &str) -> Vec<LspDiagnostic> {
        lock(&self.diags)
            .get(&diag_key(uri))
            .map(|e| e.1.clone())
            .unwrap_or_default()
    }

    /// Asks the server to shut down, then ends it and its children, within
    /// [`STOP_LIMIT`] whatever it does.
    pub async fn stop(&self) {
        let polite = async {
            if self.alive() {
                let _ = self
                    .request("shutdown", Value::Null, Duration::from_secs(2))
                    .await;
                let _ = self.notify("exit", Value::Null);
            }
        };
        let _ = tokio::time::timeout(STOP_LIMIT, polite).await;
        let child = lock(&self.child).take();
        // Without the child there's no pid known to be ours: no kill.
        if let Some(mut child) = child {
            // On Unix the group goes first, while the leader is unreaped
            // (its pid, the group's id, can't be reused yet): the group
            // outlives a leader that exits while a child (cargo check,
            // tsserver) runs.
            if cfg!(unix) {
                if let Some(id) = self.tree {
                    kill_tree(id).await;
                }
            }
            let gone = tokio::time::timeout(Duration::from_secs(1), child.wait())
                .await
                .is_ok();
            if !gone {
                // On Windows the tree goes while the server still holds
                // its pid, so `taskkill /T` can't hit a process that
                // reused it.
                if cfg!(windows) {
                    if let Some(id) = self.tree {
                        kill_tree(id).await;
                    }
                }
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
            }
        }
        if let Some(told) = lock(&self.on_stop).take() {
            told();
        }
        self.end("stopped".into());
    }
}

/// `file://` and the path, percent-encoded.
pub fn file_uri(p: &Path) -> String {
    #[cfg(windows)]
    let s = windows_uri_body(&p.to_string_lossy());
    #[cfg(not(windows))]
    let s = p.to_string_lossy().into_owned();
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

/// A Windows path as what follows `file://`: `C:\x` (or canonicalize's
/// verbatim `\\?\C:\x`) is `/C:/x`, and a share, `\\host\share\x` or
/// `\\?\UNC\host\share\x`, is `host/share/x` (the host is the URI's
/// authority).
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_uri_body(raw: &str) -> String {
    if let Some(unc) = raw
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| raw.strip_prefix(r"\\").filter(|r| !r.starts_with(r"?\")))
    {
        return unc.replace('\\', "/");
    }
    let s = raw.strip_prefix(r"\\?\").unwrap_or(raw).replace('\\', "/");
    if s.starts_with('/') {
        s
    } else {
        format!("/{s}")
    }
}

/// What follows `file://` (decoded) as a Windows path: `/C:/x` is `C:/x`,
/// `host/share/x` is `\\host\share\x`.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_path_of(s: &str) -> String {
    match s.strip_prefix('/') {
        Some(rest) if rest.len() > 1 && rest.as_bytes()[1] == b':' => rest.to_string(),
        Some(_) => s.to_string(),
        None => format!(r"\\{}", s.replace('/', r"\")),
    }
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
    // `file:///C:/x` and `file://host/share/x` on Windows.
    #[cfg(windows)]
    let s = windows_path_of(&s);
    Some(PathBuf::from(s))
}

// ---- Starting servers -------------------------------------------------------

/// A started server's pipes.
pub struct Spawned {
    pub reader: Reader,
    pub writer: Writer,
    /// Its stderr, whose end says why it stopped.
    pub stderr: Option<Reader>,
    pub child: Option<tokio::process::Child>,
    /// Its process group (Unix) or process (Windows), killed with it.
    pub tree: Option<u32>,
    /// Told when the server is stopped (tests record it).
    pub on_stop: Option<Box<dyn FnOnce() + Send>>,
}

/// Starts servers (the real one runs the program on the login PATH; tests
/// serve the fake in-process).
pub trait Spawner: Send + Sync {
    fn available(&self, spec: &ServerSpec, login: &LoginEnv) -> bool;
    fn spawn(&self, spec: &ServerSpec, root: &Path, login: &LoginEnv) -> Result<Spawned, String>;
}

/// Servers run here, not in the repository: on Windows npm's `.cmd` shims
/// look for `node` in the current folder first. The folder reaches the
/// server through `rootUri` and `workspaceFolders`.
static WORK_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Sets the folder servers run in (`$DATA/lsp`), once at startup.
pub fn set_work_dir(dir: PathBuf) {
    let _ = WORK_DIR.set(dir);
}

fn work_dir() -> PathBuf {
    let dir = WORK_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| std::env::temp_dir().join("vibezeo-lsp"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// How long an "installed?" answer is kept.
const PROBE_TTL: Duration = Duration::from_secs(60);

/// Runs `argv` with the login environment and a 5 s limit; true when it
/// exits 0.
fn probe(argv: &[std::ffi::OsString], login: &LoginEnv) -> bool {
    let Some((program, args)) = argv.split_first() else {
        return false;
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .env_clear()
        .envs(&login.vars)
        .current_dir(work_dir())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// `argv` as run on the host when Vibezeo is a Flatpak (D-2032), with
/// exactly `env`; as is otherwise.
fn host_argv(
    argv: Vec<std::ffi::OsString>,
    cwd: &Path,
    env: &[(String, String)],
) -> Vec<std::ffi::OsString> {
    if !tandem_core::sandbox::in_flatpak() {
        return argv;
    }
    let Some((program, args)) = argv.split_first() else {
        return argv;
    };
    tandem_core::sandbox::flatpak_host_argv(program, args, Some(cwd), Some(env))
}

/// The real servers: on the login PATH, run with the login environment.
#[derive(Default)]
pub struct LoginSpawner {
    probed: Mutex<HashMap<(String, String), (bool, Instant)>>,
}

impl LoginSpawner {
    fn program(spec: &ServerSpec, login: &LoginEnv) -> Option<PathBuf> {
        let path = login.vars.get("PATH").map(String::as_str).unwrap_or("");
        let pathext = login.vars.get("PATHEXT").map(String::as_str);
        resolve_on_path(spec.program, path, pathext)
    }

    /// The server's environment: the login's, and Go never switching (and
    /// downloading) toolchains on its own.
    fn env(login: &LoginEnv) -> Vec<(String, String)> {
        let mut vars: Vec<(String, String)> = login
            .vars
            .iter()
            .filter(|(k, _)| k.as_str() != "GOTOOLCHAIN")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        vars.push(("GOTOOLCHAIN".into(), "local".into()));
        vars
    }

    fn installed(&self, spec: &ServerSpec, login: &LoginEnv) -> bool {
        let path = login.vars.get("PATH").cloned().unwrap_or_default();
        let key = (spec.program.to_string(), path);
        if let Some((ok, at)) = lock(&self.probed).get(&key) {
            if at.elapsed() < PROBE_TTL {
                return *ok;
            }
        }
        let ok = if tandem_core::sandbox::in_flatpak() {
            // The program lives on the host: ask the host's shell.
            let check = format!("command -v {} >/dev/null", spec.program);
            let mut argv: Vec<std::ffi::OsString> = vec!["sh".into(), "-c".into(), check.into()];
            if let Some(args) = spec.probe {
                argv = vec![spec.program.into()];
                argv.extend(args.iter().map(Into::into));
            }
            probe(&host_argv(argv, &work_dir(), &Self::env(login)), login)
        } else {
            match (Self::program(spec, login), spec.probe) {
                (None, _) => false,
                (Some(_), None) => true,
                (Some(p), Some(args)) => {
                    let mut argv: Vec<std::ffi::OsString> = vec![p.into()];
                    argv.extend(args.iter().map(Into::into));
                    probe(&argv, login)
                }
            }
        };
        lock(&self.probed).insert(key, (ok, Instant::now()));
        ok
    }
}

impl Spawner for LoginSpawner {
    fn available(&self, spec: &ServerSpec, login: &LoginEnv) -> bool {
        self.installed(spec, login)
    }

    fn spawn(&self, spec: &ServerSpec, _root: &Path, login: &LoginEnv) -> Result<Spawned, String> {
        let program: std::ffi::OsString = if tandem_core::sandbox::in_flatpak() {
            spec.program.into()
        } else {
            Self::program(spec, login)
                .ok_or("it isn't installed")?
                .into()
        };
        let cwd = work_dir();
        let env = Self::env(login);
        let mut argv = vec![program];
        argv.extend(spec.args.iter().map(Into::into));
        let argv = host_argv(argv, &cwd, &env);
        let mut cmd = tokio::process::Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .current_dir(&cwd)
            .env_clear()
            .envs(env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        // Its own process group, so cargo check and tsserver go with it.
        #[cfg(unix)]
        cmd.process_group(0);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP.
            cmd.creation_flags(0x0800_0000 | 0x0000_0200);
        }
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let reader = child.stdout.take().ok_or("no stdout")?;
        let writer = child.stdin.take().ok_or("no stdin")?;
        let stderr = child.stderr.take().map(|e| Box::new(e) as Reader);
        Ok(Spawned {
            tree: child.id(),
            on_stop: None,
            reader: Box::new(reader),
            writer: Box::new(writer),
            stderr,
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
    /// Folders being removed, by how many removals are under way: no server
    /// starts there until they finish (a hover during a worktree's teardown
    /// would restart one).
    closing: Mutex<HashMap<PathBuf, usize>>,
    /// Bumped when every server stops (the app quits, the sandbox setting
    /// changes): a start that began before isn't kept.
    generation: AtomicU64,
    /// Folders forgotten, numbered: a start that began before a folder
    /// covering it was forgotten isn't kept.
    forgets: AtomicU64,
    forgotten: Mutex<std::collections::VecDeque<(u64, PathBuf)>>,
    /// Starts so far (tests watch it).
    pub starts: AtomicU64,
}

static LSP: LazyLock<Lsp> = LazyLock::new(|| Lsp::new(Box::<LoginSpawner>::default()));

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
            closing: Mutex::default(),
            generation: AtomicU64::new(0),
            forgets: AtomicU64::new(0),
            forgotten: Mutex::default(),
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
        let root = &norm(root);
        // The epoch the request's trust was decided in ([`with_epoch`]), or
        // now: a forget or shutdown after it is seen below.
        let epoch = EPOCH.try_with(|e| *e).unwrap_or_else(|_| self.epoch());
        let key: Key = (root.clone(), spec.program);
        let gate = lock(&self.gates).entry(key.clone()).or_default().clone();
        let _starting = gate.lock().await;
        // Waiting on the gate (another start, up to 30 s) may have outlasted
        // the trust the request was let through with.
        if let Some(why) = self.superseded(root, epoch) {
            return Err(why);
        }
        // A server that stopped working is taken out, and stopped (its tree
        // killed) outside the lock, so it never runs on uncounted.
        let dead = {
            let mut running = self.running.lock().await;
            match running.get(&key) {
                Some(r) if r.client.alive() => {
                    *lock(&r.last) = Instant::now();
                    return Ok(r.client.clone());
                }
                Some(_) => running.remove(&key),
                None => None,
            }
        };
        if let Some(r) = dead {
            let why = r.client.ended().unwrap_or_else(|| "it exited".into());
            r.client.stop().await;
            self.failed(&key, why);
        }
        // A folder being removed (or gone) gets no new server.
        if !root.is_dir() || self.closing(root) {
            return Err(NoServer {
                state: LspState::None,
                message: "This folder is going away".into(),
            });
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
        // Once more right before spawning: stopping a dead server and the
        // probe above can take seconds, and a sandbox switch or removal in
        // them must not let a server start on the old decision.
        if let Some(why) = self.superseded(root, epoch) {
            return Err(why);
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
        let client = Client::start(spawned);
        if let Err(why) = client.initialize(root, init_options(spec)).await {
            let why = client.ended().unwrap_or_else(|| why.to_string());
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
            // Its folder was forgotten, or every server stopped, while it
            // started: it isn't kept.
            if let Some(why) = self.superseded(root, epoch) {
                drop(running);
                client.stop().await;
                return Err(why);
            }
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
    /// was removed), and any starting there now.
    pub async fn forget(&self, root: &Path) -> usize {
        let root = &norm(root);
        {
            let mut forgotten = lock(&self.forgotten);
            let n = self.forgets.fetch_add(1, Ordering::SeqCst) + 1;
            forgotten.push_back((n, root.clone()));
            while forgotten.len() > FORGOTTEN_KEPT {
                forgotten.pop_front();
            }
        }
        let gone: Vec<Arc<Running>> = {
            let mut running = self.running.lock().await;
            let keys: Vec<Key> = running
                .keys()
                .filter(|(r, _)| r.starts_with(root))
                .cloned()
                .collect();
            keys.iter().filter_map(|k| running.remove(k)).collect()
        };
        stop_all(gone.clone()).await;
        gone.len()
    }

    /// [`forget`](Self::forget), and no server starts in `root` until the
    /// returned guard drops (when the removal has finished).
    pub async fn begin_removal(&self, root: &Path) -> Closing<'_> {
        let root = norm(root);
        // The guard exists as the count goes up, so a cancelled or failed
        // forget never leaves the folder closed for good.
        *lock(&self.closing).entry(root.clone()).or_default() += 1;
        let closing = Closing {
            lsp: self,
            root: root.clone(),
        };
        self.forget(&root).await;
        closing
    }

    /// What a request's trust was decided in: take it before deciding.
    pub fn epoch(&self) -> Epoch {
        Epoch {
            generation: self.generation.load(Ordering::SeqCst),
            forgets: self.forgets.load(Ordering::SeqCst),
        }
    }

    /// Whether any server runs for `root` (a cheap check before work).
    pub async fn runs_in(&self, root: &Path) -> bool {
        let root = norm(root);
        self.running.lock().await.keys().any(|(r, _)| *r == root)
    }

    /// `root` is (inside) a folder being removed.
    fn closing(&self, root: &Path) -> bool {
        lock(&self.closing)
            .keys()
            .any(|gone| root.starts_with(gone))
    }

    /// Why a start in `epoch` isn't kept, if it isn't: every server
    /// stopped since (quit, the sandbox setting changed), or a folder
    /// covering `root` is being removed or was forgotten since.
    fn superseded(&self, root: &Path, epoch: Epoch) -> Option<NoServer> {
        if self.generation.load(Ordering::SeqCst) != epoch.generation {
            return Some(NoServer {
                state: LspState::None,
                message: RESTARTED.into(),
            });
        }
        let forgotten = {
            let list = lock(&self.forgotten);
            // Forgets the list no longer holds (it keeps the last
            // FORGOTTEN_KEPT) can't be checked: fail closed.
            let evicted = list.front().is_some_and(|(n, _)| *n > epoch.forgets + 1);
            evicted
                || list
                    .iter()
                    .any(|(n, gone)| *n > epoch.forgets && root.starts_with(gone))
        };
        (forgotten || self.closing(root)).then(|| NoServer {
            state: LspState::None,
            message: "This folder is going away".into(),
        })
    }

    /// Stops servers idle for `idle` or longer, and drops dead ones, which
    /// count toward the failure limit (a crash loop stops being retried).
    /// They stop outside the map's lock.
    pub async fn reap(&self, idle: Duration) -> usize {
        let gone: Vec<(Key, Arc<Running>)> = {
            let mut running = self.running.lock().await;
            let now = Instant::now();
            let stale: Vec<Key> = running
                .iter()
                .filter(|(_, r)| now.duration_since(*lock(&r.last)) >= idle || !r.client.alive())
                .map(|(k, _)| k.clone())
                .collect();
            stale
                .into_iter()
                .filter_map(|k| running.remove(&k).map(|r| (k, r)))
                .collect()
        };
        for (key, r) in &gone {
            if !r.client.alive() {
                self.failed(key, r.client.ended().unwrap_or_else(|| "it exited".into()));
            }
            r.client.stop().await;
        }
        gone.len()
    }

    /// Every server stops (the app quits, or the sandbox setting changed),
    /// all at once, and none starting now is kept.
    pub async fn shutdown(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let all: Vec<Arc<Running>> = self.running.lock().await.drain().map(|(_, r)| r).collect();
        stop_all(all).await;
    }

    /// The editor closed `rel`: its server reads the file from disk again.
    pub async fn close(&self, root: &Path, rel: &str) {
        let (Ok(full), Some(spec)) = (inside(root, rel), spec_for(rel)) else {
            return;
        };
        let key: Key = (norm(root), spec.program);
        let running = self.running.lock().await.get(&key).cloned();
        if let Some(r) = running {
            r.client.close(&file_uri(&full));
        }
    }

    /// Files under `root` changed on disk (an agent, git): every server for
    /// `root` hears `workspace/didChangeWatchedFiles`. Files the editor has
    /// open keep the editor's text, which the next sync brings.
    pub async fn changed(&self, root: &Path, rels: &[String]) {
        let uris: Vec<String> = rels
            .iter()
            .filter_map(|r| inside(root, r).ok())
            .map(|p| file_uri(&p))
            .collect();
        let root = norm(root);
        let clients: Vec<Arc<Running>> = self
            .running
            .lock()
            .await
            .iter()
            .filter(|((r, _), _)| *r == root)
            .map(|(_, v)| v.clone())
            .collect();
        for r in clients {
            r.client.changed(&uris);
        }
    }

    /// The server is on the login PATH.
    pub fn available(&self, spec: &ServerSpec, login: &LoginEnv) -> bool {
        self.spawner.available(spec, login)
    }

    /// How many run now.
    pub async fn count(&self) -> usize {
        self.running.lock().await.len()
    }
}

/// Said to a request whose trust was decided before every server stopped.
pub const RESTARTED: &str = "Language servers restarted: the sandbox setting changed";

/// When a request's trust was decided ([`Lsp::epoch`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Epoch {
    generation: u64,
    forgets: u64,
}

tokio::task_local! {
    static EPOCH: Epoch;
}

/// Runs a request with the epoch its trust was decided in, so a server it
/// would start after a shutdown or forget since isn't kept.
pub async fn with_epoch<F: std::future::Future>(epoch: Epoch, f: F) -> F::Output {
    EPOCH.scope(epoch, f).await
}

/// A folder being removed: no server starts in it until this drops.
#[must_use = "the folder reopens to servers when this drops"]
pub struct Closing<'a> {
    lsp: &'a Lsp,
    root: PathBuf,
}

impl Drop for Closing<'_> {
    fn drop(&mut self) {
        let mut closing = lock(&self.lsp.closing);
        if let Some(n) = closing.get_mut(&self.root) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                closing.remove(&self.root);
            }
        }
    }
}

/// How many forgotten folders a start in progress is checked against.
const FORGOTTEN_KEPT: usize = 256;

/// One spelling per folder: Windows' verbatim `\\?\C:\x` (canonicalize's)
/// and `C:\x` (a workspace's root) are the same key.
pub fn norm(p: &Path) -> PathBuf {
    dunce::simplified(p).to_path_buf()
}

/// Stops servers concurrently, so one slow stop doesn't hold up the rest.
async fn stop_all(servers: Vec<Arc<Running>>) {
    let mut set = tokio::task::JoinSet::new();
    for r in servers {
        set.spawn(async move { r.client.stop().await });
    }
    while set.join_next().await.is_some() {}
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

// ---- Trust (D-2580) ---------------------------------------------------------
//
// Loading a folder can run the repository's own code (build scripts and proc
// macros, tsserver plugins and the repository's TypeScript, toolchain
// switches, the interpreter a Python config names), so every server waits
// for the repository's trust.

/// The repositories whose language servers may run, in `$DATA`.
pub const TRUST_FILE: &str = "lsp-trust.json";

static TRUST_LOCK: Mutex<()> = Mutex::new(());

fn trust_key(main: &Path) -> String {
    std::fs::canonicalize(main)
        .unwrap_or_else(|_| main.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn read_trust(data: &DataDir) -> Vec<String> {
    std::fs::read(data.root().join(TRUST_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// The repository at `main` (its main checkout) is trusted for language
/// servers.
pub fn is_trusted(data: &DataDir, main: &Path) -> bool {
    read_trust(data).contains(&trust_key(main))
}

/// Records the repository at `main` as trusted for language servers.
pub fn trust(data: &DataDir, main: &Path) -> Result<(), IpcError> {
    let _guard = lock(&TRUST_LOCK);
    let mut all = read_trust(data);
    let key = trust_key(main);
    if !all.contains(&key) {
        all.push(key);
    }
    let bytes =
        serde_json::to_vec_pretty(&all).map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
    tandem_core::atomic::atomic_write(&data.root().join(TRUST_FILE), &bytes, Some(0o600))
        .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()).with_stage("lsp"))
}

/// Forgets the trust of `root` and the folders inside it: a removed
/// worktree's (slugs are reused, so the next one at that path asks again).
pub fn untrust(data: &DataDir, root: &Path) {
    let _guard = lock(&TRUST_LOCK);
    let key = trust_key(root);
    let mut all = read_trust(data);
    let before = all.len();
    all.retain(|k| !Path::new(k).starts_with(&key));
    if all.len() == before {
        return;
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&all) {
        if let Err(e) =
            tandem_core::atomic::atomic_write(&data.root().join(TRUST_FILE), &bytes, Some(0o600))
        {
            tracing::warn!(code = "lsp.untrust_failed", error = %e, "trust not forgotten");
        }
    }
}

/// The app's data folder, for [`forget_worktree`] and
/// [`untrust_new_worktree`].
static DATA: std::sync::OnceLock<DataDir> = std::sync::OnceLock::new();

/// Sets the data folder, once at startup.
pub fn set_data_dir(data: DataDir) {
    let _ = DATA.set(data);
}

/// The data folder trust is kept in: the app's, or (in tests, which share
/// one process) one temporary folder, set on first use. Trust is keyed by
/// path, and each test's folders are its own.
pub fn data_dir() -> Option<&'static DataDir> {
    #[cfg(test)]
    if DATA.get().is_none() {
        let dir = std::env::temp_dir().join(format!("tandem-lsp-trust-{}", std::process::id()));
        if let Ok(data) = DataDir::at(dir) {
            let _ = DATA.set(data);
        }
    }
    DATA.get()
}

/// A worktree being removed: its servers stop (and any starting), none
/// starts until the returned guard drops, and its own trust goes.
#[must_use = "the folder reopens to servers when this drops"]
pub async fn forget_worktree(root: &Path) -> Closing<'static> {
    let closing = global().begin_removal(root).await;
    if let Some(data) = data_dir() {
        untrust(data, root);
    }
    closing
}

/// A new worktree: no trust is left at its path from an earlier one (its
/// row went with its workspace, or its folder was deleted outside the
/// app), so it asks again.
pub async fn untrust_new_worktree(root: &Path) {
    untrust_new_worktree_in(global(), data_dir(), root).await;
}

/// [`untrust_new_worktree`] for a given set of servers and trust store.
pub(crate) async fn untrust_new_worktree_in(lsp: &Lsp, data: Option<&DataDir>, root: &Path) {
    if let Some(data) = data {
        untrust(data, root);
    }
    // A request that read the leftover trust before it went is superseded,
    // as after a sandbox switch: trust first, then the bump.
    lsp.forget(root).await;
}

/// Before a worktree is made in `dir` (`<repo>/.tandem/worktrees`): the
/// trust of every folder there that no longer exists goes, so the new
/// worktree's path has none when its folder appears.
pub fn untrust_missing_under(dir: &Path) {
    if let Some(data) = data_dir() {
        untrust_missing_in(data, dir);
    }
}

/// Whether any folder in or under `dir` is trusted (tests).
#[cfg(test)]
pub(crate) fn trusted_under(data: &DataDir, dir: &Path) -> bool {
    let under = trust_key(dir);
    read_trust(data)
        .iter()
        .any(|k| Path::new(k).starts_with(&under))
}

pub(crate) fn untrust_missing_in(data: &DataDir, dir: &Path) {
    let _guard = lock(&TRUST_LOCK);
    let under = trust_key(dir);
    let mut all = read_trust(data);
    let before = all.len();
    all.retain(|k| {
        let p = Path::new(k);
        !(p.starts_with(&under) && p != Path::new(&under) && !p.exists())
    });
    if all.len() == before {
        return;
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(&all) {
        if let Err(e) =
            tandem_core::atomic::atomic_write(&data.root().join(TRUST_FILE), &bytes, Some(0o600))
        {
            tracing::warn!(code = "lsp.untrust_failed", error = %e, "trust not forgotten");
        }
    }
}

/// Whether no server may start in `root` now (a removal is under way).
pub fn is_closing(root: &Path) -> bool {
    global().closing(&norm(root))
}

/// What a server is told at `initialize`. typescript-language-server's
/// automatic type acquisition would npm-install `types-registry` and
/// `@types/*` the project never declared: off (nothing leaves the machine
/// without an opt-in).
pub fn init_options(spec: &ServerSpec) -> Value {
    if spec.program == "typescript-language-server" {
        json!({ "disableAutomaticTypingAcquisition": true })
    } else {
        Value::Null
    }
}

/// What `lsp_sync` says for a server that isn't installed.
pub fn missing_doc(spec: &ServerSpec) -> LspDocument {
    no_server_doc(Some(spec), None)
}

/// A sandboxed worktree is trusted on its own (D-2580).
pub fn untrusted_doc(spec: &ServerSpec, sandboxed: bool) -> LspDocument {
    let what = if sandboxed {
        "this worktree"
    } else {
        "this repository"
    };
    LspDocument {
        language: Some(spec.language.to_string()),
        server: Some(spec.server.to_string()),
        state: LspState::Untrusted,
        message: Some(format!("{} is off until you trust {what}", spec.server)),
        diagnostics: Vec::new(),
        sandboxed,
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
        sandboxed: false,
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
        let sent = match client.sync(&uri, rel, text) {
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
                loop {
                    // Registered before the check, so a publish between the
                    // two isn't missed.
                    let notified = client.published.notified();
                    tokio::pin!(notified);
                    notified.as_mut().enable();
                    if client.publishes(&uri) != before || !client.alive() {
                        break;
                    }
                    notified.await;
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
            sandboxed: false,
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
        client.sync(&uri, rel, text).map_err(|e| {
            IpcError::new(
                ErrorCode::Unavailable,
                format!("{} stopped: {e}", spec.server),
            )
            .with_stage("lsp")
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
            Err(ClientError::Timeout) => Err(IpcError::new(
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
            Some(v) => Ok(locations(root, &v, Some((rel, text))).await),
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
        match self
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
        {
            Some(v) => Ok(locations(root, &v, Some((rel, text))).await),
            None => Ok(Vec::new()),
        }
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

/// Previews read at most this much of a file.
const PREVIEW_READ: u64 = 2 * 1024 * 1024;

/// Each location's line of text: from the editor's text for the open file,
/// else read once per file (at most [`PREVIEW_READ`]), off the async runtime.
async fn fill_previews(
    root: &Path,
    found: &mut [(PathBuf, LspLocation)],
    open: Option<(&str, &str)>,
) {
    let open_path = open.map(|(rel, text)| (root.join(rel), text.to_string()));
    let wanted: Vec<(PathBuf, u32)> = found.iter().map(|(p, l)| (p.clone(), l.line)).collect();
    let lines = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut files: HashMap<PathBuf, Option<String>> = HashMap::new();
        wanted
            .into_iter()
            .map(|(path, line)| {
                let text = files
                    .entry(path.clone())
                    .or_insert_with(|| match &open_path {
                        Some((p, t)) if *p == path => Some(t.clone()),
                        _ => {
                            let mut buf = Vec::new();
                            std::fs::File::open(&path)
                                .and_then(|f| f.take(PREVIEW_READ).read_to_end(&mut buf))
                                .ok()
                                .map(|_| String::from_utf8_lossy(&buf).into_owned())
                        }
                    });
                text.as_deref()
                    .and_then(|t| t.lines().nth(line as usize))
                    .map(|l| l.trim().chars().take(200).collect::<String>())
                    .unwrap_or_default()
            })
            .collect::<Vec<String>>()
    })
    .await
    .unwrap_or_default();
    for ((_, loc), line) in found.iter_mut().zip(lines) {
        loc.preview = line;
    }
}

/// A `Location`, `Location[]` or `LocationLink[]` answer.
async fn locations(root: &Path, v: &Value, open: Option<(&str, &str)>) -> Vec<LspLocation> {
    let items: Vec<&Value> = match v {
        Value::Array(a) => a.iter().collect(),
        Value::Null => Vec::new(),
        one => vec![one],
    };
    let mut found: Vec<(PathBuf, LspLocation)> = items
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
            let (rel, in_folder) = match norm(&path).strip_prefix(norm(root)) {
                Ok(rel) => (rel.to_string_lossy().replace('\\', "/"), true),
                Err(_) => (path.to_string_lossy().into_owned(), false),
            };
            Some((
                path,
                LspLocation {
                    preview: String::new(),
                    path: rel,
                    in_folder,
                    line: u32_of(&range["start"]["line"]),
                    character: u32_of(&range["start"]["character"]),
                    end_line: u32_of(&range["end"]["line"]),
                    end_character: u32_of(&range["end"]["character"]),
                },
            ))
        })
        .collect();
    fill_previews(root, &mut found, open).await;
    found.into_iter().map(|(_, l)| l).collect()
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
pub(crate) mod tests {
    use super::*;

    /// A client wired to `server`, whose side the test drives byte by byte.
    fn raw_client() -> (Arc<Client>, tokio::io::DuplexStream) {
        let (client_side, server_side) = tokio::io::duplex(1 << 16);
        let (cr, cw) = tokio::io::split(client_side);
        let client = Client::start(Spawned {
            reader: Box::new(cr),
            writer: Box::new(cw),
            stderr: None,
            child: None,
            tree: None,
            on_stop: None,
        });
        (client, server_side)
    }

    fn raw(body: &[u8]) -> Vec<u8> {
        let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
        out.extend_from_slice(body);
        out
    }

    #[tokio::test]
    async fn f_cd_23_framing_skips_bad_and_huge_messages_and_reads_lone_surrogates() {
        let (client, mut server) = raw_client();
        let uri = file_uri(Path::new("/w/a.py"));
        let publish = |msg: &str| {
            format!(
                r#"{{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{{"uri":"{uri}","diagnostics":[{{"range":{{"start":{{"line":0,"character":0}},"end":{{"line":0,"character":1}}}},"message":"{msg}"}}]}}}}"#
            )
        };
        // Not JSON: skipped. Too large: skipped without losing step.
        server.write_all(&raw(b"{not json")).await.expect("write");
        let big = vec![b' '; MAX_FRAME + 10];
        server.write_all(&raw(&big)).await.expect("write");
        // A lone surrogate, as JavaScript servers send: read, not fatal.
        server
            .write_all(&raw(publish("bad \\ud800 char").as_bytes()))
            .await
            .expect("write");
        let wait = async {
            while client.publishes(&uri) == 0 {
                client.published.notified().await;
            }
        };
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .expect("the diagnostic arrived");
        assert!(client.alive(), "a bad message doesn't end the server");
        assert_eq!(client.diagnostics(&uri)[0].message, "bad \u{fffd} char");
        // A server that re-encodes the URI still matches the file.
        let encoded = uri.replace(".py", "%2Epy");
        assert_eq!(diag_key(&encoded), diag_key(&uri));
    }

    #[tokio::test(start_paused = true)]
    async fn f_cd_23_a_stuck_server_times_out_is_cancelled_and_stops_in_bounded_time() {
        let (client, mut server) = raw_client();
        // The server reads nothing back: the request times out…
        let e = client
            .request("textDocument/hover", json!({}), REQUEST_LIMIT)
            .await
            .expect_err("no answer");
        assert_eq!(e, ClientError::Timeout);
        // …and $/cancelRequest follows it.
        let mut got = vec![0u8; 4096];
        let n = tokio::time::timeout(Duration::from_secs(1), server.read(&mut got))
            .await
            .expect("read")
            .expect("bytes");
        let text = String::from_utf8_lossy(&got[..n]).into_owned();
        assert!(text.contains("textDocument/hover"));
        let mut rest = text;
        while !rest.contains("$/cancelRequest") {
            let n = server.read(&mut got).await.expect("bytes");
            rest.push_str(&String::from_utf8_lossy(&got[..n]));
        }
        // stop() ends within its limit though the server never answers.
        let started = tokio::time::Instant::now();
        client.stop().await;
        assert!(started.elapsed() <= STOP_LIMIT + Duration::from_secs(1));
        assert!(!client.alive());
    }

    #[tokio::test]
    async fn f_cd_23_typescript_never_downloads_types_on_its_own() {
        let ts = spec_for("a.ts").expect("ts");
        assert_eq!(
            init_options(ts),
            json!({ "disableAutomaticTypingAcquisition": true })
        );
        assert_eq!(init_options(spec_for("a.py").expect("py")), Value::Null);
        // initialize carries it.
        let (client, server) = raw_client();
        let (sr, mut sw) = tokio::io::split(server);
        let asked = tokio::spawn(async move {
            let mut r = BufReader::new(sr);
            let Frame::Message(m) = read_frame(&mut r).await else {
                panic!("no initialize");
            };
            let reply =
                json!({ "jsonrpc": "2.0", "id": m["id"], "result": { "capabilities": {} } });
            sw.write_all(&frame(&reply)).await.expect("reply");
            m
        });
        client
            .initialize(Path::new("/w"), init_options(ts))
            .await
            .expect("initialized");
        let m = asked.await.expect("asked");
        assert_eq!(
            m["params"]["initializationOptions"]["disableAutomaticTypingAcquisition"],
            json!(true)
        );
    }

    #[tokio::test]
    async fn f_cd_23_a_dead_server_is_stopped_and_counted_and_a_folder_going_away_gets_none() {
        let (_t, root) = folder();
        let crashing = CrashingSpawner::default();
        let stops = crashing.stops.clone();
        let lsp = Lsp::new(Box::new(crashing));
        let spec = spec_for("main.py").expect("spec");
        let first = lsp.client(&root, spec, &login()).await.expect("starts");
        let wait = async {
            while first.alive() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .expect("it exits");
        // The next request finds it dead: it's stopped, counted, replaced.
        let second = lsp.client(&root, spec, &login()).await.expect("again");
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(first.ended().as_deref(), Some("it exited"));
        assert_eq!(stops.load(Ordering::SeqCst), 1, "the dead one was stopped");
        assert_eq!(lsp.starts.load(Ordering::SeqCst), 2);
        assert_eq!(
            lock(&lsp.failures).values().next().map(|f| f.0.len()),
            Some(1)
        );
        // A folder being removed gets no new server until the removal ends.
        let closing = lsp.begin_removal(&root).await;
        assert_eq!(stops.load(Ordering::SeqCst), 2, "the running one stopped");
        let why = lsp
            .client(&root, spec, &login())
            .await
            .err()
            .expect("closing");
        assert_eq!(why.state, LspState::None);
        drop(closing);
        lsp.client(&root, spec, &login())
            .await
            .expect("after the removal, it may start");
    }

    /// Servers that exit as soon as they start; stops are counted.
    #[derive(Default)]
    struct CrashingSpawner {
        stops: Arc<AtomicU64>,
    }

    impl Spawner for CrashingSpawner {
        fn available(&self, _: &ServerSpec, _: &LoginEnv) -> bool {
            true
        }
        fn spawn(&self, _: &ServerSpec, _: &Path, _: &LoginEnv) -> Result<Spawned, String> {
            let stops = self.stops.clone();
            let (client_side, server_side) = tokio::io::duplex(1 << 16);
            // Answers initialize, then exits.
            tokio::spawn(async move {
                let (sr, mut sw) = tokio::io::split(server_side);
                let mut r = BufReader::new(sr);
                if let Frame::Message(m) = read_frame(&mut r).await {
                    let reply = json!({ "jsonrpc": "2.0", "id": m["id"], "result": { "capabilities": {} } });
                    let _ = sw.write_all(&frame(&reply)).await;
                }
            });
            let (cr, cw) = tokio::io::split(client_side);
            Ok(Spawned {
                reader: Box::new(cr),
                writer: Box::new(cw),
                stderr: None,
                child: None,
                tree: None,
                on_stop: Some(Box::new(move || {
                    stops.fetch_add(1, Ordering::SeqCst);
                })),
            })
        }
    }

    /// Servers that hold `initialize` until released; stops are counted.
    #[derive(Default)]
    struct ParkedSpawner {
        release: Arc<Notify>,
        stops: Arc<AtomicU64>,
    }

    impl Spawner for ParkedSpawner {
        fn available(&self, _: &ServerSpec, _: &LoginEnv) -> bool {
            true
        }
        fn spawn(&self, _: &ServerSpec, _: &Path, _: &LoginEnv) -> Result<Spawned, String> {
            let (client_side, server_side) = tokio::io::duplex(1 << 16);
            let release = self.release.clone();
            tokio::spawn(async move {
                let (sr, mut sw) = tokio::io::split(server_side);
                let mut r = BufReader::new(sr);
                if let Frame::Message(m) = read_frame(&mut r).await {
                    release.notified().await;
                    let reply = json!({ "jsonrpc": "2.0", "id": m["id"], "result": { "capabilities": {} } });
                    let _ = sw.write_all(&frame(&reply)).await;
                }
                // Then it answers nothing, and stays up.
                while !matches!(read_frame(&mut r).await, Frame::End) {}
            });
            let (cr, cw) = tokio::io::split(client_side);
            let stops = self.stops.clone();
            Ok(Spawned {
                reader: Box::new(cr),
                writer: Box::new(cw),
                stderr: None,
                child: None,
                tree: None,
                on_stop: Some(Box::new(move || {
                    stops.fetch_add(1, Ordering::SeqCst);
                })),
            })
        }
    }

    /// Starts a server in `root` that parks in `initialize`, and returns
    /// once it's parked.
    async fn parked_start(
        lsp: &Arc<Lsp>,
        root: &Path,
    ) -> tokio::task::JoinHandle<Result<Arc<Client>, NoServer>> {
        let (l, r) = (lsp.clone(), root.to_path_buf());
        let task = tokio::spawn(async move {
            l.client(&r, spec_for("main.py").expect("spec"), &login())
                .await
        });
        while lsp.starts.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        task
    }

    #[tokio::test]
    async fn f_cd_23_a_server_starting_while_its_folder_is_removed_isnt_kept() {
        let (_t, root) = folder();
        let parked = ParkedSpawner::default();
        let (release, stops) = (parked.release.clone(), parked.stops.clone());
        let lsp = Arc::new(Lsp::new(Box::new(parked)));
        let task = parked_start(&lsp, &root).await;
        // The removal starts (a hover began during the teardown) ...
        let closing = lsp.begin_removal(&root).await;
        release.notify_one();
        let why = task.await.expect("join").err().expect("not kept");
        assert_eq!(why.state, LspState::None);
        assert_eq!(lsp.count().await, 0);
        assert_eq!(stops.load(Ordering::SeqCst), 1, "it was stopped");
        // ... and nothing starts until the removal has finished, however long.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(lsp
            .client(&root, spec_for("main.py").expect("spec"), &login())
            .await
            .is_err());
        drop(closing);
        assert!(!lsp.closing(&root));
    }

    #[tokio::test]
    async fn f_cd_23_a_server_starting_across_a_shutdown_or_a_forget_isnt_kept() {
        let (_t, root) = folder();
        for how in ["shutdown", "forget"] {
            let parked = ParkedSpawner::default();
            let (release, stops) = (parked.release.clone(), parked.stops.clone());
            let lsp = Arc::new(Lsp::new(Box::new(parked)));
            let task = parked_start(&lsp, &root).await;
            // The sandbox setting changed (shutdown), or the workspace was
            // removed (forget), while it was starting.
            if how == "shutdown" {
                lsp.shutdown().await;
            } else {
                lsp.forget(&root).await;
            }
            release.notify_one();
            assert!(task.await.expect("join").is_err(), "{how}");
            assert_eq!(lsp.count().await, 0, "{how}");
            assert_eq!(stops.load(Ordering::SeqCst), 1, "{how}");
        }
    }

    #[tokio::test]
    async fn f_cd_23_a_request_let_through_before_a_sandbox_switch_starts_nothing_after_it() {
        let (_t, root) = folder();
        let parked = ParkedSpawner::default();
        let (release, stops) = (parked.release.clone(), parked.stops.clone());
        let lsp = Arc::new(Lsp::new(Box::new(parked)));
        // The first start parks in initialize, holding the folder's gate.
        let first = parked_start(&lsp, &root).await;
        // A hover's trust is decided now, under the old setting; it waits
        // on the gate.
        let epoch = lsp.epoch();
        let (l, r) = (lsp.clone(), root.clone());
        let second = tokio::spawn(async move {
            with_epoch(
                epoch,
                l.client(&r, spec_for("main.py").expect("spec"), &login()),
            )
            .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        // The sandbox is turned on: every server stops.
        lsp.shutdown().await;
        release.notify_one();
        let first = first.await.expect("join").err().expect("first not kept");
        assert_eq!(first.message, RESTARTED);
        let second = tokio::time::timeout(Duration::from_secs(5), second)
            .await
            .expect("it doesn't start a server")
            .expect("join")
            .err()
            .expect("second not kept");
        assert_eq!(second.message, RESTARTED);
        assert_eq!(lsp.count().await, 0);
        assert_eq!(
            lsp.starts.load(Ordering::SeqCst),
            1,
            "the second never spawned"
        );
        assert_eq!(stops.load(Ordering::SeqCst), 1);
        // A request decided after the switch starts one as usual.
        release.notify_one();
        assert!(lsp
            .client(&root, spec_for("main.py").expect("spec"), &login())
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn f_cd_23_forgetting_a_covering_folder_stops_a_start_inside_it() {
        let (_t, root) = folder();
        let inner = root.join("sub");
        std::fs::create_dir_all(&inner).expect("mkdir");
        let parked = ParkedSpawner::default();
        let (release, stops) = (parked.release.clone(), parked.stops.clone());
        let lsp = Arc::new(Lsp::new(Box::new(parked)));
        let task = parked_start(&lsp, &inner).await;
        lsp.forget(&root).await;
        release.notify_one();
        assert!(task.await.expect("join").is_err());
        assert_eq!(lsp.count().await, 0);
        assert_eq!(stops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn f_cd_23_no_server_starts_in_a_worktree_while_its_discard_runs() {
        let (_t, root) = folder();
        let closing = forget_worktree(&root).await;
        assert!(global().closing(&root));
        drop(closing);
        assert!(!global().closing(&root));
    }

    /// Servers whose availability probe takes a while (it's blocking);
    /// spawns are counted.
    #[derive(Default)]
    struct SlowProbe {
        probing: Arc<AtomicBool>,
        spawns: Arc<AtomicU64>,
    }

    impl Spawner for SlowProbe {
        fn available(&self, _: &ServerSpec, _: &LoginEnv) -> bool {
            self.probing.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(300));
            true
        }
        fn spawn(&self, s: &ServerSpec, r: &Path, l: &LoginEnv) -> Result<Spawned, String> {
            self.spawns.fetch_add(1, Ordering::SeqCst);
            FakeSpawner { installed: true }.spawn(s, r, l)
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn f_cd_23_a_sandbox_switch_during_the_probe_spawns_nothing() {
        let (_t, root) = folder();
        let slow = SlowProbe::default();
        let (probing, spawns) = (slow.probing.clone(), slow.spawns.clone());
        let lsp = Arc::new(Lsp::new(Box::new(slow)));
        let (l, r) = (lsp.clone(), root.clone());
        let task = tokio::spawn(async move {
            l.client(&r, spec_for("main.py").expect("spec"), &login())
                .await
        });
        while !probing.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        lsp.shutdown().await;
        let why = task.await.expect("join").err().expect("not started");
        assert_eq!(why.message, RESTARTED);
        assert_eq!(spawns.load(Ordering::SeqCst), 0, "nothing was spawned");
    }

    #[tokio::test]
    async fn f_cd_23_forgets_a_start_can_no_longer_see_fail_closed() {
        let (_t, root) = folder();
        let lsp = Lsp::new(Box::new(FakeSpawner { installed: true }));
        let epoch = lsp.epoch();
        // More forgets of other folders than the list keeps.
        for i in 0..(FORGOTTEN_KEPT + 10) {
            lsp.forget(&root.join(format!("elsewhere-{i}"))).await;
        }
        assert!(lsp.superseded(&root, epoch).is_some(), "evicted: unknown");
        assert!(
            lsp.superseded(&root, lsp.epoch()).is_none(),
            "a fresh epoch is fine"
        );
    }

    #[tokio::test]
    async fn f_cd_23_a_start_in_flight_at_a_new_worktrees_path_isnt_kept() {
        let (_t, root) = folder();
        let tmp = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(tmp.path().to_path_buf()).expect("data");
        trust(&data, &root).expect("leftover trust");
        let parked = ParkedSpawner::default();
        let (release, stops) = (parked.release.clone(), parked.stops.clone());
        let lsp = Arc::new(Lsp::new(Box::new(parked)));
        // A request let through on the leftover trust, starting meanwhile.
        let task = parked_start(&lsp, &root).await;
        untrust_new_worktree_in(&lsp, Some(&data), &root).await;
        release.notify_one();
        assert!(task.await.expect("join").is_err());
        assert_eq!(lsp.count().await, 0);
        assert_eq!(stops.load(Ordering::SeqCst), 1);
        assert!(!is_trusted(&data, &root));
    }

    #[test]
    fn f_cd_23_before_a_worktree_is_made_trust_at_missing_paths_goes() {
        let tmp = tempfile::tempdir().expect("tmp");
        let base = std::fs::canonicalize(tmp.path()).expect("canonical");
        let data = DataDir::at(base.join("data")).expect("data");
        let dir = base.join("repo/.tandem/worktrees");
        let (here, gone) = (dir.join("here"), dir.join("gone"));
        std::fs::create_dir_all(&here).expect("mkdir");
        std::fs::create_dir_all(&gone).expect("mkdir");
        trust(&data, &here).expect("trust");
        trust(&data, &gone).expect("trust");
        trust(&data, &base.join("repo")).expect("trust");
        std::fs::remove_dir_all(&gone).expect("deleted outside the app");
        untrust_missing_in(&data, &dir);
        assert!(
            is_trusted(&data, &here),
            "an existing worktree keeps its own"
        );
        assert!(!is_trusted(&data, &gone));
        assert!(
            is_trusted(&data, &base.join("repo")),
            "the repository keeps its own"
        );
    }

    #[test]
    fn f_cd_23_windows_paths_as_uris_and_back() {
        assert_eq!(windows_uri_body(r"C:\src\a.rs"), "/C:/src/a.rs");
        assert_eq!(windows_uri_body(r"\\?\C:\src\a.rs"), "/C:/src/a.rs");
        assert_eq!(windows_uri_body(r"\\host\share\a.rs"), "host/share/a.rs");
        assert_eq!(
            windows_uri_body(r"\\?\UNC\host\share\a.rs"),
            "host/share/a.rs"
        );
        assert_eq!(windows_path_of("/C:/src/a.rs"), "C:/src/a.rs");
        assert_eq!(windows_path_of("host/share/a.rs"), r"\\host\share\a.rs");
        // One key for both spellings of a folder.
        #[cfg(windows)]
        assert_eq!(norm(Path::new(r"\\?\C:\w")), norm(Path::new(r"C:\w")));
    }

    #[tokio::test]
    async fn f_cd_23_a_crash_loop_reaches_the_retry_limit_through_the_reaper() {
        let (_t, root) = folder();
        let lsp = Lsp::new(Box::new(CrashingSpawner::default()));
        let spec = spec_for("main.py").expect("spec");
        for _ in 0..MAX_FAILURES {
            let client = lsp.client(&root, spec, &login()).await.expect("starts");
            let wait = async {
                while client.alive() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            };
            tokio::time::timeout(Duration::from_secs(5), wait)
                .await
                .expect("it exits");
            // The reaper finds it dead, and counts that.
            assert_eq!(lsp.reap(IDLE).await, 1);
        }
        let why = lsp
            .client(&root, spec, &login())
            .await
            .err()
            .expect("not retried");
        assert_eq!(why.state, LspState::Failed);
        assert!(
            why.message.starts_with("pyright stopped:"),
            "{}",
            why.message
        );
    }

    #[cfg(unix)]
    #[test]
    fn f_cd_23_rustups_proxy_without_the_component_isnt_a_server() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tmp");
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).expect("mkdir");
        let ra = bin.join("rust-analyzer");
        let write = |code: i32| {
            std::fs::write(&ra, format!("#!/bin/sh\nexit {code}\n")).expect("write");
            std::fs::set_permissions(&ra, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        };
        let mut login = login();
        login
            .vars
            .insert("PATH".into(), format!("{}:/usr/bin:/bin", bin.display()));
        let spec = spec_for("a.rs").expect("spec");
        // rustup's proxy: present, but `--version` fails.
        write(1);
        assert!(!LoginSpawner::default().available(spec, &login));
        write(0);
        assert!(LoginSpawner::default().available(spec, &login));
        // Servers without a probe count when they're on the PATH.
        assert!(!LoginSpawner::default().available(spec_for("a.go").expect("go"), &login));
    }

    /// Serves `tandem_engines::fake_lsp` in-process for every spec.
    pub(crate) struct FakeSpawner {
        pub(crate) installed: bool,
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
                stderr: None,
                child: None,
                tree: None,
                on_stop: None,
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
