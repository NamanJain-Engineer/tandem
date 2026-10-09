//! The Codex app-server protocol (ARCHITECTURE §6.8.2, F-TH-05): which
//! versions Vibezeo speaks, and the request shapes. `V2` is the thread/turn
//! API; Codex versions that only have the legacy conversation API are below
//! the supported range and fail closed (D-117).
//!
//! The method and field names follow the vendor's app-server protocol and are
//! frozen in `tests/fixtures/codex/`; checking them against a real install
//! (`codex app-server generate-json-schema`) is in HUMAN_TODO.

use serde_json::{json, Value};
use tandem_core::access::{Access, ThreadMode};
use tandem_core::approvals::{ApprovalDecision, ApprovalScope};

use crate::chat::{ChatStart, TurnInput};

/// The oldest Codex with the thread/turn API Vibezeo speaks.
pub const MIN_VERSION: (u64, u64, u64) = (0, 40, 0);

/// What a Plan-mode thread tells Codex.
const PLAN_INSTRUCTIONS: &str = "Plan only: read what you need and describe the changes you would make as a numbered plan. Don't edit files or run commands that change anything.";

/// The first `x.y.z` in `s` ("codex-cli 0.47.0", "codex_cli_rs/0.47.0 (Linux)").
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    s.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter_map(|part| {
            let mut n = part.split('.').map(|x| x.parse::<u64>().ok());
            match (n.next(), n.next(), n.next()) {
                (Some(Some(a)), Some(Some(b)), Some(Some(c))) => Some((a, b, c)),
                _ => None,
            }
        })
        .next()
}

/// F-TH-05 AC3: an unknown or too old Codex is refused before anything is
/// garbled.
pub fn check_version(version: Option<&str>) -> Result<(), String> {
    let shown = version.unwrap_or("unknown");
    match version.and_then(parse_version) {
        Some(v) if v >= MIN_VERSION => Ok(()),
        _ => Err(format!(
            "Codex version {shown} isn't supported yet — update Vibezeo or Codex"
        )),
    }
}

/// `sandbox` and `approvalPolicy` for the session's resolved access
/// (`tandem_core::access::resolve`); Plan mode reads only.
pub fn policy(access: Access, mode: ThreadMode) -> (&'static str, &'static str) {
    let access = if mode == ThreadMode::Plan {
        Access::Read
    } else {
        access
    };
    match access {
        Access::Read => ("read-only", "on-request"),
        Access::Edits => ("workspace-write", "on-request"),
        Access::Full => ("danger-full-access", "never"),
    }
}

pub trait CodexProtocol: Send + Sync {
    fn start_thread(&self, req: &ChatStart) -> (&'static str, Value);
    fn resume_thread(&self, thread_id: &str, req: &ChatStart) -> (&'static str, Value);
    fn start_turn(
        &self,
        thread_id: &str,
        turn: &TurnInput,
        req: &ChatStart,
    ) -> (&'static str, Value);
    fn interrupt(&self, thread_id: &str, turn_id: &str) -> (&'static str, Value);
    /// The answer to an approval request.
    fn decision(&self, decision: &ApprovalDecision) -> Value;
}

pub struct V2;

impl V2 {
    fn thread_params(req: &ChatStart) -> Value {
        let (sandbox, approval) = policy(req.access, req.mode);
        let mut p = json!({
            "cwd": req.cwd,
            "approvalPolicy": approval,
            "sandbox": sandbox,
        });
        if let Some(model) = &req.model {
            p["model"] = json!(model);
        }
        // A teammate's briefing goes as developer instructions, read from
        // its file and sent over stdin (F-AG-06), never argv.
        let mut instructions: Vec<String> = req.briefing_text().into_iter().collect();
        if req.mode == ThreadMode::Plan {
            instructions.push(PLAN_INSTRUCTIONS.to_string());
        }
        if !instructions.is_empty() {
            p["developerInstructions"] = json!(instructions.join("\n\n"));
        }
        p
    }
}

impl CodexProtocol for V2 {
    fn start_thread(&self, req: &ChatStart) -> (&'static str, Value) {
        ("thread/start", Self::thread_params(req))
    }

    fn resume_thread(&self, thread_id: &str, req: &ChatStart) -> (&'static str, Value) {
        let mut p = Self::thread_params(req);
        p["threadId"] = json!(thread_id);
        ("thread/resume", p)
    }

    fn start_turn(
        &self,
        thread_id: &str,
        turn: &TurnInput,
        req: &ChatStart,
    ) -> (&'static str, Value) {
        let mut text = turn.text.clone();
        for f in &turn.files {
            text.push_str(&format!(" @{f}"));
        }
        let mut input = vec![json!({"type": "text", "text": text})];
        for image in &turn.images {
            input.push(json!({"type": "localImage", "path": image}));
        }
        let mut p = json!({"threadId": thread_id, "input": input});
        if let Some(model) = &req.model {
            p["model"] = json!(model);
        }
        if let Some(effort) = &req.effort {
            p["effort"] = json!(effort);
        }
        ("turn/start", p)
    }

    fn interrupt(&self, thread_id: &str, turn_id: &str) -> (&'static str, Value) {
        (
            "turn/interrupt",
            json!({"threadId": thread_id, "turnId": turn_id}),
        )
    }

    fn decision(&self, decision: &ApprovalDecision) -> Value {
        let d = match decision {
            ApprovalDecision::Allow {
                scope: ApprovalScope::Once,
            } => "accept",
            ApprovalDecision::Allow { .. } => "acceptForSession",
            ApprovalDecision::Deny { .. } => "decline",
        };
        json!({ "decision": d })
    }
}
