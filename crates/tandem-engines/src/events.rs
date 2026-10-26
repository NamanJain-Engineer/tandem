//! The normalized thread event stream (F-TH-03, ARCHITECTURE §6.9). Every
//! chat adapter emits these; the UI renders nothing else.

use serde::{Deserialize, Serialize};
use tandem_core::approvals::{ApprovalKind, ApprovalScope};
use tandem_core::ids::{ApprovalId, AttachmentId, MailId, ThreadId, TurnId};
use ts_rs::TS;

/// Tool output kept inline; the full text goes to a file (F-TH-03).
pub const TOOL_OUTPUT_MAX: usize = 64 * 1024;

/// One event of one turn, in order (`seq` counts per turn).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ThreadEventEnvelope {
    pub thread_id: ThreadId,
    pub turn_id: TurnId,
    pub seq: u64,
    pub ts: i64,
    pub event: ThreadEvent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum ThreadEvent {
    User {
        text: String,
        attachments: Vec<AttachmentId>,
    },
    Reasoning {
        text: String,
        duration_ms: Option<u64>,
    },
    ReasoningDelta {
        text: String,
    },
    Tool {
        id: String,
        verb: ToolVerb,
        arg_summary: String,
        status: ToolStatus,
        output: Option<String>,
        output_truncated: bool,
        output_file: Option<AttachmentId>,
        diff: Option<String>,
        detail: Option<String>,
        /// The files an edit names, absolute: what the thread's working
        /// tree claims (F-TK-07). Left out for other tools.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        paths: Option<Vec<String>>,
    },
    AssistantDelta {
        text: String,
    },
    /// A finished assistant block (deltas compacted).
    Assistant {
        markdown: String,
    },
    ApprovalRequest {
        id: ApprovalId,
        kind: ApprovalKind,
        summary: String,
        detail: String,
        options: Vec<ApprovalScope>,
        expires_at: i64,
    },
    ApprovalResolved {
        id: ApprovalId,
        decision: ApprovalDecisionView,
    },
    Usage {
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
        context_fill_pct: Option<f32>,
        cost_usd: Option<f64>,
    },
    Limit {
        kind: LimitKind,
        resets_at: Option<i64>,
        message: String,
    },
    Note {
        text: String,
    },
    Error {
        code: String,
        message: String,
        stage: ErrorStage,
        hint: Option<String>,
        action: Option<ErrorAction>,
    },
    PlanRevision {
        version: u32,
    },
    /// "Continued on <engine> (<account>) with a summary".
    Divider {
        text: String,
    },
    MailRow {
        mail_id: MailId,
        from: String,
        to: String,
        preview: String,
        /// The recipient's delivered turn: shown in place of its message.
        inbound: bool,
    },
    TurnEnd {
        reason: TurnEndReason,
        blocked_actions: u32,
    },
}

impl ThreadEvent {
    /// The serialized tag (`thread_messages.type`).
    pub fn tag(&self) -> &'static str {
        match self {
            ThreadEvent::User { .. } => "user",
            ThreadEvent::Reasoning { .. } => "reasoning",
            ThreadEvent::ReasoningDelta { .. } => "reasoning_delta",
            ThreadEvent::Tool { .. } => "tool",
            ThreadEvent::AssistantDelta { .. } => "assistant_delta",
            ThreadEvent::Assistant { .. } => "assistant",
            ThreadEvent::ApprovalRequest { .. } => "approval_request",
            ThreadEvent::ApprovalResolved { .. } => "approval_resolved",
            ThreadEvent::Usage { .. } => "usage",
            ThreadEvent::Limit { .. } => "limit",
            ThreadEvent::Note { .. } => "note",
            ThreadEvent::Error { .. } => "error",
            ThreadEvent::PlanRevision { .. } => "plan_revision",
            ThreadEvent::Divider { .. } => "divider",
            ThreadEvent::MailRow { .. } => "mail_row",
            ThreadEvent::TurnEnd { .. } => "turn_end",
        }
    }

    /// Streaming pieces: shown live, never stored as rows.
    pub fn is_delta(&self) -> bool {
        matches!(
            self,
            ThreadEvent::AssistantDelta { .. } | ThreadEvent::ReasoningDelta { .. }
        )
    }

    /// A tool row whose output is cut to [`TOOL_OUTPUT_MAX`]; the caller
    /// stores `full` elsewhere when `output_truncated`.
    pub fn tool(id: &str, verb: ToolVerb, arg_summary: &str, status: ToolStatus) -> ThreadEvent {
        ThreadEvent::Tool {
            id: id.to_string(),
            verb,
            arg_summary: arg_summary.to_string(),
            status,
            output: None,
            output_truncated: false,
            output_file: None,
            diff: None,
            detail: None,
            paths: None,
        }
    }
}

/// `path` as an absolute path: as given, or under `cwd` (F-TK-07).
pub fn absolute(cwd: &std::path::Path, path: &str) -> String {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        path.to_string()
    } else {
        cwd.join(p).to_string_lossy().into_owned()
    }
}

/// `output` cut to at most [`TOOL_OUTPUT_MAX`] bytes on a char boundary, and
/// whether it was cut.
pub fn truncate_output(output: &str) -> (String, bool) {
    if output.len() <= TOOL_OUTPUT_MAX {
        return (output.to_string(), false);
    }
    let mut end = TOOL_OUTPUT_MAX;
    while !output.is_char_boundary(end) {
        end -= 1;
    }
    (output[..end].to_string(), true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ToolVerb {
    Bash,
    Read,
    Grep,
    Glob,
    Edit,
    Write,
    Multiedit,
    Webfetch,
    Websearch,
    Task,
    Mcp,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ToolStatus {
    Pending,
    Running,
    Ok,
    Error,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum ApprovalDecisionView {
    Allow {
        scope: ApprovalScope,
    },
    Deny {
        reason: Option<String>,
    },
    /// Nobody answered within 30 minutes (counts as a deny).
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LimitKind {
    Session,
    Weekly,
    Rate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorStage {
    Spawn,
    Auth,
    Model,
    Network,
    Tool,
    Unknown,
}

/// Codes that mean the engine itself broke (F-EN-08 AC3): it couldn't
/// start, or its process died during a turn.
const ENGINE_FAULTS: &[&str] = &["spawn_failed", "engine_exited"];

impl ErrorStage {
    /// Whether an error with this stage and code says the engine itself is
    /// broken, and so counts against its version (F-EN-08, D-2501). An
    /// engine's generic error result, an API overload, being offline or
    /// signed out, a model the account can't use or a tool's own failure
    /// don't: the engine reported them, so it works.
    pub fn engine_fault(self, code: &str) -> bool {
        self == ErrorStage::Spawn || ENGINE_FAULTS.contains(&code)
    }
}

/// The button an error offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorAction {
    Retry,
    SummarizeContinue,
    SignIn,
    OpenTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TurnEndReason {
    Completed,
    Cancelled,
    Limit,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_th_03_events_serialize_with_snake_tags_and_camel_fields() {
        let e = ThreadEvent::TurnEnd {
            reason: TurnEndReason::Cancelled,
            blocked_actions: 1,
        };
        let v = serde_json::to_value(&e).expect("json");
        assert_eq!(
            v,
            serde_json::json!({"type": "turn_end", "reason": "cancelled", "blockedActions": 1})
        );
        assert_eq!(e.tag(), "turn_end");
        let tool = ThreadEvent::tool("t1", ToolVerb::Bash, "npm test", ToolStatus::Running);
        let v = serde_json::to_value(&tool).expect("json");
        assert_eq!(v["type"], "tool");
        assert_eq!(v["argSummary"], "npm test");
        assert_eq!(v["outputTruncated"], false);
        let back: ThreadEvent = serde_json::from_value(v).expect("back");
        assert_eq!(back, tool);
        assert!(ThreadEvent::AssistantDelta { text: "a".into() }.is_delta());
        assert!(!ThreadEvent::Assistant {
            markdown: "a".into()
        }
        .is_delta());
    }

    #[test]
    fn f_th_03_tool_output_is_cut_at_64_kb_on_a_char_boundary() {
        let short = "ok";
        assert_eq!(truncate_output(short), ("ok".to_string(), false));
        let long = "é".repeat(TOOL_OUTPUT_MAX); // 2 bytes each
        let (cut, truncated) = truncate_output(&long);
        assert!(truncated);
        assert!(cut.len() <= TOOL_OUTPUT_MAX);
        assert!(cut.len() >= TOOL_OUTPUT_MAX - 1);
    }
}
