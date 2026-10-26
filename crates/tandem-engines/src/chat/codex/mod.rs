//! The Codex adapter (F-TH-05, ARCHITECTURE §6.8.2): `codex app-server`,
//! JSON-RPC over stdio. One app-server per thread session, in the account's
//! `CODEX_HOME`; the gateway joins as an MCP server through `-c` overrides,
//! its lease in an env var. Codex asks for approvals in-band; the threads
//! service answers them.

pub mod protocol;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use tandem_core::allow_rules::ApprovalSubject;
use tandem_core::approvals::{ApprovalDecision, ApprovalKind};
use tandem_core::ids::{ApprovalId, TurnId};
use tandem_core::limits;
use tokio::sync::{mpsc, oneshot};

use self::protocol::{CodexProtocol, V2};
use super::process::{ChatProcess, ProcessLine, ProcessSpec};
use super::{
    AdapterError, AdapterEvent, ChatAdapter, ChatSession, ChatStart, InterruptOutcome, TurnInput,
};
use crate::events::{
    truncate_output, ErrorAction, ErrorStage, LimitKind, ThreadEvent, ToolStatus, ToolVerb,
    TurnEndReason,
};

/// The env var the gateway lease is read from (`bearer_token_env_var`).
pub const LEASE_ENV: &str = "TANDEM_GATEWAY_LEASE";
/// How long Codex gets to answer `initialize` and `thread/start`.
const HANDSHAKE: Duration = Duration::from_secs(20);
/// Longest command shown on a tool row; the rest is in the detail.
const COMMAND_SHOWN: usize = 120;

/// `npm run build && … && npm test`: the middle of a long line goes, both
/// ends stay (F-TH-05).
pub fn middle(s: &str, max: usize) -> String {
    let line: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= max {
        return line;
    }
    let keep = (max.saturating_sub(3)) / 2;
    let head: String = chars[..keep].iter().collect();
    let tail: String = chars[chars.len() - keep..].iter().collect();
    // Whole words at both ends when there are any.
    let head = head
        .rsplit_once(' ')
        .map_or(head.as_str(), |(h, _)| h)
        .trim_end()
        .to_string();
    let tail = tail
        .split_once(' ')
        .map_or(tail.as_str(), |(_, t)| t)
        .trim_start()
        .to_string();
    format!("{head} … {tail}")
}

/// A command as text (Codex sends a string, or argv).
fn command_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// What a server message means to the session.
#[derive(Debug, Clone, PartialEq)]
pub enum Codex {
    Event(ThreadEvent),
    /// An approval request (a JSON-RPC request from the server).
    Approval {
        rpc_id: Value,
        kind: ApprovalKind,
        summary: String,
        detail: String,
        subject: ApprovalSubject,
    },
    /// Codex's own id for the running turn (to interrupt it).
    TurnStarted(String),
}

/// Codex's notifications and requests as normalized events.
pub struct CodexParser {
    limits: Vec<Regex>,
    cwd: PathBuf,
    /// Item id → its command or paths, for approval cards and results.
    items: HashMap<String, (ToolVerb, String, Option<String>)>,
}

impl CodexParser {
    pub fn new(cwd: PathBuf, limit_patterns: &[String]) -> Self {
        Self {
            limits: limits::compile(limit_patterns),
            cwd,
            items: HashMap::new(),
        }
    }

    /// One message from the server (not a response to our requests).
    pub fn message(
        &mut self,
        msg: &Value,
        now: chrono::DateTime<chrono::FixedOffset>,
    ) -> Vec<Codex> {
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            return Vec::new();
        };
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        match msg.get("id") {
            Some(id) => self.request(id.clone(), method, &params),
            None => self.notification(method, &params, now),
        }
    }

    fn relative(&self, path: &str) -> String {
        std::path::Path::new(path)
            .strip_prefix(&self.cwd)
            .map(|r| r.display().to_string())
            .unwrap_or_else(|_| path.to_string())
    }

    /// A file change's files, absolute (F-TK-07).
    fn edited(&self, changes: &Value) -> Option<Vec<String>> {
        let paths: Vec<String> = changes
            .as_array()?
            .iter()
            .filter_map(|ch| ch.get("path").and_then(Value::as_str))
            .filter(|p| !p.is_empty())
            .map(|p| crate::events::absolute(&self.cwd, p))
            .collect();
        (!paths.is_empty()).then_some(paths)
    }

    fn paths(&self, changes: &Value) -> String {
        changes
            .as_array()
            .map(|c| {
                c.iter()
                    .filter_map(|ch| ch.get("path").and_then(Value::as_str))
                    .map(|p| self.relative(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    }

    /// Each file's diff under its own `---`/`+++` header.
    fn diffs(&self, changes: &[Value]) -> String {
        changes
            .iter()
            .filter_map(|ch| {
                let diff = ch
                    .get("diff")
                    .and_then(Value::as_str)
                    .filter(|d| !d.is_empty())?;
                let path = self.relative(ch.get("path").and_then(Value::as_str).unwrap_or(""));
                Some(if diff.starts_with("---") {
                    diff.to_string()
                } else {
                    format!("--- a/{path}\n+++ b/{path}\n{diff}")
                })
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn started(&mut self, item: &Value) -> Option<ThreadEvent> {
        let id = item.get("id").and_then(Value::as_str)?.to_string();
        let (verb, summary, detail) = match item.get("type").and_then(Value::as_str)? {
            "commandExecution" => {
                let full = command_text(&item["command"]);
                (ToolVerb::Bash, middle(&full, COMMAND_SHOWN), Some(full))
            }
            "fileChange" => (ToolVerb::Edit, self.paths(&item["changes"]), None),
            "mcpToolCall" => {
                let server = item.get("server").and_then(Value::as_str).unwrap_or("");
                let tool = item.get("tool").and_then(Value::as_str).unwrap_or("");
                let args = item
                    .get("arguments")
                    .and_then(|a| serde_json::to_string_pretty(a).ok());
                (ToolVerb::Mcp, format!("{server} › {tool}"), args)
            }
            "webSearch" => (
                ToolVerb::Websearch,
                item.get("query")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                None,
            ),
            _ => return None,
        };
        self.items
            .insert(id.clone(), (verb, summary.clone(), detail.clone()));
        let edited = match item.get("type").and_then(Value::as_str) {
            Some("fileChange") => self.edited(&item["changes"]),
            _ => None,
        };
        let mut ev = ThreadEvent::tool(&id, verb, &summary, ToolStatus::Running);
        if let ThreadEvent::Tool {
            detail: d,
            paths: p,
            ..
        } = &mut ev
        {
            *d = detail;
            *p = edited;
        }
        Some(ev)
    }

    fn completed(&mut self, item: &Value) -> Option<ThreadEvent> {
        let id = item.get("id").and_then(Value::as_str)?.to_string();
        let kind = item.get("type").and_then(Value::as_str)?;
        match kind {
            "agentMessage" => Some(ThreadEvent::Assistant {
                markdown: item.get("text").and_then(Value::as_str)?.to_string(),
            }),
            "reasoning" => {
                let parts: Vec<String> = item
                    .get("summary")
                    .and_then(Value::as_array)
                    .map(|s| {
                        s.iter()
                            .filter_map(|p| {
                                p.as_str().or_else(|| p.get("text").and_then(Value::as_str))
                            })
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                if parts.is_empty() {
                    return None;
                }
                Some(ThreadEvent::Reasoning {
                    text: parts.join("\n\n"),
                    duration_ms: None,
                })
            }
            "commandExecution" | "fileChange" | "mcpToolCall" | "webSearch" => {
                if !self.items.contains_key(&id) {
                    self.started(item);
                }
                let (verb, summary, detail) = self.items.get(&id).cloned()?;
                let status = item
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("completed");
                let exit = item.get("exitCode").and_then(Value::as_i64);
                let status = match status {
                    "declined" => ToolStatus::Denied,
                    "failed" => ToolStatus::Error,
                    _ if exit.is_some_and(|c| c != 0) => ToolStatus::Error,
                    _ if item.get("error").is_some_and(|e| !e.is_null()) => ToolStatus::Error,
                    _ => ToolStatus::Ok,
                };
                let text = match kind {
                    "commandExecution" => item
                        .get("aggregatedOutput")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    "mcpToolCall" => item
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| item.get("result").map(|r| r.to_string())),
                    _ => None,
                };
                let (output, truncated) = match text {
                    Some(t) => {
                        let (o, cut) = truncate_output(&t);
                        (Some(o), cut)
                    }
                    None => (None, false),
                };
                let diff = (kind == "fileChange")
                    .then(|| item["changes"].as_array().map(|c| self.diffs(c)))
                    .flatten()
                    .filter(|d| !d.is_empty());
                let paths = (kind == "fileChange")
                    .then(|| self.edited(&item["changes"]))
                    .flatten();
                Some(ThreadEvent::Tool {
                    id,
                    verb,
                    arg_summary: summary,
                    status,
                    output,
                    output_truncated: truncated,
                    output_file: None,
                    diff,
                    detail,
                    paths,
                })
            }
            _ => None,
        }
    }

    fn turn_completed(
        &self,
        turn: &Value,
        now: chrono::DateTime<chrono::FixedOffset>,
    ) -> Vec<Codex> {
        let end = |reason| {
            Codex::Event(ThreadEvent::TurnEnd {
                reason,
                blocked_actions: 0,
            })
        };
        match turn
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("completed")
        {
            "interrupted" => vec![end(TurnEndReason::Cancelled)],
            "failed" => {
                let err = &turn["error"];
                let message = err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Codex stopped with an error.")
                    .to_string();
                let info = err
                    .get("codexErrorInfo")
                    .map(|i| match i {
                        Value::String(s) => s.clone(),
                        Value::Object(o) => o.keys().next().cloned().unwrap_or_default(),
                        _ => String::new(),
                    })
                    .unwrap_or_default();
                let hit = limits::detect(&message, &self.limits, now);
                if info == "usageLimitExceeded" || hit.is_some() {
                    let lower = message.to_lowercase();
                    let kind = if lower.contains("week") {
                        LimitKind::Weekly
                    } else if info == "usageLimitExceeded" || lower.contains("usage limit") {
                        LimitKind::Session
                    } else {
                        LimitKind::Rate
                    };
                    return vec![
                        Codex::Event(ThreadEvent::Limit {
                            kind,
                            resets_at: hit.and_then(|h| h.resets_at),
                            message,
                        }),
                        end(TurnEndReason::Limit),
                    ];
                }
                let (stage, action) = match info.as_str() {
                    "unauthorized" => (ErrorStage::Auth, ErrorAction::SignIn),
                    "contextWindowExceeded" => (ErrorStage::Model, ErrorAction::SummarizeContinue),
                    "httpConnectionFailed"
                    | "responseStreamConnectionFailed"
                    | "responseStreamDisconnected" => (ErrorStage::Network, ErrorAction::Retry),
                    _ => (ErrorStage::Unknown, ErrorAction::Retry),
                };
                vec![
                    Codex::Event(ThreadEvent::Error {
                        code: if info.is_empty() {
                            "turn_failed".into()
                        } else {
                            info
                        },
                        message,
                        stage,
                        hint: None,
                        action: Some(action),
                    }),
                    end(TurnEndReason::Error),
                ]
            }
            _ => vec![end(TurnEndReason::Completed)],
        }
    }

    fn notification(
        &mut self,
        method: &str,
        p: &Value,
        now: chrono::DateTime<chrono::FixedOffset>,
    ) -> Vec<Codex> {
        let text = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
        match method {
            "turn/started" => p
                .pointer("/turn/id")
                .and_then(Value::as_str)
                .map(|id| vec![Codex::TurnStarted(id.into())])
                .unwrap_or_default(),
            "item/agentMessage/delta" => text("delta")
                .map(|t| vec![Codex::Event(ThreadEvent::AssistantDelta { text: t })])
                .unwrap_or_default(),
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => text("delta")
                .map(|t| vec![Codex::Event(ThreadEvent::ReasoningDelta { text: t })])
                .unwrap_or_default(),
            "item/started" => self
                .started(&p["item"])
                .map(Codex::Event)
                .into_iter()
                .collect(),
            "item/completed" => self
                .completed(&p["item"])
                .map(Codex::Event)
                .into_iter()
                .collect(),
            "thread/tokenUsage/updated" => {
                let usage = &p["tokenUsage"];
                let last = usage.get("last").unwrap_or(&usage["total"]);
                let n = |k: &str| last.get(k).and_then(Value::as_u64).unwrap_or(0);
                let window = usage
                    .get("modelContextWindow")
                    .and_then(Value::as_u64)
                    .filter(|w| *w > 0);
                vec![Codex::Event(ThreadEvent::Usage {
                    input: n("inputTokens"),
                    output: n("outputTokens"),
                    cache_read: n("cachedInputTokens"),
                    cache_write: 0,
                    context_fill_pct: window.map(|w| {
                        ((n("inputTokens") as f32 / w as f32 * 1000.0).round() / 10.0).min(100.0)
                    }),
                    cost_usd: None,
                })]
            }
            "turn/completed" => self.turn_completed(&p["turn"], now),
            // A retried error is news; a final one arrives with turn/completed.
            "error" if p.get("willRetry").and_then(Value::as_bool) == Some(true) => p
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(|m| {
                    vec![Codex::Event(ThreadEvent::Note {
                        text: format!("Retrying: {m}"),
                    })]
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn request(&mut self, id: Value, method: &str, p: &Value) -> Vec<Codex> {
        let item = p.get("itemId").and_then(Value::as_str).unwrap_or("");
        let reason = p.get("reason").and_then(Value::as_str);
        let known = self.items.get(item).cloned();
        let (kind, summary, mut detail, subject) = match method {
            "item/commandExecution/requestApproval" => {
                let full = p
                    .get("command")
                    .map(command_text)
                    .filter(|c| !c.is_empty())
                    .or_else(|| known.as_ref().and_then(|k| k.2.clone()))
                    .unwrap_or_default();
                (
                    ApprovalKind::Command,
                    format!("Run `{}`", middle(&full, COMMAND_SHOWN)),
                    full.clone(),
                    ApprovalSubject::Command(full),
                )
            }
            "item/fileChange/requestApproval" => {
                let what = known
                    .map(|k| k.1)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "files".into());
                (
                    ApprovalKind::FileWrite,
                    format!("Edit `{what}`"),
                    String::new(),
                    ApprovalSubject::Tool("file_change".into()),
                )
            }
            _ => return Vec::new(),
        };
        if let Some(r) = reason {
            if !detail.is_empty() {
                detail.push_str("\n\n");
            }
            detail.push_str(r);
        }
        vec![Codex::Approval {
            rpc_id: id,
            kind,
            summary,
            detail: tandem_core::redact::redact(&detail),
            subject,
        }]
    }
}

/// The `app-server` arguments, with the gateway as an MCP server.
pub fn args(req: &ChatStart) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec!["app-server".into()];
    if let Some(mcp) = &req.mcp {
        a.push("-c".into());
        a.push(format!("mcp_servers.tandem.url={}", toml_string(&mcp.url)).into());
        a.push("-c".into());
        a.push(
            format!(
                "mcp_servers.tandem.bearer_token_env_var={}",
                toml_string(LEASE_ENV)
            )
            .into(),
        );
    }
    for server in &req.stdio_servers {
        let key = format!("mcp_servers.{}", server.name);
        let list = |items: &[String]| {
            format!(
                "[{}]",
                items
                    .iter()
                    .map(|i| toml_string(i))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        a.push("-c".into());
        a.push(
            format!(
                "{key}.command={}",
                toml_string(&server.command.to_string_lossy())
            )
            .into(),
        );
        a.push("-c".into());
        a.push(format!("{key}.args={}", list(&server.args)).into());
        if !server.env_vars.is_empty() {
            a.push("-c".into());
            a.push(format!("{key}.env_vars={}", list(&server.env_vars)).into());
        }
    }
    a
}

fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

pub struct CodexAdapter {
    limit_patterns: Vec<String>,
}

impl CodexAdapter {
    pub fn new(limit_patterns: Vec<String>) -> Self {
        Self { limit_patterns }
    }
}

type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, String>>>>>;

#[derive(Clone)]
struct CodexSession {
    process: Arc<ChatProcess>,
    protocol: Arc<dyn CodexProtocol>,
    req: Arc<ChatStart>,
    next_id: Arc<AtomicI64>,
    pending: Pending,
    thread_id: Arc<Mutex<Option<String>>>,
    /// Codex's id for the running turn, and ours.
    codex_turn: Arc<Mutex<Option<String>>>,
    turn: Arc<Mutex<Option<TurnId>>>,
    /// Our approval id → the server request it answers.
    approvals: Arc<Mutex<HashMap<String, Value>>>,
}

#[async_trait]
impl ChatAdapter for CodexAdapter {
    fn engine_id(&self) -> &str {
        "codex"
    }

    async fn start(
        &self,
        req: ChatStart,
        events: mpsc::Sender<AdapterEvent>,
    ) -> Result<Box<dyn ChatSession>, AdapterError> {
        if let Some(v) = &req.engine_version {
            protocol::check_version(Some(v)).map_err(AdapterError::Unsupported)?;
        }
        let mut env: Vec<(String, String)> = req
            .env
            .iter()
            .filter(|(k, _)| !k.starts_with("TANDEM_STATUS"))
            .cloned()
            .collect();
        if let Some(mcp) = &req.mcp {
            env.push((LEASE_ENV.into(), mcp.token.clone()));
        }
        let spec = ProcessSpec::new(req.program.clone(), args(&req), env, req.cwd.clone());
        let spec = super::Relocate::apply(req.relocate.as_ref(), spec);
        let (process, lines) = ChatProcess::spawn(spec)?;
        let session = CodexSession {
            process: Arc::new(process),
            protocol: Arc::new(V2),
            req: Arc::new(req.clone()),
            next_id: Arc::new(AtomicI64::new(1)),
            pending: Arc::default(),
            thread_id: Arc::new(Mutex::new(None)),
            codex_turn: Arc::new(Mutex::new(None)),
            turn: Arc::new(Mutex::new(None)),
            approvals: Arc::default(),
        };
        let parser = CodexParser::new(req.cwd.clone(), &self.limit_patterns);
        tokio::spawn(session.clone().read(parser, lines, events.clone()));
        session.handshake(&events).await?;
        Ok(Box::new(session))
    }
}

impl CodexSession {
    async fn send(&self, msg: Value) -> Result<(), AdapterError> {
        self.process.write_line(&msg.to_string()).await
    }

    async fn request(
        &self,
        method: &str,
        params: Value,
        limit: Duration,
    ) -> Result<Value, AdapterError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(id, tx);
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        match tokio::time::timeout(limit, rx).await {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(e))) => Err(AdapterError::Protocol(e)),
            Ok(Err(_)) => Err(AdapterError::Closed),
            Err(_) => {
                lock(&self.pending).remove(&id);
                Err(AdapterError::Protocol(format!(
                    "Codex didn't answer {method}"
                )))
            }
        }
    }

    /// `initialize` (version-checked), then a new or resumed thread.
    async fn handshake(&self, events: &mpsc::Sender<AdapterEvent>) -> Result<(), AdapterError> {
        let init = self
            .request(
                "initialize",
                json!({"clientInfo": {"name": "vibezeo", "title": "Vibezeo", "version": env!("CARGO_PKG_VERSION")}}),
                HANDSHAKE,
            )
            .await?;
        if self.req.engine_version.is_none() {
            let agent = init.get("userAgent").and_then(Value::as_str);
            protocol::check_version(agent).map_err(AdapterError::Unsupported)?;
        }
        self.send(json!({"jsonrpc": "2.0", "method": "initialized"}))
            .await?;
        let (method, params) = match &self.req.resume_session {
            Some(id) => self.protocol.resume_thread(id, &self.req),
            None => self.protocol.start_thread(&self.req),
        };
        let started = self.request(method, params, HANDSHAKE).await?;
        let id = started
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| AdapterError::Protocol("Codex started no thread".into()))?
            .to_string();
        *lock(&self.thread_id) = Some(id.clone());
        let _ = events.send(AdapterEvent::SessionId(id)).await;
        Ok(())
    }

    async fn read(
        self,
        mut parser: CodexParser,
        mut lines: mpsc::Receiver<ProcessLine>,
        events: mpsc::Sender<AdapterEvent>,
    ) {
        while let Some(line) = lines.recv().await {
            let msg = match line {
                ProcessLine::Line(l) => match serde_json::from_str::<Value>(&l) {
                    Ok(v) => v,
                    Err(_) => continue,
                },
                ProcessLine::TooLong => continue,
            };
            // A response to one of our requests.
            if msg.get("method").is_none() {
                if let Some(id) = msg.get("id").and_then(Value::as_i64) {
                    if let Some(tx) = lock(&self.pending).remove(&id) {
                        let answer = match msg.get("error") {
                            Some(e) => Err(e
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("error")
                                .to_string()),
                            None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        let _ = tx.send(answer);
                    }
                }
                continue;
            }
            for out in parser.message(&msg, limits::local(chrono::Utc::now())) {
                let turn = lock(&self.turn).clone();
                match out {
                    Codex::TurnStarted(id) => *lock(&self.codex_turn) = Some(id),
                    Codex::Event(event) => {
                        if let Some(turn_id) = turn {
                            let _ = events.send(AdapterEvent::Event { turn_id, event }).await;
                        }
                    }
                    Codex::Approval {
                        rpc_id,
                        kind,
                        summary,
                        detail,
                        subject,
                    } => {
                        let Some(turn_id) = turn else {
                            // Nothing to show it in: decline.
                            let _ = self
                                .send(json!({"jsonrpc": "2.0", "id": rpc_id, "result": {"decision": "decline"}}))
                                .await;
                            continue;
                        };
                        let id = ApprovalId::new();
                        lock(&self.approvals).insert(id.as_str().to_string(), rpc_id);
                        let _ = events
                            .send(AdapterEvent::Approval {
                                turn_id,
                                id,
                                kind,
                                summary,
                                detail,
                                subject,
                            })
                            .await;
                    }
                }
            }
        }
        self.process.wait_exit(Duration::from_secs(2)).await;
        let _ = events
            .send(AdapterEvent::Exited {
                code: self.process.exit_code().flatten(),
                stderr_tail: self.process.stderr_tail(),
            })
            .await;
    }
}

#[async_trait]
impl ChatSession for CodexSession {
    fn engine_session_id(&self) -> Option<String> {
        lock(&self.thread_id).clone()
    }

    async fn send_turn(&self, turn: TurnInput) -> Result<TurnId, AdapterError> {
        let thread = lock(&self.thread_id).clone().ok_or(AdapterError::Closed)?;
        *lock(&self.turn) = Some(turn.turn_id.clone());
        *lock(&self.codex_turn) = None;
        let (method, params) = self.protocol.start_turn(&thread, &turn, &self.req);
        let started = self.request(method, params, HANDSHAKE).await?;
        if let Some(id) = started.pointer("/turn/id").and_then(Value::as_str) {
            *lock(&self.codex_turn) = Some(id.to_string());
        }
        Ok(turn.turn_id)
    }

    async fn interrupt(&self, _turn: &TurnId) -> Result<InterruptOutcome, AdapterError> {
        let thread = lock(&self.thread_id).clone();
        let codex_turn = lock(&self.codex_turn).clone();
        let me = self.clone();
        Ok(self
            .process
            .stop(async move {
                let (Some(thread), Some(turn)) = (thread, codex_turn) else {
                    return false;
                };
                let (method, params) = me.protocol.interrupt(&thread, &turn);
                me.request(method, params, Duration::from_secs(3))
                    .await
                    .is_ok()
            })
            .await)
    }

    async fn resolve_approval(
        &self,
        id: &ApprovalId,
        decision: ApprovalDecision,
    ) -> Result<(), AdapterError> {
        let Some(rpc_id) = lock(&self.approvals).remove(id.as_str()) else {
            return Ok(());
        };
        self.send(
            json!({"jsonrpc": "2.0", "id": rpc_id, "result": self.protocol.decision(&decision)}),
        )
        .await
    }

    async fn set_model(
        &self,
        _model: Option<String>,
        _effort: Option<String>,
    ) -> Result<(), AdapterError> {
        // Each turn/start carries the thread's model and effort.
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), AdapterError> {
        self.process.kill_tree().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
