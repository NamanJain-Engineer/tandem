//! `tandem_approvals__request` (ARCHITECTURE §6.8.1, F-TH-04 AC4): Claude's
//! permission prompt tool. Claude sends `{tool_name, input, tool_use_id?}`;
//! the answer is `{"behavior":"allow","updatedInput":…}` or
//! `{"behavior":"deny","message":…}`. Who decides is the app's
//! [`ApprovalAsker`] (thread allow rules, then the approval card).

use std::sync::Arc;

use async_trait::async_trait;
use rmcp::model::Tool;
use serde_json::{json, Value};

use crate::lease::Caller;

pub const APPROVALS_TOOL: &str = "tandem_approvals__request";
/// What Claude is told when the user (or the 30-minute expiry) says no.
const DENIED: &str = "Denied by the user";

/// One tool use the engine asks about.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolApproval {
    pub tool_name: String,
    pub input: Value,
    pub tool_use_id: Option<String>,
}

impl ToolApproval {
    pub fn from_args(args: &Value) -> Option<Self> {
        Some(Self {
            tool_name: args.get("tool_name")?.as_str()?.to_string(),
            input: args.get("input").cloned().unwrap_or_else(|| json!({})),
            tool_use_id: args
                .get("tool_use_id")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalAnswer {
    Allow,
    Deny { message: Option<String> },
}

impl ApprovalAnswer {
    /// The tool result text Claude expects.
    pub fn reply(&self, ask: &ToolApproval) -> String {
        match self {
            ApprovalAnswer::Allow => json!({"behavior": "allow", "updatedInput": ask.input}),
            ApprovalAnswer::Deny { message } => json!({
                "behavior": "deny",
                "message": message.as_deref().unwrap_or(DENIED),
            }),
        }
        .to_string()
    }
}

/// Decides a tool use for a caller. Blocks until someone decides (or the
/// request expires: a deny).
#[async_trait]
pub trait ApprovalAsker: Send + Sync {
    async fn ask(&self, caller: &Caller, ask: ToolApproval) -> ApprovalAnswer;
}

pub(crate) fn tool() -> Tool {
    let schema = json!({
        "type": "object",
        "properties": {
            "tool_name": {"type": "string"},
            "input": {"type": "object"},
            "tool_use_id": {"type": "string"}
        },
        "required": ["tool_name", "input"]
    });
    let schema = match schema {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    Tool::new(
        APPROVALS_TOOL,
        "Asks the Vibezeo user to allow or deny a tool use.",
        Arc::new(schema),
    )
}
