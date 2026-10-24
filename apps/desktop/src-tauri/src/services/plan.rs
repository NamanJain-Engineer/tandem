//! F-TK-06 Plan with Lead (bold bet B3, first slice): Lead turns a goal into
//! tasks, each with a suggested engine. Lead plans in its own chat in the
//! workspace; its reply's JSON block, between markers with a fresh code,
//! becomes a draft the user edits. Create tasks writes them into the tracker
//! (F-TK-01) and Execute all runs them through F-TK-04's queue (D-2430).

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

use crate::services::tasks_exec::{ExecuteInput, Isolation};

pub const EVENT: &str = "tasks:plan";
pub const GOAL_MAX: usize = 4_000;
pub const TASKS_MAX: usize = 12;
pub const TITLE_MAX: usize = 200;
pub const DESCRIPTION_MAX: usize = 8_000;
pub const WHY_MAX: usize = 300;
/// A project's name is the goal's first line, cut here.
pub const PROJECT_NAME_MAX: usize = 80;
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
    pub project_id: Option<String>,
    pub issue_ids: Vec<String>,
    pub created_at: i64,
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

/// The plan in Lead's reply: the block between the last begin marker and
/// the end marker after it.
pub fn parse(
    reply: &str,
    nonce: &str,
    engines: &[PlanEngine],
    fallback: &str,
) -> Result<Vec<PlanTask>, String> {
    let none = || "Lead's reply had no plan".to_string();
    let (b, e) = (begin(nonce), end(nonce));
    let start = reply.rfind(&b).ok_or_else(none)? + b.len();
    let stop = reply[start..].find(&e).ok_or_else(none)? + start;
    let mut body = reply[start..stop].trim();
    // A model may wrap the JSON in a code fence.
    if let Some(rest) = body.strip_prefix("```") {
        body = rest
            .split_once('\n')
            .map(|(_, r)| r)
            .unwrap_or("")
            .trim_end()
            .trim_end_matches('`')
            .trim();
    }
    let parsed: Reply = serde_json::from_str(body).map_err(|_| none())?;
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
            let title = cut(t.title.trim(), TITLE_MAX);
            if title.is_empty() {
                return Err("A task in Lead's plan has no title".to_string());
            }
            let engine = t.engine.trim();
            let fell_back = !offered(engine);
            Ok(PlanTask {
                title,
                description: cut(t.description.trim(), DESCRIPTION_MAX),
                engine_id: if fell_back { fallback } else { engine }.to_string(),
                why: t
                    .why
                    .map(|w| cut(w.trim(), WHY_MAX))
                    .filter(|w| !w.is_empty()),
                fell_back,
            })
        })
        .collect()
}

/// The account with the most room: the lowest fullest window among those
/// with a reading, else the default, else the first.
pub fn roomiest(engine: &PlanEngine) -> Option<AccountId> {
    let read = engine
        .accounts
        .iter()
        .filter(|a| a.used_pct.is_some())
        .min_by(|a, b| {
            a.used_pct
                .unwrap_or(0.0)
                .partial_cmp(&b.used_pct.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    read.or_else(|| engine.accounts.iter().find(|a| a.is_default))
        .or_else(|| engine.accounts.first())
        .map(|a| a.account_id.clone())
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
    async fn send(&self, thread: &ThreadId, text: String) -> Result<(), IpcError>;
    async fn state(&self, thread: &ThreadId) -> Option<ThreadState>;
    async fn last_reply(&self, thread: &ThreadId) -> Option<String>;
    fn tracker(&self) -> Option<TaskStore>;
    /// F-TK-04's sign-in and plan checks.
    fn can_execute(&self) -> Result<(), IpcError>;
    async fn execute(&self, input: ExecuteInput) -> Result<(), IpcError>;
    fn now(&self) -> i64;
}

#[derive(Serialize, Deserialize)]
struct Entry {
    view: PlanView,
    nonce: String,
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
    /// Where plans are kept; `None` keeps them in memory (F-TK-06's
    /// tests).
    file: Option<PathBuf>,
    /// Held from reading the plans to writing them: the last write has
    /// the latest plans.
    saving: Mutex<()>,
}

/// The plans in `path`; none when there's no file or it can't be read.
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
            .map(|e| (e.view.id.clone(), e))
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

impl PlanService {
    /// The service, with the plans kept in `file` (F-TK-07).
    pub fn new(host: Arc<dyn PlanHost>, bus: EventBus, file: Option<PathBuf>) -> Arc<Self> {
        let plans = file.as_deref().map(load).unwrap_or_default();
        Arc::new(Self {
            host,
            bus,
            plans: Mutex::new(plans),
            file,
            saving: Mutex::new(()),
        })
    }

    /// Plans whose Lead was planning when Vibezeo closed: their chats are
    /// read again (F-TK-07 AC3).
    pub fn resume(self: &Arc<Self>) {
        let planning: Vec<(String, ThreadId)> = self
            .lock()
            .values()
            .filter(|e| e.view.state == PlanState::Planning)
            .map(|e| (e.view.id.clone(), e.view.thread_id.clone()))
            .collect();
        for (id, thread) in planning {
            let me = self.clone();
            tokio::spawn(async move { me.wait(&id, &thread).await });
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
            project_id: None,
            issue_ids: Vec::new(),
            created_at: now,
        };
        self.lock().insert(
            view.id.clone(),
            Entry {
                view: view.clone(),
                nonce,
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

    async fn wait(&self, id: &str, thread: &ThreadId) {
        let ended = loop {
            tokio::time::sleep(POLL).await;
            if !self.lock().contains_key(id) {
                return;
            }
            match self.host.state(thread).await {
                Some(ThreadState::Working) | Some(ThreadState::Waiting) => {}
                Some(ThreadState::Done) => break Ok(()),
                // Only a launch marks a chat interrupted: it was mid-turn
                // when Vibezeo closed (F-TK-07).
                Some(ThreadState::Interrupted) => {
                    break Err("Vibezeo closed while Lead was planning")
                }
                _ => break Err("Lead stopped before it finished the plan"),
            }
        };
        let done = ended.is_ok();
        let reply = if done {
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
        let outcome = match ended {
            Ok(()) => parse(&reply, &nonce, &engines, &fallback),
            Err(why) => Err(why.to_string()),
        };
        tracing::info!(
            code = "plan.read",
            ok = outcome.is_ok(),
            tasks = outcome.as_ref().map(Vec::len).unwrap_or(0),
            "Lead's plan read"
        );
        self.update(id, |v| match outcome {
            Ok(tasks) => {
                v.tasks = tasks;
                v.state = PlanState::Ready;
            }
            Err(e) => {
                v.error = Some(e);
                v.state = PlanState::Failed;
            }
        });
    }

    /// `plan_create_tasks`: the edited rows become To do tasks in a new
    /// project (F-TK-06 AC3).
    pub fn create_tasks(&self, id: &str, tasks: Vec<PlanTask>) -> Result<PlanView, IpcError> {
        let (goal, engines) = {
            let plans = self.lock();
            let e = plans.get(id).ok_or_else(not_found)?;
            if e.view.state != PlanState::Ready {
                return Err(IpcError::new(
                    ErrorCode::Invalid,
                    "Only a finished draft can become tasks",
                ));
            }
            (e.view.goal.clone(), e.view.engines.clone())
        };
        if tasks.is_empty() || tasks.len() > TASKS_MAX {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                format!("A plan holds 1 to {TASKS_MAX} tasks"),
            ));
        }
        for t in &tasks {
            if t.title.trim().is_empty() {
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
        let project = tracker
            .save_project(None, &project_name(&goal), None)
            .map_err(task_err)?;
        let mut ids = Vec::with_capacity(tasks.len());
        for t in &tasks {
            let issue = tracker
                .save_issue(
                    SaveIssue {
                        title: Some(cut(t.title.trim(), TITLE_MAX)),
                        description: Some(cut(t.description.trim(), DESCRIPTION_MAX)),
                        status: Some(IssueStatus::Todo),
                        project: Some(Some(project.id.clone())),
                        suggested_engine: Some(Some(t.engine_id.clone())),
                        ..Default::default()
                    },
                    &Writer::human(),
                )
                .map_err(task_err)?;
            ids.push(issue.id);
        }
        tracing::info!(
            code = "plan.created",
            tasks = ids.len(),
            "a plan's tasks were created"
        );
        self.update(id, |v| {
            v.tasks = tasks;
            v.project_id = Some(project.id.clone());
            v.issue_ids = ids;
            v.state = PlanState::Created;
        })
        .ok_or_else(not_found)
    }

    /// `plan_execute`: each task through F-TK-04's queue on its engine, in a
    /// worktree, at Edits, reviewed. Refused as a whole on Free or signed
    /// out (F-TK-06 AC4).
    pub async fn execute(&self, id: &str) -> Result<PlanView, IpcError> {
        let view = self.get(id)?;
        if view.state != PlanState::Created {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Create the plan's tasks first",
            ));
        }
        self.host.can_execute()?;
        let mut started = 0usize;
        for (issue, task) in view.issue_ids.iter().zip(&view.tasks) {
            let account = view
                .engines
                .iter()
                .find(|e| e.engine_id == task.engine_id)
                .and_then(roomiest);
            self.host
                .execute(ExecuteInput {
                    issue_id: issue.clone(),
                    workspace_id: view.workspace_id.clone(),
                    teammate_id: None,
                    engine_id: Some(task.engine_id.clone()),
                    account_id: account,
                    isolation: Isolation::Lane,
                    access: Access::Edits,
                    full_confirmed: false,
                    review: true,
                    note: None,
                })
                .await?;
            started += 1;
        }
        tracing::info!(
            code = "plan.executed",
            tasks = started,
            "a plan's tasks were queued"
        );
        self.update(id, |v| v.state = PlanState::Started)
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
        let engines = &state.terminals.engines;
        let accounts = tandem_store::repo::accounts::list(&state.store)
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
                        let view = state.usage.get(a.id.as_str());
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

    fn tracker(&self) -> Option<TaskStore> {
        self.state().tasks.tracker.clone()
    }

    fn can_execute(&self) -> Result<(), IpcError> {
        self.state().task_exec.can_execute()
    }

    async fn execute(&self, input: ExecuteInput) -> Result<(), IpcError> {
        self.state().task_exec.execute(input).await.map(|_| ())
    }

    fn now(&self) -> i64 {
        self.state().clock.now_ms()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        chats: Mutex<Vec<(WorkspaceId, String, String)>>,
        sent: Mutex<Vec<String>>,
        state: Mutex<Option<ThreadState>>,
        reply: Mutex<String>,
        tracker: Option<TaskStore>,
        free: bool,
        executed: Mutex<Vec<ExecuteInput>>,
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
            offered()
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
            Ok(ThreadId::from_db("th_plan".to_string()))
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
        fn tracker(&self) -> Option<TaskStore> {
            self.tracker.clone()
        }
        fn can_execute(&self) -> Result<(), IpcError> {
            if self.free {
                Err(IpcError::new(
                    ErrorCode::ForbiddenPlan,
                    "Running tasks is part of Pro.",
                ))
            } else {
                Ok(())
            }
        }
        async fn execute(&self, input: ExecuteInput) -> Result<(), IpcError> {
            self.executed.lock().expect("l").push(input);
            Ok(())
        }
        fn now(&self) -> i64 {
            T0
        }
    }

    fn rig(free: bool) -> (tempfile::TempDir, Arc<Fake>, Arc<PlanService>, EventBus) {
        let dir = tempfile::tempdir().expect("dir");
        let fake = Arc::new(Fake {
            tracker: Some(TaskStore::open(dir.path().join("tasks")).expect("tracker")),
            free,
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
        let (_fake2, after) = restarted(&dir, ThreadState::Done, String::new());
        let kept = after.get(&view.id).expect("kept");
        assert_eq!(kept.state, PlanState::Created);
        assert_eq!(kept.issue_ids, view.issue_ids);
        assert_eq!(kept.tasks, view.tasks);
        assert_eq!(after.list().len(), 1);
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

    #[tokio::test]
    async fn f_tk_06_2_3_lead_finishing_makes_a_draft_and_create_tasks_writes_them() {
        let (_d, fake, plans, bus) = rig(false);
        let mut rx = bus.subscribe(Topic::Tasks);
        let view = plans
            .start(WorkspaceId::from_db("ws_1".to_string()), "Add OAuth".into())
            .await
            .expect("start");
        let n = nonce_of(&fake.sent.lock().expect("l")[0]);
        *fake.reply.lock().expect("l") = reply(
            &n,
            r#"{"tasks": [{"title": "Routes", "engine": "codex"}, {"title": "Page", "description": "The page.", "engine": "claude"}]}"#,
        );
        *fake.state.lock().expect("l") = Some(ThreadState::Done);
        let draft = finished(&plans, &view.id).await;
        assert_eq!(draft.state, PlanState::Ready);
        assert_eq!(draft.tasks.len(), 2);
        let ev = rx.try_recv().expect("published");
        assert_eq!(ev.name, EVENT);
        let tracker = fake.tracker.clone().expect("tracker");
        assert!(
            tracker.all().expect("all").is_empty(),
            "nothing before Create tasks"
        );
        // The user renamed one and moved it to Claude.
        let mut edited = draft.tasks.clone();
        edited[0].title = "OAuth routes".into();
        edited[0].engine_id = "claude".into();
        let created = plans.create_tasks(&view.id, edited).expect("create");
        assert_eq!(created.state, PlanState::Created);
        assert_eq!(
            created.issue_ids,
            vec!["TAN-1".to_string(), "TAN-2".to_string()]
        );
        let first = tracker.get("TAN-1").expect("TAN-1");
        assert_eq!(first.title, "OAuth routes");
        assert_eq!(first.status, IssueStatus::Todo);
        assert_eq!(first.suggested_engine.as_deref(), Some("claude"));
        assert_eq!(first.project, created.project_id);
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
        assert!(plans.create_tasks(&view.id, created.tasks.clone()).is_err());
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

    async fn created(fake: &Arc<Fake>, plans: &Arc<PlanService>) -> PlanView {
        let view = plans
            .start(WorkspaceId::from_db("ws_1".to_string()), "Add OAuth".into())
            .await
            .expect("start");
        let n = nonce_of(&fake.sent.lock().expect("l")[0]);
        *fake.reply.lock().expect("l") = reply(
            &n,
            r#"{"tasks": [{"title": "Routes", "engine": "claude"}, {"title": "Page", "engine": "codex"}]}"#,
        );
        *fake.state.lock().expect("l") = Some(ThreadState::Done);
        let draft = finished(plans, &view.id).await;
        plans.create_tasks(&view.id, draft.tasks).expect("create")
    }

    #[tokio::test]
    async fn f_tk_06_4_execute_all_queues_each_task_on_its_engine_in_a_worktree() {
        let (_d, fake, plans, _bus) = rig(false);
        let view = created(&fake, &plans).await;
        let started = plans.execute(&view.id).await.expect("execute");
        assert_eq!(started.state, PlanState::Started);
        let runs = fake.executed.lock().expect("l").clone();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].issue_id, "TAN-1");
        assert_eq!(runs[0].engine_id.as_deref(), Some("claude"));
        // Claude's account with the most room: "home" at 10 %.
        assert_eq!(
            runs[0].account_id.as_ref().map(|a| a.as_str()),
            Some("home")
        );
        assert_eq!(runs[1].engine_id.as_deref(), Some("codex"));
        // No reading: the default account.
        assert_eq!(runs[1].account_id.as_ref().map(|a| a.as_str()), Some("cx"));
        for r in &runs {
            assert_eq!(r.isolation, Isolation::Lane);
            assert_eq!(r.access, Access::Edits);
            assert!(r.review && !r.full_confirmed && r.teammate_id.is_none());
        }
        assert!(plans.execute(&view.id).await.is_err(), "once");
    }

    #[tokio::test]
    async fn f_tk_06_4_on_free_execute_all_starts_nothing_and_says_why() {
        let (_d, fake, plans, _bus) = rig(true);
        let view = created(&fake, &plans).await;
        let err = plans.execute(&view.id).await.expect_err("free");
        assert_eq!(err.code, ErrorCode::ForbiddenPlan);
        assert!(fake.executed.lock().expect("l").is_empty());
        assert_eq!(plans.get(&view.id).expect("plan").state, PlanState::Created);
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
        let none = PlanEngine {
            engine_id: "x".into(),
            label: "X".into(),
            accounts: Vec::new(),
        };
        assert_eq!(roomiest(&none), None);
    }
}
