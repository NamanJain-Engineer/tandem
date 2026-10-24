//! Routines in the app (F-RT-02, F-RT-04; ARCHITECTURE §10.2): the store,
//! the 30 s scheduler on the injected clock, and runs. Each run makes a
//! fresh chat with the routine's teammate, titled with the routine's name,
//! sends the instruction unattended at the routine's access, and when the
//! turn ends records the result and says "Ready for review — {routine}".
//!
//! Everything the run needs from the rest of the app comes through
//! [`RoutineHost`], so the scheduler is tested with a fake.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use tandem_core::access::Access;
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::clock::Clock;
use tandem_core::entitlement::Limits;
use tandem_core::ids::{RoutineId, RunId, TeammateId, ThreadId};
use tandem_core::settings::SettingKey;
use tandem_core::source::SourceRef;
use tandem_core::{ErrorCode, IpcError};
use tandem_routines::schedule::{describe_en, words, ScheduleWords};
use tandem_routines::scheduler::{self, Decision, TickState};
use tandem_routines::{RoutineInput, ScheduleError, ScheduleSpec};
use tandem_store::repo::notifications::NotificationKind;
use tandem_store::repo::routines::{
    self as repo, NewRoutine, Routine, RoutineRun, RunStatus, Runner,
};
use tandem_store::repo::teammates::Teammate;
use tandem_store::repo::threads::ThreadState;
use tandem_store::{Store, StoreError};
use ts_rs::TS;

/// How long a headless pass waits for its runs (the next launcher tick is
/// 5 minutes later, so runs never overlap a second pass by much).
pub const HEADLESS_CAP: Duration = Duration::from_secs(50 * 60);
/// How often a run looks at its chat's state.
const POLL: Duration = Duration::from_millis(400);
/// Run history kept per routine (UI-53).
pub const KEEP_RUNS: usize = 100;

/// What a run needs from the app.
#[async_trait]
pub trait RoutineHost: Send + Sync {
    /// A live (not deleted) teammate.
    async fn teammate(&self, id: &TeammateId) -> Option<Teammate>;
    /// The engine is a chat engine and installed.
    fn chat_engine_ready(&self, engine_id: &str) -> bool;
    fn limits(&self) -> Limits;
    /// Makes the run's chat (unattended, the routine's access, titled with
    /// its name) and sends the instruction.
    async fn start_chat(
        &self,
        teammate: &Teammate,
        routine: &Routine,
    ) -> Result<ThreadId, IpcError>;
    async fn chat_state(&self, chat: &ThreadId) -> Option<ThreadState>;
    async fn notify(&self, kind: NotificationKind, title: String, body: String, source: SourceRef);
}

/// A routine with what its row shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoutineView {
    pub routine: Routine,
    /// `None` when the schedule can't be read.
    pub words: Option<ScheduleWords>,
    pub description: String,
    /// The run in progress.
    pub running: Option<RoutineRun>,
    /// Today's runs (for the timeline strip), oldest first.
    pub today: Vec<RoutineRun>,
    /// The newest run, for the row's last result.
    pub last: Option<RoutineRun>,
}

/// `routine_preview_schedule`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SchedulePreview {
    pub words: ScheduleWords,
    pub description: String,
    /// The next 3 runs (ms).
    pub next: Vec<i64>,
}

/// One page of run history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunPage {
    pub runs: Vec<RoutineRun>,
    /// Pass back as `before` for older runs.
    pub next_before: Option<i64>,
}

/// What a teammate's schedule tool asks for (F-RT-05); on update every
/// field but `id` is optional.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentRoutine {
    pub id: Option<String>,
    pub name: Option<String>,
    pub instruction: Option<String>,
    pub cron: Option<String>,
    pub timezone: Option<String>,
    pub access: Option<String>,
    pub paused: Option<bool>,
}

/// Why a run didn't start (`last_error_kind`, ARCHITECTURE §10.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailKind {
    TeammateMissing,
    EngineMissing,
    Plan,
    Limit,
    EngineError,
}

impl FailKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FailKind::TeammateMissing => "teammate_missing",
            FailKind::EngineMissing => "engine_missing",
            FailKind::Plan => "plan",
            FailKind::Limit => "limit",
            FailKind::EngineError => "engine_error",
        }
    }
}

pub struct RoutinesService {
    store: Store,
    bus: EventBus,
    clock: Arc<dyn Clock>,
    /// Gui, or Headless for `--run-routines`.
    headless: std::sync::atomic::AtomicBool,
    host: OnceLock<Arc<dyn RoutineHost>>,
    /// Runs in flight.
    runs: StdMutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// The GUI holds `run/app.lock` (or couldn't take it and runs anyway).
    /// Until then a headless pass may be running work (F-RT-06): the GUI
    /// neither ticks Night Shift runs nor settles task attempts.
    locked: std::sync::atomic::AtomicBool,
    lock_ready: tokio::sync::Notify,
}

fn store_err(e: StoreError) -> IpcError {
    match e {
        StoreError::NotFound(m) => IpcError::new(ErrorCode::NotFound, m),
        other => IpcError::new(ErrorCode::Io, other.to_string()),
    }
}

fn invalid(e: ScheduleError) -> IpcError {
    IpcError::new(ErrorCode::Invalid, e.to_string())
}

fn ms(t: chrono::DateTime<chrono::Utc>) -> i64 {
    t.timestamp_millis()
}

/// The next run after `after_ms`, by the routine's own schedule.
pub fn next_run(spec: &ScheduleSpec, after_ms: i64) -> Option<i64> {
    let at = DateTime::from_timestamp_millis(after_ms)?;
    spec.next_after(at).ok().flatten().map(ms)
}

impl RoutinesService {
    pub fn new(store: Store, bus: EventBus, clock: Arc<dyn Clock>, runner: Runner) -> Arc<Self> {
        Arc::new(Self {
            store,
            bus,
            clock,
            headless: std::sync::atomic::AtomicBool::new(runner == Runner::Headless),
            host: OnceLock::new(),
            runs: StdMutex::default(),
            locked: std::sync::atomic::AtomicBool::new(false),
            lock_ready: tokio::sync::Notify::new(),
        })
    }

    /// Whether this GUI holds the app lock (F-RT-06).
    pub fn holds_lock(&self) -> bool {
        self.locked.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Waits until this GUI holds the app lock (a headless pass running
    /// work has ended).
    pub async fn wait_lock(&self) {
        loop {
            let notified = self.lock_ready.notified();
            if self.holds_lock() {
                return;
            }
            notified.await;
        }
    }

    fn mark_locked(&self) {
        self.locked
            .store(true, std::sync::atomic::Ordering::Release);
        self.lock_ready.notify_waiters();
    }

    /// `--run-routines` runs the same service headless.
    pub fn set_runner(&self, runner: Runner) {
        self.headless.store(
            runner == Runner::Headless,
            std::sync::atomic::Ordering::SeqCst,
        );
    }

    fn runner(&self) -> Runner {
        if self.headless.load(std::sync::atomic::Ordering::SeqCst) {
            Runner::Headless
        } else {
            Runner::Gui
        }
    }

    pub fn set_host(&self, host: Arc<dyn RoutineHost>) {
        let _ = self.host.set(host);
    }

    fn host(&self) -> Result<&Arc<dyn RoutineHost>, IpcError> {
        self.host
            .get()
            .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "Routines aren't ready yet"))
    }

    fn now(&self) -> i64 {
        self.clock.now_ms()
    }

    fn changed(&self) {
        self.bus.publish(BusEvent::new(
            Topic::Routines,
            "routines:changed",
            serde_json::json!({}),
        ));
    }

    /// Tells the UI to read the list again (a setting changed).
    pub fn notify_changed(&self) {
        self.changed();
    }

    fn run_event(&self, run: &RoutineRun) {
        self.bus.publish(BusEvent::new(
            Topic::Routines,
            "routines:run",
            serde_json::to_value(run).unwrap_or_default(),
        ));
    }

    // ── reads ───────────────────────────────────────────────────────────

    pub async fn view(&self, r: Routine) -> RoutineView {
        let w = words(&r.schedule.schedule).ok();
        let description = w.as_ref().map(describe_en).unwrap_or_default();
        let running = repo::running(&self.store, &r.id).await.ok().flatten();
        let day_start = self.day_start();
        let recent = repo::runs(&self.store, &r.id, None, 50)
            .await
            .unwrap_or_default();
        let last = recent.first().cloned();
        let mut today = recent
            .into_iter()
            .filter(|run| run.due_at >= day_start)
            .collect::<Vec<_>>();
        today.reverse();
        RoutineView {
            routine: r,
            words: w,
            description,
            running,
            today,
            last,
        }
    }

    /// Local midnight today, in ms.
    fn day_start(&self) -> i64 {
        let now = self.clock.now_utc().with_timezone(&chrono::Local);
        now.date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|d| d.and_local_timezone(chrono::Local).earliest())
            .map(|d| d.timestamp_millis())
            .unwrap_or(0)
    }

    pub async fn list(&self) -> Result<Vec<RoutineView>, IpcError> {
        let routines = repo::list(&self.store).await.map_err(store_err)?;
        let mut out = Vec::with_capacity(routines.len());
        for r in routines {
            out.push(self.view(r).await);
        }
        Ok(out)
    }

    pub async fn get(&self, id: &RoutineId) -> Result<Routine, IpcError> {
        repo::get(&self.store, id).await.map_err(store_err)
    }

    pub fn preview(&self, spec: &ScheduleSpec) -> Result<SchedulePreview, IpcError> {
        let resolved = spec.resolve().map_err(invalid)?;
        let next: Vec<i64> = resolved
            .next_n(self.clock.now_utc(), 3)
            .into_iter()
            .map(ms)
            .collect();
        if next.is_empty() {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "This schedule never runs.",
            ));
        }
        let words = words(&spec.schedule).map_err(invalid)?;
        Ok(SchedulePreview {
            description: describe_en(&words),
            words,
            next,
        })
    }

    pub async fn runs(&self, id: &RoutineId, before: Option<i64>) -> Result<RunPage, IpcError> {
        const PAGE: usize = 25;
        let runs = repo::runs(&self.store, id, before, PAGE)
            .await
            .map_err(store_err)?;
        let next_before = (runs.len() == PAGE)
            .then(|| runs.last().map(|r| r.due_at))
            .flatten();
        Ok(RunPage { runs, next_before })
    }

    // ── writes ──────────────────────────────────────────────────────────

    /// Creates or edits a routine (F-RT-01). Routines run with no account
    /// (D-2900) unless the limits turn them off.
    pub async fn save(
        &self,
        input: RoutineInput,
        created_by: Option<TeammateId>,
    ) -> Result<Routine, IpcError> {
        let host = self.host()?;
        if !host.limits().routines {
            return Err(IpcError::new(
                ErrorCode::ForbiddenPlan,
                "Routines are turned off for this account.",
            ));
        }
        let name = input.validate().map_err(invalid)?;
        let teammate_id = TeammateId::from_db(input.teammate_id.clone());
        if host.teammate(&teammate_id).await.is_none() {
            return Err(IpcError::new(ErrorCode::Invalid, "Pick a teammate"));
        }
        let now = self.now();
        let next = next_run(&input.schedule, now);
        if next.is_none() {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "This schedule never runs.",
            ));
        }
        let new = NewRoutine {
            id: input.id.clone().map(RoutineId::from_db).unwrap_or_default(),
            teammate_id,
            name,
            instruction: input.instruction.clone(),
            schedule: input.schedule.clone(),
            access: input.access,
            run_mode: input.run_mode,
            created_by_teammate_id: created_by,
            next_run_at: next,
        };
        let saved = match &input.id {
            Some(id) => {
                let id = RoutineId::from_db(id.clone());
                let current = repo::get(&self.store, &id).await.map_err(store_err)?;
                let next = if current.paused {
                    None
                } else {
                    new.next_run_at
                };
                repo::update(
                    &self.store,
                    &id,
                    NewRoutine {
                        next_run_at: next,
                        ..new
                    },
                    now,
                )
                .await
                .map_err(store_err)?
            }
            None => repo::insert(&self.store, new, now)
                .await
                .map_err(store_err)?,
        };
        self.changed();
        Ok(saved)
    }

    /// Deletes a routine; its past chats stay.
    pub async fn delete(&self, id: &RoutineId) -> Result<(), IpcError> {
        if let Some(h) = self
            .runs
            .lock()
            .ok()
            .and_then(|mut r| r.remove(id.as_str()))
        {
            h.abort();
        }
        repo::delete(&self.store, id).await.map_err(store_err)?;
        self.changed();
        Ok(())
    }

    /// Pausing keeps the routine; resuming schedules from now (no catch-up
    /// for the paused stretch).
    pub async fn set_paused(&self, id: &RoutineId, paused: bool) -> Result<(), IpcError> {
        let r = self.get(id).await?;
        let now = self.now();
        let next = if paused {
            None
        } else {
            next_run(&r.schedule, now)
        };
        repo::set_paused(&self.store, id, paused, next, now)
            .await
            .map_err(store_err)?;
        self.changed();
        Ok(())
    }

    /// Plan lapse pauses every routine; an upgrade resumes them (F-BL-04).
    pub async fn apply_plan(&self, limits: &Limits) {
        match repo::set_paused_by_plan(&self.store, !limits.routines).await {
            Ok(n) if n > 0 => {
                if limits.routines {
                    // Back on: schedule from now, no catch-up for the gap.
                    if let Ok(all) = repo::list(&self.store).await {
                        let now = self.now();
                        for r in all.into_iter().filter(|r| !r.paused) {
                            let _ = repo::set_next(&self.store, &r.id, next_run(&r.schedule, now))
                                .await;
                        }
                    }
                }
                self.changed();
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(code = "routines.plan_failed", error = %e, "couldn't apply the plan to routines")
            }
        }
    }

    /// Run now: a run right away, unless one is going (F-RT-04 AC1).
    pub async fn run_now(self: &Arc<Self>, id: &RoutineId) -> Result<RunId, IpcError> {
        let r = self.get(id).await?;
        if repo::running(&self.store, id)
            .await
            .map_err(store_err)?
            .is_some()
        {
            return Err(IpcError::new(
                ErrorCode::Conflict,
                scheduler::OVERLAP_REASON,
            ));
        }
        let now = self.now();
        let run = repo::claim(&self.store, id, now, false, true, self.runner(), now)
            .await
            .map_err(store_err)?
            .ok_or_else(|| IpcError::new(ErrorCode::Conflict, scheduler::OVERLAP_REASON))?;
        self.spawn_run(r, run.clone(), false);
        Ok(run)
    }

    // ── a teammate's own routines (F-RT-05) ─────────────────────────────

    /// The routines `teammate` made with the schedule tools.
    pub async fn made_by(&self, teammate: &TeammateId) -> Result<Vec<Routine>, IpcError> {
        Ok(repo::list(&self.store)
            .await
            .map_err(store_err)?
            .into_iter()
            .filter(|r| r.created_by_teammate_id.as_ref() == Some(teammate))
            .collect())
    }

    async fn own(&self, teammate: &TeammateId, id: &str) -> Result<Routine, IpcError> {
        let r = repo::get(&self.store, &RoutineId::from_db(id.to_string()))
            .await
            .ok()
            .filter(|r| r.created_by_teammate_id.as_ref() == Some(teammate));
        r.ok_or_else(|| {
            IpcError::new(
                ErrorCode::NotFound,
                "You have no routine with that id; tandem__schedule_list shows yours.",
            )
        })
    }

    /// `tandem__schedule_create` / `_update`: the teammate's own routines,
    /// run as itself, Edits at most; the user is told each time.
    pub async fn save_for_teammate(
        &self,
        teammate: &Teammate,
        req: AgentRoutine,
    ) -> Result<Routine, IpcError> {
        let access = match req.access.as_deref() {
            None => None,
            Some("read") => Some(Access::Read),
            Some("edits") => Some(Access::Edits),
            Some("full") => {
                return Err(IpcError::new(
                    ErrorCode::Invalid,
                    "Routines a teammate makes can't have Full access; use read or edits.",
                ))
            }
            Some(other) => {
                return Err(IpcError::new(
                    ErrorCode::Invalid,
                    format!("access is read or edits, not {other}"),
                ))
            }
        };
        let current = match &req.id {
            Some(id) => Some(self.own(&teammate.id, id).await?),
            None => None,
        };
        let need = |v: Option<String>, what: &str| {
            v.ok_or_else(|| IpcError::new(ErrorCode::Invalid, format!("{what} is required")))
        };
        let name = match (&req.name, &current) {
            (Some(n), _) => n.clone(),
            (None, Some(c)) => c.name.clone(),
            (None, None) => need(None, "name")?,
        };
        let instruction = match (&req.instruction, &current) {
            (Some(i), _) => i.clone(),
            (None, Some(c)) => c.instruction.clone(),
            (None, None) => need(None, "instruction")?,
        };
        let timezone = req
            .timezone
            .clone()
            .or_else(|| current.as_ref().map(|c| c.schedule.timezone.clone()))
            .unwrap_or_else(tandem_routines::schedule::local_timezone);
        let schedule = match (&req.cron, &current) {
            (Some(expr), _) => ScheduleSpec {
                schedule: tandem_routines::Schedule::Cron { expr: expr.clone() },
                timezone,
            },
            (None, Some(c)) => ScheduleSpec {
                timezone,
                ..c.schedule.clone()
            },
            (None, None) => {
                return Err(IpcError::new(ErrorCode::Invalid, "cron is required"));
            }
        };
        // An update that doesn't say keeps the access, Full included when
        // the user chose it; a teammate can only lower it.
        let (access, full_confirmed) = match (access, &current) {
            (Some(a), _) => (a, false),
            (None, Some(c)) => (c.access, c.access == Access::Full),
            (None, None) => (Access::Edits, false),
        };
        let input = RoutineInput {
            id: current.as_ref().map(|c| c.id.as_str().to_string()),
            teammate_id: current
                .as_ref()
                .and_then(|c| c.teammate_id.clone())
                .unwrap_or_else(|| teammate.id.clone())
                .as_str()
                .to_string(),
            name,
            instruction,
            schedule,
            access,
            full_confirmed,
            run_mode: current
                .as_ref()
                .map(|c| c.run_mode)
                .unwrap_or(tandem_routines::RunMode::InApp),
        };
        let mut saved = self.save(input, Some(teammate.id.clone())).await?;
        if let Some(paused) = req.paused {
            if paused != saved.paused {
                self.set_paused(&saved.id, paused).await?;
                saved = self.get(&saved.id).await?;
            }
        }
        let words = words(&saved.schedule.schedule)
            .map(|w| describe_en(&w))
            .unwrap_or_default();
        let title = if current.is_some() {
            format!("{} changed the routine {}", teammate.name, saved.name)
        } else {
            format!("{} scheduled a routine: {}", teammate.name, saved.name)
        };
        if let Ok(host) = self.host() {
            host.notify(
                NotificationKind::RoutineChanged,
                title,
                words,
                SourceRef::Routine {
                    routine_id: saved.id.as_str().to_string(),
                    run_id: None,
                },
            )
            .await;
        }
        Ok(saved)
    }

    /// `tandem__schedule_delete`: only the teammate's own.
    pub async fn delete_for_teammate(
        &self,
        teammate: &TeammateId,
        id: &str,
    ) -> Result<(), IpcError> {
        let r = self.own(teammate, id).await?;
        self.delete(&r.id).await
    }

    // ── the scheduler ───────────────────────────────────────────────────

    /// One pass: fires what's due (missed runs within 24 h once, late),
    /// skips overlaps, schedules routines that have no next run. Returns
    /// the runs it started.
    pub async fn tick(self: &Arc<Self>) -> Vec<RunId> {
        self.tick_at(self.now()).await
    }

    /// "Pause routines" (the tray, the palette, Settings): nothing fires,
    /// and the slots that pass meanwhile are dropped, not caught up.
    async fn paused_all(&self) -> bool {
        tandem_store::repo::settings::get(&self.store, SettingKey::PauseAllRoutines)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// [`tick`](Self::tick) as if the clock said `now` (E2E's
    /// `test_routines_tick` moves the scheduler's time without moving the
    /// app's).
    pub async fn tick_at(self: &Arc<Self>, now: i64) -> Vec<RunId> {
        let mut started = Vec::new();
        let Ok(routines) = repo::list(&self.store).await else {
            return started;
        };
        if self.paused_all().await {
            for r in routines {
                if r.next_run_at.is_some_and(|due| due <= now) {
                    let _ = repo::set_next(&self.store, &r.id, next_run(&r.schedule, now)).await;
                    self.changed();
                }
            }
            return started;
        }
        let headless = self.runner() == Runner::Headless;
        for r in routines {
            if headless && r.run_mode != tandem_routines::RunMode::Background {
                continue;
            }
            let running = repo::running(&self.store, &r.id)
                .await
                .ok()
                .flatten()
                .is_some();
            let state = TickState {
                next_run_at: r.next_run_at,
                paused: r.paused || r.paused_by_plan,
                running,
            };
            match scheduler::decide(state, now) {
                Decision::Wait => {}
                Decision::Schedule => {
                    let _ = repo::set_next(&self.store, &r.id, next_run(&r.schedule, now)).await;
                    self.changed();
                }
                Decision::TooOld { due_at } => {
                    tracing::info!(
                        code = "routines.missed",
                        "a routine's run was missed more than a day ago; not caught up"
                    );
                    let after = scheduler::after_firing(due_at, now);
                    let _ = repo::set_next(&self.store, &r.id, next_run(&r.schedule, after)).await;
                    self.changed();
                }
                Decision::Skip { due_at } => {
                    let _ = repo::skip(
                        &self.store,
                        &r.id,
                        due_at,
                        scheduler::OVERLAP_REASON,
                        self.runner(),
                        now,
                    )
                    .await;
                    let _ =
                        repo::set_next(&self.store, &r.id, next_run(&r.schedule, due_at.max(now)))
                            .await;
                    self.changed();
                }
                Decision::Fire { due_at, late } => {
                    let claimed =
                        repo::claim(&self.store, &r.id, due_at, late, false, self.runner(), now)
                            .await
                            .ok()
                            .flatten();
                    let after = scheduler::after_firing(due_at, now);
                    let _ = repo::set_next(&self.store, &r.id, next_run(&r.schedule, after)).await;
                    if let Some(run) = claimed {
                        started.push(run.clone());
                        self.spawn_run(r, run, late);
                    }
                    self.changed();
                }
            }
        }
        started
    }

    /// Runs a process left `running` when it went away are settled, and
    /// old history is pruned.
    pub async fn settle(&self) {
        let _ = repo::settle_interrupted(&self.store, self.now()).await;
        let _ = repo::prune_runs(&self.store, KEEP_RUNS).await;
    }

    /// The GUI's scheduler: it takes the app lock (waiting while a headless
    /// pass has it, ARCHITECTURE §10.4), holds it for the app's life, then
    /// ticks at once (catch-up) and every 30 s of the clock. The tick loop
    /// runs under the watchdog as "scheduler/routines" (§5.2): a panicking
    /// tick restarts it, and three in ten minutes are reported.
    pub fn start(
        self: &Arc<Self>,
        run_dir: std::path::PathBuf,
        sup: tandem_core::supervisor::Supervision,
    ) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let lock = loop {
                match crate::headless::try_lock(&run_dir, "gui") {
                    Ok(Some(lock)) => break Some(lock),
                    Ok(None) => tokio::time::sleep(Duration::from_secs(5)).await,
                    Err(e) => {
                        tracing::warn!(code = "routines.lock_failed", error = %e, "couldn't take the app lock; routines run anyway");
                        break None;
                    }
                }
            };
            let _held = lock;
            me.mark_locked();
            me.settle().await;
            me.changed();
            tandem_core::supervisor::supervise_task("scheduler/routines", sup, None, move || {
                let me = me.clone();
                async move {
                    loop {
                        me.tick().await;
                        me.clock.sleep(scheduler::TICK).await;
                    }
                }
            })
            .await;
        });
    }

    /// Waits for the runs in flight (headless), up to `cap`.
    pub async fn wait_idle(&self, cap: Duration) {
        let deadline = tokio::time::Instant::now() + cap;
        loop {
            let pending: Vec<tokio::task::JoinHandle<()>> = match self.runs.lock() {
                Ok(mut r) => r.drain().map(|(_, h)| h).collect(),
                Err(_) => return,
            };
            if pending.is_empty() {
                return;
            }
            for h in pending {
                if tokio::time::timeout_at(deadline, h).await.is_err() {
                    tracing::warn!(
                        code = "routines.headless_cap",
                        "a background run passed its time cap"
                    );
                    return;
                }
            }
        }
    }

    fn spawn_run(self: &Arc<Self>, r: Routine, run: RunId, late: bool) {
        let me = self.clone();
        let key = r.id.as_str().to_string();
        // Runs are always spawned from inside the runtime (a command, the
        // scheduler's loop or the headless pass).
        let handle = tokio::spawn(async move {
            me.run(r, run, late).await;
        });
        if let Ok(mut runs) = self.runs.lock() {
            runs.retain(|_, h| !h.is_finished());
            runs.insert(key, handle);
        }
    }

    async fn fail(&self, r: &Routine, run: &RunId, kind: FailKind, message: String) {
        let now = self.now();
        let _ = repo::finish_run(
            &self.store,
            run,
            RunStatus::Error,
            Some(message.clone()),
            now,
        )
        .await;
        let _ = repo::set_last(
            &self.store,
            &r.id,
            "error",
            Some((kind.as_str().into(), message.clone())),
            None,
        )
        .await;
        if let Ok(run) = repo::get_run(&self.store, run).await {
            self.run_event(&run);
        }
        self.changed();
        // One notification per error type per routine per day.
        let day = self
            .clock
            .now_utc()
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string();
        if repo::first_notice(&self.store, kind.as_str(), r.id.as_str(), &day)
            .await
            .unwrap_or(false)
        {
            if let Some(host) = self.host.get() {
                host.notify(
                    NotificationKind::Failed,
                    format!("{} didn't run", r.name),
                    message,
                    SourceRef::Routine {
                        routine_id: r.id.as_str().to_string(),
                        run_id: Some(run.as_str().to_string()),
                    },
                )
                .await;
            }
        }
    }

    /// Checks, then the chat, then waits for the turn to end.
    async fn run(&self, r: Routine, run: RunId, _late: bool) {
        let Some(host) = self.host.get().cloned() else {
            return;
        };
        if let Ok(started) = repo::get_run(&self.store, &run).await {
            self.run_event(&started);
        }
        let teammate = match &r.teammate_id {
            Some(id) => host.teammate(id).await,
            None => None,
        };
        let Some(teammate) = teammate else {
            return self
                .fail(
                    &r,
                    &run,
                    FailKind::TeammateMissing,
                    "The teammate this routine runs as was deleted.".into(),
                )
                .await;
        };
        let limits = host.limits();
        if !limits.routines || (self.runner() == Runner::Headless && !limits.background_routines) {
            return self
                .fail(
                    &r,
                    &run,
                    FailKind::Plan,
                    "Routines are turned off for this account.".into(),
                )
                .await;
        }
        if !host.chat_engine_ready(&teammate.engine_id) {
            return self
                .fail(
                    &r,
                    &run,
                    FailKind::EngineMissing,
                    format!("{}'s engine isn't installed.", teammate.name),
                )
                .await;
        }
        let chat = match host.start_chat(&teammate, &r).await {
            Ok(c) => c,
            Err(e) => {
                let kind = if e.code == ErrorCode::LimitReached {
                    FailKind::Limit
                } else {
                    FailKind::EngineError
                };
                return self.fail(&r, &run, kind, e.message).await;
            }
        };
        let _ = repo::set_run_chat(&self.store, &run, &chat).await;
        let _ = repo::set_last(&self.store, &r.id, "running", None, Some(&chat)).await;
        self.changed();
        let outcome = loop {
            tokio::time::sleep(POLL).await;
            match host.chat_state(&chat).await {
                None => break Err("The run's chat was deleted.".to_string()),
                Some(ThreadState::Done) => break Ok(()),
                Some(ThreadState::Failed) => {
                    break Err("The run stopped with an error.".to_string())
                }
                Some(ThreadState::Interrupted) | Some(ThreadState::Idle) => {
                    break Err("The run was stopped before it finished.".to_string())
                }
                Some(ThreadState::Working) | Some(ThreadState::Waiting) => {}
            }
        };
        match outcome {
            Ok(()) => {
                let now = self.now();
                let _ = repo::finish_run(&self.store, &run, RunStatus::Ok, None, now).await;
                let _ = repo::set_last(&self.store, &r.id, "ok", None, Some(&chat)).await;
                if let Ok(done) = repo::get_run(&self.store, &run).await {
                    self.run_event(&done);
                }
                self.changed();
                host.notify(
                    NotificationKind::RoutineReady,
                    format!("Ready for review — {}", r.name),
                    teammate.name.clone(),
                    SourceRef::TeammateChat {
                        teammate_id: teammate.id.as_str().to_string(),
                        thread_id: chat.as_str().to_string(),
                    },
                )
                .await;
            }
            Err(message) => self.fail(&r, &run, FailKind::EngineError, message).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tandem_core::clock::MockClock;
    use tandem_core::DataDir;
    use tandem_routines::{RunMode, Schedule};
    use tandem_store::repo::teammates::{self as tm_repo, NewTeammate};

    /// A host whose chats finish after `polls` looks.
    struct FakeHost {
        teammate: StdMutex<Option<Teammate>>,
        limits: StdMutex<Limits>,
        engine_ready: bool,
        chats: StdMutex<Vec<(ThreadId, String, Access)>>,
        polls: AtomicUsize,
        finish_after: usize,
        notes: StdMutex<Vec<(NotificationKind, String)>>,
    }

    impl FakeHost {
        fn new(teammate: Teammate) -> Self {
            Self {
                teammate: StdMutex::new(Some(teammate)),
                limits: StdMutex::new(on()),
                engine_ready: true,
                chats: StdMutex::default(),
                polls: AtomicUsize::new(0),
                finish_after: 2,
                notes: StdMutex::default(),
            }
        }

        fn chats(&self) -> usize {
            self.chats.lock().expect("lock").len()
        }

        fn notes(&self) -> Vec<(NotificationKind, String)> {
            self.notes.lock().expect("lock").clone()
        }
    }

    #[async_trait]
    impl RoutineHost for FakeHost {
        async fn teammate(&self, id: &TeammateId) -> Option<Teammate> {
            self.teammate
                .lock()
                .expect("lock")
                .clone()
                .filter(|t| &t.id == id)
        }
        fn chat_engine_ready(&self, _engine_id: &str) -> bool {
            self.engine_ready
        }
        fn limits(&self) -> Limits {
            self.limits.lock().expect("lock").clone()
        }
        async fn start_chat(&self, _t: &Teammate, r: &Routine) -> Result<ThreadId, IpcError> {
            let id = ThreadId::new();
            self.chats
                .lock()
                .expect("lock")
                .push((id.clone(), r.name.clone(), r.access));
            self.polls.store(0, Ordering::SeqCst);
            Ok(id)
        }
        async fn chat_state(&self, _chat: &ThreadId) -> Option<ThreadState> {
            let n = self.polls.fetch_add(1, Ordering::SeqCst);
            Some(if n + 1 >= self.finish_after {
                ThreadState::Done
            } else {
                ThreadState::Working
            })
        }
        async fn notify(&self, kind: NotificationKind, title: String, _b: String, _s: SourceRef) {
            self.notes.lock().expect("lock").push((kind, title));
        }
    }

    /// What runs with no account: routines and background runs are on
    /// (D-869, D-2900).
    fn on() -> Limits {
        Limits::free()
    }

    /// Limits that turn routines off (a signed token still can).
    fn off() -> Limits {
        Limits {
            routines: false,
            background_routines: false,
            ..Limits::free()
        }
    }

    fn t(s: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(s)
            .expect("t")
            .with_timezone(&chrono::Utc)
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
        clock: MockClock,
        host: Arc<FakeHost>,
        svc: Arc<RoutinesService>,
        teammate: Teammate,
    }

    async fn fixture(start: &str, runner: Runner) -> Fixture {
        let dir = tempfile::tempdir().expect("dir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let teammate = tm_repo::insert(
            &store,
            NewTeammate {
                id: TeammateId::new(),
                name: "Lead".into(),
                avatar_hue: None,
                brief: String::new(),
                engine_id: "claude".into(),
                model: None,
                effort: None,
                preferred_account_id: None,
                home: dir.path().join("home").to_string_lossy().into_owned(),
                places: Vec::new(),
                settings: Default::default(),
                is_lead: true,
                pinned: true,
            },
        )
        .await
        .expect("teammate");
        let clock = MockClock::new(t(start));
        let host = Arc::new(FakeHost::new(teammate.clone()));
        let svc = RoutinesService::new(
            store.clone(),
            EventBus::new(),
            Arc::new(clock.clone()),
            runner,
        );
        svc.set_host(host.clone());
        Fixture {
            _dir: dir,
            store,
            clock,
            host,
            svc,
            teammate,
        }
    }

    fn input(f: &Fixture, schedule: Schedule) -> RoutineInput {
        RoutineInput {
            id: None,
            teammate_id: f.teammate.id.as_str().to_string(),
            name: "Morning summary".into(),
            instruction: "Summarize yesterday's merged pull requests in five bullets.".into(),
            schedule: ScheduleSpec {
                schedule,
                timezone: "UTC".into(),
            },
            access: Access::Edits,
            full_confirmed: false,
            run_mode: RunMode::InApp,
        }
    }

    fn daily9() -> Schedule {
        Schedule::Daily {
            times: vec!["09:00".into()],
        }
    }

    async fn ticks(f: &Fixture, n: usize) -> usize {
        let mut started = 0;
        for _ in 0..n {
            started += f.svc.tick().await.len();
            f.svc.wait_idle(Duration::from_secs(10)).await;
            f.clock.advance(scheduler::TICK);
        }
        started
    }

    #[tokio::test]
    async fn f_rt_06_9_the_gui_waits_for_the_app_lock_before_night_work() {
        let f = fixture("2026-10-15T09:00:00Z", Runner::Gui).await;
        assert!(!f.svc.holds_lock());
        let svc = f.svc.clone();
        let waiter = tokio::spawn(async move { svc.wait_lock().await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !waiter.is_finished(),
            "waits while a headless pass holds it"
        );
        f.svc.mark_locked();
        tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .expect("woken")
            .expect("joined");
        assert!(f.svc.holds_lock());
    }

    #[tokio::test]
    async fn f_rt_02_1_due_at_t_fires_once_and_makes_a_chat_named_after_the_routine() {
        let f = fixture("2026-10-12T08:58:00Z", Runner::Gui).await;
        let r = f.svc.save(input(&f, daily9()), None).await.expect("save");
        assert_eq!(
            r.next_run_at,
            Some(t("2026-10-12T09:00:00Z").timestamp_millis())
        );
        // 08:58 … 09:08: one fire.
        assert_eq!(ticks(&f, 21).await, 1);
        assert_eq!(f.host.chats(), 1);
        let (_, title, access) = f.host.chats.lock().expect("lock")[0].clone();
        assert_eq!(title, "Morning summary");
        assert_eq!(access, Access::Edits);
        let after = f.svc.get(&r.id).await.expect("get");
        assert_eq!(after.last_status.as_deref(), Some("ok"));
        assert!(after.last_run_chat_id.is_some());
        assert_eq!(
            after.next_run_at,
            Some(t("2026-10-13T09:00:00Z").timestamp_millis())
        );
        let runs = f.svc.runs(&r.id, None).await.expect("runs").runs;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Ok);
        assert!(!runs[0].late);
        let started = runs[0].started_at.expect("started");
        assert!((started - t("2026-10-12T09:00:00Z").timestamp_millis()).abs() <= 30_000);
        assert_eq!(
            f.host.notes(),
            vec![(
                NotificationKind::RoutineReady,
                "Ready for review — Morning summary".to_string()
            )]
        );
    }

    #[tokio::test]
    async fn f_rt_02_2_closed_across_the_due_time_gives_exactly_one_late_run() {
        let f = fixture("2026-10-12T08:00:00Z", Runner::Gui).await;
        let r = f
            .svc
            .save(input(&f, Schedule::Interval { every_minutes: 30 }), None)
            .await
            .expect("save");
        // "Closed" until 11:20: no ticks. Then reopened.
        f.clock.set(t("2026-10-12T11:20:00Z"));
        f.svc.settle().await;
        assert_eq!(ticks(&f, 3).await, 1);
        let runs = f.svc.runs(&r.id, None).await.expect("runs").runs;
        assert_eq!(runs.len(), 1);
        assert!(runs[0].late, "marked late");
        // The next slot (11:30) runs on time.
        assert_eq!(ticks(&f, 20).await, 1);
    }

    #[tokio::test]
    async fn f_rt_05_a_teammates_routines_are_its_own_edits_at_most_and_the_user_is_told() {
        let f = fixture("2026-10-12T08:00:00Z", Runner::Gui).await;
        let create = AgentRoutine {
            name: Some("Changelog".into()),
            instruction: Some("Add yesterday's merged work to CHANGELOG.md.".into()),
            cron: Some("0 7 * * 1-5".into()),
            timezone: Some("UTC".into()),
            ..AgentRoutine::default()
        };
        let r = f
            .svc
            .save_for_teammate(&f.teammate, create.clone())
            .await
            .expect("create");
        assert_eq!(r.access, Access::Edits, "Edits by default");
        assert_eq!(r.created_by_teammate_id.as_ref(), Some(&f.teammate.id));
        assert_eq!(
            r.teammate_id.as_ref(),
            Some(&f.teammate.id),
            "runs as itself"
        );
        assert_eq!(
            r.next_run_at,
            Some(t("2026-10-12T07:00:00Z").timestamp_millis() + 86_400_000)
        );
        // Full is refused, on create and on update.
        let full = AgentRoutine {
            access: Some("full".into()),
            ..create.clone()
        };
        assert!(f.svc.save_for_teammate(&f.teammate, full).await.is_err());
        let raise = AgentRoutine {
            id: Some(r.id.as_str().to_string()),
            access: Some("full".into()),
            ..AgentRoutine::default()
        };
        assert!(f.svc.save_for_teammate(&f.teammate, raise).await.is_err());
        // Update: only what it says changes.
        let renamed = f
            .svc
            .save_for_teammate(
                &f.teammate,
                AgentRoutine {
                    id: Some(r.id.as_str().to_string()),
                    name: Some("Changelog draft".into()),
                    paused: Some(true),
                    ..AgentRoutine::default()
                },
            )
            .await
            .expect("update");
        assert_eq!(renamed.name, "Changelog draft");
        assert_eq!(renamed.instruction, r.instruction);
        assert!(renamed.paused);
        // A routine the user made isn't the teammate's to list, change or
        // delete.
        let mine = f.svc.save(input(&f, daily9()), None).await.expect("mine");
        let listed = f.svc.made_by(&f.teammate.id).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, r.id);
        let hijack = AgentRoutine {
            id: Some(mine.id.as_str().to_string()),
            name: Some("Mine now".into()),
            ..AgentRoutine::default()
        };
        assert!(f.svc.save_for_teammate(&f.teammate, hijack).await.is_err());
        assert!(f
            .svc
            .delete_for_teammate(&f.teammate.id, mine.id.as_str())
            .await
            .is_err());
        f.svc
            .delete_for_teammate(&f.teammate.id, r.id.as_str())
            .await
            .expect("delete own");
        // The user heard about the create and the update.
        let told: Vec<String> = f
            .host
            .notes()
            .into_iter()
            .filter(|(k, _)| *k == NotificationKind::RoutineChanged)
            .map(|(_, title)| title)
            .collect();
        assert_eq!(
            told,
            vec![
                "Lead scheduled a routine: Changelog".to_string(),
                "Lead changed the routine Changelog draft".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn f_sh_13_pause_routines_fires_nothing_and_drops_the_slots_it_covered() {
        let f = fixture("2026-10-12T08:58:00Z", Runner::Gui).await;
        let r = f.svc.save(input(&f, daily9()), None).await.expect("save");
        tandem_store::repo::settings::set(
            &f.store,
            SettingKey::PauseAllRoutines,
            serde_json::json!(true),
        )
        .await
        .expect("pause");
        assert_eq!(ticks(&f, 21).await, 0, "nothing fires while paused");
        assert_eq!(f.host.chats(), 0);
        assert_eq!(
            f.svc.get(&r.id).await.expect("get").next_run_at,
            Some(t("2026-10-13T09:00:00Z").timestamp_millis()),
            "09:00 passed while paused: dropped, not caught up"
        );
        tandem_store::repo::settings::set(
            &f.store,
            SettingKey::PauseAllRoutines,
            serde_json::json!(false),
        )
        .await
        .expect("resume");
        assert_eq!(ticks(&f, 3).await, 0, "no late run for the paused slot");
    }

    #[tokio::test]
    async fn f_rt_02_tick_at_runs_the_scheduler_at_a_given_time() {
        let f = fixture("2026-10-12T08:00:00Z", Runner::Gui).await;
        let r = f.svc.save(input(&f, daily9()), None).await.expect("save");
        let at = t("2026-10-12T09:00:10Z").timestamp_millis();
        assert_eq!(f.svc.tick_at(at).await.len(), 1);
        assert!(f.svc.tick_at(at + 30_000).await.is_empty(), "once");
        f.svc.wait_idle(Duration::from_secs(5)).await;
        assert_eq!(f.svc.runs(&r.id, None).await.expect("runs").runs.len(), 1);
    }

    #[tokio::test]
    async fn f_rt_02_3_a_deleted_teammate_errors_on_the_row_and_spawns_nothing() {
        let f = fixture("2026-10-12T08:59:50Z", Runner::Gui).await;
        let r = f.svc.save(input(&f, daily9()), None).await.expect("save");
        *f.host.teammate.lock().expect("lock") = None;
        ticks(&f, 3).await;
        assert_eq!(f.host.chats(), 0, "no spawn");
        let row = f.svc.get(&r.id).await.expect("get");
        assert_eq!(row.last_status.as_deref(), Some("error"));
        assert_eq!(row.last_error_kind.as_deref(), Some("teammate_missing"));
        assert!(row
            .last_error
            .as_deref()
            .is_some_and(|e| e.contains("deleted")));
        // One notification per error type per day.
        f.svc.run_now(&r.id).await.expect("again");
        f.svc.wait_idle(Duration::from_secs(5)).await;
        let failed: Vec<_> = f
            .host
            .notes()
            .into_iter()
            .filter(|(k, _)| *k == NotificationKind::Failed)
            .collect();
        assert_eq!(failed.len(), 1);
    }

    #[tokio::test]
    async fn f_rt_04_1_run_now_makes_a_chat_at_once_then_ready_for_review() {
        let f = fixture("2026-10-12T12:00:00Z", Runner::Gui).await;
        let r = f.svc.save(input(&f, daily9()), None).await.expect("save");
        let run = f.svc.run_now(&r.id).await.expect("run now");
        // Running first …
        tokio::time::sleep(Duration::from_millis(50)).await;
        let view = f.svc.view(f.svc.get(&r.id).await.expect("get")).await;
        assert_eq!(view.running.as_ref().map(|r| &r.id), Some(&run));
        assert!(view.running.as_ref().is_some_and(|r| r.manual));
        // … a second Run now meanwhile is refused …
        let again = f.svc.run_now(&r.id).await.expect_err("busy");
        assert_eq!(again.message, "previous run still in progress");
        // … then Ready for review.
        f.svc.wait_idle(Duration::from_secs(5)).await;
        assert_eq!(f.host.chats(), 1);
        let row = f.svc.get(&r.id).await.expect("get");
        assert_eq!(row.last_status.as_deref(), Some("ok"));
        let view = f.svc.view(row).await;
        assert!(view.running.is_none());
        assert_eq!(view.today.len(), 1, "on today's timeline");
    }

    #[tokio::test]
    async fn f_rt_04_2_a_paused_routine_does_not_fire_at_its_time() {
        let f = fixture("2026-10-12T08:59:00Z", Runner::Gui).await;
        let r = f.svc.save(input(&f, daily9()), None).await.expect("save");
        f.svc.set_paused(&r.id, true).await.expect("pause");
        assert_eq!(ticks(&f, 10).await, 0);
        assert_eq!(f.host.chats(), 0);
        // Resuming schedules from now: no catch-up for the paused stretch.
        f.svc.set_paused(&r.id, false).await.expect("resume");
        let row = f.svc.get(&r.id).await.expect("get");
        assert_eq!(
            row.next_run_at,
            Some(t("2026-10-13T09:00:00Z").timestamp_millis())
        );
        assert_eq!(ticks(&f, 4).await, 0);
    }

    #[tokio::test]
    async fn f_rt_02_overlap_is_skipped_with_its_reason() {
        let f = fixture("2026-10-12T08:00:00Z", Runner::Gui).await;
        let r = f
            .svc
            .save(input(&f, Schedule::Interval { every_minutes: 15 }), None)
            .await
            .expect("save");
        // A run still going (claimed, never finished).
        repo::claim(&f.store, &r.id, 1, false, true, Runner::Gui, 1)
            .await
            .expect("claim")
            .expect("run");
        f.clock.set(t("2026-10-12T08:15:10Z"));
        assert_eq!(f.svc.tick().await.len(), 0);
        let runs = f.svc.runs(&r.id, None).await.expect("runs").runs;
        let skipped: Vec<_> = runs
            .iter()
            .filter(|r| r.status == RunStatus::Skipped)
            .collect();
        assert_eq!(skipped.len(), 1);
        assert_eq!(
            skipped[0].error.as_deref(),
            Some("previous run still in progress")
        );
    }

    #[tokio::test]
    async fn f_rt_01_saving_needs_routines_on_an_explicit_full_and_a_teammate() {
        let f = fixture("2026-10-12T08:00:00Z", Runner::Gui).await;
        let mut full = input(&f, daily9());
        full.access = Access::Full;
        let e = f.svc.save(full.clone(), None).await.expect_err("full");
        assert_eq!(e.code, ErrorCode::Invalid);
        full.full_confirmed = true;
        let saved = f.svc.save(full, None).await.expect("explicit full");
        assert_eq!(saved.access, Access::Full);
        *f.host.limits.lock().expect("lock") = off();
        let e = f
            .svc
            .save(input(&f, daily9()), None)
            .await
            .expect_err("off");
        assert_eq!(e.code, ErrorCode::ForbiddenPlan);
        // Limits that turn routines off pause everything; turning them back on resumes.
        f.svc.apply_plan(&off()).await;
        assert!(f.svc.get(&saved.id).await.expect("get").paused_by_plan);
        f.svc.apply_plan(&on()).await;
        assert!(!f.svc.get(&saved.id).await.expect("get").paused_by_plan);
    }

    #[tokio::test]
    async fn f_rt_01_routines_save_and_stay_unpaused_with_no_account() {
        let f = fixture("2026-10-12T08:00:00Z", Runner::Gui).await;
        *f.host.limits.lock().expect("lock") = Limits::free();
        let saved = f
            .svc
            .save(input(&f, daily9()), None)
            .await
            .expect("routines with no account (D-2900)");
        f.svc.apply_plan(&Limits::free()).await;
        assert!(!f.svc.get(&saved.id).await.expect("get").paused_by_plan);
    }

    #[tokio::test]
    async fn f_rt_03_headless_runs_only_background_routines() {
        let f = fixture("2026-10-12T08:59:50Z", Runner::Headless).await;
        let in_app = f.svc.save(input(&f, daily9()), None).await.expect("save");
        let mut bg = input(&f, daily9());
        bg.name = "Background".into();
        bg.run_mode = RunMode::Background;
        let bg = f.svc.save(bg, None).await.expect("save");
        f.clock.set(t("2026-10-12T09:00:10Z"));
        let started = f.svc.tick().await;
        f.svc.wait_idle(Duration::from_secs(5)).await;
        assert_eq!(started.len(), 1);
        assert_eq!(
            f.svc.get(&bg.id).await.expect("bg").last_status.as_deref(),
            Some("ok")
        );
        assert_eq!(
            f.svc.get(&in_app.id).await.expect("in app").last_status,
            None
        );
    }

    #[tokio::test]
    async fn f_rt_03_3_with_the_gui_holding_the_lock_a_headless_pass_runs_nothing() {
        use crate::headless::{pass, try_lock, Pass};
        let f = fixture("2026-10-12T08:59:50Z", Runner::Headless).await;
        let mut bg = input(&f, daily9());
        bg.run_mode = RunMode::Background;
        let r = f.svc.save(bg, None).await.expect("save");
        f.clock.set(t("2026-10-12T09:00:10Z"));
        let run = f._dir.path().join("run");
        let gui = try_lock(&run, "gui").expect("lock").expect("free");
        let info: crate::headless::LockInfo =
            serde_json::from_slice(&std::fs::read(run.join("app.lock.json")).expect("info"))
                .expect("json");
        assert_eq!(info.mode, "gui");
        let cap = Duration::from_secs(5);
        assert_eq!(
            pass(&run, true, &f.svc, cap).await.expect("pass"),
            Pass::Busy
        );
        assert_eq!(f.host.chats(), 0, "nothing ran");
        assert!(f.svc.runs(&r.id, None).await.expect("runs").runs.is_empty());
        drop(gui);
        // F-RT-03 AC2: no GUI, a due routine runs and the pass ends.
        assert_eq!(
            pass(&run, true, &f.svc, cap).await.expect("pass"),
            Pass::Ran(1)
        );
        assert_eq!(f.host.chats(), 1);
        assert_eq!(
            f.svc.get(&r.id).await.expect("get").last_status.as_deref(),
            Some("ok")
        );
        let runs = f.svc.runs(&r.id, None).await.expect("runs").runs;
        assert_eq!(runs[0].runner, Runner::Headless);
        // The lock is free again, and a plan without background runs does
        // nothing.
        assert_eq!(
            pass(&run, false, &f.svc, cap).await.expect("pass"),
            Pass::NotOnPlan
        );
        assert!(!run.join("app.lock.json").exists());
    }
}
