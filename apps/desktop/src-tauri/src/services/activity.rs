//! The activity ledger (F-EX-07, ARCHITECTURE §6.13): one timeline per agent
//! pane or thread. Events only the ledger knows (turns sent, tools,
//! subagents, pane approvals, `vibezeo run`) go in `activity`; commands,
//! checkpoints and thread approvals are read from their own tables. Nothing
//! stored here is prompt text, file contents, output or a secret value.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::clock::Clock;
use tandem_core::redact::Redactor;
use tandem_core::IpcError;
use tandem_store::repo::activity::{self as repo, Keep, NewActivity};
use tandem_store::Store;

/// One event of a session's ledger (`activity_list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActivityEvent {
    /// `turn` | `files` | `command` | `tool` | `subagent` | `approval` | `run`
    /// | `note` (events of a flood weren't kept)
    pub kind: String,
    pub at: i64,
    /// One line, redacted.
    pub summary: String,
    /// The checkpoint taken at or before it (a turn: only its own).
    pub checkpoint_id: Option<String>,
    /// That checkpoint's workspace, so the link opens its folder.
    pub checkpoint_workspace: Option<String>,
    /// A command's row in command recall.
    #[ts(type = "number | null")]
    pub command_id: Option<i64>,
    pub exit_code: Option<i32>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    /// The secrets a run used: names only.
    pub secrets: Vec<String>,
    /// Who decided a run or an approval: `once`, `agent`, `rule`,
    /// `allowed_before`, `declined`, `expired`, `cancelled`; or why it wasn't
    /// asked (`busy`, `too_many`); or an approval's state and where it was
    /// answered (`allowed:dashboard`), `client_gone` or `app_quit`.
    pub decided: Option<String>,
    /// The engine a turn went to (`claude`).
    pub engine: Option<String>,
    /// How a run ended other than by exiting, or why it didn't start:
    /// `client_gone` (`vibezeo run` stopped waiting), `app_quit`,
    /// `time_limit`, `folder_moved`, `secrets_unreadable`.
    pub ended: Option<String>,
}

impl ActivityEvent {
    fn new(kind: &str, at: i64, summary: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            at,
            summary: summary.into(),
            checkpoint_id: None,
            checkpoint_workspace: None,
            command_id: None,
            exit_code: None,
            duration_ms: None,
            secrets: Vec::new(),
            decided: None,
            engine: None,
            ended: None,
        }
    }
}

/// What a `vibezeo run` did, for its row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub command: String,
    pub secrets: Vec<String>,
    /// See [`ActivityEvent::decided`].
    pub decided: &'static str,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<i64>,
    /// See [`ActivityEvent::ended`].
    pub ended: Option<&'static str>,
    /// The command was spawned.
    pub started: bool,
    /// Who decided it: sets the row's tier.
    pub by: Decider,
}

/// Who made a run's or an answer's decision, as `agent_run` knows it. It
/// sets the row's [`Keep`] tier, so no string decides what's pruned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decider {
    /// The person: their Allow or Deny, or their question running out or
    /// being cancelled.
    Person,
    /// A rule or an earlier "Allow for this agent", without asking.
    Unasked,
    /// Nobody: refused before asking, or ended by the agent going away or
    /// the app quitting.
    Nobody,
}

/// `activity_list`: a window of a session's events, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActivityPage {
    pub events: Vec<ActivityEvent>,
    /// Earlier events exist: ask again with `before` set to the first one's
    /// `at` and `skip` to how many shown events share that `at`.
    pub more: bool,
}

/// Events per page.
pub const PAGE: i64 = 500;

/// The summary of a pane approval: what kind of question, never its text
/// (the run's own row carries the redacted line).
pub const APPROVAL_SUMMARY: &str = "Asked to run a command with secrets";

/// A tool's name as engines report it (`Bash`, `mcp__github__search`);
/// anything else is dropped, since any process in the pane can sign a report.
pub fn tool_name(tool: &str) -> Option<&str> {
    (!tool.is_empty()
        && tool.len() <= 64
        && tool
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c)))
    .then_some(tool)
}

/// `pane` or `thread`.
fn kind_of(source: &str) -> Result<&'static str, IpcError> {
    match source {
        "pane" => Ok("pane"),
        "thread" => Ok("thread"),
        _ => Err(IpcError::new(
            tandem_core::ErrorCode::Invalid,
            "Activity is kept for agent panes and threads",
        )),
    }
}

/// A row's summary is at most this many bytes: a command line can be long,
/// and a rule lets an agent write many runs.
pub const MAX_SUMMARY_BYTES: usize = 1024;

/// `summary` cut to [`MAX_SUMMARY_BYTES`] at a character boundary, with "…".
fn clip_summary(summary: String) -> String {
    if summary.len() <= MAX_SUMMARY_BYTES {
        return summary;
    }
    let mut end = MAX_SUMMARY_BYTES - '…'.len_utf8();
    while !summary.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &summary[..end])
}

pub struct ActivityService {
    store: Store,
    clock: Arc<dyn Clock>,
    /// Per pane and kind of flood: this minute's window.
    rates: std::sync::Mutex<std::collections::HashMap<(String, Flood), Window>>,
}

/// What an agent can make the ledger write as often as it likes: tool
/// reports and prompt reports are signed with the pane's secret, which the
/// agent holds, and it can ask for runs that are refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flood {
    Tool,
    /// A turn counted at Enter in the pane.
    Turn,
    /// A turn from the engine's prompt report, which the agent can sign.
    ReportedTurn,
    Refusal,
}

impl Flood {
    /// Rows kept per pane per minute.
    pub const fn limit(self) -> u32 {
        match self {
            Flood::Tool => 240,
            Flood::Turn | Flood::ReportedTurn => 60,
            Flood::Refusal => 30,
        }
    }

    fn what(self) -> &'static str {
        match self {
            Flood::Tool => "tool events",
            Flood::Turn | Flood::ReportedTurn => "turns",
            Flood::Refusal => "runs that didn't start",
        }
    }
}

/// Tool and subagent rows kept per pane per minute.
pub const TOOL_ROWS_PER_MINUTE: u32 = Flood::Tool.limit();

/// A minute of one pane's rows of one [`Flood`], by the app's clock.
#[derive(Debug, Clone, Copy)]
struct Window {
    start: i64,
    kept: u32,
    dropped: u32,
}

/// Whether a row is written, and the notes that come with it.
struct Admit {
    keep: bool,
    notes: Vec<String>,
}

impl ActivityService {
    pub fn new(store: Store, clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            store,
            clock,
            rates: Default::default(),
        })
    }

    /// Whether this pane may write another row of `flood` this minute. The
    /// first one over the limit brings a note at once (a flood followed by
    /// silence, or a quit, still leaves it); the next minute's first row
    /// brings one with the number dropped.
    fn admit(&self, pane: &str, flood: Flood) -> Admit {
        let now = self.clock.now_ms();
        let Ok(mut rates) = self.rates.lock() else {
            return Admit {
                keep: false,
                notes: Vec::new(),
            };
        };
        let w = rates.entry((pane.to_string(), flood)).or_insert(Window {
            start: now,
            kept: 0,
            dropped: 0,
        });
        let mut notes = Vec::new();
        // A clock that stepped back starts a new minute rather than one
        // that never ends.
        if now < w.start {
            w.start = now;
        }
        if now - w.start >= 60_000 {
            if w.dropped > 0 {
                notes.push(format!(
                    "{} more {} in that minute weren't kept",
                    w.dropped,
                    flood.what()
                ));
            }
            *w = Window {
                start: now,
                kept: 0,
                dropped: 0,
            };
        }
        if w.kept < flood.limit() {
            w.kept += 1;
            return Admit { keep: true, notes };
        }
        w.dropped += 1;
        if w.dropped == 1 {
            notes.push(format!(
                "Over {} {} in a minute: the rest of the minute's aren't kept",
                flood.limit(),
                flood.what()
            ));
        }
        Admit { keep: false, notes }
    }

    /// `row` if `flood` allows it, and its notes: [`Keep::Ceiling`], so a
    /// later flood of counted rows (tools, turns) can't prune them; they
    /// stop at their own ceiling instead, per source and in all.
    async fn record_limited(&self, flood: Flood, row: NewActivity) {
        let admit = self.admit(&row.source_id, flood);
        for note in admit.notes {
            self.record(NewActivity {
                source_kind: row.source_kind,
                source_id: row.source_id.clone(),
                kind: "note",
                at: self.clock.now_ms(),
                summary: note,
                detail: serde_json::json!({}),
                checkpoint_id: None,
                kept: Keep::Ceiling,
            })
            .await;
        }
        if admit.keep {
            self.record(row).await;
        }
    }

    async fn record(&self, mut row: NewActivity) {
        row.summary = clip_summary(row.summary);
        if let Err(err) = repo::insert(&self.store, row).await {
            tracing::warn!(code = "activity.store_failed", error = %err, "an activity event wasn't recorded");
        }
    }

    /// A turn went to a thread's engine, or a prompt to an agent pane's,
    /// at `at` (taken before its checkpoint). Never its text.
    ///
    /// A pane's turn is `reported` when it comes from the engine's prompt
    /// report (signed with the pane's secret, so the agent can send it),
    /// else from Enter in the pane; each has its own [`Flood`] cap.
    pub async fn turn(
        &self,
        source_kind: &'static str,
        id: &str,
        engine: Option<&str>,
        checkpoint: Option<String>,
        at: i64,
        reported: bool,
    ) {
        let row = NewActivity {
            source_kind,
            source_id: id.to_string(),
            kind: "turn",
            at,
            summary: "Turn sent".into(),
            detail: serde_json::json!({ "engine": engine.and_then(tool_name) }),
            checkpoint_id: checkpoint,
            kept: Keep::Counted,
        };
        // A pane's turns come from reports the agent can sign.
        if source_kind == "pane" {
            let flood = if reported {
                Flood::ReportedTurn
            } else {
                Flood::Turn
            };
            self.record_limited(flood, row).await;
        } else {
            self.record(row).await;
        }
    }

    /// The clock the ledger dates events by.
    pub fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// An agent pane's engine started a tool (its name, from its hooks), at
    /// the report's time. A name that isn't a tool's is dropped.
    pub async fn tool(&self, pane: &str, tool: &str, subagent: bool, at: i64) {
        let Some(tool) = tool_name(tool) else {
            return;
        };
        self.record_limited(
            Flood::Tool,
            NewActivity {
                source_kind: "pane",
                source_id: pane.to_string(),
                kind: if subagent { "subagent" } else { "tool" },
                at,
                summary: if subagent {
                    format!("Subagent started ({tool})")
                } else {
                    tool.to_string()
                },
                detail: serde_json::json!({ "tool": tool }),
                checkpoint_id: None,
                kept: Keep::Counted,
            },
        )
        .await;
    }

    /// The person answered a pane's approval (`decided` as in
    /// [`ActivityEvent::decided`]). A fixed summary: the question's text
    /// could carry what the agent typed before the redactor knew the values.
    ///
    /// Every answer is out of the counts (each follows a question, at most
    /// 10 a minute per pane): the person's ([`Decider::Person`]) is a
    /// [`Keep::Decision`]; one the agent caused by going away or the app's
    /// quit is [`Keep::Ceiling`].
    pub async fn approval(&self, pane: &str, decided: &str, by: Decider) {
        let person = by == Decider::Person;
        self.record(NewActivity {
            source_kind: "pane",
            source_id: pane.to_string(),
            kind: "approval",
            at: self.clock.now_ms(),
            summary: APPROVAL_SUMMARY.into(),
            detail: serde_json::json!({ "decided": decided }),
            checkpoint_id: None,
            kept: if person {
                Keep::Decision
            } else {
                Keep::Ceiling
            },
        })
        .await;
    }

    /// A `vibezeo run` ended, or was refused after its decision. Its tier
    /// comes from [`RunRecord::by`]: the person's decision (their Allow,
    /// started or not, their Deny, a question run out or cancelled) is a
    /// [`Keep::Decision`], by age only; a run a rule or allowance started
    /// is a [`Keep::Ceiling`] (also capped at 30 a minute in `agent_run`);
    /// anything else that didn't start (busy, too many, gone without an
    /// answer) is a [`Flood::Refusal`].
    pub async fn run(&self, pane: &str, run: RunRecord) {
        let kept = match (run.by, run.started) {
            // The person's Allow (started or not), Deny, or question run
            // out or cancelled.
            (Decider::Person, _) => Keep::Decision,
            // Started without asking: the agent drives these, at most 30 a
            // minute per pane.
            (Decider::Unasked, true) => Keep::Ceiling,
            // Didn't start, and nobody decided: a refusal.
            _ => Keep::Counted,
        };
        let row = NewActivity {
            source_kind: "pane",
            source_id: pane.to_string(),
            kind: "run",
            at: self.clock.now_ms(),
            summary: Redactor::global().redact(&run.command).into_owned(),
            detail: serde_json::json!({
                "secrets": run.secrets,
                "decided": run.decided,
                "exit_code": run.exit_code,
                "duration_ms": run.duration_ms,
                "ended": run.ended,
            }),
            checkpoint_id: None,
            kept,
        };
        if kept == Keep::Counted {
            self.record_limited(Flood::Refusal, row).await;
        } else {
            self.record(row).await;
        }
    }

    /// Drops old events (at startup and hourly).
    pub async fn prune(&self) {
        if let Err(err) = repo::prune(&self.store, self.clock.now_ms()).await {
            tracing::warn!(code = "activity.prune_failed", error = %err, "activity wasn't pruned");
        }
    }

    /// `activity_list`: the newest [`PAGE`] events before the cursor, oldest
    /// first, each linked to the checkpoint taken at or before it. The
    /// cursor is the first shown event's `at` and how many shown events
    /// share it (`skip`), so a millisecond split across pages loses none.
    pub async fn list(
        &self,
        source: &str,
        id: &str,
        before: Option<i64>,
        skip: usize,
    ) -> Result<ActivityPage, IpcError> {
        let skip = if before.is_some() { skip } else { 0 };
        let page = repo::Page {
            until: before,
            limit: PAGE + 1 + skip as i64,
        };
        let mut events = self.merged(source, id, page).await?;
        if let Some(at) = before {
            // Those at the boundary sort last; the newest `skip` were shown.
            let shown = events.iter().rev().take_while(|e| e.at == at).count();
            events.truncate(events.len() - skip.min(shown));
        }
        let more = events.len() as i64 > PAGE;
        if more {
            events.drain(..events.len() - PAGE as usize);
        }
        Ok(ActivityPage { events, more })
    }

    /// The source's events in `page`, from every table, oldest first.
    async fn merged(
        &self,
        source: &str,
        id: &str,
        page: repo::Page,
    ) -> Result<Vec<ActivityEvent>, IpcError> {
        let source = kind_of(source)?;
        // One more checkpoint than the page: the one events at its start link to.
        let checkpoints = repo::checkpoints(
            &self.store,
            source,
            id,
            repo::Page {
                limit: page.limit.saturating_add(1),
                ..page
            },
        )
        .await?;
        let mut out: Vec<(i64, u8, ActivityEvent)> = Vec::new();
        for c in &checkpoints {
            let summary = match (c.reason.as_str(), c.first_of_folder) {
                ("restore", _) => "Saved before a restore",
                ("manual", _) => "Checkpoint saved",
                (_, true) => "Folder saved as it was",
                (_, false) => "Files changed since the last checkpoint",
            };
            let mut e = ActivityEvent::new("files", c.at, summary);
            e.checkpoint_id = Some(c.id.clone());
            e.checkpoint_workspace = c.workspace_id.clone();
            // Before the turn it was taken for, at the same moment.
            out.push((c.at, 0, e));
        }
        for row in repo::list(&self.store, source, id, page).await? {
            let mut e = ActivityEvent::new(&row.kind, row.at, row.summary);
            e.checkpoint_id = row.checkpoint_id;
            let d = &row.detail;
            let text = |k: &str| d.get(k).and_then(|v| v.as_str()).map(str::to_string);
            e.decided = text("decided");
            e.engine = text("engine");
            e.ended = text("ended");
            e.exit_code = d
                .get("exit_code")
                .and_then(serde_json::Value::as_i64)
                .and_then(|c| i32::try_from(c).ok());
            e.duration_ms = d.get("duration_ms").and_then(serde_json::Value::as_i64);
            e.secrets = d
                .get("secrets")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            out.push((row.at, 1, e));
        }
        if source == "pane" {
            for c in repo::commands(&self.store, id, page).await? {
                let mut e = ActivityEvent::new("command", c.at, c.command);
                e.command_id = Some(c.id);
                e.exit_code = c.exit_code;
                e.duration_ms = c.duration_ms;
                out.push((c.at, 1, e));
            }
        } else {
            for a in repo::approvals(&self.store, id, page).await? {
                let mut e = ActivityEvent::new("approval", a.at, a.summary);
                e.decided = Some(match &a.via {
                    Some(via) => format!("{}:{via}", a.state),
                    None => a.state.clone(),
                });
                out.push((a.at, 1, e));
            }
        }
        out.sort_by_key(|(at, order, _)| (*at, *order));
        let workspaces: std::collections::HashMap<String, Option<String>> = checkpoints
            .iter()
            .map(|c| (c.id.clone(), c.workspace_id.clone()))
            .collect();
        let mut last: Option<String> = None;
        let mut events = Vec::with_capacity(out.len());
        for (_, _, mut e) in out {
            match e.kind.as_str() {
                "files" => last = e.checkpoint_id.clone(),
                // A turn links only to the checkpoint it made.
                "turn" => {
                    if e.checkpoint_id.is_some() {
                        last = e.checkpoint_id.clone();
                    }
                }
                _ => e.checkpoint_id = last.clone(),
            }
            e.checkpoint_workspace = e
                .checkpoint_id
                .as_ref()
                .and_then(|id| workspaces.get(id).cloned().flatten());
            events.push(e);
        }
        // The extra checkpoint only lent its link; the page keeps its size.
        let keep = page.limit.min(events.len() as i64) as usize;
        let drop = events.len().saturating_sub(keep);
        events.drain(..drop);
        Ok(events)
    }

    /// `activity_export`: every event as JSON Lines.
    pub async fn export(&self, source: &str, id: &str) -> Result<String, IpcError> {
        Ok(jsonl(&self.merged(source, id, repo::Page::ALL).await?))
    }
}

/// One JSON object per line, in order, ending with a newline.
pub fn jsonl(events: &[ActivityEvent]) -> String {
    let mut out = String::new();
    for e in events {
        if let Ok(line) = serde_json::to_string(e) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tandem_core::clock::MockClock;
    use tandem_store::repo::checkpoints::{self as cps, Checkpoint, CheckpointReason};
    use tandem_store::repo::command_history::{self as history, NewRun};

    fn fixture() -> (tempfile::TempDir, Store, MockClock, Arc<ActivityService>) {
        let dir = tempfile::tempdir().expect("dir");
        let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let clock = MockClock::new(chrono::Utc::now());
        let svc = ActivityService::new(store.clone(), Arc::new(clock.clone()));
        (dir, store, clock, svc)
    }

    fn checkpoint(id: &str, pane: &str, at: i64) -> Checkpoint {
        Checkpoint {
            id: id.into(),
            folder: "/w/shop".into(),
            workspace_id: Some("ws_shop".into()),
            thread_id: None,
            pane_id: Some(pane.into()),
            turn_id: None,
            reason: CheckpointReason::Prompt,
            label: String::new(),
            tree: format!("tree-{id}"),
            files: 3,
            skipped: Vec::new(),
            partial: false,
            late: false,
            created_at: at,
            differs: 0,
        }
    }

    async fn command(store: &Store, pane: &str, cmd: &str, started_at: i64, exit: i32) -> i64 {
        history::insert(
            store,
            NewRun {
                pane_id: pane.into(),
                workspace_id: None,
                cwd: "/w/shop".into(),
                command: cmd.into(),
                secret: false,
                exit_code: Some(exit),
                started_at,
                duration_ms: Some(100),
            },
        )
        .await
        .expect("command")
    }

    fn tick(clock: &MockClock) -> i64 {
        clock.advance(Duration::from_secs(1));
        clock.now_ms()
    }

    async fn all(svc: &ActivityService, source: &str, id: &str) -> Vec<ActivityEvent> {
        let page = svc.list(source, id, None, 0).await.expect("list");
        assert!(!page.more);
        page.events
    }

    #[tokio::test]
    async fn f_ex_07_1_a_panes_turns_commands_edit_and_approval_show_in_order_with_their_links() {
        let (_d, store, clock, svc) = fixture();
        let t0 = tick(&clock);
        cps::insert(&store, checkpoint("cp_1", "pn_1", t0))
            .await
            .expect("cp");
        svc.turn(
            "pane",
            "pn_1",
            Some("claude"),
            Some("cp_1".into()),
            t0,
            false,
        )
        .await;
        let first = command(&store, "pn_1", "pnpm test", tick(&clock), 0).await;
        let t = tick(&clock);
        svc.tool("pn_1", "Edit", false, t).await;
        let second = command(&store, "pn_1", "git diff --stat", tick(&clock), 1).await;
        // The edit shows at the next prompt's checkpoint.
        let t3 = tick(&clock);
        cps::insert(&store, checkpoint("cp_2", "pn_1", t3))
            .await
            .expect("cp");
        svc.turn(
            "pane",
            "pn_1",
            Some("claude"),
            Some("cp_2".into()),
            t3,
            false,
        )
        .await;
        // A turn whose folder didn't change made no checkpoint: no link.
        let t4 = tick(&clock);
        svc.turn("pane", "pn_1", Some("claude"), None, t4, false)
            .await;
        tick(&clock);
        svc.approval("pn_1", "allowed:inline", Decider::Person)
            .await;
        // Another pane's events stay out.
        command(&store, "pn_2", "ls", t0, 0).await;

        let events = all(&svc, "pane", "pn_1").await;
        let got: Vec<(&str, &str)> = events
            .iter()
            .map(|e| (e.kind.as_str(), e.summary.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("files", "Folder saved as it was"),
                ("turn", "Turn sent"),
                ("command", "pnpm test"),
                ("tool", "Edit"),
                ("command", "git diff --stat"),
                ("files", "Files changed since the last checkpoint"),
                ("turn", "Turn sent"),
                ("turn", "Turn sent"),
                ("approval", APPROVAL_SUMMARY),
            ]
        );
        assert_eq!(events[2].command_id, Some(first));
        assert_eq!(events[4].command_id, Some(second));
        assert_eq!(events[4].exit_code, Some(1));
        let links: Vec<Option<&str>> = events.iter().map(|e| e.checkpoint_id.as_deref()).collect();
        assert_eq!(
            links,
            vec![
                Some("cp_1"),
                Some("cp_1"),
                Some("cp_1"),
                Some("cp_1"),
                Some("cp_1"),
                Some("cp_2"),
                Some("cp_2"),
                None,
                Some("cp_2")
            ]
        );
        assert_eq!(events[5].checkpoint_workspace.as_deref(), Some("ws_shop"));
        assert_eq!(events[1].engine.as_deref(), Some("claude"));
        assert_eq!(events[8].decided.as_deref(), Some("allowed:inline"));
    }

    #[tokio::test]
    async fn f_ex_07_tool_names_that_arent_names_are_dropped() {
        let (_d, store, clock, svc) = fixture();
        let t = tick(&clock);
        for bad in [
            "Bash; please rename the quarterly flamingo report",
            "x y",
            "",
            &"a".repeat(65),
            "naïve",
        ] {
            svc.tool("pn_1", bad, false, t).await;
        }
        svc.tool("pn_1", "mcp__github__search_code", false, t).await;
        let events = all(&svc, "pane", "pn_1").await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].summary, "mcp__github__search_code");
        let rows: i64 = store
            .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM activity", [], |r| r.get(0))?))
            .await
            .expect("count");
        assert_eq!(rows, 1);
    }

    #[tokio::test]
    async fn f_ex_07_3_a_page_is_the_newest_events_and_export_is_all_of_them_as_json_lines() {
        let (_d, store, clock, svc) = fixture();
        for n in 0..(PAGE + 20) {
            // Under the tool rows' rate limit.
            clock.advance(Duration::from_millis(300));
            svc.tool("pn_1", &format!("T{n}"), n % 7 == 0, clock.now_ms())
                .await;
        }
        let t = tick(&clock);
        command(&store, "pn_1", "make", t, 0).await;
        let first = svc.list("pane", "pn_1", None, 0).await.expect("list");
        assert!(first.more);
        assert_eq!(first.events.len() as i64, PAGE);
        assert_eq!(
            first.events.last().map(|e| e.kind.as_str()),
            Some("command")
        );
        let before = first.events[0].at;
        let earlier = svc
            .list("pane", "pn_1", Some(before), 1)
            .await
            .expect("earlier");
        assert!(!earlier.more);
        assert_eq!(earlier.events.len(), 21);
        assert!(earlier.events.iter().all(|e| e.at < before));
        let text = svc.export("pane", "pn_1").await.expect("export");
        let back: Vec<ActivityEvent> = text
            .lines()
            .map(|l| serde_json::from_str(l).expect("a JSON object per line"))
            .collect();
        let mut joined = earlier.events.clone();
        joined.extend(first.events.clone());
        assert_eq!(back, joined);
        assert_eq!(back[0].kind, "subagent");
        assert_eq!(back[0].summary, "Subagent started (T0)");
    }

    #[tokio::test]
    async fn f_ex_07_4_a_run_shows_its_command_variables_decider_exit_and_duration_only() {
        let (_d, store, _clock, svc) = fixture();
        svc.run(
            "pn_1",
            RunRecord {
                command: "printenv API_KEY".into(),
                secrets: vec!["API_KEY (stripe-test)".into()],
                decided: "agent",
                exit_code: Some(3),
                duration_ms: Some(840),
                ended: None,
                started: true,
                by: Decider::Person,
            },
        )
        .await;
        let e = &all(&svc, "pane", "pn_1").await[0];
        assert_eq!(e.kind, "run");
        assert_eq!(e.summary, "printenv API_KEY");
        assert_eq!(e.secrets, vec!["API_KEY (stripe-test)"]);
        assert_eq!(e.decided.as_deref(), Some("agent"));
        assert_eq!((e.exit_code, e.duration_ms), (Some(3), Some(840)));
        // A command line carrying a token-shaped value is stored redacted.
        let token = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";
        svc.run(
            "pn_1",
            RunRecord {
                command: format!("curl -H 'Authorization: token {token}' x"),
                secrets: Vec::new(),
                decided: "once",
                exit_code: Some(0),
                duration_ms: Some(1),
                ended: None,
                started: true,
                by: Decider::Person,
            },
        )
        .await;
        let all_text: String = store
            .read(|c| {
                Ok(
                    c.query_row("SELECT group_concat(summary) FROM activity", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .await
            .expect("summaries");
        assert!(!all_text.contains(token), "{all_text}");
    }

    #[tokio::test]
    async fn f_ex_07_a_threads_ledger_has_its_turns_and_only_panes_and_threads_have_one() {
        let (_d, _store, clock, svc) = fixture();
        svc.turn("thread", "th_1", Some("codex"), None, clock.now_ms(), false)
            .await;
        let events = all(&svc, "thread", "th_1").await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "turn");
        assert_eq!(events[0].engine.as_deref(), Some("codex"));
        assert!(svc.list("routine", "r_1", None, 0).await.is_err());
        assert!(all(&svc, "pane", "pn_none").await.is_empty());
    }

    #[tokio::test]
    async fn f_ex_07_3_events_sharing_the_page_boundary_millisecond_are_all_listed() {
        let (_d, store, clock, svc) = fixture();
        let at = clock.now_ms();
        for n in 0..(PAGE + 10) {
            repo::insert(
                &store,
                NewActivity {
                    source_kind: "pane",
                    source_id: "pn_1".into(),
                    kind: "tool",
                    at,
                    summary: format!("T{n}"),
                    detail: serde_json::json!({}),
                    checkpoint_id: None,
                    kept: Keep::Counted,
                },
            )
            .await
            .expect("insert");
        }
        let first = svc.list("pane", "pn_1", None, 0).await.expect("list");
        assert!(first.more);
        let skip = first.events.iter().filter(|e| e.at == at).count();
        let earlier = svc
            .list("pane", "pn_1", Some(at), skip)
            .await
            .expect("earlier");
        assert!(!earlier.more);
        let mut all: Vec<String> = earlier
            .events
            .iter()
            .chain(first.events.iter())
            .map(|e| e.summary.clone())
            .collect();
        assert_eq!(all.len() as i64, PAGE + 10);
        all.sort();
        all.dedup();
        assert_eq!(all.len() as i64, PAGE + 10, "none twice, none lost");
    }

    #[tokio::test]
    async fn f_ex_07_tool_rows_are_rate_limited_per_pane_and_the_drop_is_noted() {
        let (_d, store, clock, svc) = fixture();
        for n in 0..(TOOL_ROWS_PER_MINUTE + 50) {
            svc.tool("pn_flood", &format!("T{n}"), false, clock.now_ms())
                .await;
        }
        // Another pane isn't held back by it.
        svc.tool("pn_quiet", "Read", false, clock.now_ms()).await;
        // The flood's note is there at once, even if nothing else comes.
        let notes = |events: &[ActivityEvent]| {
            events
                .iter()
                .filter(|e| e.kind == "note")
                .map(|e| e.summary.clone())
                .collect::<Vec<_>>()
        };
        let now = svc
            .list("pane", "pn_flood", None, 0)
            .await
            .expect("list")
            .events;
        assert_eq!(now.len() as u32, TOOL_ROWS_PER_MINUTE + 1);
        assert_eq!(
            notes(&now),
            [format!("Over {TOOL_ROWS_PER_MINUTE} tool events in a minute: the rest of the minute's aren't kept")]
        );
        clock.advance(Duration::from_secs(61));
        svc.tool("pn_flood", "Read", false, clock.now_ms()).await;
        let events = svc
            .list("pane", "pn_flood", None, 0)
            .await
            .expect("list")
            .events;
        assert_eq!(
            events.len() as u32,
            TOOL_ROWS_PER_MINUTE + 3,
            "kept, two notes, the next"
        );
        assert_eq!(
            notes(&events)[1],
            "50 more tool events in that minute weren't kept"
        );
        assert_eq!(
            svc.list("pane", "pn_quiet", None, 0)
                .await
                .expect("list")
                .events
                .len(),
            1
        );
        // Notes are out of the counts (`Keep::Ceiling`): a later flood of
        // counted rows can't prune them.
        let pinned: i64 = store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM activity WHERE kind = 'note' AND kept = 1",
                    [],
                    |r| r.get(0),
                )?)
            })
            .await
            .expect("count");
        assert_eq!(pinned, 2);
    }

    #[tokio::test]
    async fn f_ex_07_refused_runs_and_pane_turns_are_capped_but_decisions_are_not() {
        let (_d, store, clock, svc) = fixture();
        let record = |decided: &'static str, started: bool| RunRecord {
            command: "x".into(),
            secrets: Vec::new(),
            decided,
            exit_code: None,
            duration_ms: None,
            ended: None,
            started,
            by: match decided {
                "once" | "agent" | "declined" | "expired" | "cancelled" => Decider::Person,
                "rule" | "allowed_before" => Decider::Unasked,
                _ => Decider::Nobody,
            },
        };
        let over = Flood::Refusal.limit() + 20;
        // What the agent triggers at will: refused before asking, or allowed
        // but never started (gone first, a moved folder, unreadable secrets).
        for n in 0..over {
            svc.run(
                "pn_1",
                record(if n % 2 == 0 { "busy" } else { "rule" }, false),
            )
            .await;
        }
        // The person's own decisions after that flood, and every answer
        // (each follows a question, at most 10 a minute): all kept.
        for _ in 0..over {
            // Allowed, then stopped before it started (gone, a moved folder,
            // unreadable secrets): the person's decision too.
            svc.run("pn_1", record("once", false)).await;
            svc.run("pn_1", record("declined", false)).await;
            svc.run("pn_1", record("expired", false)).await;
            svc.approval("pn_1", "denied:inline", Decider::Person).await;
            svc.approval("pn_1", "client_gone", Decider::Nobody).await;
        }
        // Runs that started.
        for _ in 0..over {
            svc.run("pn_1", record("rule", true)).await;
            svc.run("pn_1", record("once", true)).await;
        }
        // Turns at Enter and reported turns have a cap each.
        for _ in 0..(Flood::Turn.limit() + 5) {
            svc.turn("pane", "pn_1", None, None, clock.now_ms(), false)
                .await;
            svc.turn("pane", "pn_1", None, None, clock.now_ms(), true)
                .await;
        }
        let count = |sql: &'static str| {
            let store = store.clone();
            async move {
                store
                    .read(move |c| Ok(c.query_row(sql, [], |r| r.get::<_, i64>(0))?))
                    .await
                    .expect("count")
            }
        };
        assert_eq!(
            count("SELECT COUNT(*) FROM activity WHERE kept = 0 AND kind = 'run'").await,
            i64::from(Flood::Refusal.limit())
        );
        // The person's decisions (their Allow, started or not, their Deny,
        // expiry) are kept by age only; what the agent drove (a rule run, an
        // answer it caused by going away) sits under the ceiling.
        assert_eq!(
            count("SELECT COUNT(*) FROM activity WHERE kept = 2 AND kind IN ('run', 'approval')")
                .await,
            5 * i64::from(over)
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM activity WHERE kept = 1 AND kind IN ('run', 'approval')")
                .await,
            2 * i64::from(over)
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM activity WHERE kind = 'turn'").await,
            2 * i64::from(Flood::Turn.limit())
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM activity WHERE kind = 'note'").await,
            3
        );
        // A thread's turns aren't the agent's to flood.
        for _ in 0..(Flood::Turn.limit() + 5) {
            svc.turn("thread", "th_1", None, None, clock.now_ms(), false)
                .await;
        }
        assert_eq!(
            count("SELECT COUNT(*) FROM activity WHERE kind = 'turn' AND source_kind = 'thread'")
                .await,
            i64::from(Flood::Turn.limit() + 5)
        );
    }

    #[tokio::test]
    async fn f_ex_07_a_long_summary_is_stored_cut_to_1_kib() {
        let (_d, store, _clock, svc) = fixture();
        // "é" (2 bytes) sits across the cut at 1,021 bytes (1 KiB less the
        // 3-byte "…"), so the cut walks back to the character's start.
        let long = format!("echo {}é{}", "x".repeat(1015), "y".repeat(8 * 1024));
        svc.run(
            "pn_1",
            RunRecord {
                command: long,
                secrets: Vec::new(),
                decided: "rule",
                exit_code: Some(0),
                duration_ms: Some(1),
                ended: None,
                started: true,
                by: Decider::Unasked,
            },
        )
        .await;
        let summary: String = store
            .read(|c| Ok(c.query_row("SELECT summary FROM activity", [], |r| r.get(0))?))
            .await
            .expect("row");
        assert!(summary.len() <= MAX_SUMMARY_BYTES, "{}", summary.len());
        assert_eq!(summary, format!("echo {}…", "x".repeat(1015)));
    }

    #[tokio::test]
    async fn f_ex_07_through_the_service_a_flood_past_the_ceiling_leaves_every_decision() {
        let (_d, store, clock, svc) = fixture();
        let record = |decided: &'static str, by: Decider| RunRecord {
            command: "x".into(),
            secrets: Vec::new(),
            decided,
            exit_code: Some(0),
            duration_ms: Some(1),
            ended: None,
            started: true,
            by,
        };
        // The person's decisions, first and so the oldest.
        for _ in 0..5 {
            svc.run("pn_1", record("once", Decider::Person)).await;
            svc.approval("pn_1", "denied:inline", Decider::Person).await;
        }
        // Then what agents drive, in two panes: rule runs and answers they
        // caused by going away, well past the ceiling, and a flood of tool
        // rows.
        for _ in 0..40 {
            clock.advance(Duration::from_secs(2));
            for pane in ["pn_1", "pn_2"] {
                svc.run(pane, record("rule", Decider::Unasked)).await;
                svc.approval(pane, "client_gone", Decider::Nobody).await;
                svc.tool(pane, "Read", false, clock.now_ms()).await;
            }
        }
        // Each pane's ceiling (5) under the total (7), and the two panes
        // together over it: both bind.
        let limits = repo::Limits {
            per_source: 3,
            total: 3,
            ceiling: 5,
            ceiling_total: 7,
        };
        repo::prune_with(&store, clock.now_ms(), limits)
            .await
            .expect("prune");
        let count = |sql: &'static str| {
            let store = store.clone();
            async move {
                store
                    .read(move |c| Ok(c.query_row(sql, [], |r| r.get::<_, i64>(0))?))
                    .await
                    .expect("count")
            }
        };
        assert_eq!(
            count("SELECT COUNT(*) FROM activity WHERE kept = 2").await,
            10
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM activity WHERE kept = 1").await,
            7,
            "the total ceiling"
        );
        assert!(
            count(
                "SELECT MAX(n) FROM (SELECT COUNT(*) AS n FROM activity
                 WHERE kept = 1 GROUP BY source_id)"
            )
            .await
                <= 5,
            "each pane's ceiling"
        );
        assert!(count("SELECT COUNT(*) FROM activity WHERE kept = 0").await <= 3);
    }
}
