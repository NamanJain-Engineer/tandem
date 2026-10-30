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
        /// A successful edit's lines added and removed, counted from its
        /// input by an engine that sends no diff (F-TH-12, D-2608). Left out
        /// when unknown, and for edits that didn't happen.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        lines: Option<LineCounts>,
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
            lines: None,
        }
    }
}

/// The lines an edit added and removed (F-TH-12, D-2608).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LineCounts {
    pub added: u32,
    pub removed: u32,
}

/// Line pairs compared exactly; past this, an edit's lines aren't counted.
const COMPARE_MAX: usize = 4_000_000;

impl LineCounts {
    /// A new file's lines, every one added.
    pub fn created(content: &str) -> Self {
        Self {
            added: count(content.lines().count()),
            removed: 0,
        }
    }

    /// The lines an edit that replaces the snippet `old` with `new` adds and
    /// removes, as git counts them. Both are split at each `\n`, keeping the
    /// piece after the last one, and compared by a line diff (the longest
    /// common subsequence of the pieces between the common head and tail), so
    /// an unchanged line between two changes doesn't count. That's exact when
    /// the snippet starts at a line's start and ends at a line's end, as edits
    /// usually do. When `old` ends with a newline, the line after the snippet
    /// joins whatever `new` leaves unfinished, so a line join counts the line
    /// it joined (taken to be a line of its own, not blank). An empty `old`
    /// writes a new file's content. `None` past [`COMPARE_MAX`] pairs: no
    /// count rather than a guess.
    pub fn edit(old: &str, new: &str) -> Option<Self> {
        if old.is_empty() {
            return Some(Self::created(new));
        }
        let joined = old.ends_with('\n');
        diff_lines(&pieces(old, joined), &pieces(new, joined))
    }

    /// What a patch's body adds and removes: each `+` and each `-` line
    /// (Claude Code's `structuredPatch` hunks carry no file headers).
    pub fn patch<'a>(lines: impl IntoIterator<Item = &'a str>) -> Self {
        let mut c = Self::default();
        for l in lines {
            if l.starts_with('+') {
                c.added = c.added.saturating_add(1);
            } else if l.starts_with('-') {
                c.removed = c.removed.saturating_add(1);
            }
        }
        c
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `s` split at each `\n`, each piece paired with whether it runs into the
/// line after the snippet (only the last, and only when `joined`).
fn pieces(s: &str, joined: bool) -> Vec<(&str, bool)> {
    let mut v: Vec<(&str, bool)> = s.split('\n').map(|p| (p, false)).collect();
    if let Some(last) = v.last_mut() {
        last.1 = joined;
    }
    v
}

/// `a` → `b` as lines added and removed: their common head and tail, then
/// the longest common subsequence of what's between. `None` past
/// [`COMPARE_MAX`] pairs.
fn diff_lines<T: PartialEq>(a: &[T], b: &[T]) -> Option<LineCounts> {
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[head..], &b[head..]);
    let tail = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a, b) = (&a[..a.len() - tail], &b[..b.len() - tail]);
    let common = if a.is_empty() || b.is_empty() {
        0
    } else if a.len().saturating_mul(b.len()) > COMPARE_MAX {
        return None;
    } else {
        common_lines(a, b)
    };
    Some(LineCounts {
        added: count(b.len() - common),
        removed: count(a.len() - common),
    })
}

impl std::ops::Add for LineCounts {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            added: self.added.saturating_add(other.added),
            removed: self.removed.saturating_add(other.removed),
        }
    }
}

impl std::iter::Sum for LineCounts {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), |a, b| a + b)
    }
}

/// The length of the longest common subsequence of `a` and `b`, in two
/// rows as long as the shorter side: within [`COMPARE_MAX`] pairs, at most
/// about 2,000 entries each.
fn common_lines<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut prev = vec![0; short.len() + 1];
    let mut cur = vec![0; short.len() + 1];
    for x in long {
        for (j, y) in short.iter().enumerate() {
            cur[j + 1] = if x == y {
                prev[j] + 1
            } else {
                prev[j + 1].max(cur[j])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[short.len()]
}

/// `path` as an absolute path: as given, or under `cwd` (F-TK-07), with
/// `.` and `..` worked out, so one file named two ways counts once. Only
/// the text: symbolic links aren't followed, since nothing reads the disk.
/// An absolute path in the other system's form (a Windows one here, a Unix
/// one on Windows) is kept as given: this computer can't join or walk it.
pub fn absolute(cwd: &std::path::Path, path: &str) -> String {
    use std::path::{Component, PathBuf};
    let p = std::path::Path::new(path);
    if is_absolute(path) && !p.is_absolute() {
        return path.to_string();
    }
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::CurDir => {}
            // Never above the root.
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out.to_string_lossy().into_owned()
}

/// Whether two absolute paths name one file. On a Windows path (a drive
/// letter or a `\\` share) case and slashes don't count, as they don't on
/// Windows; elsewhere the text is the file (F-TH-12, D-2622).
pub fn same_file(a: &str, b: &str) -> bool {
    if windows_path(a) && windows_path(b) {
        let key = |p: &str| p.replace('/', "\\").to_lowercase();
        key(a) == key(b)
    } else {
        a == b
    }
}

/// Whether `p` is absolute, in Unix's form or Windows', whatever this
/// computer is: an engine's events can come from another one.
pub fn is_absolute(p: &str) -> bool {
    p.starts_with('/') || windows_path(p)
}

/// A drive letter and a slash, or a `\\` share.
fn windows_path(p: &str) -> bool {
    let b = p.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
        || p.starts_with("\\\\")
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
    fn f_th_12_6_an_absolute_path_in_the_other_systems_form_is_kept_as_given() {
        let cwd = std::path::Path::new("/work/shop");
        // A Windows path read on Unix (and a Unix one read on Windows) isn't
        // joined under the folder: this computer can't walk it, so it's kept.
        assert_eq!(absolute(cwd, r"C:\work\a.ts"), r"C:\work\a.ts");
        assert_eq!(
            absolute(cwd, r"\\server\share\a.ts"),
            r"\\server\share\a.ts"
        );
        #[cfg(not(windows))]
        assert_eq!(absolute(cwd, "src/../a.ts"), "/work/shop/a.ts");
        #[cfg(windows)]
        assert_eq!(absolute(cwd, "/work/a.ts"), "/work/a.ts");
    }

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
