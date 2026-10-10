//! `routines` and `routine_runs` (F-RT-01, F-RT-02). A due slot fires at
//! most once across processes: `routine_runs` is unique on
//! `(routine_id, due_at)`, so the GUI and a headless run can't both claim
//! it. Callers pass `now`, so the scheduler runs on the injected clock.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::access::Access;
use tandem_core::ids::{RoutineId, RunId, TeammateId, ThreadId};
use tandem_routines::{RunMode, Schedule, ScheduleSpec};
use ts_rs::TS;

use crate::{Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Routine {
    pub id: RoutineId,
    /// `None` once its teammate is deleted: the next run fails with a reason.
    pub teammate_id: Option<TeammateId>,
    pub name: String,
    pub instruction: String,
    pub schedule: ScheduleSpec,
    pub access: Access,
    pub run_mode: RunMode,
    pub paused: bool,
    /// Paused because the plan no longer has routines (F-BL-04).
    pub paused_by_plan: bool,
    /// Made by a teammate's schedule tool (F-RT-05).
    pub created_by_teammate_id: Option<TeammateId>,
    pub last_run_chat_id: Option<ThreadId>,
    /// `ok` (Ready for review), `error`, `running`, `skipped`.
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_error_kind: Option<String>,
    pub next_run_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RunStatus {
    Running,
    Ok,
    Error,
    Skipped,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Ok => "ok",
            RunStatus::Error => "error",
            RunStatus::Skipped => "skipped",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "running" => RunStatus::Running,
            "ok" => RunStatus::Ok,
            "skipped" => RunStatus::Skipped,
            _ => RunStatus::Error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Runner {
    Gui,
    Headless,
}

impl Runner {
    pub fn as_str(self) -> &'static str {
        match self {
            Runner::Gui => "gui",
            Runner::Headless => "headless",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoutineRun {
    pub id: RunId,
    pub routine_id: RoutineId,
    /// The slot it was due at; `Run now` uses the time it was asked for.
    pub due_at: i64,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub status: RunStatus,
    /// A catch-up after the app was closed across the due time.
    pub late: bool,
    pub runner: Runner,
    /// `true` for Run now.
    pub manual: bool,
    pub chat_id: Option<ThreadId>,
    pub error: Option<String>,
}

/// A new routine, already validated.
#[derive(Debug, Clone)]
pub struct NewRoutine {
    pub id: RoutineId,
    pub teammate_id: TeammateId,
    pub name: String,
    pub instruction: String,
    pub schedule: ScheduleSpec,
    pub access: Access,
    pub run_mode: RunMode,
    pub created_by_teammate_id: Option<TeammateId>,
    pub next_run_at: Option<i64>,
}

const COLS: &str = "id, teammate_id, name, instruction, schedule_json, timezone, access, run_mode, paused, paused_by_plan, created_by_teammate_id, last_run_chat_id, last_status, last_error, last_error_kind, next_run_at, created_at, updated_at";
const RUN_COLS: &str =
    "id, routine_id, due_at, started_at, ended_at, status, late, runner, chat_id, error";

fn row(r: &Row<'_>) -> rusqlite::Result<Routine> {
    let schedule_json: String = r.get(4)?;
    let schedule: Schedule = serde_json::from_str(&schedule_json).unwrap_or(Schedule::Cron {
        // A row this build can't read never runs (its cron is empty).
        expr: String::new(),
    });
    let access: String = r.get(6)?;
    let run_mode: String = r.get(7)?;
    Ok(Routine {
        id: RoutineId::from_db(r.get(0)?),
        teammate_id: r.get::<_, Option<String>>(1)?.map(TeammateId::from_db),
        name: r.get(2)?,
        instruction: r.get(3)?,
        schedule: ScheduleSpec {
            schedule,
            timezone: r.get(5)?,
        },
        access: Access::parse(&access).unwrap_or(Access::Edits),
        run_mode: RunMode::parse(&run_mode).unwrap_or(RunMode::InApp),
        paused: r.get::<_, i64>(8)? != 0,
        paused_by_plan: r.get::<_, i64>(9)? != 0,
        created_by_teammate_id: r.get::<_, Option<String>>(10)?.map(TeammateId::from_db),
        last_run_chat_id: r.get::<_, Option<String>>(11)?.map(ThreadId::from_db),
        last_status: r.get(12)?,
        last_error: r.get(13)?,
        last_error_kind: r.get(14)?,
        next_run_at: r.get(15)?,
        created_at: r.get(16)?,
        updated_at: r.get(17)?,
    })
}

/// `error` holds `manual:` first for Run now (the schema has no column).
const MANUAL: &str = "\u{1}manual";

fn run_row(r: &Row<'_>) -> rusqlite::Result<RoutineRun> {
    let status: String = r.get(5)?;
    let runner: String = r.get(7)?;
    let error: Option<String> = r.get(9)?;
    let (manual, error) = match error {
        Some(e) if e.starts_with(MANUAL) => {
            let rest = e[MANUAL.len()..].to_string();
            (true, (!rest.is_empty()).then_some(rest))
        }
        other => (false, other),
    };
    Ok(RoutineRun {
        id: RunId::from_db(r.get(0)?),
        routine_id: RoutineId::from_db(r.get(1)?),
        due_at: r.get(2)?,
        started_at: r.get(3)?,
        ended_at: r.get(4)?,
        status: RunStatus::parse(&status),
        late: r.get::<_, i64>(6)? != 0,
        runner: if runner == "headless" {
            Runner::Headless
        } else {
            Runner::Gui
        },
        manual,
        chat_id: r.get::<_, Option<String>>(8)?.map(ThreadId::from_db),
        error,
    })
}

fn schedule_json(spec: &ScheduleSpec) -> String {
    serde_json::to_string(&spec.schedule).unwrap_or_default()
}

pub async fn insert(store: &Store, new: NewRoutine, now: i64) -> Result<Routine, StoreError> {
    let id = new.id.clone();
    store
        .write(move |tx| {
            tx.execute(
                "INSERT INTO routines (id, teammate_id, name, instruction, schedule_json, timezone, access, run_mode, created_by_teammate_id, next_run_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                params![
                    new.id.as_str(),
                    new.teammate_id.as_str(),
                    new.name,
                    new.instruction,
                    schedule_json(&new.schedule),
                    new.schedule.timezone,
                    new.access.as_str(),
                    new.run_mode.as_str(),
                    new.created_by_teammate_id.as_ref().map(|t| t.as_str().to_string()),
                    new.next_run_at,
                    now,
                ],
            )?;
            Ok(())
        })
        .await?;
    get(store, &id).await
}

/// Replaces what the form edits; keeps the run state.
pub async fn update(
    store: &Store,
    id: &RoutineId,
    new: NewRoutine,
    now: i64,
) -> Result<Routine, StoreError> {
    let key = id.clone();
    let changed = store
        .write(move |tx| {
            Ok(tx.execute(
                "UPDATE routines SET teammate_id = ?2, name = ?3, instruction = ?4, schedule_json = ?5, timezone = ?6, access = ?7, run_mode = ?8, next_run_at = ?9, updated_at = ?10 WHERE id = ?1",
                params![
                    key.as_str(),
                    new.teammate_id.as_str(),
                    new.name,
                    new.instruction,
                    schedule_json(&new.schedule),
                    new.schedule.timezone,
                    new.access.as_str(),
                    new.run_mode.as_str(),
                    new.next_run_at,
                    now,
                ],
            )?)
        })
        .await?;
    if changed == 0 {
        return Err(StoreError::NotFound("That routine doesn't exist".into()));
    }
    get(store, id).await
}

pub async fn get(store: &Store, id: &RoutineId) -> Result<Routine, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            c.query_row(
                &format!("SELECT {COLS} FROM routines WHERE id = ?1"),
                [&id],
                row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("That routine doesn't exist".into()))
        })
        .await
}

/// Every routine, by name.
pub async fn list(store: &Store) -> Result<Vec<Routine>, StoreError> {
    store
        .read(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {COLS} FROM routines ORDER BY name COLLATE NOCASE, created_at"
            ))?;
            let out = st.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(out)
        })
        .await
}

/// Deleting a routine keeps its past chats (their `routine_id` clears).
pub async fn delete(store: &Store, id: &RoutineId) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |tx| {
            tx.execute("DELETE FROM routines WHERE id = ?1", [&id])?;
            Ok(())
        })
        .await
}

pub async fn set_paused(
    store: &Store,
    id: &RoutineId,
    paused: bool,
    next_run_at: Option<i64>,
    now: i64,
) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |tx| {
            tx.execute(
                "UPDATE routines SET paused = ?2, next_run_at = ?3, updated_at = ?4 WHERE id = ?1",
                params![id, paused as i64, next_run_at, now],
            )?;
            Ok(())
        })
        .await
}

/// Plan lapse pauses every routine (kept, not deleted); an upgrade
/// resumes them (F-BL-04).
pub async fn set_paused_by_plan(store: &Store, paused: bool) -> Result<usize, StoreError> {
    store
        .write(move |tx| {
            Ok(tx.execute(
                "UPDATE routines SET paused_by_plan = ?1 WHERE paused_by_plan != ?1",
                [paused as i64],
            )?)
        })
        .await
}

pub async fn set_next(
    store: &Store,
    id: &RoutineId,
    next_run_at: Option<i64>,
) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |tx| {
            tx.execute(
                "UPDATE routines SET next_run_at = ?2 WHERE id = ?1",
                params![id, next_run_at],
            )?;
            Ok(())
        })
        .await
}

/// The routine's last outcome, shown on its row.
pub async fn set_last(
    store: &Store,
    id: &RoutineId,
    status: &str,
    error: Option<(String, String)>,
    chat: Option<&ThreadId>,
) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    let status = status.to_string();
    let chat = chat.map(|c| c.as_str().to_string());
    store
        .write(move |tx| {
            let (kind, message) = match error {
                Some((k, m)) => (Some(k), Some(m)),
                None => (None, None),
            };
            tx.execute(
                "UPDATE routines SET last_status = ?2, last_error_kind = ?3, last_error = ?4,
                   last_run_chat_id = COALESCE(?5, last_run_chat_id) WHERE id = ?1",
                params![id, status, kind, message, chat],
            )?;
            Ok(())
        })
        .await
}

/// Claims the slot `due_at` for one run. `None`: another process (or an
/// earlier tick) already has it.
pub async fn claim(
    store: &Store,
    routine: &RoutineId,
    due_at: i64,
    late: bool,
    manual: bool,
    runner: Runner,
    now: i64,
) -> Result<Option<RunId>, StoreError> {
    let routine = routine.as_str().to_string();
    let id = RunId::new();
    let run = id.clone();
    let inserted = store
        .write(move |tx| {
            Ok(tx.execute(
                "INSERT OR IGNORE INTO routine_runs (id, routine_id, due_at, started_at, status, late, runner, error)
                 VALUES (?1, ?2, ?3, ?4, 'running', ?5, ?6, ?7)",
                params![
                    run.as_str(),
                    routine,
                    due_at,
                    now,
                    late as i64,
                    runner.as_str(),
                    manual.then(|| MANUAL.to_string()),
                ],
            )?)
        })
        .await?;
    Ok((inserted == 1).then_some(id))
}

/// A slot skipped (previous run still going): recorded, never run.
pub async fn skip(
    store: &Store,
    routine: &RoutineId,
    due_at: i64,
    reason: &str,
    runner: Runner,
    now: i64,
) -> Result<bool, StoreError> {
    let routine = routine.as_str().to_string();
    let reason = reason.to_string();
    let inserted = store
        .write(move |tx| {
            Ok(tx.execute(
                "INSERT OR IGNORE INTO routine_runs (id, routine_id, due_at, started_at, ended_at, status, runner, error)
                 VALUES (?1, ?2, ?3, ?4, ?4, 'skipped', ?5, ?6)",
                params![RunId::new().as_str(), routine, due_at, now, runner.as_str(), reason],
            )?)
        })
        .await?;
    Ok(inserted == 1)
}

pub async fn set_run_chat(store: &Store, run: &RunId, chat: &ThreadId) -> Result<(), StoreError> {
    let (run, chat) = (run.as_str().to_string(), chat.as_str().to_string());
    store
        .write(move |tx| {
            tx.execute(
                "UPDATE routine_runs SET chat_id = ?2 WHERE id = ?1",
                params![run, chat],
            )?;
            Ok(())
        })
        .await
}

pub async fn finish_run(
    store: &Store,
    run: &RunId,
    status: RunStatus,
    error: Option<String>,
    now: i64,
) -> Result<(), StoreError> {
    let run = run.as_str().to_string();
    store
        .write(move |tx| {
            let manual: bool = tx
                .query_row(
                    "SELECT COALESCE(error, '') FROM routine_runs WHERE id = ?1",
                    [&run],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .is_some_and(|e| e.starts_with(MANUAL));
            let error = match (manual, error) {
                (true, e) => Some(format!("{MANUAL}{}", e.unwrap_or_default())),
                (false, e) => e,
            };
            tx.execute(
                "UPDATE routine_runs SET status = ?2, error = ?3, ended_at = ?4 WHERE id = ?1",
                params![run, status.as_str(), error, now],
            )?;
            Ok(())
        })
        .await
}

pub async fn get_run(store: &Store, run: &RunId) -> Result<RoutineRun, StoreError> {
    let run = run.as_str().to_string();
    store
        .read(move |c| {
            c.query_row(
                &format!("SELECT {RUN_COLS} FROM routine_runs WHERE id = ?1"),
                [&run],
                run_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("That run doesn't exist".into()))
        })
        .await
}

/// The routine's run in progress, if any.
pub async fn running(store: &Store, routine: &RoutineId) -> Result<Option<RoutineRun>, StoreError> {
    let routine = routine.as_str().to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!(
                    "SELECT {RUN_COLS} FROM routine_runs WHERE routine_id = ?1 AND status = 'running' ORDER BY started_at DESC LIMIT 1"
                ),
                [&routine],
                run_row,
            )
            .optional()?)
        })
        .await
}

/// Newest first, `limit` at a time, before `before_due` when given.
pub async fn runs(
    store: &Store,
    routine: &RoutineId,
    before_due: Option<i64>,
    limit: usize,
) -> Result<Vec<RoutineRun>, StoreError> {
    let routine = routine.as_str().to_string();
    store
        .read(move |c| {
            let mut st = c.prepare(&format!(
                "SELECT {RUN_COLS} FROM routine_runs WHERE routine_id = ?1 AND due_at < ?2 ORDER BY due_at DESC LIMIT ?3"
            ))?;
            let out = st
                .query_map(
                    params![routine, before_due.unwrap_or(i64::MAX), limit as i64],
                    run_row,
                )?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(out)
        })
        .await
}

/// Runs a process left `running` when it went away: settled as errors.
pub async fn settle_interrupted(store: &Store, now: i64) -> Result<usize, StoreError> {
    store
        .write(move |tx| {
            Ok(tx.execute(
                "UPDATE routine_runs SET status = 'error', ended_at = ?1,
                   error = CASE WHEN error LIKE ?2 THEN error || 'Vibezeo closed during the run.' ELSE 'Vibezeo closed during the run.' END
                 WHERE status = 'running'",
                params![now, format!("{MANUAL}%")],
            )?)
        })
        .await
}

/// Keeps the last `keep` runs per routine (UI-53's retention).
pub async fn prune_runs(store: &Store, keep: usize) -> Result<usize, StoreError> {
    store
        .write(move |tx| {
            Ok(tx.execute(
                "DELETE FROM routine_runs WHERE id IN (
                   SELECT id FROM (
                     SELECT id, ROW_NUMBER() OVER (PARTITION BY routine_id ORDER BY due_at DESC) AS n
                     FROM routine_runs) WHERE n > ?1)",
                [keep as i64],
            )?)
        })
        .await
}

/// True the first time `(kind, subject, day)` is seen: one failure
/// notification per error type per routine per day (F-RT-02).
pub async fn first_notice(
    store: &Store,
    kind: &str,
    subject: &str,
    day: &str,
) -> Result<bool, StoreError> {
    let (kind, subject, day) = (kind.to_string(), subject.to_string(), day.to_string());
    store
        .write(move |tx| {
            Ok(tx.execute(
                "INSERT OR IGNORE INTO error_notice_log (kind, subject_id, day) VALUES (?1, ?2, ?3)",
                params![kind, subject, day],
            )? == 1)
        })
        .await
}

/// A run's chat: titled with the routine's name (`title_source` routine,
/// so auto-title never renames it) and pointing at the routine until it's
/// deleted.
pub async fn set_chat_routine(
    store: &Store,
    chat: &ThreadId,
    routine: &RoutineId,
    name: &str,
) -> Result<(), StoreError> {
    let (chat, routine) = (chat.as_str().to_string(), routine.as_str().to_string());
    let name = name.to_string();
    store
        .write(move |tx| {
            tx.execute(
                "UPDATE threads SET routine_id = ?2, title = ?3, title_source = 'routine' WHERE id = ?1",
                params![chat, routine, name],
            )?;
            Ok(())
        })
        .await
}

/// The routine a chat was made for, if any.
pub async fn routine_of_chat(
    store: &Store,
    chat: &ThreadId,
) -> Result<Option<Routine>, StoreError> {
    let chat = chat.as_str().to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!(
                    "SELECT {} FROM routines r JOIN threads t ON t.routine_id = r.id WHERE t.id = ?1",
                    COLS.split(", ").map(|c| format!("r.{c}")).collect::<Vec<_>>().join(", ")
                ),
                [&chat],
                row,
            )
            .optional()?)
        })
        .await
}
