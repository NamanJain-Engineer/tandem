//! The tracker's records (ARCHITECTURE §9.2). One shape everywhere: the
//! files under `$DATA/tasks/`, the MCP tools and the app's IPC all use these
//! snake_case fields (D-900).

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Issue ids are `TAN-<n>`.
pub const ISSUE_PREFIX: &str = "TAN";
/// The local human (the app's user).
pub const HUMAN_ACTOR: &str = "me";
pub const TITLE_MAX: usize = 200;
pub const DESCRIPTION_MAX: usize = 50_000;
pub const COMMENT_MAX: usize = 20_000;
pub const IDEMPOTENCY_KEY_MAX: usize = 128;
/// An engine id (`claude`, `gemini-cli`): what `suggested_engine` holds.
pub const ENGINE_ID_MAX: usize = 64;

/// Lowercase letters, digits and `-`, 1–64 characters.
pub fn valid_engine_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= ENGINE_ID_MAX
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum IssueStatus {
    Backlog,
    Todo,
    InProgress,
    InReview,
    Done,
    Canceled,
}

impl IssueStatus {
    pub const ALL: [IssueStatus; 6] = [
        IssueStatus::Backlog,
        IssueStatus::Todo,
        IssueStatus::InProgress,
        IssueStatus::InReview,
        IssueStatus::Done,
        IssueStatus::Canceled,
    ];

    pub fn id(self) -> &'static str {
        match self {
            IssueStatus::Backlog => "backlog",
            IssueStatus::Todo => "todo",
            IssueStatus::InProgress => "in_progress",
            IssueStatus::InReview => "in_review",
            IssueStatus::Done => "done",
            IssueStatus::Canceled => "canceled",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            IssueStatus::Backlog => "Backlog",
            IssueStatus::Todo => "Todo",
            IssueStatus::InProgress => "In progress",
            IssueStatus::InReview => "In review",
            IssueStatus::Done => "Done",
            IssueStatus::Canceled => "Canceled",
        }
    }

    /// The common tracker grouping: unstarted, started, completed, canceled.
    pub fn kind(self) -> &'static str {
        match self {
            IssueStatus::Backlog => "backlog",
            IssueStatus::Todo => "unstarted",
            IssueStatus::InProgress | IssueStatus::InReview => "started",
            IssueStatus::Done => "completed",
            IssueStatus::Canceled => "canceled",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == raw)
    }
}

/// One execution of an issue (F-TK-04); the app's `task_attempts` row has
/// the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "TaskAttemptRecord")]
pub struct AttemptRecord {
    pub id: String,
    pub started_at: i64,
    #[serde(default)]
    pub ended_at: Option<i64>,
    pub state: String,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Issue {
    /// `TAN-<n>`
    pub id: String,
    pub number: u64,
    pub revision: u64,
    pub title: String,
    /// Markdown.
    pub description: String,
    pub status: IssueStatus,
    /// 0 none, 1 urgent … 4 low.
    pub priority: u8,
    pub labels: Vec<String>,
    pub project: Option<String>,
    /// An actor id (`me`, or a teammate's id).
    pub assignee: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived: bool,
    /// The Markdown summary of the work (F-TK-04).
    pub result: Option<String>,
    pub attempts: Vec<AttemptRecord>,
    /// The engine a plan suggested for it (F-TK-06): Execute starts there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub suggested_engine: Option<String>,
}

/// A row in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IssueSummary {
    pub id: String,
    pub number: u64,
    pub revision: u64,
    pub title: String,
    pub status: IssueStatus,
    pub priority: u8,
    pub labels: Vec<String>,
    pub project: Option<String>,
    pub assignee: Option<String>,
    pub updated_at: i64,
    pub archived: bool,
}

impl From<&Issue> for IssueSummary {
    fn from(i: &Issue) -> Self {
        Self {
            id: i.id.clone(),
            number: i.number,
            revision: i.revision,
            title: i.title.clone(),
            status: i.status,
            priority: i.priority,
            labels: i.labels.clone(),
            project: i.project.clone(),
            assignee: i.assignee.clone(),
            updated_at: i.updated_at,
            archived: i.archived,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "TaskComment")]
pub struct Comment {
    pub id: String,
    pub issue_id: String,
    pub ts: i64,
    pub actor: String,
    pub body: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

/// One changed field (`history.jsonl`, append-only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, rename = "TaskHistoryEntry")]
pub struct HistoryEntry {
    pub ts: i64,
    pub actor: String,
    #[serde(default)]
    pub connection: Option<String>,
    pub field: String,
    #[ts(type = "unknown")]
    pub from: Value,
    #[ts(type = "unknown")]
    pub to: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "TaskProject")]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "TaskLabel")]
pub struct Label {
    pub id: String,
    pub name: String,
    /// `#rrggbb`
    pub color: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "TaskActorKind")]
pub enum ActorKind {
    Human,
    Teammate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "TaskActor")]
pub struct Actor {
    pub id: String,
    pub kind: ActorKind,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "TaskTeam")]
pub struct Team {
    pub id: String,
    pub name: String,
}

/// `meta.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "TasksMeta")]
pub struct Meta {
    pub version: u32,
    pub next_number: u64,
    pub projects: Vec<Project>,
    pub labels: Vec<Label>,
    pub statuses: Vec<IssueStatus>,
    pub actors: Vec<Actor>,
    pub teams: Vec<Team>,
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            version: 1,
            next_number: 1,
            projects: Vec::new(),
            labels: Vec::new(),
            statuses: IssueStatus::ALL.to_vec(),
            actors: vec![Actor {
                id: HUMAN_ACTOR.into(),
                kind: ActorKind::Human,
                name: "You".into(),
            }],
            teams: vec![Team {
                id: "local".into(),
                name: "Local".into(),
            }],
        }
    }
}

/// `Some(None)` for an explicit `null`, `None` when the field is absent.
fn nullable<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

/// `save_issue`: create (no `id`) or patch. Absent fields stay as they are;
/// `assignee`/`project: null` clears them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SaveIssue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<IssueStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub priority: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub labels: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string | null")]
    pub project: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string | null")]
    pub assignee: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[ts(optional, type = "string | null")]
    pub suggested_engine: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub idempotency_key: Option<String>,
}

/// `list_issues` filters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IssueFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<Vec<IssueStatus>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub text: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub include_archived: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, rename = "TaskPage")]
pub struct Page<T: TS> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

/// Who writes: an actor id and, for agents, the MCP connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Writer {
    pub actor: String,
    pub connection: Option<String>,
}

impl Writer {
    pub fn human() -> Self {
        Self {
            actor: HUMAN_ACTOR.into(),
            connection: None,
        }
    }

    pub fn agent(actor: impl Into<String>, connection: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            connection: Some(connection.into()),
        }
    }
}

/// `connections.json`: one per engine session (F-TK-02).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "TaskConnection")]
pub struct Connection {
    pub id: String,
    pub created_at: i64,
    pub label: String,
    /// The actor its writes are recorded as.
    pub actor: String,
    #[serde(default)]
    pub revoked_at: Option<i64>,
    /// The working tree the session runs in, when it's in a git repository:
    /// what its claims are made for (F-TK-05).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ClaimScope>,
}

/// Paths in a claim (F-TK-05): at most this many per call.
pub const CLAIM_PATHS_MAX: usize = 50;
pub const CLAIM_PATH_MAX: usize = 512;
pub const CLAIM_INTENT_MAX: usize = 500;
/// A working tree keeps at most this many claims; its oldest inferred ones
/// go first.
pub const CLAIMS_PER_FOLDER_MAX: usize = 500;

/// Where a session works: the repository (its main checkout's top folder)
/// and the working tree (a lane's worktree, or the main checkout itself).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, rename = "TaskClaimScope")]
pub struct ClaimScope {
    pub repo: String,
    pub folder: String,
    /// The session's folder relative to `folder`, `/`-separated; empty at
    /// the top. An agent's relative paths start there.
    #[serde(default)]
    pub prefix: String,
    /// `folder` is the repository's main checkout, not a linked worktree.
    #[serde(default)]
    pub main: bool,
}

/// `claims.json`: one path a working tree's agent said (or was seen) it
/// is changing (F-TK-05). A claim belongs to the working tree, not the
/// session: a relaunched agent keeps its lane's claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "TaskClaim")]
pub struct Claim {
    /// Relative to the working tree, `/`-separated; a folder ends in `/`.
    pub path: String,
    /// What the agent says it's doing. Another agent's intent is untrusted.
    pub intent: String,
    pub repo: String,
    pub folder: String,
    /// The working tree's name (a lane's slug), for notices.
    pub agent: String,
    /// The connection that made it; `None` for one the app inferred.
    #[serde(default)]
    pub connection: Option<String>,
    /// Seen in an edit, not claimed with the tool.
    pub inferred: bool,
    pub at: i64,
    /// Made in the main checkout (its scope's `main`).
    #[serde(default)]
    pub main: bool,
}

/// Two working trees of one repository claiming the same files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "TaskClaimOverlap")]
pub struct ClaimOverlap {
    pub repo: String,
    /// The two working trees, in path order.
    pub folders: [String; 2],
    /// Their names, in the same order.
    pub agents: [String; 2],
    /// The overlapping paths (the more specific of each pair), sorted.
    pub paths: Vec<String>,
}

pub fn issue_id(number: u64) -> String {
    format!("{ISSUE_PREFIX}-{number}")
}

/// `TAN-12` → 12.
pub fn parse_issue_id(id: &str) -> Option<u64> {
    let n = id.strip_prefix(ISSUE_PREFIX)?.strip_prefix('-')?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) || n.starts_with('0') {
        return None;
    }
    n.parse().ok()
}
