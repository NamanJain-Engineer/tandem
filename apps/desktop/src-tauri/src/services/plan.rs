//! F-TK-06 Plan with Lead (bold bet B3, first slice): Lead turns a goal into
//! tasks, each with a suggested engine. Lead plans in its own chat in the
//! workspace; its reply's JSON block, between markers with a fresh code,
//! becomes a draft the user edits. Create tasks writes them into the tracker
//! (F-TK-01) and Execute all, once you've seen what it starts, runs them
//! through F-TK-04's queue (D-2430).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::RngCore;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::access::{Access, ThreadMode};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::{AccountId, TeammateId, ThreadId, WorkspaceId};
use tandem_core::{untrusted, ErrorCode, IpcError};
use tandem_store::repo::threads::ThreadState;
use tandem_tasks::{IssueStatus, SaveIssue, TaskStore, Writer};

use crate::services::tasks_exec::{ExecuteInput, Isolation, RunBlock};

pub const EVENT: &str = "tasks:plan";
pub const GOAL_MAX: usize = 4_000;
pub const TASKS_MAX: usize = 12;
pub const TITLE_MAX: usize = 200;
pub const DESCRIPTION_MAX: usize = 8_000;
pub const WHY_MAX: usize = 300;
/// A project's name is the goal's first line, cut here.
pub const PROJECT_NAME_MAX: usize = 80;
/// Lead's reply is read as a plan only up to this size (bytes).
pub const REPLY_MAX: usize = 512 * 1024;
/// Begin markers tried, from the last one back.
const BLOCKS_TRIED: usize = 8;
/// Lead's working time before the plan stops it. Time waiting on your
/// answer doesn't count.
pub const PLAN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const POLL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PlanState {
    /// Lead's turn is running.
    Planning,
    /// A draft to edit.
    Ready,
    Failed,
    /// Its tasks are in the tracker.
    Created,
    /// Execute all is queuing its tasks.
    Starting,
    /// Execute all queued its tasks.
    Started,
}

/// One account of an offered engine, with its fullest usage window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanAccount {
    pub account_id: AccountId,
    pub label: String,
    pub is_default: bool,
    /// The fullest window's kind and use, when there's a reading.
    pub window: Option<String>,
    pub used_pct: Option<f64>,
    pub resets_at: Option<i64>,
}

/// An engine a task can run on: installed, with a chat adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanEngine {
    pub engine_id: String,
    pub label: String,
    pub accounts: Vec<PlanAccount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanTask {
    pub title: String,
    pub description: String,
    pub engine_id: String,
    /// Lead's reason for the engine, if it gave one.
    #[ts(optional)]
    #[serde(default)]
    pub why: Option<String>,
    /// Lead named an engine that wasn't offered: this is Lead's own.
    #[serde(default)]
    pub fell_back: bool,
}

/// `tasks:plan`, and what the `plan_*` commands return.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanView {
    pub id: String,
    pub goal: String,
    pub workspace_id: WorkspaceId,
    pub thread_id: ThreadId,
    pub state: PlanState,
    pub tasks: Vec<PlanTask>,
    pub engines: Vec<PlanEngine>,
    /// Lead, whose chat the plan is in.
    pub lead_id: TeammateId,
    pub lead_engine: String,
    pub error: Option<String>,
    /// Lead's chat waits for your answer there (an approval).
    pub waiting: bool,
    pub project_id: Option<String>,
    /// The tasks written so far: all of them once created.
    pub issue_ids: Vec<String>,
    /// The tasks Execute all queued.
    pub queued: Vec<String>,
    /// The tasks Execute all left alone, and why.
    pub skipped: Vec<PlanSkip>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanSkip {
    pub issue_id: String,
    pub reason: RunBlock,
}

/// One run Execute all would start (F-TK-06 AC4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanRun {
    pub issue_id: String,
    pub title: String,
    pub engine_id: String,
    pub engine_label: String,
    /// The engine's account with the most room as usage reads now; none:
    /// the engine's own sign-in.
    pub account_id: Option<AccountId>,
    pub account_label: Option<String>,
    /// The engine the run's independent review uses (F-TK-04).
    pub review_engine_id: String,
    pub review_engine_label: String,
    /// Why the task is left alone, if it is.
    pub skip: Option<RunBlock>,
}

/// `plan_prepare`: what Execute all starts, for you to confirm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanLaunch {
    /// Why no run can start here (F-TK-04's limits), if none can.
    pub blocked: Option<String>,
    pub runs: Vec<PlanRun>,
}

/// What Lead's reply is read into.
#[derive(Deserialize)]
struct Reply {
    tasks: Vec<ReplyTask>,
}

#[derive(Deserialize)]
struct ReplyTask {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    engine: String,
    #[serde(default)]
    why: Option<String>,
}

fn nonce() -> String {
    let mut bytes = [0u8; 4];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn begin(nonce: &str) -> String {
    format!("PLAN-BEGIN-{nonce}")
}

fn end(nonce: &str) -> String {
    format!("PLAN-END-{nonce}")
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Text from a plan without the characters that can hide words from you:
/// controls (a description keeps its line breaks and tabs), bidi controls
/// and Unicode tags. A title is one line.
fn clean(text: &str, multiline: bool) -> String {
    let kept: String = text
        .chars()
        .filter(|c| match c {
            '\n' | '\t' => true,
            c if c.is_control() => false,
            '\u{061C}' | '\u{200E}' | '\u{200F}' => false,
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => false,
            '\u{E0000}'..='\u{E007F}' => false,
            _ => true,
        })
        .collect();
    if multiline {
        kept.trim().to_string()
    } else {
        kept.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

/// A row as it's kept: cleaned and cut.
fn tidy(t: PlanTask) -> PlanTask {
    PlanTask {
        title: cut(&clean(&t.title, false), TITLE_MAX),
        description: cut(&clean(&t.description, true), DESCRIPTION_MAX),
        why: t
            .why
            .map(|w| cut(&clean(&w, false), WHY_MAX))
            .filter(|w| !w.is_empty()),
        ..t
    }
}

/// The JSON between a pair of markers, out of a code fence if it's in one.
fn block_body(raw: &str) -> &str {
    let body = raw.trim();
    match body.strip_prefix("```") {
        Some(rest) => rest
            .split_once('\n')
            .map(|(_, r)| r)
            .unwrap_or("")
            .trim_end()
            .trim_end_matches('`')
            .trim(),
        None => body,
    }
}

/// "resets in 2 h 10 min".
fn resets_in(at: i64, now: i64) -> String {
    let mins = ((at - now).max(0) + 59_999) / 60_000;
    if mins < 60 {
        format!("resets in {mins} min")
    } else {
        format!("resets in {} h {} min", mins / 60, mins % 60)
    }
}

/// An offered engine's line in the prompt.
fn engine_line(e: &PlanEngine, now: i64) -> String {
    let accounts: Vec<String> = e
        .accounts
        .iter()
        .map(|a| {
            let used = match (&a.window, a.used_pct) {
                (Some(w), Some(p)) => {
                    let reset = a
                        .resets_at
                        .map(|r| format!(", {}", resets_in(r, now)))
                        .unwrap_or_default();
                    format!("{p:.0}% of the {w} window used{reset}")
                }
                _ => "no usage reading".to_string(),
            };
            format!("account \"{}\": {used}", a.label)
        })
        .collect();
    let accounts = if accounts.is_empty() {
        "its default sign-in".to_string()
    } else {
        accounts.join("; ")
    };
    format!("- `{}` ({}): {accounts}", e.engine_id, e.label)
}

/// Lead's planning message. The words are the engine's instructions, so
/// they stay in English like F-WT-05's prompt.
pub fn prompt(goal: &str, engines: &[PlanEngine], nonce: &str, now: i64) -> String {
    let lines: Vec<String> = engines.iter().map(|e| engine_line(e, now)).collect();
    format!(
        "Plan this goal as tasks for coding agents. Each task runs on its own, in its own git worktree of this repository, so make each one self-contained and say in its description what done looks like.\n\n\
         The user's goal:\n{}\n\n\
         Engines a task can run on, with how much of each account's usage window is used:\n{}\n\
         Pick the engine that suits each task, and prefer engines with room left.\n\n\
         Read the repository as much as you need, but don't change files and don't create tasks yourself: Vibezeo creates them once the user approves the plan.\n\n\
         Reply with the plan as one JSON object between these two lines, each on a line of its own:\n\
         {}\n\
         {{\"tasks\": [{{\"title\": \"…\", \"description\": \"…\", \"engine\": \"<engine id>\", \"why\": \"…\"}}]}}\n\
         {}\n\
         Use 1 to {TASKS_MAX} tasks, titles of at most {TITLE_MAX} characters, and an engine id from the list.",
        untrusted::fence("text", goal),
        lines.join("\n"),
        begin(nonce),
        end(nonce),
    )
}

/// The plan in Lead's reply: the last begin marker whose text up to the
/// next end marker reads as a plan, so Lead naming the markers after its
/// plan doesn't hide it. A reply over [`REPLY_MAX`] isn't read.
pub fn parse(
    reply: &str,
    nonce: &str,
    engines: &[PlanEngine],
    fallback: &str,
) -> Result<Vec<PlanTask>, String> {
    if reply.len() > REPLY_MAX {
        return Err("Lead's reply is too long to read as a plan".to_string());
    }
    let (b, e) = (begin(nonce), end(nonce));
    let starts: Vec<usize> = reply.match_indices(&b).map(|(i, _)| i + b.len()).collect();
    let parsed: Reply = starts
        .iter()
        .rev()
        .take(BLOCKS_TRIED)
        .find_map(|&start| {
            let stop = reply[start..].find(&e)? + start;
            serde_json::from_str(block_body(&reply[start..stop])).ok()
        })
        .ok_or_else(|| "Lead's reply had no plan".to_string())?;
    if parsed.tasks.is_empty() || parsed.tasks.len() > TASKS_MAX {
        return Err(format!(
            "Lead planned {} tasks; a plan holds 1 to {TASKS_MAX}",
            parsed.tasks.len()
        ));
    }
    let offered = |id: &str| engines.iter().any(|e| e.engine_id == id);
    parsed
        .tasks
        .into_iter()
        .map(|t| {
            let engine = t.engine.trim();
            let fell_back = !offered(engine);
            let task = tidy(PlanTask {
                title: t.title,
                description: t.description,
                engine_id: if fell_back { fallback } else { engine }.to_string(),
                why: t.why,
                fell_back,
            });
            if task.title.is_empty() {
                return Err("A task in Lead's plan has no title".to_string());
            }
            Ok(task)
        })
        .collect()
}

/// The account with the most room (D-2431): the lowest fullest window
/// among accounts with room left; else one with no reading, the default
/// first, since no reading isn't no room; else the full one that resets
/// first.
pub fn roomiest(engine: &PlanEngine) -> Option<AccountId> {
    fn rank(a: &PlanAccount) -> (u8, f64) {
        match a.used_pct {
            Some(p) if p < 100.0 => (0, p),
            None => (1, if a.is_default { 0.0 } else { 1.0 }),
            Some(_) => (2, a.resets_at.map_or(f64::MAX, |r| r as f64)),
        }
    }
    engine
        .accounts
        .iter()
        .enumerate()
        .min_by(|(i, a), (j, b)| {
            let ((ta, va), (tb, vb)) = (rank(a), rank(b));
            ta.cmp(&tb)
                .then(va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal))
                .then(i.cmp(j))
        })
        .map(|(_, a)| a.account_id.clone())
}

/// The project's name: the goal's first line, cut.
pub fn project_name(goal: &str) -> String {
    let first = goal
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Plan");
    let name = cut(first, PROJECT_NAME_MAX);
    if name.chars().count() < first.chars().count() {
        format!("{}…", name.trim_end())
    } else {
        name
    }
}

#[async_trait::async_trait]
pub trait PlanHost: Send + Sync {
    /// Lead's id and engine.
    async fn lead(&self) -> Option<(TeammateId, String)>;
    /// A local workspace's folder; an error for a missing or remote one.
    async fn local_root(&self, ws: &WorkspaceId) -> Result<String, IpcError>;
    async fn engines(&self) -> Vec<PlanEngine>;
    fn chat_capable(&self, engine: &str) -> bool;
    async fn start_chat(
        &self,
        ws: &WorkspaceId,
        root: &str,
        lead: &TeammateId,
        engine: &str,
        title: String,
    ) -> Result<ThreadId, IpcError>;
    /// Lead's planning chat gets no tasks server: nothing reaches the
    /// tracker before you approve the plan.
    fn withhold_tasks(&self, thread: &ThreadId);
    async fn send(&self, thread: &ThreadId, text: String) -> Result<(), IpcError>;
    async fn state(&self, thread: &ThreadId) -> Option<ThreadState>;
    async fn last_reply(&self, thread: &ThreadId) -> Option<String>;
    /// Stops Lead's turn (a plan that ran too long).
    async fn stop(&self, thread: &ThreadId);
    fn tracker(&self) -> Option<TaskStore>;
    /// F-TK-04's check: the limits leave runs on (no account, D-2902).
    fn can_execute(&self) -> Result<(), IpcError>;
    /// Why the task can't run from a plan now, if it can't.
    async fn run_block(&self, issue_id: &str) -> Result<Option<RunBlock>, IpcError>;
    /// One task through F-TK-04's queue: `Some` when it was left alone.
    async fn execute(&self, input: ExecuteInput) -> Result<Option<RunBlock>, IpcError>;
    /// The engine a run's independent review uses.
    fn review_engine(&self, engine: &str) -> String;
    fn now(&self) -> i64;
}

#[derive(Serialize, Deserialize)]
struct Entry {
    view: PlanView,
    nonce: String,
    /// The runs `prepare` showed: what Execute all starts. Not kept: after
    /// a restart Execute all shows its runs again.
    #[serde(skip)]
    launch: Option<Vec<PlanRun>>,
}

/// `plans.json` (F-TK-07): every plan and its reply code, so a plan
/// survives a restart.
#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    plans: Vec<Entry>,
}

pub struct PlanService {
    host: Arc<dyn PlanHost>,
    bus: EventBus,
    plans: Mutex<HashMap<String, Entry>>,
    /// Create tasks, one at a time: two clicks make one project.
    creating: Mutex<()>,
    /// Where plans are kept; `None` keeps them in memory.
    file: Option<PathBuf>,
    /// Held from reading the plans to writing them: the last write has
    /// the latest plans.
    saving: Mutex<()>,
}

/// The plans in `path`; none when there's no file or it can't be read. A
/// plan that was starting its runs when Vibezeo closed goes back to
/// created, keeping what it queued.
fn load(path: &Path) -> HashMap<String, Entry> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return HashMap::new(),
        Err(e) => {
            tracing::warn!(code = "plan.unreadable", error = %e, "the saved plans couldn't be read");
            return HashMap::new();
        }
    };
    match serde_json::from_slice::<Saved>(&bytes) {
        Ok(saved) => saved
            .plans
            .into_iter()
            .map(|mut e| {
                e.view.waiting = false;
                if e.view.state == PlanState::Starting {
                    e.view.state = PlanState::Created;
                    e.view.error = Some(
                        "Vibezeo closed while Execute all was starting. The runs it queued are kept."
                            .to_string(),
                    );
                }
                (e.view.id.clone(), e)
            })
            .collect(),
        Err(e) => {
            tracing::warn!(code = "plan.unreadable", error = %e, "the saved plans couldn't be read");
            HashMap::new()
        }
    }
}

fn not_found() -> IpcError {
    IpcError::new(ErrorCode::NotFound, "That plan is gone")
}

/// Execute all on a plan that isn't waiting for it.
fn not_created(state: PlanState) -> IpcError {
    IpcError::new(
        ErrorCode::Invalid,
        match state {
            PlanState::Starting => "Execute all is already starting this plan's tasks",
            PlanState::Started => "This plan's tasks already started",
            _ => "Create the plan's tasks first",
        },
    )
}

impl PlanService {
    /// The service, with the plans kept in `file` (F-TK-07).
    pub fn new(host: Arc<dyn PlanHost>, bus: EventBus, file: Option<PathBuf>) -> Arc<Self> {
        let plans = file.as_deref().map(load).unwrap_or_default();
        Arc::new(Self {
            host,
            bus,
            plans: Mutex::new(plans),
            creating: Mutex::default(),
            file,
            saving: Mutex::new(()),
        })
    }

    /// After a launch: the kept plans' chats get no tasks server again, and
    /// plans whose Lead was planning when Vibezeo closed are read again
    /// (F-TK-07 AC3).
    pub fn resume(self: &Arc<Self>) {
        let kept: Vec<(String, ThreadId, PlanState)> = self
            .lock()
            .values()
            .map(|e| (e.view.id.clone(), e.view.thread_id.clone(), e.view.state))
            .collect();
        for (id, thread, state) in kept {
            self.host.withhold_tasks(&thread);
            if state == PlanState::Planning {
                let me = self.clone();
                tokio::spawn(async move { me.wait(&id, &thread).await });
            }
        }
    }

    /// Writes every plan to the file. Small, so it's written whole.
    fn save(&self) {
        let Some(path) = &self.file else {
            return;
        };
        let _turn = self.saving.lock().unwrap_or_else(|p| p.into_inner());
        let saved = Saved {
            version: 1,
            plans: self
                .lock()
                .values()
                .map(|e| Entry {
                    view: e.view.clone(),
                    nonce: e.nonce.clone(),
                    launch: None,
                })
                .collect(),
        };
        let written = serde_json::to_vec_pretty(&saved)
            .map_err(std::io::Error::other)
            .and_then(|bytes| tandem_core::atomic::atomic_write(path, &bytes, Some(0o600)));
        if let Err(e) = written {
            tracing::warn!(code = "plan.save_failed", error = %e, "the plans weren't saved");
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.plans.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn publish(&self, view: &PlanView) {
        self.bus
            .publish(BusEvent::new(Topic::Tasks, EVENT, view.clone()));
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut PlanView)) -> Option<PlanView> {
        let view = {
            let mut plans = self.lock();
            let e = plans.get_mut(id)?;
            f(&mut e.view);
            e.view.clone()
        };
        self.save();
        self.publish(&view);
        Some(view)
    }

    /// Every plan, newest first.
    pub fn list(&self) -> Vec<PlanView> {
        let mut out: Vec<PlanView> = self.lock().values().map(|e| e.view.clone()).collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        out
    }

    pub fn get(&self, id: &str) -> Result<PlanView, IpcError> {
        self.lock()
            .get(id)
            .map(|e| e.view.clone())
            .ok_or_else(not_found)
    }

    /// `plan_start`: opens Lead's planning chat and waits for its turn in
    /// the background (F-TK-06 AC1, AC2).
    pub async fn start(
        self: &Arc<Self>,
        ws: WorkspaceId,
        goal: String,
    ) -> Result<PlanView, IpcError> {
        let goal = goal.trim().to_string();
        if goal.is_empty() {
            return Err(IpcError::new(ErrorCode::Invalid, "Say what the goal is"));
        }
        if goal.chars().count() > GOAL_MAX {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                format!("A goal is at most {GOAL_MAX} characters"),
            ));
        }
        let root = self.host.local_root(&ws).await?;
        let (lead, lead_engine) = self
            .host
            .lead()
            .await
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "Lead isn't set up yet"))?;
        if !self.host.chat_capable(&lead_engine) {
            return Err(IpcError::new(
                ErrorCode::EngineUnsupported,
                "Lead's engine can't plan in a chat. Give Lead a chat engine in its settings.",
            ));
        }
        let engines = self.host.engines().await;
        if engines.is_empty() {
            return Err(IpcError::new(
                ErrorCode::EngineMissing,
                "No engine that can run tasks is installed",
            ));
        }
        let nonce = nonce();
        let thread = self
            .host
            .start_chat(
                &ws,
                &root,
                &lead,
                &lead_engine,
                format!("Plan: {}", project_name(&goal)),
            )
            .await?;
        self.host.withhold_tasks(&thread);
        let now = self.host.now();
        self.host
            .send(&thread, prompt(&goal, &engines, &nonce, now))
            .await?;
        let view = PlanView {
            id: tandem_core::ids::new_id("pl"),
            goal,
            workspace_id: ws,
            thread_id: thread.clone(),
            state: PlanState::Planning,
            tasks: Vec::new(),
            engines,
            lead_id: lead.clone(),
            lead_engine,
            error: None,
            waiting: false,
            project_id: None,
            issue_ids: Vec::new(),
            queued: Vec::new(),
            skipped: Vec::new(),
            created_at: now,
        };
        self.lock().insert(
            view.id.clone(),
            Entry {
                view: view.clone(),
                nonce,
                launch: None,
            },
        );
        tracing::info!(code = "plan.started", "Lead is planning");
        self.save();
        self.publish(&view);
        let me = self.clone();
        let id = view.id.clone();
        tokio::spawn(async move { me.wait(&id, &thread).await });
        Ok(view)
    }

    /// Follows Lead's turn: says when it waits on you, and stops it past
    /// [`PLAN_TIMEOUT`] of work.
    async fn wait(&self, id: &str, thread: &ThreadId) {
        let mut worked = Duration::ZERO;
        let mut waiting = false;
        let ended: Result<(), String> = loop {
            tokio::time::sleep(POLL).await;
            if !self.lock().contains_key(id) {
                return;
            }
            let state = self.host.state(thread).await;
            if (state == Some(ThreadState::Waiting)) != waiting {
                waiting = !waiting;
                self.update(id, |v| v.waiting = waiting);
            }
            match state {
                Some(ThreadState::Working) => {
                    worked += POLL;
                    if worked >= PLAN_TIMEOUT {
                        self.host.stop(thread).await;
                        break Err(format!(
                            "Lead took more than {} minutes, so Vibezeo stopped it. Its thread keeps what it wrote.",
                            PLAN_TIMEOUT.as_secs() / 60
                        ));
                    }
                }
                Some(ThreadState::Waiting) => {}
                Some(ThreadState::Done) => break Ok(()),
                // Only a launch marks a chat interrupted: it was mid-turn
                // when Vibezeo closed (F-TK-07).
                Some(ThreadState::Interrupted) => {
                    break Err("Vibezeo closed while Lead was planning".to_string())
                }
                _ => break Err("Lead stopped before it finished the plan".to_string()),
            }
        };
        let reply = if ended.is_ok() {
            self.host.last_reply(thread).await.unwrap_or_default()
        } else {
            String::new()
        };
        let (nonce, engines, fallback) = match self.lock().get(id) {
            Some(e) => (
                e.nonce.clone(),
                e.view.engines.clone(),
                e.view.lead_engine.clone(),
            ),
            None => return,
        };
        let outcome = ended.and_then(|()| parse(&reply, &nonce, &engines, &fallback));
        tracing::info!(
            code = "plan.read",
            ok = outcome.is_ok(),
            tasks = outcome.as_ref().map(Vec::len).unwrap_or(0),
            "Lead's plan read"
        );
        self.update(id, |v| {
            v.waiting = false;
            match outcome {
                Ok(tasks) => {
                    v.tasks = tasks;
                    v.state = PlanState::Ready;
                }
                Err(e) => {
                    v.error = Some(e);
                    v.state = PlanState::Failed;
                }
            }
        });
    }

    /// `plan_create_tasks`: the edited rows become To do tasks in a new
    /// project, written as Lead (F-TK-06 AC3). A try that failed partway
    /// keeps its project and tasks, and the next one writes the rest.
    pub fn create_tasks(&self, id: &str, tasks: Vec<PlanTask>) -> Result<PlanView, IpcError> {
        let _one = self.creating.lock().unwrap_or_else(|p| p.into_inner());
        let (goal, engines, lead, written, made, project) = {
            let plans = self.lock();
            let e = plans.get(id).ok_or_else(not_found)?;
            if e.view.state != PlanState::Ready {
                return Err(IpcError::new(
                    ErrorCode::Invalid,
                    "Only a finished draft can become tasks",
                ));
            }
            (
                e.view.goal.clone(),
                e.view.engines.clone(),
                e.view.lead_id.clone(),
                e.view.tasks.clone(),
                e.view.issue_ids.clone(),
                e.view.project_id.clone(),
            )
        };
        if tasks.is_empty() || tasks.len() > TASKS_MAX {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                format!("A plan holds 1 to {TASKS_MAX} tasks"),
            ));
        }
        if tasks.len() < made.len() {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Keep the tasks that were already created",
            ));
        }
        // The rows already written stay as they were written.
        let mut tasks: Vec<PlanTask> = tasks.into_iter().map(tidy).collect();
        for (row, kept) in tasks.iter_mut().zip(written.iter().take(made.len())) {
            *row = kept.clone();
        }
        for t in &tasks {
            if t.title.is_empty() {
                return Err(IpcError::new(
                    ErrorCode::Invalid,
                    "Every task needs a title",
                ));
            }
            if !engines.iter().any(|e| e.engine_id == t.engine_id) {
                return Err(IpcError::new(
                    ErrorCode::Invalid,
                    format!("{} isn't an engine this plan offers", t.engine_id),
                ));
            }
        }
        let tracker = self
            .host
            .tracker()
            .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "The task store isn't open"))?;
        let task_err = |e: tandem_tasks::TaskError| IpcError::new(ErrorCode::Io, e.to_string());
        let project = match project {
            Some(p) => p,
            None => {
                tracker
                    .save_project(None, &project_name(&goal), None)
                    .map_err(task_err)?
                    .id
            }
        };
        // Lead's tasks, written once you approved them.
        let by = Writer {
            actor: lead.as_str().to_string(),
            connection: None,
        };
        let mut ids = made;
        while ids.len() < tasks.len() {
            let t = &tasks[ids.len()];
            let saved = tracker.save_issue(
                SaveIssue {
                    title: Some(t.title.clone()),
                    description: Some(t.description.clone()),
                    status: Some(IssueStatus::Todo),
                    project: Some(Some(project.clone())),
                    suggested_engine: Some(Some(t.engine_id.clone())),
                    ..Default::default()
                },
                &by,
            );
            match saved {
                Ok(issue) => ids.push(issue.id),
                Err(e) => {
                    let err = task_err(e);
                    tracing::warn!(
                        code = "plan.create_failed",
                        created = ids.len(),
                        "a plan's tasks were created partway"
                    );
                    self.update(id, |v| {
                        v.tasks = tasks;
                        v.project_id = Some(project);
                        v.issue_ids = ids;
                        v.error = Some(err.message.clone());
                    });
                    return Err(err);
                }
            }
        }
        tracing::info!(
            code = "plan.created",
            tasks = ids.len(),
            "a plan's tasks were created"
        );
        self.update(id, |v| {
            v.tasks = tasks;
            v.project_id = Some(project);
            v.issue_ids = ids;
            v.error = None;
            v.state = PlanState::Created;
        })
        .ok_or_else(not_found)
    }

    /// `plan_prepare`: what Execute all starts, read now, for you to
    /// confirm (F-TK-06 AC4): each task's engine and the account with the
    /// most room as usage reads now, its review's engine, and the tasks it
    /// leaves alone. Execute all starts exactly these.
    pub async fn prepare(&self, id: &str) -> Result<PlanLaunch, IpcError> {
        let view = self.get(id)?;
        if view.state != PlanState::Created {
            return Err(not_created(view.state));
        }
        let blocked = self.host.can_execute().err().map(|e| e.message);
        let engines = self.host.engines().await;
        let label = |id: &str| {
            engines
                .iter()
                .find(|e| e.engine_id == id)
                .map_or_else(|| id.to_string(), |e| e.label.clone())
        };
        let mut runs = Vec::with_capacity(view.issue_ids.len());
        for (issue, task) in view.issue_ids.iter().zip(&view.tasks) {
            let engine = engines.iter().find(|e| e.engine_id == task.engine_id);
            let account = engine.and_then(roomiest);
            let account_label = engine
                .and_then(|e| {
                    e.accounts
                        .iter()
                        .find(|a| Some(&a.account_id) == account.as_ref())
                })
                .map(|a| a.label.clone());
            let review = self.host.review_engine(&task.engine_id);
            runs.push(PlanRun {
                issue_id: issue.clone(),
                title: task.title.clone(),
                engine_id: task.engine_id.clone(),
                engine_label: label(&task.engine_id),
                account_id: account,
                account_label,
                review_engine_label: label(&review),
                review_engine_id: review,
                skip: self.host.run_block(issue).await?,
            });
        }
        if let Some(e) = self.lock().get_mut(id) {
            e.launch = Some(runs.clone());
        }
        Ok(PlanLaunch { blocked, runs })
    }

    /// `plan_execute`: the runs `prepare` showed, each through F-TK-04's
    /// queue on its engine and account, in a worktree, at Edits, reviewed.
    /// The plan is Starting while they queue, so it starts once. A task
    /// that isn't To do or already has a run is left alone, and what was
    /// queued is kept: a start that fails partway goes back to Created, and
    /// the next one doesn't queue those again (F-TK-06 AC4).
    pub async fn execute(&self, id: &str) -> Result<PlanView, IpcError> {
        let (view, runs) = {
            let mut plans = self.lock();
            let e = plans.get_mut(id).ok_or_else(not_found)?;
            if e.view.state != PlanState::Created {
                return Err(not_created(e.view.state));
            }
            let runs = e.launch.take().ok_or_else(|| {
                IpcError::new(ErrorCode::Invalid, "Check what Execute all starts first")
            })?;
            e.view.state = PlanState::Starting;
            e.view.error = None;
            (e.view.clone(), runs)
        };
        self.save();
        self.publish(&view);
        let back = |err: &IpcError, queued: Vec<String>, skipped: Vec<PlanSkip>| {
            self.update(id, |v| {
                v.state = PlanState::Created;
                v.error = Some(err.message.clone());
                v.queued = queued;
                v.skipped = skipped;
            });
        };
        if let Err(err) = self.host.can_execute() {
            back(&err, view.queued.clone(), view.skipped.clone());
            return Err(err);
        }
        let mut queued = view.queued.clone();
        let mut skipped = Vec::new();
        for run in runs {
            if queued.contains(&run.issue_id) {
                continue;
            }
            let started = self
                .host
                .execute(ExecuteInput {
                    issue_id: run.issue_id.clone(),
                    workspace_id: view.workspace_id.clone(),
                    teammate_id: None,
                    engine_id: Some(run.engine_id),
                    account_id: run.account_id,
                    isolation: Isolation::Lane,
                    access: Access::Edits,
                    full_confirmed: false,
                    review: true,
                    note: None,
                })
                .await;
            match started {
                Ok(None) => queued.push(run.issue_id),
                Ok(Some(reason)) => skipped.push(PlanSkip {
                    issue_id: run.issue_id,
                    reason,
                }),
                Err(err) => {
                    tracing::warn!(
                        code = "plan.execute_failed",
                        queued = queued.len(),
                        "Execute all stopped partway"
                    );
                    back(&err, queued, skipped);
                    return Err(err);
                }
            }
        }
        tracing::info!(
            code = "plan.executed",
            tasks = queued.len(),
            skipped = skipped.len(),
            "a plan's tasks were queued"
        );
        self.update(id, |v| {
            v.queued = queued;
            v.skipped = skipped;
            v.state = PlanState::Started;
        })
        .ok_or_else(not_found)
    }

    /// `plan_discard` (Discard plan, Remove plan): the plan goes; Lead's
    /// thread, its tasks and their runs stay.
    pub fn discard(&self, id: &str) -> Result<(), IpcError> {
        self.lock().remove(id).ok_or_else(not_found)?;
        self.save();
        Ok(())
    }
}

/// The engines a task can run on: installed, with a chat adapter, each with
/// its accounts' fullest usage window as it reads now (D-2431).
pub async fn offered(
    store: &tandem_store::Store,
    engines: &crate::services::engines::EngineService,
    usage: &crate::services::usage::UsageBoard,
) -> Vec<PlanEngine> {
    let accounts = tandem_store::repo::accounts::list(store)
        .await
        .unwrap_or_default();
    engines
        .registry()
        .all()
        .iter()
        .filter(|s| crate::services::threads::has_chat_adapter(s))
        .filter(|s| engines.installed(&s.id).is_some())
        .map(|s| PlanEngine {
            engine_id: s.id.clone(),
            label: s.label.to_string(),
            accounts: accounts
                .iter()
                .filter(|a| a.engine_id == s.id)
                .map(|a| {
                    let view = usage.get(a.id.as_str());
                    let fullest = view
                        .as_ref()
                        .and_then(|v| tandem_engines::usage::fullest(&v.windows).cloned());
                    PlanAccount {
                        account_id: a.id.clone(),
                        label: a.label.clone(),
                        is_default: a.is_default,
                        window: fullest.as_ref().map(|w| w.kind.clone()),
                        used_pct: fullest.as_ref().map(|w| w.used_pct),
                        resets_at: fullest.and_then(|w| w.resets_at),
                    }
                })
                .collect(),
        })
        .collect()
}

/// The app's host.
pub struct AppHost {
    pub app: tauri::AppHandle,
}

impl AppHost {
    fn state(&self) -> tauri::State<'_, crate::state::AppState> {
        use tauri::Manager;
        self.app.state::<crate::state::AppState>()
    }
}

#[async_trait::async_trait]
impl PlanHost for AppHost {
    async fn lead(&self) -> Option<(TeammateId, String)> {
        let state = self.state();
        let all = tandem_store::repo::teammates::list(&state.store)
            .await
            .ok()?;
        all.into_iter()
            .find(|t| t.is_lead)
            .map(|t| (t.id, t.engine_id))
    }

    async fn local_root(&self, ws: &WorkspaceId) -> Result<String, IpcError> {
        let state = self.state();
        let w = tandem_store::repo::workspaces::get(&state.store, ws)
            .await
            .map_err(|_| IpcError::new(ErrorCode::NotFound, "That workspace is gone"))?;
        if w.kind != "local" {
            return Err(IpcError::new(
                ErrorCode::Unavailable,
                "Plans run in folders on this computer for now. Pick a workspace that isn't on an SSH host or in WSL.",
            ));
        }
        Ok(w.root)
    }

    async fn engines(&self) -> Vec<PlanEngine> {
        let state = self.state();
        offered(&state.store, &state.terminals.engines, &state.usage).await
    }

    fn chat_capable(&self, engine: &str) -> bool {
        self.state()
            .terminals
            .engines
            .get(engine)
            .is_some_and(|s| crate::services::threads::has_chat_adapter(&s))
    }

    async fn start_chat(
        &self,
        ws: &WorkspaceId,
        root: &str,
        lead: &TeammateId,
        engine: &str,
        title: String,
    ) -> Result<ThreadId, IpcError> {
        use tandem_store::repo::threads::{ChatExtra, NewThread, ThreadKind};
        let state = self.state();
        let teammate = tandem_store::repo::teammates::get(&state.store, lead)
            .await
            .ok()
            .flatten()
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "Lead isn't set up yet"))?;
        let thread = state
            .threads
            .create_chat(
                NewThread {
                    id: ThreadId::new(),
                    kind: ThreadKind::TeammateChat,
                    workspace_id: Some(ws.clone()),
                    cwd: root.to_string(),
                    engine_id: engine.to_string(),
                    account_id: teammate.preferred_account_id.clone(),
                    model: teammate.model.clone(),
                    effort: teammate.effort.clone(),
                    // Lead reads to plan; the tasks are written after you
                    // approve (D-2430).
                    access: Access::Read,
                    mode: ThreadMode::Act,
                    attended: true,
                },
                ChatExtra {
                    teammate_id: Some(lead.clone()),
                    ..Default::default()
                },
            )
            .await?;
        let _ = state
            .threads
            .update(
                &thread.id,
                tandem_store::repo::threads::ThreadPatch {
                    title: Some(title),
                    ..Default::default()
                },
            )
            .await;
        Ok(thread.id)
    }

    fn withhold_tasks(&self, thread: &ThreadId) {
        self.state()
            .tasks
            .withhold(&crate::services::threads::session_key(thread));
    }

    async fn send(&self, thread: &ThreadId, text: String) -> Result<(), IpcError> {
        self.state()
            .threads
            .send(thread, text, Vec::new(), false)
            .await
            .map(|_| ())
    }

    async fn state(&self, thread: &ThreadId) -> Option<ThreadState> {
        tandem_store::repo::threads::get(&self.state().store, thread)
            .await
            .ok()
            .flatten()
            .map(|t| t.state)
    }

    async fn last_reply(&self, thread: &ThreadId) -> Option<String> {
        crate::services::second_opinion::last_reply(&self.state().store, thread).await
    }

    async fn stop(&self, thread: &ThreadId) {
        if let Err(e) = self.state().threads.stop(thread).await {
            tracing::warn!(code = "plan.stop_failed", error = %e.message, "Lead's turn didn't stop");
        }
    }

    fn tracker(&self) -> Option<TaskStore> {
        self.state().tasks.tracker.clone()
    }

    fn can_execute(&self) -> Result<(), IpcError> {
        self.state().task_exec.can_execute()
    }

    async fn run_block(&self, issue_id: &str) -> Result<Option<RunBlock>, IpcError> {
        self.state().task_exec.run_block(issue_id, true).await
    }

    async fn execute(&self, input: ExecuteInput) -> Result<Option<RunBlock>, IpcError> {
        use crate::services::tasks_exec::Started;
        Ok(match self.state().task_exec.execute_planned(input).await? {
            Started::Queued(_) => None,
            Started::Blocked(block) => Some(block),
        })
    }

    fn review_engine(&self, engine: &str) -> String {
        self.state().task_exec.review_engine(engine)
    }

    fn now(&self) -> i64 {
        self.state().clock.now_ms()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[derive(Default)]
    struct Fake {
        chats: Mutex<Vec<(WorkspaceId, String, String)>>,
        sent: Mutex<Vec<String>>,
        state: Mutex<Option<ThreadState>>,
        reply: Mutex<String>,
        tracker: Option<TaskStore>,
        /// The limits turn runs off.
        off: bool,
        /// What usage reads now, when it moved since Lead planned.
        engines: Mutex<Option<Vec<PlanEngine>>>,
        executed: Mutex<Vec<ExecuteInput>>,
        /// Tasks a plan can't run, and why.
        blocked: Mutex<HashMap<String, RunBlock>>,
        /// Tasks whose run fails to start.
        failing: Mutex<HashSet<String>>,
        withheld: Mutex<Vec<ThreadId>>,
        stopped: Mutex<Vec<ThreadId>>,
    }

    fn account(id: &str, used: Option<f64>, default: bool) -> PlanAccount {
        PlanAccount {
            account_id: AccountId::from_db(id.to_string()),
            label: id.to_string(),
            is_default: default,
            window: used.map(|_| "five_hour".to_string()),
            used_pct: used,
            resets_at: used.map(|_| T0 + 130 * 60_000),
        }
    }

    const T0: i64 = 1_790_000_000_000;

    fn offered() -> Vec<PlanEngine> {
        vec![
            PlanEngine {
                engine_id: "claude".into(),
                label: "Claude Code".into(),
                accounts: vec![
                    account("work", Some(62.0), true),
                    account("home", Some(10.0), false),
                ],
            },
            PlanEngine {
                engine_id: "codex".into(),
                label: "Codex".into(),
                accounts: vec![account("cx", None, true)],
            },
        ]
    }

    fn plan_thread() -> ThreadId {
        ThreadId::from_db("th_plan".to_string())
    }

    #[async_trait::async_trait]
    impl PlanHost for Fake {
        async fn lead(&self) -> Option<(TeammateId, String)> {
            Some((TeammateId::from_db("tm_lead".to_string()), "claude".into()))
        }
        async fn local_root(&self, ws: &WorkspaceId) -> Result<String, IpcError> {
            if ws.as_str() == "ws_ssh" {
                return Err(IpcError::new(ErrorCode::Unavailable, "remote"));
            }
            Ok("/r/app".into())
        }
        async fn engines(&self) -> Vec<PlanEngine> {
            self.engines
                .lock()
                .expect("l")
                .clone()
                .unwrap_or_else(offered)
        }
        fn chat_capable(&self, _: &str) -> bool {
            true
        }
        async fn start_chat(
            &self,
            ws: &WorkspaceId,
            root: &str,
            _: &TeammateId,
            _: &str,
            title: String,
        ) -> Result<ThreadId, IpcError> {
            self.chats
                .lock()
                .expect("l")
                .push((ws.clone(), root.to_string(), title));
            *self.state.lock().expect("l") = Some(ThreadState::Working);
            Ok(plan_thread())
        }
        fn withhold_tasks(&self, thread: &ThreadId) {
            self.withheld.lock().expect("l").push(thread.clone());
        }
        async fn send(&self, _: &ThreadId, text: String) -> Result<(), IpcError> {
            self.sent.lock().expect("l").push(text);
            Ok(())
        }
        async fn state(&self, _: &ThreadId) -> Option<ThreadState> {
            *self.state.lock().expect("l")
        }
        async fn last_reply(&self, _: &ThreadId) -> Option<String> {
            Some(self.reply.lock().expect("l").clone())
        }
        async fn stop(&self, thread: &ThreadId) {
            self.stopped.lock().expect("l").push(thread.clone());
            *self.state.lock().expect("l") = Some(ThreadState::Idle);
        }
        fn tracker(&self) -> Option<TaskStore> {
            self.tracker.clone()
        }
        fn can_execute(&self) -> Result<(), IpcError> {
            if self.off {
                Err(IpcError::new(
                    ErrorCode::ForbiddenPlan,
                    "Running tasks is turned off for this account. You can still track tasks.",
                ))
            } else {
                Ok(())
            }
        }
        async fn run_block(&self, issue_id: &str) -> Result<Option<RunBlock>, IpcError> {
            Ok(self.blocked.lock().expect("l").get(issue_id).copied())
        }
        async fn execute(&self, input: ExecuteInput) -> Result<Option<RunBlock>, IpcError> {
            if self.failing.lock().expect("l").contains(&input.issue_id) {
                return Err(IpcError::new(ErrorCode::Io, "The disk is full"));
            }
            let mut blocked = self.blocked.lock().expect("l");
            if let Some(b) = blocked.get(&input.issue_id) {
                return Ok(Some(*b));
            }
            // Its run is under way now.
            blocked.insert(input.issue_id.clone(), RunBlock::Running);
            self.executed.lock().expect("l").push(input);
            Ok(None)
        }
        fn review_engine(&self, engine: &str) -> String {
            if engine == "codex" { "claude" } else { "codex" }.into()
        }
        fn now(&self) -> i64 {
            T0
        }
    }

    fn rig(off: bool) -> (tempfile::TempDir, Arc<Fake>, Arc<PlanService>, EventBus) {
        let dir = tempfile::tempdir().expect("dir");
        let fake = Arc::new(Fake {
            tracker: Some(TaskStore::open(dir.path().join("tasks")).expect("tracker")),
            off,
            ..Fake::default()
        });
        let bus = EventBus::new();
        let file = dir.path().join("plans.json");
        (
            dir,
            fake.clone(),
            PlanService::new(fake, bus.clone(), Some(file)),
            bus,
        )
    }

    /// The app again, after a restart: the same plans file, and a host
    /// whose Lead chat is in `state` with `reply`.
    fn restarted(
        dir: &tempfile::TempDir,
        state: ThreadState,
        reply: String,
    ) -> (Arc<Fake>, Arc<PlanService>) {
        let fake = Arc::new(Fake {
            tracker: Some(TaskStore::open(dir.path().join("tasks")).expect("tracker")),
            ..Fake::default()
        });
        *fake.state.lock().expect("l") = Some(state);
        *fake.reply.lock().expect("l") = reply;
        let file = dir.path().join("plans.json");
        let plans = PlanService::new(fake.clone(), EventBus::new(), Some(file));
        (fake, plans)
    }

    #[tokio::test]
    async fn f_tk_07_3_plans_are_kept_across_a_restart_until_removed() {
        let (dir, fake, plans, _bus) = rig(false);
        let view = created(&fake, &plans).await;
        let (fake2, after) = restarted(&dir, ThreadState::Done, String::new());
        let kept = after.get(&view.id).expect("kept");
        assert_eq!(kept.state, PlanState::Created);
        assert_eq!(kept.issue_ids, view.issue_ids);
        assert_eq!(kept.tasks, view.tasks);
        assert_eq!(after.list().len(), 1);
        // Its chat gets no tasks server after the restart either.
        after.resume();
        assert_eq!(*fake2.withheld.lock().expect("l"), vec![plan_thread()]);
        // Remove plan forgets it there too; the tasks stay.
        after.discard(&view.id).expect("remove");
        let (_fake3, again) = restarted(&dir, ThreadState::Done, String::new());
        assert!(again.list().is_empty());
        let tracker = fake.tracker.clone().expect("tracker");
        assert_eq!(tracker.all().expect("all").len(), 2);
        // A damaged file starts with no plans rather than failing.
        std::fs::write(dir.path().join("plans.json"), b"{not json").expect("damage");
        let (_fake4, damaged) = restarted(&dir, ThreadState::Done, String::new());
        assert!(damaged.list().is_empty());
    }

    #[tokio::test]
    async fn f_tk_07_3_a_plan_interrupted_by_a_restart_fails_with_its_reason() {
        let (dir, _fake, plans, _bus) = rig(false);
        let view = plans
            .start(WorkspaceId::from_db("ws_1".to_string()), "Add OAuth".into())
            .await
            .expect("start");
        // Vibezeo closes while Lead plans; the launch marks the chat
        // interrupted.
        let (_fake2, after) = restarted(&dir, ThreadState::Interrupted, String::new());
        assert_eq!(
            after.get(&view.id).expect("kept").state,
            PlanState::Planning
        );
        after.resume();
        let failed = finished(&after, &view.id).await;
        assert_eq!(failed.state, PlanState::Failed);
        assert_eq!(
            failed.error.as_deref(),
            Some("Vibezeo closed while Lead was planning")
        );
    }

    #[tokio::test]
    async fn f_tk_07_3_a_plan_whose_lead_finished_while_closed_is_read() {
        let (dir, fake, plans, _bus) = rig(false);
        let view = plans
            .start(WorkspaceId::from_db("ws_1".to_string()), "Add OAuth".into())
            .await
            .expect("start");
        let n = nonce_of(&fake.sent.lock().expect("l")[0]);
        let (_fake2, after) = restarted(
            &dir,
            ThreadState::Done,
            reply(&n, r#"{"tasks": [{"title": "Routes", "engine": "codex"}]}"#),
        );
        after.resume();
        let ready = finished(&after, &view.id).await;
        assert_eq!(ready.state, PlanState::Ready);
        assert_eq!(ready.tasks[0].title, "Routes");
    }

    #[tokio::test]
    async fn f_tk_07_3_a_plan_closed_while_starting_its_runs_goes_back_keeping_what_it_queued() {
        let (dir, fake, plans, _bus) = rig(false);
        let view = created(&fake, &plans).await;
        plans.prepare(&view.id).await.expect("prepare");
        plans.execute(&view.id).await.expect("execute");
        // As if Vibezeo closed before the plan left Starting.
        let file = dir.path().join("plans.json");
        let text = std::fs::read_to_string(&file).expect("kept");
        std::fs::write(&file, text.replace("\"started\"", "\"starting\"")).expect("write");
        let (_fake2, after) = restarted(&dir, ThreadState::Done, String::new());
        let back = after.get(&view.id).expect("kept");
        assert_eq!(back.state, PlanState::Created);
        assert_eq!(back.queued, vec!["TAN-1".to_string(), "TAN-2".to_string()]);
        assert!(back.error.is_some());
    }

    fn nonce_of(prompt: &str) -> String {
        prompt
            .split("PLAN-BEGIN-")
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
            .expect("nonce")
            .to_string()
    }

    fn reply(nonce: &str, json: &str) -> String {
        format!("I read the code.\nPLAN-BEGIN-{nonce}\n{json}\nPLAN-END-{nonce}\nThat's the plan.")
    }

    async fn finished(plans: &PlanService, id: &str) -> PlanView {
        for _ in 0..40 {
            let v = plans.get(id).expect("plan");
            if v.state != PlanState::Planning {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the plan never finished");
    }

    #[tokio::test]
    async fn f_tk_06_1_plan_opens_lead_in_the_workspace_with_the_goal_engines_and_markers() {
        let (_d, fake, plans, _bus) = rig(false);
        let ws = WorkspaceId::from_db("ws_1".to_string());
        let view = plans
            .start(
                ws.clone(),
                "  Add OAuth sign-in\nwith Google and GitHub  ".into(),
            )
            .await
            .expect("start");
        assert_eq!(view.state, PlanState::Planning);
        let chats = fake.chats.lock().expect("l").clone();
        assert_eq!(
            chats,
            vec![(
                ws,
                "/r/app".to_string(),
                "Plan: Add OAuth sign-in".to_string()
            )]
        );
        // Lead's planning chat has no tasks tools.
        assert_eq!(*fake.withheld.lock().expect("l"), vec![plan_thread()]);
        let sent = fake.sent.lock().expect("l")[0].clone();
        assert!(
            sent.contains("Add OAuth sign-in\nwith Google and GitHub"),
            "{sent}"
        );
        assert!(sent.contains(
            "- `claude` (Claude Code): account \"work\": 62% of the five_hour window used, resets in 2 h 10 min; account \"home\": 10%"
        ), "{sent}");
        assert!(
            sent.contains("- `codex` (Codex): account \"cx\": no usage reading"),
            "{sent}"
        );
        let n = nonce_of(&sent);
        assert_eq!(n.len(), 8);
        assert!(sent.contains(&format!("PLAN-END-{n}")));
        // Nothing is written while Lead plans.
        let tracker = fake.tracker.clone().expect("tracker");
        assert!(tracker.all().expect("all").is_empty());
        // A goal that's empty, too long, or in a remote folder is refused.
        assert!(plans
            .start(WorkspaceId::from_db("ws_1".to_string()), " ".into())
            .await
            .is_err());
        let long = "x".repeat(GOAL_MAX + 1);
        assert!(plans
            .start(WorkspaceId::from_db("ws_1".to_string()), long)
            .await
            .is_err());
        let remote = plans
            .start(WorkspaceId::from_db("ws_ssh".to_string()), "goal".into())
            .await
            .expect_err("remote");
        assert_eq!(remote.code, ErrorCode::Unavailable);
    }

    #[test]
    fn f_tk_06_2_a_reply_becomes_tasks_an_unoffered_engine_falls_back_and_no_block_fails() {
        let engines = offered();
        let json = r#"{"tasks": [
            {"title": "Add the OAuth routes", "description": "Done when /auth/google works.", "engine": "codex", "why": "Codex has room"},
            {"title": "  Write the sign-in page  ", "engine": "gemini"}
        ]}"#;
        let tasks = parse(&reply("ab12cd34", json), "ab12cd34", &engines, "claude").expect("plan");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].engine_id, "codex");
        assert!(!tasks[0].fell_back);
        assert_eq!(tasks[0].why.as_deref(), Some("Codex has room"));
        assert_eq!(tasks[1].title, "Write the sign-in page");
        assert_eq!(
            (tasks[1].engine_id.as_str(), tasks[1].fell_back),
            ("claude", true)
        );
        // A fenced block reads the same; another code's markers don't count.
        let fenced = reply("ab12cd34", &format!("```json\n{json}\n```"));
        assert_eq!(
            parse(&fenced, "ab12cd34", &engines, "claude")
                .expect("fenced")
                .len(),
            2
        );
        let none = parse(&reply("ffffffff", json), "ab12cd34", &engines, "claude");
        assert_eq!(none, Err("Lead's reply had no plan".to_string()));
        assert!(parse("No plan here.", "ab12cd34", &engines, "claude").is_err());
        let thirteen: Vec<String> = (0..13).map(|i| format!(r#"{{"title": "t{i}"}}"#)).collect();
        let many = reply(
            "ab12cd34",
            &format!(r#"{{"tasks": [{}]}}"#, thirteen.join(",")),
        );
        assert!(parse(&many, "ab12cd34", &engines, "claude")
            .expect_err("13")
            .contains("1 to 12"));
        let untitled = reply("ab12cd34", r#"{"tasks": [{"title": " "}]}"#);
        assert!(parse(&untitled, "ab12cd34", &engines, "claude").is_err());
    }

    #[test]
    fn f_tk_06_2_the_plan_is_found_past_later_mentions_of_the_markers_and_its_text_is_cleaned() {
        let engines = offered();
        let n = "ab12cd34";
        let raw = serde_json::json!({"tasks": [
            {
                "title": "Add\u{202E} the\u{2066} routes\u{E0041}\u{E007F}",
                "description": "Line one\u{7}\r\nLine two\u{200F}\u{9B}",
                "engine": "codex",
                "why": "Room\u{0} left"
            },
            {"title": "Two\nlines\tand  a tab", "engine": "codex"}
        ]})
        .to_string();
        // The plan, then Lead names the markers again.
        let text = format!(
            "{}\nI put the plan between PLAN-BEGIN-{n} and PLAN-END-{n}.",
            reply(n, &raw)
        );
        let tasks = parse(&text, n, &engines, "claude").expect("plan");
        assert_eq!(tasks[0].title, "Add the routes");
        assert_eq!(tasks[0].description, "Line one\nLine two");
        assert_eq!(tasks[0].why.as_deref(), Some("Room left"));
        assert_eq!(tasks[1].title, "Two lines and a tab");
        // Markers inside a line read too.
        let inline = format!("Here: PLAN-BEGIN-{n} {raw} PLAN-END-{n} done.");
        assert_eq!(
            parse(&inline, n, &engines, "claude").expect("inline").len(),
            2
        );
        // A reply too long to read isn't parsed at all.
        let long = reply(n, &format!("{raw}{}", " ".repeat(REPLY_MAX)));
        assert_eq!(
            parse(&long, n, &engines, "claude"),
            Err("Lead's reply is too long to read as a plan".to_string())
        );
        // A title of nothing but hidden characters is no title.
        let hidden = reply(
            n,
            &serde_json::json!({"tasks": [{"title": "\u{202E}\u{E0041}"}]}).to_string(),
        );
        assert_eq!(
            parse(&hidden, n, &engines, "claude"),
            Err("A task in Lead's plan has no title".to_string())
        );
    }

    /// Lead's draft of two tasks: Routes on Claude, Page on Codex.
    async fn drafted(fake: &Arc<Fake>, plans: &Arc<PlanService>) -> PlanView {
        let view = plans
            .start(WorkspaceId::from_db("ws_1".to_string()), "Add OAuth".into())
            .await
            .expect("start");
        let n = nonce_of(&fake.sent.lock().expect("l")[0]);
        *fake.reply.lock().expect("l") = reply(
            &n,
            r#"{"tasks": [{"title": "Routes", "engine": "claude"}, {"title": "Page", "description": "The page.", "engine": "codex"}]}"#,
        );
        *fake.state.lock().expect("l") = Some(ThreadState::Done);
        finished(plans, &view.id).await
    }

    async fn created(fake: &Arc<Fake>, plans: &Arc<PlanService>) -> PlanView {
        let draft = drafted(fake, plans).await;
        plans.create_tasks(&draft.id, draft.tasks).expect("create")
    }

    #[tokio::test]
    async fn f_tk_06_2_3_lead_finishing_makes_a_draft_and_create_tasks_writes_them_as_lead() {
        let (_d, fake, plans, bus) = rig(false);
        let mut rx = bus.subscribe(Topic::Tasks);
        let draft = drafted(&fake, &plans).await;
        assert_eq!(draft.state, PlanState::Ready);
        assert_eq!(draft.tasks.len(), 2);
        let ev = rx.try_recv().expect("published");
        assert_eq!(ev.name, EVENT);
        let tracker = fake.tracker.clone().expect("tracker");
        assert!(
            tracker.all().expect("all").is_empty(),
            "nothing before Create tasks"
        );
        // The user renamed one, moved it to Codex, and hid nothing in it.
        let mut edited = draft.tasks.clone();
        edited[0].title = "OAuth\u{202E} routes".into();
        edited[0].engine_id = "codex".into();
        let created = plans.create_tasks(&draft.id, edited).expect("create");
        assert_eq!(created.state, PlanState::Created);
        assert_eq!(
            created.issue_ids,
            vec!["TAN-1".to_string(), "TAN-2".to_string()]
        );
        let first = tracker.get("TAN-1").expect("TAN-1");
        assert_eq!(first.title, "OAuth routes");
        assert_eq!(first.status, IssueStatus::Todo);
        assert_eq!(first.suggested_engine.as_deref(), Some("codex"));
        assert_eq!(first.project, created.project_id);
        // Lead's tasks: their history says Lead wrote them.
        assert_eq!(
            tracker.history("TAN-1").expect("history")[0].actor,
            "tm_lead"
        );
        let project = tracker
            .meta()
            .expect("meta")
            .projects
            .into_iter()
            .find(|p| Some(&p.id) == created.project_id.as_ref())
            .expect("project");
        assert_eq!(project.name, "Add OAuth");
        assert_eq!(
            tracker.get("TAN-2").expect("TAN-2").description,
            "The page."
        );
        // Created once; an engine that wasn't offered is refused.
        assert!(plans
            .create_tasks(&draft.id, created.tasks.clone())
            .is_err());
    }

    #[tokio::test]
    async fn f_tk_06_3_create_tasks_again_after_a_failure_writes_nothing_twice() {
        let (dir, fake, plans, _bus) = rig(false);
        let draft = drafted(&fake, &plans).await;
        // The second task's file can't be written.
        let blocker = dir.path().join("tasks").join("issues").join("ISSUE-2.json");
        std::fs::create_dir_all(&blocker).expect("blocker");
        plans
            .create_tasks(&draft.id, draft.tasks.clone())
            .expect_err("partway");
        let partial = plans.get(&draft.id).expect("plan");
        assert_eq!(partial.state, PlanState::Ready);
        assert_eq!(partial.issue_ids, vec!["TAN-1".to_string()]);
        assert!(partial.project_id.is_some() && partial.error.is_some());
        // A row already written can't be dropped.
        assert!(plans.create_tasks(&draft.id, Vec::new()).is_err());
        std::fs::remove_dir(&blocker).expect("unblock");
        let created = plans
            .create_tasks(&draft.id, draft.tasks.clone())
            .expect("again");
        assert_eq!(created.state, PlanState::Created);
        assert_eq!(created.project_id, partial.project_id);
        assert_eq!(created.issue_ids.len(), 2);
        assert_eq!(created.issue_ids[0], "TAN-1");
        assert_eq!(created.error, None);
        let tracker = fake.tracker.clone().expect("tracker");
        assert_eq!(tracker.all().expect("all").len(), 2, "no task twice");
        assert_eq!(
            tracker.meta().expect("meta").projects.len(),
            1,
            "one project"
        );
    }

    #[tokio::test]
    async fn f_tk_06_2_a_reply_without_a_plan_fails_it() {
        let (_d, fake, plans, _bus) = rig(false);
        let view = plans
            .start(WorkspaceId::from_db("ws_1".to_string()), "Add OAuth".into())
            .await
            .expect("start");
        *fake.reply.lock().expect("l") = "I'd rather chat about it.".into();
        *fake.state.lock().expect("l") = Some(ThreadState::Done);
        let failed = finished(&plans, &view.id).await;
        assert_eq!(failed.state, PlanState::Failed);
        assert_eq!(failed.error.as_deref(), Some("Lead's reply had no plan"));
        assert!(plans.create_tasks(&view.id, Vec::new()).is_err());
        plans.discard(&view.id).expect("discard");
        assert!(plans.get(&view.id).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn f_tk_06_2_lead_waiting_on_you_says_so_and_a_plan_that_runs_too_long_stops() {
        let (_d, fake, plans, _bus) = rig(false);
        let view = plans
            .start(WorkspaceId::from_db("ws_1".to_string()), "Add OAuth".into())
            .await
            .expect("start");
        // Lead asks you something: the plan says so, for as long as you take.
        *fake.state.lock().expect("l") = Some(ThreadState::Waiting);
        tokio::time::sleep(PLAN_TIMEOUT * 2).await;
        let waiting = plans.get(&view.id).expect("plan");
        assert_eq!(
            (waiting.state, waiting.waiting),
            (PlanState::Planning, true)
        );
        // Lead works on, past the limit: it's stopped and the plan fails.
        *fake.state.lock().expect("l") = Some(ThreadState::Working);
        tokio::time::sleep(POLL * 2).await;
        assert!(!plans.get(&view.id).expect("plan").waiting);
        tokio::time::sleep(PLAN_TIMEOUT).await;
        let failed = plans.get(&view.id).expect("plan");
        assert_eq!(failed.state, PlanState::Failed);
        assert_eq!(
            failed.error.as_deref(),
            Some("Lead took more than 30 minutes, so Vibezeo stopped it. Its thread keeps what it wrote.")
        );
        assert_eq!(*fake.stopped.lock().expect("l"), vec![plan_thread()]);
    }

    #[tokio::test]
    async fn f_tk_06_4_execute_all_shows_its_runs_first_then_starts_what_it_showed() {
        let (_d, fake, plans, _bus) = rig(false);
        let view = created(&fake, &plans).await;
        // Not without seeing the runs first.
        let unchecked = plans.execute(&view.id).await.expect_err("unchecked");
        assert_eq!(unchecked.code, ErrorCode::Invalid);
        assert!(fake.executed.lock().expect("l").is_empty());
        // Usage moved since Lead planned: the runs use what it reads now.
        let mut now = offered();
        now[0].accounts = vec![
            account("work", Some(5.0), true),
            account("home", Some(70.0), false),
        ];
        *fake.engines.lock().expect("l") = Some(now);
        let launch = plans.prepare(&view.id).await.expect("prepare");
        assert_eq!(launch.blocked, None);
        let shown: Vec<_> = launch
            .runs
            .iter()
            .map(|r| {
                (
                    r.issue_id.as_str(),
                    r.title.as_str(),
                    r.engine_label.as_str(),
                    r.account_label.as_deref(),
                    r.review_engine_label.as_str(),
                    r.skip,
                )
            })
            .collect();
        assert_eq!(
            shown,
            vec![
                (
                    "TAN-1",
                    "Routes",
                    "Claude Code",
                    Some("work"),
                    "Codex",
                    None
                ),
                ("TAN-2", "Page", "Codex", Some("cx"), "Claude Code", None),
            ]
        );
        let started = plans.execute(&view.id).await.expect("execute");
        assert_eq!(started.state, PlanState::Started);
        assert_eq!(
            started.queued,
            vec!["TAN-1".to_string(), "TAN-2".to_string()]
        );
        assert!(started.skipped.is_empty());
        let runs = fake.executed.lock().expect("l").clone();
        assert_eq!(runs.len(), 2);
        assert_eq!(
            (
                runs[0].issue_id.as_str(),
                runs[0].engine_id.as_deref(),
                runs[0].account_id.as_ref().map(|a| a.as_str())
            ),
            ("TAN-1", Some("claude"), Some("work"))
        );
        assert_eq!(
            (
                runs[1].engine_id.as_deref(),
                runs[1].account_id.as_ref().map(|a| a.as_str())
            ),
            (Some("codex"), Some("cx"))
        );
        for r in &runs {
            assert_eq!(r.isolation, Isolation::Lane);
            assert_eq!(r.access, Access::Edits);
            assert!(r.review && !r.full_confirmed && r.teammate_id.is_none());
        }
        assert!(plans.execute(&view.id).await.is_err(), "once");
        assert!(plans.prepare(&view.id).await.is_err(), "started");
    }

    #[tokio::test]
    async fn f_tk_06_4_execute_all_leaves_alone_tasks_that_cant_run_and_starts_once() {
        let (_d, fake, plans, _bus) = rig(false);
        let view = created(&fake, &plans).await;
        // TAN-1 already runs from its own Execute…, and TAN-2 is In progress.
        fake.blocked.lock().expect("l").extend([
            ("TAN-1".to_string(), RunBlock::Running),
            ("TAN-2".to_string(), RunBlock::NotToDo),
        ]);
        let launch = plans.prepare(&view.id).await.expect("prepare");
        assert_eq!(
            launch.runs.iter().map(|r| r.skip).collect::<Vec<_>>(),
            vec![Some(RunBlock::Running), Some(RunBlock::NotToDo)]
        );
        // Two clicks: one start.
        let (a, b) = tokio::join!(plans.execute(&view.id), plans.execute(&view.id));
        assert!(a.is_ok() != b.is_ok(), "one start");
        let started = plans.get(&view.id).expect("plan");
        assert_eq!(started.state, PlanState::Started);
        assert!(started.queued.is_empty());
        assert_eq!(
            started.skipped,
            vec![
                PlanSkip {
                    issue_id: "TAN-1".into(),
                    reason: RunBlock::Running
                },
                PlanSkip {
                    issue_id: "TAN-2".into(),
                    reason: RunBlock::NotToDo
                },
            ]
        );
        assert!(fake.executed.lock().expect("l").is_empty());
    }

    #[tokio::test]
    async fn f_tk_06_4_a_start_that_fails_partway_keeps_what_it_queued_and_runs_nothing_twice() {
        let (_d, fake, plans, _bus) = rig(false);
        let view = created(&fake, &plans).await;
        fake.failing.lock().expect("l").insert("TAN-2".into());
        plans.prepare(&view.id).await.expect("prepare");
        let err = plans.execute(&view.id).await.expect_err("partway");
        assert_eq!(err.message, "The disk is full");
        let back = plans.get(&view.id).expect("plan");
        assert_eq!(back.state, PlanState::Created);
        assert_eq!(back.queued, vec!["TAN-1".to_string()]);
        assert_eq!(back.error.as_deref(), Some("The disk is full"));
        // Again: TAN-1 shows as running and isn't queued twice.
        fake.failing.lock().expect("l").clear();
        let launch = plans.prepare(&view.id).await.expect("prepare");
        assert_eq!(
            launch.runs.iter().map(|r| r.skip).collect::<Vec<_>>(),
            vec![Some(RunBlock::Running), None]
        );
        let started = plans.execute(&view.id).await.expect("execute");
        assert_eq!(started.state, PlanState::Started);
        assert_eq!(
            started.queued,
            vec!["TAN-1".to_string(), "TAN-2".to_string()]
        );
        assert!(started.skipped.is_empty() && started.error.is_none());
        let runs: Vec<String> = fake
            .executed
            .lock()
            .expect("l")
            .iter()
            .map(|r| r.issue_id.clone())
            .collect();
        assert_eq!(runs, vec!["TAN-1", "TAN-2"]);
    }

    #[tokio::test]
    async fn f_tk_06_4_when_the_limits_turn_runs_off_execute_all_says_why_and_starts_nothing() {
        let (_d, fake, plans, _bus) = rig(true);
        let view = created(&fake, &plans).await;
        let launch = plans.prepare(&view.id).await.expect("prepare");
        assert_eq!(
            launch.blocked.as_deref(),
            Some("Running tasks is turned off for this account. You can still track tasks.")
        );
        let err = plans.execute(&view.id).await.expect_err("off");
        assert_eq!(err.code, ErrorCode::ForbiddenPlan);
        assert!(fake.executed.lock().expect("l").is_empty());
        assert_eq!(plans.get(&view.id).expect("plan").state, PlanState::Created);
    }

    #[test]
    fn f_tk_06_4_the_roomiest_account_has_room_and_no_reading_beats_a_full_one() {
        let pick = |accounts: Vec<PlanAccount>| {
            roomiest(&PlanEngine {
                engine_id: "claude".into(),
                label: "Claude Code".into(),
                accounts,
            })
            .map(|a| a.as_str().to_string())
        };
        // Room left wins, the lowest window first.
        assert_eq!(
            pick(vec![
                account("full", Some(100.0), true),
                account("unread", None, false),
                account("most", Some(80.0), false),
                account("half", Some(50.0), false),
            ]),
            Some("half".into())
        );
        // No reading isn't no room: it beats a full account, the default first.
        assert_eq!(
            pick(vec![
                account("full", Some(100.0), true),
                account("a", None, false),
                account("b", None, true),
            ]),
            Some("b".into())
        );
        assert_eq!(
            pick(vec![
                account("full", Some(100.0), true),
                account("a", None, false),
            ]),
            Some("a".into())
        );
        // All full: the one that resets first.
        let mut late = account("late", Some(100.0), true);
        late.resets_at = Some(T0 + 90 * 60_000);
        let mut soon = account("soon", Some(120.0), false);
        soon.resets_at = Some(T0 + 10 * 60_000);
        assert_eq!(pick(vec![late, soon]), Some("soon".into()));
        assert_eq!(pick(Vec::new()), None);
    }

    #[test]
    fn f_tk_06_names_and_accounts() {
        assert_eq!(
            project_name("\n  Fix the login bug  \nmore"),
            "Fix the login bug"
        );
        let long = "word ".repeat(30);
        let name = project_name(&long);
        assert!(name.ends_with('…') && name.chars().count() <= PROJECT_NAME_MAX + 1);
        assert_eq!(resets_in(T0 + 5 * 60_000, T0), "resets in 5 min");
    }
}
