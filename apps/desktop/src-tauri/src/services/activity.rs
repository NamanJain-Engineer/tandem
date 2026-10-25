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
use tandem_store::repo::activity::{self as repo, NewActivity};
use tandem_store::Store;

/// One event of a session's ledger (`activity_list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActivityEvent {
    /// `turn` | `files` | `command` | `tool` | `subagent` | `approval` | `run`
    pub kind: String,
    pub at: i64,
    /// One line, redacted.
    pub summary: String,
    /// The checkpoint taken at or before it.
    pub checkpoint_id: Option<String>,
    /// A command's row in command recall.
    #[ts(type = "number | null")]
    pub command_id: Option<i64>,
    pub exit_code: Option<i32>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    /// The secrets a run used: names only.
    pub secrets: Vec<String>,
    /// Who decided a run or an approval: `once`, `agent`, `rule`,
    /// `allowed_before`, `declined`, `expired`, `cancelled`, or an approval's
    /// state and where it was answered (`allowed:dashboard`).
    pub decided: Option<String>,
    /// The engine a turn went to (`claude`).
    pub engine: Option<String>,
}

impl ActivityEvent {
    fn new(kind: &str, at: i64, summary: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            at,
            summary: summary.into(),
            checkpoint_id: None,
            command_id: None,
            exit_code: None,
            duration_ms: None,
            secrets: Vec::new(),
            decided: None,
            engine: None,
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

pub struct ActivityService {
    store: Store,
    clock: Arc<dyn Clock>,
}

impl ActivityService {
    pub fn new(store: Store, clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self { store, clock })
    }

    async fn record(&self, row: NewActivity) {
        if let Err(err) = repo::insert(&self.store, row).await {
            tracing::warn!(code = "activity.store_failed", error = %err, "an activity event wasn't recorded");
        }
    }

    /// A turn went to a thread's engine, or a prompt to an agent pane's.
    /// Never its text.
    pub async fn turn(
        &self,
        source_kind: &'static str,
        id: &str,
        engine: Option<&str>,
        checkpoint: Option<String>,
    ) {
        self.record(NewActivity {
            source_kind,
            source_id: id.to_string(),
            kind: "turn",
            at: self.clock.now_ms(),
            summary: "Turn sent".into(),
            detail: serde_json::json!({ "engine": engine }),
            checkpoint_id: checkpoint,
        })
        .await;
    }

    /// An agent pane's engine started a tool (its name, from its hooks).
    pub async fn tool(&self, pane: &str, tool: &str, subagent: bool) {
        let tool: String = tool.chars().take(80).collect();
        self.record(NewActivity {
            source_kind: "pane",
            source_id: pane.to_string(),
            kind: if subagent { "subagent" } else { "tool" },
            at: self.clock.now_ms(),
            summary: if subagent {
                format!("Subagent started ({tool})")
            } else {
                tool.clone()
            },
            detail: serde_json::json!({ "tool": tool }),
            checkpoint_id: None,
        })
        .await;
    }

    /// The person answered a pane's approval (`decided` as in
    /// [`ActivityEvent::decided`]).
    pub async fn approval(&self, pane: &str, summary: &str, decided: &str) {
        self.record(NewActivity {
            source_kind: "pane",
            source_id: pane.to_string(),
            kind: "approval",
            at: self.clock.now_ms(),
            summary: Redactor::global().redact(summary).into_owned(),
            detail: serde_json::json!({ "decided": decided }),
            checkpoint_id: None,
        })
        .await;
    }

    /// A `vibezeo run` ended, or was refused after its decision.
    pub async fn run(&self, pane: &str, run: RunRecord) {
        self.record(NewActivity {
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
            }),
            checkpoint_id: None,
        })
        .await;
    }

    /// Drops old events (at startup and hourly).
    pub async fn prune(&self) {
        if let Err(err) = repo::prune(&self.store, self.clock.now_ms()).await {
            tracing::warn!(code = "activity.prune_failed", error = %err, "activity wasn't pruned");
        }
    }

    /// `activity_list`: the source's events, oldest first, each linked to
    /// the checkpoint taken at or before it.
    pub async fn list(&self, source: &str, id: &str) -> Result<Vec<ActivityEvent>, IpcError> {
        let source = kind_of(source)?;
        let checkpoints = repo::checkpoints(&self.store, source, id).await?;
        let mut out: Vec<(i64, u8, ActivityEvent)> = Vec::new();
        for c in &checkpoints {
            let summary = if c.first_of_folder {
                "Folder saved as it was"
            } else {
                "Files changed since the last checkpoint"
            };
            let mut e = ActivityEvent::new("files", c.at, summary);
            e.checkpoint_id = Some(c.id.clone());
            // Before the turn it was taken for, at the same moment.
            out.push((c.at, 0, e));
        }
        for row in repo::list(&self.store, source, id).await? {
            let mut e = ActivityEvent::new(&row.kind, row.at, row.summary);
            e.checkpoint_id = row.checkpoint_id;
            let d = &row.detail;
            e.decided = d
                .get("decided")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            e.engine = d.get("engine").and_then(|v| v.as_str()).map(str::to_string);
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
            for c in repo::commands(&self.store, id).await? {
                let mut e = ActivityEvent::new("command", c.at, c.command);
                e.command_id = Some(c.id);
                e.exit_code = c.exit_code;
                e.duration_ms = c.duration_ms;
                out.push((c.at, 1, e));
            }
        } else {
            for a in repo::approvals(&self.store, id).await? {
                let mut e = ActivityEvent::new("approval", a.at, a.summary);
                e.decided = Some(match &a.via {
                    Some(via) => format!("{}:{via}", a.state),
                    None => a.state.clone(),
                });
                out.push((a.at, 1, e));
            }
        }
        out.sort_by_key(|(at, order, _)| (*at, *order));
        let mut last: Option<String> = None;
        let mut events = Vec::with_capacity(out.len());
        for (_, _, mut e) in out {
            if e.kind == "files" {
                last = e.checkpoint_id.clone();
            } else if e.checkpoint_id.is_none() {
                e.checkpoint_id = last.clone();
            } else {
                last = e.checkpoint_id.clone();
            }
            events.push(e);
        }
        Ok(events)
    }

    /// `activity_export`: the same events as JSON Lines.
    pub async fn export(&self, source: &str, id: &str) -> Result<String, IpcError> {
        Ok(jsonl(&self.list(source, id).await?))
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
            workspace_id: None,
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

    #[tokio::test]
    async fn f_ex_07_1_a_panes_commands_edit_and_approval_show_in_order_with_their_links() {
        let (_d, store, clock, svc) = fixture();
        let t0 = tick(&clock);
        cps::insert(&store, checkpoint("cp_1", "pn_1", t0))
            .await
            .expect("cp");
        svc.turn("pane", "pn_1", Some("claude"), Some("cp_1".into()))
            .await;
        let t1 = tick(&clock);
        let first = command(&store, "pn_1", "pnpm test", t1, 0).await;
        tick(&clock);
        svc.tool("pn_1", "Edit", false).await;
        let t2 = tick(&clock);
        let second = command(&store, "pn_1", "git diff --stat", t2, 1).await;
        // The edit shows at the next prompt's checkpoint.
        let t3 = tick(&clock);
        cps::insert(&store, checkpoint("cp_2", "pn_1", t3))
            .await
            .expect("cp");
        svc.turn("pane", "pn_1", Some("claude"), Some("cp_2".into()))
            .await;
        tick(&clock);
        svc.approval(
            "pn_1",
            "run `pnpm deploy` with stripe-test",
            "allowed:inline",
        )
        .await;
        // Another pane's events stay out.
        command(&store, "pn_2", "ls", t1, 0).await;

        let events = svc.list("pane", "pn_1").await.expect("list");
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
                ("approval", "run `pnpm deploy` with stripe-test"),
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
                Some("cp_2")
            ]
        );
        assert_eq!(events[7].decided.as_deref(), Some("allowed:inline"));
    }

    #[tokio::test]
    async fn f_ex_07_2_turn_and_run_rows_hold_names_and_numbers_only() {
        let (_d, store, _clock, svc) = fixture();
        // Everything the recorders are handed for a turn and a run.
        svc.turn("thread", "th_1", Some("codex"), None).await;
        svc.turn("pane", "pn_1", Some("claude"), None).await;
        svc.run(
            "pn_1",
            RunRecord {
                command: "pnpm deploy".into(),
                secrets: vec!["stripe-test".into()],
                decided: "once",
                exit_code: Some(0),
                duration_ms: Some(12),
            },
        )
        .await;
        let rows: Vec<String> = store
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT source_id || kind || summary || detail_json || COALESCE(checkpoint_id, '') FROM activity",
                )?;
                let rows = stmt
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .expect("rows");
        assert_eq!(rows.len(), 3);
        // Only names and numbers: the recorders are never handed text.
        assert!(rows.iter().all(|r| r.len() < 200), "{rows:?}");
    }

    #[tokio::test]
    async fn f_ex_07_3_export_is_json_lines_of_the_same_events_in_order() {
        let (_d, store, clock, svc) = fixture();
        svc.turn("pane", "pn_1", Some("claude"), None).await;
        let t = tick(&clock);
        command(&store, "pn_1", "make", t, 0).await;
        tick(&clock);
        svc.tool("pn_1", "Task", true).await;
        let events = svc.list("pane", "pn_1").await.expect("list");
        let text = svc.export("pane", "pn_1").await.expect("export");
        assert!(text.ends_with('\n'));
        let back: Vec<ActivityEvent> = text
            .lines()
            .map(|l| serde_json::from_str(l).expect("a JSON object per line"))
            .collect();
        assert_eq!(back, events);
        assert_eq!(back.len(), 3);
        assert_eq!(back[2].kind, "subagent");
        assert_eq!(back[2].summary, "Subagent started (Task)");
    }

    #[tokio::test]
    async fn f_ex_07_4_a_run_shows_its_command_secrets_decider_exit_and_duration_only() {
        let (_d, store, _clock, svc) = fixture();
        svc.run(
            "pn_1",
            RunRecord {
                command: "printenv API_KEY".into(),
                secrets: vec!["stripe-test".into()],
                decided: "agent",
                exit_code: Some(3),
                duration_ms: Some(840),
            },
        )
        .await;
        let e = &svc.list("pane", "pn_1").await.expect("list")[0];
        assert_eq!(e.kind, "run");
        assert_eq!(e.summary, "printenv API_KEY");
        assert_eq!(e.secrets, vec!["stripe-test"]);
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
            },
        )
        .await;
        let all: String = store
            .read(|c| {
                Ok(
                    c.query_row("SELECT group_concat(summary) FROM activity", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .await
            .expect("summaries");
        assert!(!all.contains(token), "{all}");
    }

    #[tokio::test]
    async fn f_ex_07_a_threads_ledger_has_its_turns_and_only_panes_and_threads_have_one() {
        let (_d, _store, _clock, svc) = fixture();
        svc.turn("thread", "th_1", Some("codex"), None).await;
        let events = svc.list("thread", "th_1").await.expect("list");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "turn");
        assert_eq!(events[0].engine.as_deref(), Some("codex"));
        assert!(svc.list("routine", "r_1").await.is_err());
        assert!(svc.list("pane", "pn_none").await.expect("list").is_empty());
    }
}
