//! The Claude Code adapter (F-TH-04, ARCHITECTURE §6.8.1): `claude -p` with
//! stream-json on both sides. Repo settings can't grant anything
//! (`--setting-sources user`), the only MCP servers are Vibezeo's
//! (`--strict-mcp-config`), and tool permissions go to the gateway's
//! `tandem_approvals__request`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, FixedOffset};
use regex::Regex;
use serde_json::{json, Value};
use tandem_core::access::{Access, ThreadMode};
use tandem_core::approvals::ApprovalDecision;
use tandem_core::ids::{ApprovalId, TurnId};
use tandem_core::limits;
use tokio::sync::{mpsc, Notify};

use super::process::{ChatProcess, ProcessLine, ProcessSpec};
use super::{
    AdapterError, AdapterEvent, ChatAdapter, ChatSession, ChatStart, InterruptOutcome, TurnInput,
};
use crate::events::{
    truncate_output, ErrorAction, ErrorStage, LimitKind, ThreadEvent, ToolStatus, ToolVerb,
    TurnEndReason,
};
use crate::transcript::window_for;

/// The permission prompt tool, as Claude names the gateway's tool.
pub const PERMISSION_TOOL: &str = "mcp__tandem__tandem_approvals__request";
/// Tools a Read-only or Plan-mode thread can't use at all.
const NO_WRITES: &str = "Bash,Edit,Write,MultiEdit,NotebookEdit";
const PROMPT_TOO_LONG_HINT: &str =
    "The conversation is too long. Start a new thread with a summary?";
/// Tool input kept as the row's detail.
const DETAIL_MAX: usize = 4 * 1024;

/// `--permission-mode` and `--disallowedTools` for the session's resolved
/// access (`tandem_core::access::resolve`) and mode (§6.8.1 table).
pub fn permissions(access: Access, mode: ThreadMode) -> (&'static str, Option<&'static str>) {
    match (mode, access) {
        (ThreadMode::Plan, _) => ("plan", Some(NO_WRITES)),
        (ThreadMode::Act, Access::Read) => ("default", Some(NO_WRITES)),
        (ThreadMode::Act, Access::Edits) => ("acceptEdits", None),
        (ThreadMode::Act, Access::Full) => ("bypassPermissions", None),
    }
}

/// The spawn arguments (F-TH-04 AC1/AC2: every spawn has
/// `--setting-sources user` and `--strict-mcp-config`).
pub fn args(req: &ChatStart) -> Vec<OsString> {
    let mut a: Vec<OsString> = [
        "-p",
        "--output-format",
        "stream-json",
        "--input-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--setting-sources",
        "user",
        "--strict-mcp-config",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    if let Some(cfg) = &req.mcp_config {
        a.push("--mcp-config".into());
        a.push(cfg.into());
        a.push("--permission-prompt-tool".into());
        a.push(PERMISSION_TOOL.into());
    }
    // The teammate briefing, from its file: a large one never reaches argv
    // (F-AG-06 AC2).
    if let Some(briefing) = &req.briefing {
        a.push("--append-system-prompt-file".into());
        a.push(briefing.into());
    }
    let (mode, disallowed) = permissions(req.access, req.mode);
    a.push("--permission-mode".into());
    a.push(mode.into());
    if let Some(d) = disallowed {
        a.push("--disallowedTools".into());
        a.push(d.into());
    }
    if let Some(model) = &req.model {
        a.push("--model".into());
        a.push(model.into());
    }
    if let Some(session) = &req.resume_session {
        a.push("--resume".into());
        a.push(session.into());
    }
    a
}

/// What a stdout line means to the session.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Event(ThreadEvent),
    Session(String),
    /// The engine answered an interrupt (or ended the turn).
    Ack,
}

pub fn verb(tool: &str) -> ToolVerb {
    match tool {
        "Bash" => ToolVerb::Bash,
        "Read" => ToolVerb::Read,
        "Grep" => ToolVerb::Grep,
        "Glob" => ToolVerb::Glob,
        "Edit" => ToolVerb::Edit,
        "Write" => ToolVerb::Write,
        "MultiEdit" => ToolVerb::Multiedit,
        "WebFetch" => ToolVerb::Webfetch,
        "WebSearch" => ToolVerb::Websearch,
        "Task" => ToolVerb::Task,
        t if t.starts_with("mcp__") => ToolVerb::Mcp,
        _ => ToolVerb::Other,
    }
}

/// One line for a tool row: the command, path, pattern or URL.
pub fn arg_summary(tool: &str, input: &Value, cwd: &Path) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);
    let path = |k: &str| {
        s(k).map(|p| {
            Path::new(&p)
                .strip_prefix(cwd)
                .map(|r| r.display().to_string())
                .unwrap_or(p)
        })
    };
    let summary = match tool {
        "Bash" => s("command"),
        "Read" | "Edit" | "Write" | "MultiEdit" => path("file_path"),
        "NotebookEdit" => path("notebook_path"),
        "Grep" | "Glob" => s("pattern"),
        "WebFetch" => s("url"),
        "WebSearch" => s("query"),
        "Task" => s("description"),
        t if t.starts_with("mcp__") => Some(t.trim_start_matches("mcp__").replace("__", " › ")),
        _ => None,
    };
    let line = summary.unwrap_or_else(|| tool.to_string());
    let first = line.lines().next().unwrap_or("").trim();
    first.chars().take(300).collect()
}

fn detail(input: &Value) -> Option<String> {
    let text = serde_json::to_string_pretty(input).ok()?;
    Some(truncate_output(&text).0.chars().take(DETAIL_MAX).collect())
}

/// A tool result's text (a string, or text blocks).
fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Claude's stream-json output, one line at a time, as normalized events.
pub struct ClaudeParser {
    cwd: PathBuf,
    limits: Vec<Regex>,
    /// "Session started" is said once, and not for a resumed session.
    announce: bool,
    model: String,
    /// Tool id → (name, summary, detail), for its result row.
    tools: HashMap<String, (String, String, Option<String>)>,
}

impl ClaudeParser {
    pub fn new(cwd: PathBuf, limit_patterns: &[String], resumed: bool) -> Self {
        Self {
            cwd,
            limits: limits::compile(limit_patterns),
            announce: !resumed,
            model: String::new(),
            tools: HashMap::new(),
        }
    }

    pub fn line(&mut self, line: &str, now: DateTime<FixedOffset>) -> Vec<Parsed> {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        // A subagent's own messages stay inside its Task row.
        if !v.get("parent_tool_use_id").is_none_or(Value::is_null) {
            return Vec::new();
        }
        match kind {
            "system" => self.system(&v),
            "stream_event" => self.stream(&v),
            "assistant" => self.assistant(&v),
            "user" => self.tool_results(&v),
            "result" => self.result(&v, now),
            "control_response" => vec![Parsed::Ack],
            _ => Vec::new(),
        }
    }

    fn system(&mut self, v: &Value) -> Vec<Parsed> {
        if v.get("subtype").and_then(Value::as_str) != Some("init") {
            return Vec::new();
        }
        let mut out = Vec::new();
        if let Some(model) = v.get("model").and_then(Value::as_str) {
            self.model = model.to_string();
        }
        if let Some(id) = v.get("session_id").and_then(Value::as_str) {
            out.push(Parsed::Session(id.to_string()));
        }
        if std::mem::take(&mut self.announce) {
            out.push(Parsed::Event(ThreadEvent::Note {
                text: "Session started".into(),
            }));
        }
        out
    }

    fn stream(&mut self, v: &Value) -> Vec<Parsed> {
        let ev = &v["event"];
        if ev.get("type").and_then(Value::as_str) != Some("content_block_delta") {
            return Vec::new();
        }
        let delta = &ev["delta"];
        match delta.get("type").and_then(Value::as_str) {
            Some("text_delta") => delta
                .get("text")
                .and_then(Value::as_str)
                .map(|t| {
                    vec![Parsed::Event(ThreadEvent::AssistantDelta {
                        text: t.into(),
                    })]
                })
                .unwrap_or_default(),
            Some("thinking_delta") => delta
                .get("thinking")
                .and_then(Value::as_str)
                .map(|t| {
                    vec![Parsed::Event(ThreadEvent::ReasoningDelta {
                        text: t.into(),
                    })]
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn assistant(&mut self, v: &Value) -> Vec<Parsed> {
        let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for b in blocks {
            match b.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let Some(t) = b.get("text").and_then(Value::as_str) else {
                        continue;
                    };
                    // A bare limit line: the result's limit card says it.
                    if t.len() < 300 && self.limits.iter().any(|p| p.is_match(t)) {
                        continue;
                    }
                    out.push(Parsed::Event(ThreadEvent::Assistant { markdown: t.into() }));
                }
                Some("thinking") => {
                    if let Some(t) = b.get("thinking").and_then(Value::as_str) {
                        out.push(Parsed::Event(ThreadEvent::Reasoning {
                            text: t.into(),
                            duration_ms: None,
                        }));
                    }
                }
                Some("tool_use") => {
                    let id = b
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let name = b
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let input = b.get("input").cloned().unwrap_or(Value::Null);
                    let summary = arg_summary(&name, &input, &self.cwd);
                    let detail = detail(&input);
                    let mut ev = ThreadEvent::tool(&id, verb(&name), &summary, ToolStatus::Running);
                    if let ThreadEvent::Tool { detail: d, .. } = &mut ev {
                        d.clone_from(&detail);
                    }
                    self.tools.insert(id, (name, summary, detail));
                    out.push(Parsed::Event(ev));
                }
                _ => {}
            }
        }
        out
    }

    fn tool_results(&mut self, v: &Value) -> Vec<Parsed> {
        let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for b in blocks {
            if b.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let id = b.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
            let (name, summary, detail) = self
                .tools
                .get(id)
                .cloned()
                .unwrap_or_else(|| (String::new(), String::new(), None));
            let text = result_text(&b["content"]);
            let is_error = b.get("is_error").and_then(Value::as_bool).unwrap_or(false);
            let status = if !is_error {
                ToolStatus::Ok
            } else if denied(&text) {
                ToolStatus::Denied
            } else {
                ToolStatus::Error
            };
            let (output, truncated) = truncate_output(&text);
            out.push(Parsed::Event(ThreadEvent::Tool {
                id: id.to_string(),
                verb: verb(&name),
                arg_summary: summary,
                status,
                output: Some(output),
                output_truncated: truncated,
                output_file: None,
                diff: None,
                detail,
            }));
        }
        out
    }

    fn result(&mut self, v: &Value, now: DateTime<FixedOffset>) -> Vec<Parsed> {
        let mut out = vec![Parsed::Ack];
        if let Some(u) = v.get("usage") {
            let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
            let (input, read, write) = (
                n("input_tokens"),
                n("cache_read_input_tokens"),
                n("cache_creation_input_tokens"),
            );
            let window = window_for(&self.model) as f32;
            out.push(Parsed::Event(ThreadEvent::Usage {
                input,
                output: n("output_tokens"),
                cache_read: read,
                cache_write: write,
                context_fill_pct: Some(
                    (((input + read + write) as f32 / window * 1000.0).round() / 10.0).min(100.0),
                ),
                cost_usd: v.get("total_cost_usd").and_then(Value::as_f64),
            }));
        }
        let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("");
        let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false)
            || subtype.starts_with("error");
        let text = v.get("result").and_then(Value::as_str).unwrap_or("");
        let end = |reason| {
            Parsed::Event(ThreadEvent::TurnEnd {
                reason,
                blocked_actions: 0,
            })
        };
        if is_error {
            if text.contains("Prompt is too long") || text.contains("prompt_too_long") {
                out.push(Parsed::Event(ThreadEvent::Error {
                    code: "prompt_too_long".into(),
                    message: "The conversation is too long for the model.".into(),
                    stage: ErrorStage::Model,
                    hint: Some(PROMPT_TOO_LONG_HINT.into()),
                    action: Some(ErrorAction::SummarizeContinue),
                }));
                out.push(end(TurnEndReason::Error));
                return out;
            }
            if let Some(hit) = limits::detect(text, &self.limits, now) {
                let lower = hit.message.to_lowercase();
                let kind = if lower.contains("week") {
                    LimitKind::Weekly
                } else if lower.contains("rate") || lower.contains("429") {
                    LimitKind::Rate
                } else {
                    LimitKind::Session
                };
                // "…limit reached|1760000000": the epoch is for us, not the card.
                let message = match hit.message.split_once('|') {
                    Some((m, epoch)) if epoch.chars().all(|c| c.is_ascii_digit()) => {
                        m.trim().to_string()
                    }
                    _ => hit.message.clone(),
                };
                out.push(Parsed::Event(ThreadEvent::Limit {
                    kind,
                    resets_at: hit.resets_at,
                    message,
                }));
                out.push(end(TurnEndReason::Limit));
                return out;
            }
            let message = if text.is_empty() {
                "Claude Code stopped with an error.".to_string()
            } else {
                text.chars().take(500).collect()
            };
            out.push(Parsed::Event(ThreadEvent::Error {
                code: if subtype.is_empty() {
                    "error".into()
                } else {
                    subtype.into()
                },
                message,
                stage: ErrorStage::Unknown,
                hint: None,
                action: Some(ErrorAction::Retry),
            }));
            out.push(end(TurnEndReason::Error));
            return out;
        }
        out.push(end(TurnEndReason::Completed));
        out
    }
}

/// A tool error that is a refusal (the approvals tool's deny, or a tool
/// the access level doesn't allow), not a failure.
fn denied(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("denied") || t.contains("permission to use") || t.contains("not allowed")
}

/// The stream-json user message for a turn: text (with `@path` references
/// for files) and images inline.
pub fn user_message(turn: &TurnInput) -> Result<String, AdapterError> {
    let mut text = turn.text.clone();
    for f in &turn.files {
        text.push_str(&format!(" @{f}"));
    }
    let mut content = vec![json!({"type": "text", "text": text})];
    for image in &turn.images {
        let bytes = std::fs::read(image)
            .map_err(|e| AdapterError::Protocol(format!("couldn't read an image: {e}")))?;
        let media = match image
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            _ => "image/png",
        };
        content.push(json!({
            "type": "image",
            "source": {"type": "base64", "media_type": media, "data": STANDARD.encode(bytes)}
        }));
    }
    Ok(json!({"type": "user", "message": {"role": "user", "content": content}}).to_string())
}

pub struct ClaudeAdapter {
    limit_patterns: Vec<String>,
}

impl ClaudeAdapter {
    pub fn new(limit_patterns: Vec<String>) -> Self {
        Self { limit_patterns }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

#[async_trait]
impl ChatAdapter for ClaudeAdapter {
    fn engine_id(&self) -> &str {
        "claude"
    }

    async fn start(
        &self,
        req: ChatStart,
        events: mpsc::Sender<AdapterEvent>,
    ) -> Result<Box<dyn ChatSession>, AdapterError> {
        // Status hooks are for panes; a thread's process never reports there.
        let env: Vec<(String, String)> = req
            .env
            .iter()
            .filter(|(k, _)| !k.starts_with("TANDEM_STATUS"))
            .cloned()
            .collect();
        let spec = ProcessSpec::new(req.program.clone(), args(&req), env, req.cwd.clone());
        let (process, lines) = ChatProcess::spawn(spec)?;
        let session = ClaudeSession {
            process: Arc::new(process),
            turn: Arc::new(Mutex::new(None)),
            session_id: Arc::new(Mutex::new(req.resume_session.clone())),
            acked: Arc::new(Notify::new()),
        };
        let parser = ClaudeParser::new(
            req.cwd.clone(),
            &self.limit_patterns,
            req.resume_session.is_some(),
        );
        tokio::spawn(session.clone().read(parser, lines, events));
        Ok(Box::new(session))
    }
}

#[derive(Clone)]
struct ClaudeSession {
    process: Arc<ChatProcess>,
    turn: Arc<Mutex<Option<TurnId>>>,
    session_id: Arc<Mutex<Option<String>>>,
    acked: Arc<Notify>,
}

impl ClaudeSession {
    async fn read(
        self,
        mut parser: ClaudeParser,
        mut lines: mpsc::Receiver<ProcessLine>,
        events: mpsc::Sender<AdapterEvent>,
    ) {
        while let Some(line) = lines.recv().await {
            let parsed = match line {
                ProcessLine::Line(l) => parser.line(&l, limits::local(chrono::Utc::now())),
                ProcessLine::TooLong => vec![Parsed::Event(ThreadEvent::Error {
                    code: "line_too_long".into(),
                    message: "Claude Code sent a message too large to show; it was skipped.".into(),
                    stage: ErrorStage::Unknown,
                    hint: None,
                    action: None,
                })],
            };
            for p in parsed {
                match p {
                    Parsed::Ack => self.acked.notify_waiters(),
                    Parsed::Session(id) => {
                        *lock(&self.session_id) = Some(id.clone());
                        let _ = events.send(AdapterEvent::SessionId(id)).await;
                    }
                    Parsed::Event(event) => {
                        let turn = lock(&self.turn).clone();
                        if let Some(turn_id) = turn {
                            let _ = events.send(AdapterEvent::Event { turn_id, event }).await;
                        }
                    }
                }
            }
        }
        self.process
            .wait_exit(std::time::Duration::from_secs(2))
            .await;
        let _ = events
            .send(AdapterEvent::Exited {
                code: self.process.exit_code().flatten(),
                stderr_tail: self.process.stderr_tail(),
            })
            .await;
    }
}

#[async_trait]
impl ChatSession for ClaudeSession {
    fn engine_session_id(&self) -> Option<String> {
        lock(&self.session_id).clone()
    }

    async fn send_turn(&self, turn: TurnInput) -> Result<TurnId, AdapterError> {
        let line = user_message(&turn)?;
        *lock(&self.turn) = Some(turn.turn_id.clone());
        self.process.write_line(&line).await?;
        Ok(turn.turn_id)
    }

    async fn interrupt(&self, _turn: &TurnId) -> Result<InterruptOutcome, AdapterError> {
        let request = json!({
            "type": "control_request",
            "request_id": ulid::Ulid::new().to_string(),
            "request": {"subtype": "interrupt"}
        })
        .to_string();
        let acked = self.acked.clone();
        let process = self.process.clone();
        Ok(self
            .process
            .stop(async move {
                let waiting = acked.notified();
                tokio::pin!(waiting);
                waiting.as_mut().enable();
                if process.write_line(&request).await.is_err() {
                    return false;
                }
                waiting.await;
                true
            })
            .await)
    }

    async fn resolve_approval(
        &self,
        _id: &ApprovalId,
        _decision: ApprovalDecision,
    ) -> Result<(), AdapterError> {
        // Claude's approvals go through the gateway's permission tool.
        Ok(())
    }

    async fn set_model(
        &self,
        _model: Option<String>,
        _effort: Option<String>,
    ) -> Result<(), AdapterError> {
        // The model is a spawn flag: the threads service starts a new
        // session (resumed) with it.
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), AdapterError> {
        self.process.kill_tree().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
