//! The ACP adapter (F-TH-06, ARCHITECTURE §6.8.3): the Agent Client Protocol
//! over stdio (JSON-RPC 2.0, one message per line) for every registry engine
//! whose `chatAdapter` is `acp` (Cursor Agent, Grok, OpenCode, …). One agent
//! process per thread session. The gateway joins as an HTTP MCP server when
//! the agent takes one; the agent asks for permission in-band, and reads and
//! writes files through this client, inside the thread's folder only.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use regex::Regex;
use serde_json::{json, Value};
use tandem_core::access::Access;
use tandem_core::allow_rules::ApprovalSubject;
use tandem_core::approvals::{ApprovalDecision, ApprovalKind, ApprovalScope};
use tandem_core::contain::{contain, ContainPolicy};
use tandem_core::ids::{ApprovalId, TurnId};
use tandem_core::limits;
use tokio::sync::{mpsc, oneshot, watch};

use super::codex::middle;
use super::process::{ChatProcess, ProcessLine, ProcessSpec};
use super::{
    AdapterError, AdapterEvent, ChatAdapter, ChatSession, ChatStart, InterruptOutcome, ModelOffer,
    TurnInput,
};
use crate::events::{
    truncate_output, ErrorAction, ErrorStage, LimitKind, ThreadEvent, ToolStatus, ToolVerb,
    TurnEndReason,
};

/// The protocol version this client speaks.
pub const PROTOCOL_VERSION: u64 = 1;
/// How long an agent gets to answer `initialize`, `session/new` and `session/load`.
const HANDSHAKE: Duration = Duration::from_secs(20);
/// Longest command shown on a tool row; the rest is in the detail.
const COMMAND_SHOWN: usize = 120;
/// The JSON-RPC error an agent answers with when it needs a sign-in.
const AUTH_REQUIRED: i64 = -32000;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// A permission option the agent offers (`allow_once`, `allow_always`,
/// `reject_once`, `reject_always`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionOption {
    pub id: String,
    pub kind: String,
}

/// What an agent message means to the session.
#[derive(Debug, Clone, PartialEq)]
pub enum Acp {
    Event(ThreadEvent),
    /// `session/request_permission`.
    Approval {
        rpc_id: Value,
        kind: ApprovalKind,
        summary: String,
        detail: String,
        subject: ApprovalSubject,
        options: Vec<PermissionOption>,
    },
    /// `fs/read_text_file` (`line` is 1-based).
    Read {
        rpc_id: Value,
        path: String,
        line: Option<u64>,
        limit: Option<u64>,
    },
    /// `fs/write_text_file`.
    Write {
        rpc_id: Value,
        path: String,
        content: String,
    },
    /// A request this client doesn't serve (terminals and anything newer).
    Unsupported {
        rpc_id: Value,
        method: String,
    },
}

#[derive(Debug, Clone)]
struct ToolState {
    kind: String,
    verb: ToolVerb,
    arg_summary: String,
    detail: Option<String>,
    status: ToolStatus,
    output: Option<String>,
    diff: Option<String>,
    /// The files an edit names, absolute (F-TK-07).
    paths: Option<Vec<String>>,
}

/// An ACP tool kind as a row's verb. ACP's delete and move are file changes.
fn verb(kind: &str) -> ToolVerb {
    match kind {
        "read" => ToolVerb::Read,
        "edit" | "delete" | "move" => ToolVerb::Edit,
        "search" => ToolVerb::Grep,
        "execute" => ToolVerb::Bash,
        "fetch" => ToolVerb::Webfetch,
        _ => ToolVerb::Other,
    }
}

fn status(s: &str) -> Option<ToolStatus> {
    match s {
        "pending" => Some(ToolStatus::Pending),
        "in_progress" => Some(ToolStatus::Running),
        "completed" => Some(ToolStatus::Ok),
        "failed" => Some(ToolStatus::Error),
        _ => None,
    }
}

/// A command as text (a string, or argv).
fn command_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    }
}

/// A one-hunk unified diff of `old` → `new` (the changed lines between the
/// common head and tail, with up to 3 lines of context).
pub fn unified(path: &str, old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let ctx = 3;
    let start = head.saturating_sub(ctx);
    let a_end = (a.len() - tail + ctx).min(a.len());
    let b_end = (b.len() - tail + ctx).min(b.len());
    let mut out = if old.is_empty() {
        format!("--- /dev/null\n+++ b/{path}\n")
    } else {
        format!("--- a/{path}\n+++ b/{path}\n")
    };
    out.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        if a_end > start { start + 1 } else { start },
        a_end - start,
        if b_end > start { start + 1 } else { start },
        b_end - start
    ));
    for line in &a[start..head] {
        out.push_str(&format!(" {line}\n"));
    }
    for line in &a[head..a.len() - tail] {
        out.push_str(&format!("-{line}\n"));
    }
    for line in &b[head..b.len() - tail] {
        out.push_str(&format!("+{line}\n"));
    }
    for line in &a[a.len() - tail..a_end] {
        out.push_str(&format!(" {line}\n"));
    }
    out
}

/// Agent notifications and requests as normalized events.
pub struct AcpParser {
    cwd: PathBuf,
    tools: HashMap<String, ToolState>,
}

impl AcpParser {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            tools: HashMap::new(),
        }
    }

    /// One message from the agent that isn't an answer to our requests.
    pub fn message(&mut self, msg: &Value) -> Vec<Acp> {
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            return Vec::new();
        };
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        match msg.get("id") {
            Some(id) => self.request(id.clone(), method, &params),
            None if method == "session/update" => self.update(&params["update"]),
            None => Vec::new(),
        }
    }

    fn relative(&self, path: &str) -> String {
        Path::new(path)
            .strip_prefix(&self.cwd)
            .map(|r| r.display().to_string())
            .unwrap_or_else(|_| path.to_string())
    }

    /// A tool call's state from a `tool_call`, or merged into the known one
    /// from a `tool_call_update` (which sends only what changed).
    fn tool(&mut self, u: &Value) -> Option<ThreadEvent> {
        let id = u.get("toolCallId").and_then(Value::as_str)?.to_string();
        let known = self.tools.get(&id).cloned();
        let kind = u
            .get("kind")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| known.as_ref().map(|k| k.kind.clone()))
            .unwrap_or_else(|| "other".into());
        let title = u.get("title").and_then(Value::as_str);
        let raw = u.get("rawInput").unwrap_or(&Value::Null);
        let path = u
            .pointer("/locations/0/path")
            .and_then(Value::as_str)
            .map(|p| self.relative(p));
        let command = raw.get("command").and_then(command_text);
        let (arg_summary, detail) = match (kind.as_str(), &command, &path) {
            ("execute", Some(cmd), _) => (middle(cmd, COMMAND_SHOWN), Some(cmd.clone())),
            (_, _, Some(p)) => (p.clone(), title.map(str::to_string)),
            _ => match (title, &known) {
                (Some(t), _) => (t.to_string(), None),
                (None, Some(k)) => (k.arg_summary.clone(), k.detail.clone()),
                (None, None) => (kind.clone(), None),
            },
        };
        let mut output = known.as_ref().and_then(|k| k.output.clone());
        let mut diff = known.as_ref().and_then(|k| k.diff.clone());
        // An edit's files: what it said before, then its locations and the
        // files its diffs name (F-TK-07).
        let mut edited: Vec<String> = known
            .as_ref()
            .and_then(|k| k.paths.clone())
            .unwrap_or_default();
        let mut name = |p: &str| {
            let p = crate::events::absolute(&self.cwd, p);
            if !p.is_empty() && !edited.contains(&p) {
                edited.push(p);
            }
        };
        for l in u
            .get("locations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(p) = l
                .get("path")
                .and_then(Value::as_str)
                .filter(|p| !p.is_empty())
            {
                name(p);
            }
        }
        if let Some(content) = u.get("content").and_then(Value::as_array) {
            for c in content {
                if c.get("type").and_then(Value::as_str) == Some("diff") {
                    if let Some(p) = c
                        .get("path")
                        .and_then(Value::as_str)
                        .filter(|p| !p.is_empty())
                    {
                        name(p);
                    }
                }
            }
        }
        let paths = matches!(kind.as_str(), "edit" | "delete" | "move")
            .then_some(edited)
            .filter(|e| !e.is_empty());
        if let Some(content) = u.get("content").and_then(Value::as_array) {
            let mut texts = Vec::new();
            let mut diffs = Vec::new();
            for c in content {
                match c.get("type").and_then(Value::as_str) {
                    Some("content") => {
                        if let Some(t) = c.pointer("/content/text").and_then(Value::as_str) {
                            texts.push(t.to_string());
                        }
                    }
                    Some("diff") => {
                        let p = self.relative(c.get("path").and_then(Value::as_str).unwrap_or(""));
                        let old = c.get("oldText").and_then(Value::as_str).unwrap_or("");
                        let new = c.get("newText").and_then(Value::as_str).unwrap_or("");
                        diffs.push(unified(&p, old, new));
                    }
                    _ => {}
                }
            }
            if !texts.is_empty() {
                output = Some(texts.join("\n"));
            }
            if !diffs.is_empty() {
                diff = Some(diffs.join("\n"));
            }
        }
        let status = u
            .get("status")
            .and_then(Value::as_str)
            .and_then(status)
            .or_else(|| known.as_ref().map(|k| k.status))
            .unwrap_or(ToolStatus::Pending);
        // An update keeps what the call said when it didn't say it again.
        let (arg_summary, detail) = match &known {
            Some(k) if title.is_none() && command.is_none() && path.is_none() => {
                (k.arg_summary.clone(), k.detail.clone())
            }
            _ => (arg_summary, detail),
        };
        let state = ToolState {
            verb: verb(&kind),
            kind,
            arg_summary,
            detail,
            status,
            output,
            diff,
            paths,
        };
        self.tools.insert(id.clone(), state.clone());
        let (output, output_truncated) = match state.output.as_deref() {
            Some(o) => {
                let (o, cut) = truncate_output(o);
                (Some(o), cut)
            }
            None => (None, false),
        };
        Some(ThreadEvent::Tool {
            id,
            verb: state.verb,
            arg_summary: state.arg_summary,
            status: state.status,
            output,
            output_truncated,
            output_file: None,
            diff: state.diff,
            detail: state.detail,
            paths: state.paths,
        })
    }

    fn update(&mut self, u: &Value) -> Vec<Acp> {
        let text = || {
            u.pointer("/content/text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let event = match u.get("sessionUpdate").and_then(Value::as_str).unwrap_or("") {
            "agent_message_chunk" => Some(text())
                .filter(|t| !t.is_empty())
                .map(|text| ThreadEvent::AssistantDelta { text }),
            "agent_thought_chunk" => Some(text())
                .filter(|t| !t.is_empty())
                .map(|text| ThreadEvent::ReasoningDelta { text }),
            "tool_call" | "tool_call_update" => self.tool(u),
            "plan" => {
                let lines: Vec<String> = u
                    .get("entries")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|e| {
                                let content = e.get("content").and_then(Value::as_str)?;
                                let mark = match e.get("status").and_then(Value::as_str) {
                                    Some("completed") => "x",
                                    _ => " ",
                                };
                                Some(format!("- [{mark}] {content}"))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (!lines.is_empty()).then(|| ThreadEvent::Note {
                    text: format!("Plan\n{}", lines.join("\n")),
                })
            }
            // Slash commands, modes and the replayed user side aren't rows.
            _ => None,
        };
        event.map(Acp::Event).into_iter().collect()
    }

    fn request(&mut self, rpc_id: Value, method: &str, p: &Value) -> Vec<Acp> {
        match method {
            "session/request_permission" => self.permission(rpc_id, p),
            "fs/read_text_file" => vec![Acp::Read {
                rpc_id,
                path: p.get("path").and_then(Value::as_str).unwrap_or("").into(),
                line: p.get("line").and_then(Value::as_u64),
                limit: p.get("limit").and_then(Value::as_u64),
            }],
            "fs/write_text_file" => vec![Acp::Write {
                rpc_id,
                path: p.get("path").and_then(Value::as_str).unwrap_or("").into(),
                content: p
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .into(),
            }],
            _ => vec![Acp::Unsupported {
                rpc_id,
                method: method.into(),
            }],
        }
    }

    fn permission(&mut self, rpc_id: Value, p: &Value) -> Vec<Acp> {
        let call = &p["toolCall"];
        let id = call.get("toolCallId").and_then(Value::as_str).unwrap_or("");
        let mut out = Vec::new();
        // A call the agent didn't announce first still gets its row.
        let announced = self.tools.contains_key(id);
        let row = self.tool(call);
        if !announced {
            out.extend(row.map(Acp::Event));
        }
        let state = self.tools.get(id).cloned();
        let kind = state.as_ref().map_or("other", |s| s.kind.as_str());
        let shown = state
            .as_ref()
            .map_or(String::new(), |s| s.arg_summary.clone());
        let full = state
            .as_ref()
            .and_then(|s| s.detail.clone())
            .unwrap_or_else(|| shown.clone());
        let (approval, summary, subject) = match kind {
            "execute" => (
                ApprovalKind::Command,
                format!("Run `{shown}`"),
                ApprovalSubject::Command(full.clone()),
            ),
            "edit" | "move" => (
                ApprovalKind::FileWrite,
                format!("Edit `{shown}`"),
                ApprovalSubject::Tool(kind.into()),
            ),
            "delete" => (
                ApprovalKind::FileDelete,
                format!("Delete `{shown}`"),
                ApprovalSubject::Tool(kind.into()),
            ),
            "fetch" => (
                ApprovalKind::Network,
                format!("Fetch `{shown}`"),
                ApprovalSubject::Tool(kind.into()),
            ),
            _ => (
                ApprovalKind::Other,
                if shown.is_empty() {
                    "Use a tool".into()
                } else {
                    shown.clone()
                },
                ApprovalSubject::Tool(kind.into()),
            ),
        };
        let detail = state.as_ref().and_then(|s| s.diff.clone()).unwrap_or(full);
        let options = p
            .get("options")
            .and_then(Value::as_array)
            .map(|opts| {
                opts.iter()
                    .filter_map(|o| {
                        Some(PermissionOption {
                            id: o.get("optionId")?.as_str()?.to_string(),
                            kind: o.get("kind")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(Acp::Approval {
            rpc_id,
            kind: approval,
            summary,
            detail,
            subject,
            options,
        });
        out
    }
}

/// The option that carries a decision, if the agent offered one.
pub fn choose(options: &[PermissionOption], decision: &ApprovalDecision) -> Option<String> {
    let wanted: &[&str] = match decision {
        ApprovalDecision::Allow {
            scope: ApprovalScope::Once,
        } => &["allow_once", "allow_always"],
        ApprovalDecision::Allow { .. } => &["allow_always", "allow_once"],
        ApprovalDecision::Deny { .. } => &["reject_once", "reject_always"],
    };
    wanted
        .iter()
        .find_map(|k| options.iter().find(|o| o.kind == *k))
        .map(|o| o.id.clone())
}

/// The models an agent reports (`models.availableModels`), if it does.
pub fn offered(result: &Value) -> Option<Vec<ModelOffer>> {
    let list = result.pointer("/models/availableModels")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|m| {
                let id = m.get("modelId")?.as_str()?.to_string();
                let label = m
                    .get("name")
                    .and_then(Value::as_str)
                    .map_or_else(|| id.clone(), str::to_string);
                Some(ModelOffer { id, label })
            })
            .collect(),
    )
}

/// Whether an error from the agent is about signing in.
fn is_auth(code: i64, message: &str) -> bool {
    let m = message.to_lowercase();
    code == AUTH_REQUIRED && (m.contains("auth") || m.contains("sign") || m.contains("log"))
        || [
            "unauthorized",
            "not authenticated",
            "authentication required",
            "please log in",
            "sign in",
        ]
        .iter()
        .any(|k| m.contains(k))
}

/// A failed `session/prompt` as the turn's last events.
pub fn prompt_failed(
    label: &str,
    code: i64,
    message: &str,
    patterns: &[Regex],
    now: chrono::DateTime<chrono::FixedOffset>,
) -> Vec<ThreadEvent> {
    if let Some(hit) = limits::detect(message, patterns, now) {
        let kind = if message.to_lowercase().contains("week") {
            LimitKind::Weekly
        } else {
            LimitKind::Rate
        };
        return vec![
            ThreadEvent::Limit {
                kind,
                resets_at: hit.resets_at,
                message: message.to_string(),
            },
            ThreadEvent::TurnEnd {
                reason: TurnEndReason::Limit,
                blocked_actions: 0,
            },
        ];
    }
    let (stage, action, code_text) = if is_auth(code, message) {
        (ErrorStage::Auth, ErrorAction::SignIn, "auth_required")
    } else if message.to_lowercase().contains("model") {
        (ErrorStage::Model, ErrorAction::Retry, "model_error")
    } else {
        (ErrorStage::Unknown, ErrorAction::Retry, "prompt_failed")
    };
    let message = if stage == ErrorStage::Auth {
        format!("{label} needs you to sign in.")
    } else if message.is_empty() {
        format!("{label} stopped with an error.")
    } else {
        message.to_string()
    };
    vec![
        ThreadEvent::Error {
            code: code_text.into(),
            message,
            stage,
            hint: None,
            action: Some(action),
        },
        ThreadEvent::TurnEnd {
            reason: TurnEndReason::Error,
            blocked_actions: 0,
        },
    ]
}

/// A `session/prompt` answer as the turn's last events.
pub fn stopped(label: &str, result: &Value) -> Vec<ThreadEvent> {
    let end = |reason| ThreadEvent::TurnEnd {
        reason,
        blocked_actions: 0,
    };
    let error = |code: &str, message: String| ThreadEvent::Error {
        code: code.into(),
        message,
        stage: ErrorStage::Model,
        hint: None,
        action: Some(ErrorAction::Retry),
    };
    match result
        .get("stopReason")
        .and_then(Value::as_str)
        .unwrap_or("end_turn")
    {
        "cancelled" => vec![end(TurnEndReason::Cancelled)],
        "max_tokens" => vec![
            error(
                "max_tokens",
                format!("{label} stopped: the reply reached its length limit."),
            ),
            end(TurnEndReason::Error),
        ],
        "max_turn_requests" => vec![
            error(
                "max_turn_requests",
                format!("{label} stopped after too many steps in one turn."),
            ),
            end(TurnEndReason::Error),
        ],
        "refusal" => vec![
            error("refusal", format!("{label} declined to continue.")),
            end(TurnEndReason::Error),
        ],
        _ => vec![end(TurnEndReason::Completed)],
    }
}

/// One adapter per ACP engine in the registry.
pub struct AcpAdapter {
    engine_id: String,
    label: String,
    /// `acpCmd` after the program (the resolved executable runs them).
    args: Vec<String>,
    limit_patterns: Vec<String>,
}

impl AcpAdapter {
    pub fn new(
        engine_id: &str,
        label: &str,
        acp_cmd: &[String],
        limit_patterns: Vec<String>,
    ) -> Self {
        Self {
            engine_id: engine_id.into(),
            label: label.into(),
            args: acp_cmd.iter().skip(1).cloned().collect(),
            limit_patterns,
        }
    }

    pub(crate) fn launch(&self, req: &ChatStart) -> ProcessSpec {
        let env: Vec<(String, String)> = req
            .env
            .iter()
            .filter(|(k, _)| !k.starts_with("TANDEM_STATUS"))
            .cloned()
            .collect();
        let spec = ProcessSpec::new(
            req.program.clone(),
            self.args.iter().map(OsString::from).collect(),
            env,
            req.cwd.clone(),
        );
        super::Relocate::apply(req.relocate.as_ref(), spec)
    }

    pub(crate) async fn start_with(
        &self,
        spec: ProcessSpec,
        req: ChatStart,
        events: mpsc::Sender<AdapterEvent>,
    ) -> Result<Box<dyn ChatSession>, AdapterError> {
        let (process, lines) = ChatProcess::spawn(spec)?;
        let (running, _) = watch::channel(false);
        let session = AcpSession {
            process: Arc::new(process),
            label: Arc::from(self.label.as_str()),
            limits: Arc::new(limits::compile(&self.limit_patterns)),
            req: Arc::new(req.clone()),
            events: events.clone(),
            next_id: Arc::new(AtomicI64::new(1)),
            pending: Arc::default(),
            session_id: Arc::new(Mutex::new(None)),
            turn: Arc::new(Mutex::new(None)),
            running: Arc::new(running),
            approvals: Arc::default(),
            note: Arc::new(Mutex::new(None)),
            images: Arc::new(Mutex::new(false)),
            briefing_due: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        };
        tokio::spawn(session.clone().read(AcpParser::new(req.cwd.clone()), lines));
        if let Err(e) = session.handshake().await {
            session.process.kill_tree().await;
            return Err(e);
        }
        Ok(Box::new(session))
    }
}

#[async_trait]
impl ChatAdapter for AcpAdapter {
    fn engine_id(&self) -> &str {
        &self.engine_id
    }

    async fn start(
        &self,
        req: ChatStart,
        events: mpsc::Sender<AdapterEvent>,
    ) -> Result<Box<dyn ChatSession>, AdapterError> {
        let spec = self.launch(&req);
        self.start_with(spec, req, events).await
    }
}

/// A JSON-RPC error from the agent.
#[derive(Debug, Clone)]
struct RpcError {
    code: i64,
    message: String,
}

type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, RpcError>>>>>;
/// Our approval id → the agent's request id and the options it offered.
type OpenApprovals = Arc<Mutex<HashMap<String, (Value, Vec<PermissionOption>)>>>;

#[derive(Clone)]
struct AcpSession {
    process: Arc<ChatProcess>,
    label: Arc<str>,
    limits: Arc<Vec<Regex>>,
    req: Arc<ChatStart>,
    events: mpsc::Sender<AdapterEvent>,
    next_id: Arc<AtomicI64>,
    pending: Pending,
    session_id: Arc<Mutex<Option<String>>>,
    turn: Arc<Mutex<Option<TurnId>>>,
    /// Whether a prompt is waiting for its answer (Stop waits for it).
    running: Arc<watch::Sender<bool>>,
    approvals: OpenApprovals,
    /// Said at the start of the next turn (a withdrawn model).
    note: Arc<Mutex<Option<String>>>,
    /// The agent takes images in prompts.
    images: Arc<Mutex<bool>>,
    /// The teammate briefing still has to open a prompt (F-AG-06).
    briefing_due: Arc<std::sync::atomic::AtomicBool>,
}

impl AcpSession {
    async fn send(&self, msg: Value) -> Result<(), AdapterError> {
        self.process.write_line(&msg.to_string()).await
    }

    async fn answer(&self, rpc_id: Value, result: Value) {
        let _ = self
            .send(json!({"jsonrpc": "2.0", "id": rpc_id, "result": result}))
            .await;
    }

    async fn refuse(&self, rpc_id: Value, code: i64, message: &str) {
        let _ = self
            .send(json!({"jsonrpc": "2.0", "id": rpc_id, "error": {"code": code, "message": message}}))
            .await;
    }

    /// Sends a request; its answer arrives through the reader.
    async fn call(
        &self,
        method: &str,
        params: Value,
    ) -> Result<oneshot::Receiver<Result<Value, RpcError>>, AdapterError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        lock(&self.pending).insert(id, tx);
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        Ok(rx)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let rx = self.call(method, params).await.map_err(|e| RpcError {
            code: 0,
            message: e.to_string(),
        })?;
        match tokio::time::timeout(HANDSHAKE, rx).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => Err(RpcError {
                code: 0,
                message: format!("{} stopped", self.label),
            }),
            Err(_) => Err(RpcError {
                code: 0,
                message: format!("{} didn't answer {method}", self.label),
            }),
        }
    }

    fn mcp_servers(&self, init: &Value) -> Value {
        let http = init
            .pointer("/agentCapabilities/mcpCapabilities/http")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut servers = match (&self.req.mcp, http) {
            (Some(mcp), true) => vec![json!({
                "type": "http",
                "name": "tandem",
                "url": mcp.url,
                "headers": [{"name": "Authorization", "value": format!("Bearer {}", mcp.token)}],
            })],
            _ => Vec::new(),
        };
        // Every ACP agent takes stdio servers.
        servers.extend(self.req.stdio_servers.iter().map(|s| {
            json!({
                "name": s.name,
                "command": s.command,
                "args": s.args,
                "env": [],
            })
        }));
        Value::Array(servers)
    }

    /// `initialize`, then a loaded or new session, and the thread's model.
    async fn handshake(&self) -> Result<(), AdapterError> {
        let init = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "clientCapabilities": {"fs": {"readTextFile": true, "writeTextFile": true}, "terminal": false},
                    "clientInfo": {"name": "vibezeo", "title": "Vibezeo", "version": env!("CARGO_PKG_VERSION")},
                }),
            )
            .await
            .map_err(|e| AdapterError::Protocol(e.message))?;
        if init.get("protocolVersion").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
            return Err(AdapterError::Unsupported(format!(
                "{} speaks a different version of the Agent Client Protocol — update Vibezeo or {}",
                self.label, self.label
            )));
        }
        *lock(&self.images) = init
            .pointer("/agentCapabilities/promptCapabilities/image")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let cwd = self.req.cwd.display().to_string();
        let servers = self.mcp_servers(&init);
        let can_load = init
            .pointer("/agentCapabilities/loadSession")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut opened = None;
        if let (Some(old), true) = (&self.req.resume_session, can_load) {
            // Replayed history arrives as updates outside any turn: dropped.
            if let Ok(r) = self
                .request(
                    "session/load",
                    json!({"sessionId": old, "cwd": cwd, "mcpServers": servers}),
                )
                .await
            {
                opened = Some((old.clone(), r));
            }
        }
        let (id, result) = match opened {
            Some(o) => o,
            None => {
                let r = self
                    .request("session/new", json!({"cwd": cwd, "mcpServers": servers}))
                    .await
                    .map_err(|e| {
                        if is_auth(e.code, &e.message) {
                            AdapterError::Auth(format!("{} needs you to sign in.", self.label))
                        } else {
                            AdapterError::Protocol(e.message)
                        }
                    })?;
                let id = r
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AdapterError::Protocol(format!("{} started no session", self.label))
                    })?
                    .to_string();
                (id, r)
            }
        };
        *lock(&self.session_id) = Some(id.clone());
        let _ = self.events.send(AdapterEvent::SessionId(id.clone())).await;
        self.models(&id, &result).await;
        Ok(())
    }

    /// The models the agent offers go to the picker; the thread's model is
    /// selected, or, when the agent no longer offers it, dropped with a note.
    async fn models(&self, session: &str, result: &Value) {
        let Some(available) = offered(result) else {
            return;
        };
        let wanted = self.req.model.clone();
        let mut withdrawn = wanted
            .clone()
            .filter(|m| !available.iter().any(|o| &o.id == m));
        if let (Some(m), None) = (&wanted, &withdrawn) {
            let current = result
                .pointer("/models/currentModelId")
                .and_then(Value::as_str);
            if current != Some(m.as_str())
                && self
                    .request(
                        "session/set_model",
                        json!({"sessionId": session, "modelId": m}),
                    )
                    .await
                    .is_err()
            {
                withdrawn = Some(m.clone());
            }
        }
        if withdrawn.is_some() {
            *lock(&self.note) = Some(format!("Model no longer offered by {}", self.label));
        }
        let _ = self
            .events
            .send(AdapterEvent::Models {
                available,
                withdrawn,
            })
            .await;
    }

    async fn emit(&self, turn_id: &TurnId, event: ThreadEvent) {
        let _ = self
            .events
            .send(AdapterEvent::Event {
                turn_id: turn_id.clone(),
                event,
            })
            .await;
    }

    fn prompt(&self, turn: &TurnInput) -> Result<Value, AdapterError> {
        let mut blocks = Vec::new();
        // ACP agents take no system prompt: a teammate's briefing opens the
        // session's first prompt (F-AG-06).
        if self
            .briefing_due
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            if let Some(text) = self.req.briefing_text() {
                blocks.push(json!({"type": "text", "text": text}));
            }
        }
        blocks.push(json!({"type": "text", "text": turn.text}));
        let images = *lock(&self.images);
        for image in &turn.images {
            let name = image
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if images {
                let bytes = std::fs::read(image)
                    .map_err(|e| AdapterError::Protocol(format!("couldn't read an image: {e}")))?;
                let mime = match image
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref()
                {
                    Some("jpg" | "jpeg") => "image/jpeg",
                    Some("gif") => "image/gif",
                    Some("webp") => "image/webp",
                    _ => "image/png",
                };
                blocks.push(
                    json!({"type": "image", "mimeType": mime, "data": STANDARD.encode(bytes)}),
                );
            } else {
                blocks.push(json!({"type": "resource_link", "uri": format!("file://{}", image.display()), "name": name}));
            }
        }
        for file in &turn.files {
            let path = self.req.cwd.join(file);
            blocks.push(json!({"type": "resource_link", "uri": format!("file://{}", path.display()), "name": file}));
        }
        Ok(Value::Array(blocks))
    }

    /// `fs/read_text_file`, inside the thread's folder.
    fn read_file(
        &self,
        path: &str,
        line: Option<u64>,
        limit: Option<u64>,
    ) -> Result<Value, String> {
        let full = contain(&self.req.cwd, Path::new(path), ContainPolicy::Read)
            .map_err(|e| e.to_string())?;
        let text = std::fs::read_to_string(&full).map_err(|e| e.to_string())?;
        let skip = line.map_or(0, |l| l.saturating_sub(1) as usize);
        let content = match (line, limit) {
            (None, None) => text,
            _ => text
                .lines()
                .skip(skip)
                .take(limit.map_or(usize::MAX, |l| l as usize))
                .collect::<Vec<_>>()
                .join("\n"),
        };
        Ok(json!({"content": content}))
    }

    /// `fs/write_text_file`, inside the thread's folder and never at Read access.
    fn write_file(&self, path: &str, content: &str) -> Result<Value, String> {
        if self.req.access == Access::Read {
            return Err("This thread has Read access: files can't be changed.".into());
        }
        let full = contain(&self.req.cwd, Path::new(path), ContainPolicy::Write)
            .map_err(|e| e.to_string())?;
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&full, content).map_err(|e| e.to_string())?;
        Ok(Value::Null)
    }

    async fn read(self, mut parser: AcpParser, mut lines: mpsc::Receiver<ProcessLine>) {
        while let Some(line) = lines.recv().await {
            let msg = match line {
                ProcessLine::Line(l) => match serde_json::from_str::<Value>(&l) {
                    Ok(v) => v,
                    Err(_) => continue,
                },
                ProcessLine::TooLong => continue,
            };
            // An answer to one of our requests.
            if msg.get("method").is_none() {
                if let Some(id) = msg.get("id").and_then(Value::as_i64) {
                    if let Some(tx) = lock(&self.pending).remove(&id) {
                        let answer = match msg.get("error") {
                            Some(e) => Err(RpcError {
                                code: e.get("code").and_then(Value::as_i64).unwrap_or(0),
                                message: e
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                            }),
                            None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        let _ = tx.send(answer);
                    }
                }
                continue;
            }
            for out in parser.message(&msg) {
                let turn = lock(&self.turn).clone();
                match out {
                    Acp::Event(event) => {
                        if let Some(turn_id) = turn {
                            self.emit(&turn_id, event).await;
                        }
                    }
                    Acp::Approval {
                        rpc_id,
                        kind,
                        summary,
                        detail,
                        subject,
                        options,
                    } => {
                        let read_only = self.req.access == Access::Read
                            && matches!(kind, ApprovalKind::FileWrite | ApprovalKind::FileDelete);
                        let (Some(turn_id), false) = (turn, read_only) else {
                            // Nothing to show it in, or a change Read access
                            // doesn't allow: decline.
                            let deny = ApprovalDecision::Deny { reason: None };
                            let outcome = match choose(&options, &deny) {
                                Some(id) => {
                                    json!({"outcome": {"outcome": "selected", "optionId": id}})
                                }
                                None => json!({"outcome": {"outcome": "cancelled"}}),
                            };
                            self.answer(rpc_id, outcome).await;
                            continue;
                        };
                        let id = ApprovalId::new();
                        lock(&self.approvals).insert(id.as_str().to_string(), (rpc_id, options));
                        let _ = self
                            .events
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
                    Acp::Read {
                        rpc_id,
                        path,
                        line,
                        limit,
                    } => match self.read_file(&path, line, limit) {
                        Ok(v) => self.answer(rpc_id, v).await,
                        Err(e) => self.refuse(rpc_id, -32602, &e).await,
                    },
                    Acp::Write {
                        rpc_id,
                        path,
                        content,
                    } => match self.write_file(&path, &content) {
                        Ok(v) => self.answer(rpc_id, v).await,
                        Err(e) => self.refuse(rpc_id, -32602, &e).await,
                    },
                    Acp::Unsupported { rpc_id, method } => {
                        self.refuse(rpc_id, -32601, &format!("{method} isn't supported"))
                            .await;
                    }
                }
            }
        }
        // Requests still waiting won't be answered.
        lock(&self.pending).clear();
        self.process.wait_exit(Duration::from_secs(2)).await;
        let _ = self
            .events
            .send(AdapterEvent::Exited {
                code: self.process.exit_code().flatten(),
                stderr_tail: self.process.stderr_tail(),
            })
            .await;
    }

    /// Answers every open permission request as cancelled (after a stop).
    async fn cancel_approvals(&self) {
        let open: Vec<Value> = lock(&self.approvals)
            .drain()
            .map(|(_, (rpc_id, _))| rpc_id)
            .collect();
        for rpc_id in open {
            self.answer(rpc_id, json!({"outcome": {"outcome": "cancelled"}}))
                .await;
        }
    }
}

#[async_trait]
impl ChatSession for AcpSession {
    fn engine_session_id(&self) -> Option<String> {
        lock(&self.session_id).clone()
    }

    async fn send_turn(&self, turn: TurnInput) -> Result<TurnId, AdapterError> {
        let session = lock(&self.session_id).clone().ok_or(AdapterError::Closed)?;
        let prompt = self.prompt(&turn)?;
        *lock(&self.turn) = Some(turn.turn_id.clone());
        let note = lock(&self.note).take();
        if let Some(text) = note {
            self.emit(&turn.turn_id, ThreadEvent::Note { text }).await;
        }
        self.running.send_replace(true);
        let rx = self
            .call(
                "session/prompt",
                json!({"sessionId": session, "prompt": prompt}),
            )
            .await
            .inspect_err(|_| {
                self.running.send_replace(false);
            })?;
        let me = self.clone();
        let turn_id = turn.turn_id.clone();
        tokio::spawn(async move {
            let ends = match rx.await {
                Ok(Ok(result)) => stopped(&me.label, &result),
                Ok(Err(e)) => prompt_failed(
                    &me.label,
                    e.code,
                    &e.message,
                    &me.limits,
                    limits::local(chrono::Utc::now()),
                ),
                // The agent went away: its exit reports the turn.
                Err(_) => Vec::new(),
            };
            for event in ends {
                me.emit(&turn_id, event).await;
            }
            let mut current = lock(&me.turn);
            if current.as_ref() == Some(&turn_id) {
                *current = None;
            }
            drop(current);
            me.running.send_replace(false);
        });
        Ok(turn.turn_id)
    }

    async fn interrupt(&self, _turn: &TurnId) -> Result<InterruptOutcome, AdapterError> {
        let session = lock(&self.session_id).clone();
        let me = self.clone();
        Ok(self
            .process
            .stop(async move {
                let Some(session) = session else {
                    return false;
                };
                if me
                    .send(json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session}}))
                    .await
                    .is_err()
                {
                    return false;
                }
                me.cancel_approvals().await;
                // Acknowledged when the prompt answers (with `cancelled`).
                let mut rx = me.running.subscribe();
                let answered = rx.wait_for(|running| !running).await.is_ok();
                answered
            })
            .await)
    }

    async fn resolve_approval(
        &self,
        id: &ApprovalId,
        decision: ApprovalDecision,
    ) -> Result<(), AdapterError> {
        let Some((rpc_id, options)) = lock(&self.approvals).remove(id.as_str()) else {
            return Ok(());
        };
        let outcome = match choose(&options, &decision) {
            Some(option) => json!({"outcome": {"outcome": "selected", "optionId": option}}),
            None => json!({"outcome": {"outcome": "cancelled"}}),
        };
        self.send(json!({"jsonrpc": "2.0", "id": rpc_id, "result": outcome}))
            .await
    }

    async fn set_model(
        &self,
        model: Option<String>,
        _effort: Option<String>,
    ) -> Result<(), AdapterError> {
        let (Some(session), Some(model)) = (lock(&self.session_id).clone(), model) else {
            return Ok(());
        };
        self.request(
            "session/set_model",
            json!({"sessionId": session, "modelId": model}),
        )
        .await
        .map(|_| ())
        .map_err(|e| AdapterError::Protocol(e.message))
    }

    async fn shutdown(&self) -> Result<(), AdapterError> {
        self.process.kill_tree().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
