//! The Night Shift (F-RT-06): a task queued to start on an account the user
//! picked when that account's usage window resets, then a morning summary.
//! The user approves the plan in UI-171; the run never uses another account
//! or engine (F-AC-05). Runs live in `$DATA/night-shift.json`.
//!
//! The decisions ([`next_reset`], [`due`], [`summary`], [`summary_due`])
//! are pure and tested; [`NightShiftService::tick`] glues them to usage,
//! limits, task execution, notifications and the phone.

use std::path::PathBuf;
use std::sync::Mutex;

use chrono::{DateTime, FixedOffset, NaiveTime};
use serde::{Deserialize, Serialize};
use tandem_core::access::Access;
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::{AccountId, AttemptId, WorkspaceId};
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::usage::UsageWindow;
use tandem_store::repo::task_attempts::AttemptState;
use ts_rs::TS;

use crate::services::tasks_exec::{ExecuteInput, Isolation};
use crate::state::AppState;

/// A window at least this full when the run is due hasn't reset yet.
pub const STILL_FULL: f64 = 95.0;
/// How often the app checks for due runs and the summary.
pub const TICK: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NightState {
    Scheduled,
    /// Due, but the account hadn't reset yet: waiting for the next reset.
    Waiting,
    Started,
    Done,
    /// Failed, stopped, or couldn't start.
    NeedsYou,
    Cancelled,
}

impl NightState {
    fn ended(self) -> bool {
        matches!(self, NightState::Done | NightState::NeedsYou)
    }
}

/// One Night Shift run (UI-171, the task's "Runs at …").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NightRun {
    pub id: String,
    pub issue_id: String,
    pub title: String,
    pub workspace_id: WorkspaceId,
    pub account_id: AccountId,
    pub engine_id: String,
    pub isolation: Isolation,
    pub access: Access,
    pub full_confirmed: bool,
    pub review: bool,
    /// When it starts (Unix ms): the account's reset.
    pub run_at: i64,
    /// The window it waits for ("5h", "week", or "limit").
    pub window: String,
    pub state: NightState,
    pub attempt_id: Option<AttemptId>,
    /// Why it couldn't start.
    pub reason: Option<String>,
    pub created_at: i64,
    pub ended_at: Option<i64>,
}

/// What "Run at reset…" asks the core to save.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NightInput {
    pub issue_id: String,
    pub workspace_id: WorkspaceId,
    pub account_id: AccountId,
    pub isolation: Isolation,
    pub access: Access,
    #[serde(default)]
    pub full_confirmed: bool,
    pub review: bool,
}

/// An account's next reset, for UI-171.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NightAccount {
    pub account_id: AccountId,
    pub engine_id: String,
    /// Unix ms, when known.
    pub resets_at: Option<i64>,
    pub window: Option<String>,
}

/// The account's next reset: its fullest usage window's, else the last
/// limit a pane hit on it (F-AC-05). Only resets after `now` count.
pub fn next_reset(
    windows: &[UsageWindow],
    limit_resets: &[i64],
    now: i64,
) -> Option<(i64, String)> {
    let fullest = windows
        .iter()
        .filter(|w| w.resets_at.is_some_and(|r| r > now))
        .max_by(|a, b| {
            a.used_pct
                .partial_cmp(&b.used_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    if let Some(w) = fullest {
        return w.resets_at.map(|r| (r, w.kind.clone()));
    }
    limit_resets
        .iter()
        .filter(|r| **r > now)
        .min()
        .map(|r| (*r, "limit".to_string()))
}

/// What a scheduled run does now.
#[derive(Debug, Clone, PartialEq)]
pub enum Due {
    NotYet,
    Start,
    /// Still full: wait for this later reset.
    Wait(i64),
}

/// A run's move at `now`, given the account's current windows.
pub fn due(run: &NightRun, windows: &[UsageWindow], now: i64) -> Due {
    if !matches!(run.state, NightState::Scheduled | NightState::Waiting) || now < run.run_at {
        return Due::NotYet;
    }
    let later_full = windows
        .iter()
        .filter(|w| w.used_pct >= STILL_FULL)
        .filter_map(|w| w.resets_at)
        .filter(|r| *r > now + 60_000)
        .max();
    match later_full {
        Some(r) => Due::Wait(r),
        None => Due::Start,
    }
}

/// The morning summary over runs that ended after `since`: `(done, needs
/// you, titles)`, or `None` when none did.
pub fn summary(runs: &[NightRun], since: i64) -> Option<(usize, usize, Vec<String>)> {
    let ended: Vec<&NightRun> = runs
        .iter()
        .filter(|r| r.state.ended() && r.ended_at.is_some_and(|t| t > since))
        .collect();
    if ended.is_empty() {
        return None;
    }
    let done = ended.iter().filter(|r| r.state == NightState::Done).count();
    Some((
        done,
        ended.len() - done,
        ended.iter().map(|r| r.title.clone()).collect(),
    ))
}

/// "Night Shift: 1 done, 1 needs you".
pub fn summary_title(done: usize, needs: usize) -> String {
    let needs = match needs {
        0 => String::new(),
        1 => ", 1 needs you".to_string(),
        n => format!(", {n} need you"),
    };
    format!("Night Shift: {done} done{needs}")
}

/// Whether the summary is due: it's past `at` ("HH:MM") today, in `now`'s
/// zone, and today's summary hasn't been sent.
pub fn summary_due(now: DateTime<FixedOffset>, at: &str, last_sent: Option<&str>) -> bool {
    let time = NaiveTime::parse_from_str(at, "%H:%M")
        .unwrap_or_else(|_| NaiveTime::from_hms_opt(8, 0, 0).unwrap_or_default());
    let today = now.date_naive().format("%Y-%m-%d").to_string();
    now.time() >= time && last_sent != Some(today.as_str())
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    runs: Vec<NightRun>,
    /// The day (`YYYY-MM-DD`) of the last summary, and when it was sent.
    last_summary_day: Option<String>,
    last_summary_at: i64,
}

pub struct NightShiftService {
    path: PathBuf,
    bus: EventBus,
    file: Mutex<File>,
}

impl NightShiftService {
    pub fn new(path: PathBuf, bus: EventBus) -> Self {
        let file = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<File>(&b).ok())
            .unwrap_or_default();
        Self {
            path,
            bus,
            file: Mutex::new(file),
        }
    }

    fn save(&self, file: &File) {
        let Ok(bytes) = serde_json::to_vec_pretty(file) else {
            return;
        };
        if let Err(e) = tandem_core::atomic::atomic_write(&self.path, &bytes, None) {
            tracing::warn!(code = "night.save_failed", error = %e, "couldn't save the Night Shift runs");
        }
    }

    fn changed(&self) {
        self.bus
            .publish(BusEvent::new(Topic::Status, "night:changed", self.list()));
    }

    pub fn list(&self) -> Vec<NightRun> {
        self.file
            .lock()
            .map(|f| {
                f.runs
                    .iter()
                    .filter(|r| r.state != NightState::Cancelled)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn update(&self, f: impl FnOnce(&mut File)) {
        if let Ok(mut file) = self.file.lock() {
            f(&mut file);
            self.save(&file);
        }
        self.changed();
    }

    /// Saves a run the user approved in UI-171.
    pub fn add(&self, run: NightRun) -> NightRun {
        self.update(|f| f.runs.push(run.clone()));
        run
    }

    /// Cancel before it starts.
    pub fn cancel(&self, id: &str) -> Result<(), IpcError> {
        let mut found = false;
        self.update(|f| {
            if let Some(r) = f.runs.iter_mut().find(|r| {
                r.id == id && matches!(r.state, NightState::Scheduled | NightState::Waiting)
            }) {
                r.state = NightState::Cancelled;
                found = true;
            }
        });
        if found {
            Ok(())
        } else {
            Err(IpcError::new(
                ErrorCode::NotFound,
                "That run has started or is gone.",
            ))
        }
    }

    /// The test harness: every scheduled or waiting run is due at `now` (when
    /// given),
    /// and with `reset_summary` today's summary can be sent again.
    pub fn make_due(&self, now: Option<i64>, reset_summary: bool) {
        self.update(|f| {
            if let Some(now) = now {
                for r in f
                    .runs
                    .iter_mut()
                    .filter(|r| matches!(r.state, NightState::Scheduled | NightState::Waiting))
                {
                    r.run_at = now;
                }
            }
            if reset_summary {
                f.last_summary_day = None;
            }
        });
    }

    /// Runs waiting to start or running.
    pub fn has_pending(&self) -> bool {
        self.list().iter().any(|r| {
            matches!(
                r.state,
                NightState::Scheduled | NightState::Waiting | NightState::Started
            )
        })
    }

    /// One pass: start due runs, follow started ones, send the summary.
    pub async fn tick(&self, state: &AppState) {
        let now = state.clock.now_ms();
        let runs = self.list();
        // Due runs: read the account's usage again first.
        for run in runs.iter().filter(|r| {
            matches!(r.state, NightState::Scheduled | NightState::Waiting) && r.run_at <= now
        }) {
            let windows = account_windows(state, &run.account_id).await;
            match due(run, &windows, now) {
                Due::NotYet => {}
                Due::Wait(at) => self.update(|f| {
                    if let Some(r) = f.runs.iter_mut().find(|r| r.id == run.id) {
                        r.state = NightState::Waiting;
                        r.run_at = at;
                    }
                }),
                Due::Start => {
                    let input = ExecuteInput {
                        issue_id: run.issue_id.clone(),
                        workspace_id: run.workspace_id.clone(),
                        teammate_id: None,
                        engine_id: Some(run.engine_id.clone()),
                        account_id: Some(run.account_id.clone()),
                        isolation: run.isolation,
                        access: run.access,
                        full_confirmed: run.full_confirmed,
                        review: run.review,
                        note: None,
                    };
                    let out = state.task_exec.execute(input).await;
                    let ended = state.clock.now_ms();
                    self.update(|f| {
                        if let Some(r) = f.runs.iter_mut().find(|r| r.id == run.id) {
                            match out {
                                Ok(view) => {
                                    r.state = NightState::Started;
                                    r.attempt_id = Some(view.attempt.id);
                                }
                                Err(e) => {
                                    r.state = NightState::NeedsYou;
                                    r.reason = Some(e.message);
                                    r.ended_at = Some(ended);
                                }
                            }
                        }
                    });
                }
            }
        }
        // Started runs end with their attempt.
        let started: Vec<NightRun> = self
            .list()
            .into_iter()
            .filter(|r| r.state == NightState::Started)
            .collect();
        if !started.is_empty() {
            let attempts = state.task_exec.list().await.unwrap_or_default();
            for run in started {
                let Some(a) = attempts
                    .iter()
                    .find(|a| Some(&a.attempt.id) == run.attempt_id.as_ref())
                else {
                    continue;
                };
                let next = match a.attempt.state {
                    AttemptState::Done | AttemptState::InReview => NightState::Done,
                    AttemptState::Failed | AttemptState::Stopped => NightState::NeedsYou,
                    _ => continue,
                };
                let at = state.clock.now_ms();
                self.update(|f| {
                    if let Some(r) = f.runs.iter_mut().find(|r| r.id == run.id) {
                        r.state = next;
                        r.ended_at = Some(at);
                    }
                });
            }
        }
        self.maybe_summarize(state).await;
    }

    async fn maybe_summarize(&self, state: &AppState) {
        let at = tandem_store::repo::settings::get(
            &state.store,
            tandem_core::settings::SettingKey::NightShiftSummaryAt,
        )
        .await
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "08:00".into());
        let Some(now) = DateTime::from_timestamp_millis(state.clock.now_ms()) else {
            return;
        };
        let now = now.with_timezone(chrono::Local::now().offset());
        let (last_day, since) = match self.file.lock() {
            Ok(f) => (f.last_summary_day.clone(), f.last_summary_at),
            Err(_) => return,
        };
        if !summary_due(now, &at, last_day.as_deref()) {
            return;
        }
        let runs = self.list();
        let today = now.date_naive().format("%Y-%m-%d").to_string();
        let sent_at = now.timestamp_millis();
        if let Some((done, needs, titles)) = summary(&runs, since) {
            let title = summary_title(done, needs);
            let body = titles.join(", ");
            let issue = runs
                .iter()
                .find(|r| r.state.ended() && r.ended_at.is_some_and(|t| t > since))
                .map(|r| r.issue_id.clone())
                .unwrap_or_default();
            state
                .notifications
                .raise_night_shift(title.clone(), body.clone(), issue)
                .await;
            state
                .phone
                .queue_alert(tandem_core::remote::push::PushAlert {
                    kind: "night_shift".into(),
                    id: format!("night:{today}"),
                    title,
                    body: body.chars().take(160).collect(),
                    at: sent_at,
                });
        }
        if let Ok(mut f) = self.file.lock() {
            f.last_summary_day = Some(today);
            f.last_summary_at = sent_at;
            self.save(&f);
        }
    }
}

/// The account's windows as the usage probe reads them now.
async fn account_windows(state: &AppState, account: &AccountId) -> Vec<UsageWindow> {
    let Ok(accounts) = tandem_store::repo::accounts::list(&state.store).await else {
        return Vec::new();
    };
    let Some(a) = accounts.iter().find(|a| a.id == *account) else {
        return Vec::new();
    };
    crate::services::usage::probe(state, a).await.windows
}

/// Every account's next reset, for UI-171.
pub async fn accounts(state: &AppState) -> Vec<NightAccount> {
    let now = state.clock.now_ms();
    let Ok(list) = tandem_store::repo::accounts::list(&state.store).await else {
        return Vec::new();
    };
    let limits = state.terminals.status.limits.list();
    list.into_iter()
        .map(|a| {
            let windows = state
                .usage
                .get(a.id.as_str())
                .map(|v| v.windows)
                .unwrap_or_default();
            let resets: Vec<i64> = limits
                .iter()
                .filter(|l| l.account_id.as_ref() == Some(&a.id))
                .filter_map(|l| l.resets_at)
                .collect();
            let next = next_reset(&windows, &resets, now);
            NightAccount {
                account_id: a.id.clone(),
                engine_id: a.engine_id.clone(),
                resets_at: next.as_ref().map(|(r, _)| *r),
                window: next.map(|(_, w)| w),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;
    const T0: i64 = 1_790_000_000_000;

    fn win(kind: &str, used: f64, resets_at: Option<i64>) -> UsageWindow {
        UsageWindow {
            kind: kind.into(),
            used_pct: used,
            resets_at,
        }
    }

    fn run(state: NightState, run_at: i64) -> NightRun {
        NightRun {
            id: "nr_1".into(),
            issue_id: "TAN-1".into(),
            title: "Write tests for billing".into(),
            workspace_id: WorkspaceId::from_db("ws_1".into()),
            account_id: AccountId::from_db("acc_1".into()),
            engine_id: "codex".into(),
            isolation: Isolation::Lane,
            access: Access::Edits,
            full_confirmed: false,
            review: true,
            run_at,
            window: "5h".into(),
            state,
            attempt_id: None,
            reason: None,
            created_at: T0,
            ended_at: None,
        }
    }

    #[test]
    fn f_rt_06_1_the_next_reset_is_the_fullest_windows_else_a_limits() {
        let w = [
            win("5h", 90.0, Some(T0 + 60 * MIN)),
            win("week", 40.0, Some(T0 + 5_000 * MIN)),
        ];
        assert_eq!(next_reset(&w, &[], T0), Some((T0 + 60 * MIN, "5h".into())));
        // No windows: the earliest future limit reset.
        assert_eq!(
            next_reset(&[], &[T0 - MIN, T0 + 30 * MIN, T0 + 90 * MIN], T0),
            Some((T0 + 30 * MIN, "limit".into()))
        );
        // Nothing known.
        assert_eq!(next_reset(&[win("5h", 10.0, None)], &[], T0), None);
    }

    #[test]
    fn f_rt_06_2_a_due_run_starts_and_nothing_starts_before_its_time() {
        let r = run(NightState::Scheduled, T0);
        assert_eq!(due(&r, &[], T0 - MIN), Due::NotYet);
        assert_eq!(
            due(&r, &[win("5h", 3.0, Some(T0 + 300 * MIN))], T0),
            Due::Start
        );
        // A cancelled or started run never starts again.
        assert_eq!(due(&run(NightState::Cancelled, T0), &[], T0), Due::NotYet);
        assert_eq!(due(&run(NightState::Started, T0), &[], T0), Due::NotYet);
    }

    #[test]
    fn f_rt_06_3_a_still_full_account_waits_for_its_later_reset() {
        let r = run(NightState::Scheduled, T0);
        let later = T0 + 120 * MIN;
        assert_eq!(
            due(&r, &[win("5h", 97.0, Some(later))], T0),
            Due::Wait(later)
        );
        // A waiting run starts at the new reset.
        let mut w = run(NightState::Waiting, later);
        assert_eq!(
            due(&w, &[win("5h", 2.0, Some(later + 300 * MIN))], later),
            Due::Start
        );
        // Full but resetting within the minute: start.
        w.run_at = T0;
        assert_eq!(
            due(&w, &[win("5h", 99.0, Some(T0 + 30_000))], T0),
            Due::Start
        );
    }

    #[test]
    fn f_rt_06_6_the_summary_counts_runs_that_ended_since_the_last_one() {
        let mut done = run(NightState::Done, T0);
        done.ended_at = Some(T0 + 10 * MIN);
        let mut failed = run(NightState::NeedsYou, T0);
        failed.title = "Upgrade lint".into();
        failed.ended_at = Some(T0 + 20 * MIN);
        let pending = run(NightState::Scheduled, T0 + 600 * MIN);
        let runs = [done, failed, pending];
        let (d, n, titles) = summary(&runs, T0).expect("a summary");
        assert_eq!((d, n), (1, 1));
        assert_eq!(titles, vec!["Write tests for billing", "Upgrade lint"]);
        assert_eq!(summary_title(d, n), "Night Shift: 1 done, 1 needs you");
        assert_eq!(summary_title(2, 0), "Night Shift: 2 done");
        assert_eq!(summary_title(0, 3), "Night Shift: 0 done, 3 need you");
        assert_eq!(summary(&runs, T0 + 30 * MIN), None, "nothing new");
    }

    #[test]
    fn f_rt_06_6_the_summary_is_due_once_a_day_after_its_time() {
        let tz = FixedOffset::east_opt(3_600).expect("tz");
        let at = |s: &str| {
            DateTime::parse_from_rfc3339(s)
                .expect("time")
                .with_timezone(&tz)
        };
        assert!(!summary_due(at("2026-10-16T07:59:00+01:00"), "08:00", None));
        assert!(summary_due(at("2026-10-16T08:00:00+01:00"), "08:00", None));
        assert!(!summary_due(
            at("2026-10-16T09:00:00+01:00"),
            "08:00",
            Some("2026-10-16")
        ));
        assert!(summary_due(
            at("2026-10-17T08:30:00+01:00"),
            "08:00",
            Some("2026-10-16")
        ));
        // A bad time falls back to 8:00.
        assert!(summary_due(
            at("2026-10-16T08:01:00+01:00"),
            "nonsense",
            None
        ));
    }

    #[test]
    fn f_rt_06_4_cancel_before_the_reset_removes_the_run() {
        let dir = tempfile::tempdir().expect("tmp");
        let svc = NightShiftService::new(dir.path().join("night-shift.json"), EventBus::new());
        svc.add(run(NightState::Scheduled, T0));
        assert_eq!(svc.list().len(), 1);
        svc.cancel("nr_1").expect("cancel");
        assert!(svc.list().is_empty());
        assert!(svc.cancel("nr_1").is_err(), "only once");
        // Saved: a new service reads the same file.
        let again = NightShiftService::new(dir.path().join("night-shift.json"), EventBus::new());
        assert!(again.list().is_empty());
        again.add(run(NightState::Scheduled, T0));
        let third = NightShiftService::new(dir.path().join("night-shift.json"), EventBus::new());
        assert_eq!(third.list().len(), 1);
        assert!(third.has_pending());
    }
}
