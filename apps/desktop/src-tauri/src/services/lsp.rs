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
        headers += 1;
        if !line.ends_with(b"\n") || headers > MAX_HEADERS {
            return Frame::End;
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end();
        if text.is_empty() {
            break;
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

    async fn initialize(&self, root: &Path) -> Result<(), ClientError> {
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
            let child = lock(&self.child).take();
            if let Some(mut child) = child {
                let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
                let _ = child.start_kill();
            }
        };
        let _ = tokio::time::timeout(STOP_LIMIT, polite).await;
        if let Some(id) = self.tree {
            kill_tree(id).await;
        }
        if let Some(mut child) = lock(&self.child).take() {
            let _ = child.start_kill();
        }
        self.end("stopped".into());
    }
}

/// `file://` and the path, percent-encoded.
pub fn file_uri(p: &Path) -> String {
    #[cfg(windows)]
    let s = {
        // `C:\x` is `file:///C:/x`.
        let s = p.to_string_lossy().replace('\\', "/");
        if s.starts_with('/') {
            s
        } else {
            format!("/{s}")
        }
    };
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
    #[cfg(windows)]
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
    /// Its stderr, whose end says why it stopped.
    pub stderr: Option<Reader>,
    pub child: Option<tokio::process::Child>,
    /// Its process group (Unix) or process (Windows), killed with it.
    pub tree: Option<u32>,
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
        let client = Client::start(spawned);
        if let Err(why) = client.initialize(root).await {
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

    /// Every server stops (the app quits).
    pub async fn shutdown(&self) {
        let all: Vec<Arc<Running>> = self.running.lock().await.drain().map(|(_, r)| r).collect();
        for r in all {
            r.client.stop().await;
        }
    }

    /// The editor closed `rel`: its server reads the file from disk again.
    pub async fn close(&self, root: &Path, rel: &str) {
        let (Ok(full), Some(spec)) = (inside(root, rel), spec_for(rel)) else {
            return;
        };
        let key: Key = (root.to_path_buf(), spec.program);
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
        let clients: Vec<Arc<Running>> = self
            .running
            .lock()
            .await
            .iter()
            .filter(|((r, _), _)| r == root)
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

/// What `lsp_sync` says for an installed server that waits for trust.
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
            let (rel, in_folder) = match path.strip_prefix(root) {
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
mod tests {
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

    /// Servers that exit as soon as they start.
    struct CrashingSpawner;

    impl Spawner for CrashingSpawner {
        fn available(&self, _: &ServerSpec, _: &LoginEnv) -> bool {
            true
        }
        fn spawn(&self, _: &ServerSpec, _: &Path, _: &LoginEnv) -> Result<Spawned, String> {
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
            })
        }
    }

    #[tokio::test]
    async fn f_cd_23_a_crash_loop_reaches_the_retry_limit_through_the_reaper() {
        let (_t, root) = folder();
        let lsp = Lsp::new(Box::new(CrashingSpawner));
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
                stderr: None,
                child: None,
                tree: None,
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
