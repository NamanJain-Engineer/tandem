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
use tandem_core::remote::push::PushAlert;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::usage::UsageWindow;
use tandem_store::repo::task_attempts::AttemptState;
use ts_rs::TS;

use crate::services::tasks_exec::{ExecuteInput, Isolation};
use crate::state::AppState;

/// A window at least this full when the run is due hasn't reset yet.
pub const STILL_FULL: f64 = 95.0;
/// How long the background runner keeps a Night Shift run going with the
/// app closed; then it stops the run and reports it.
pub const NIGHT_CAP: std::time::Duration = std::time::Duration::from_secs(8 * 60 * 60);

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

/// Where a started run goes when its attempt is in `state` (with its
/// error): done, needs you (with the reason), or still going.
pub fn follow(state: AttemptState, error: Option<&str>) -> Option<(NightState, Option<String>)> {
    match state {
        AttemptState::Done | AttemptState::InReview => Some((NightState::Done, None)),
        AttemptState::Failed | AttemptState::Stopped => Some((
            NightState::NeedsYou,
            Some(
                error
                    .unwrap_or("The run stopped before it finished.")
                    .to_string(),
            ),
        )),
        _ => None,
    }
}

/// Whether the background runner keeps waiting: a run is still started
/// and the cap hasn't passed.
pub fn keep_waiting(runs: &[NightRun], now: i64, until: i64) -> bool {
    now < until && runs.iter().any(|r| r.state == NightState::Started)
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

/// A summary's body as the phone gets it. The runs' titles name issues a
/// tracker holds and the phone is off this machine, so they're redacted as
/// written and past what's removable, before the cut (D-3173). The
/// notification here keeps them as written (D-3036).
fn phone_body(body: &str) -> String {
    tandem_core::untrusted::redacted(body)
        .chars()
        .take(160)
        .collect()
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
    /// The summary's phone alert, until a GUI's push loop takes it: a
    /// headless pass has no push loop (D-2542).
    #[serde(default)]
    pending_alerts: Vec<PushAlert>,
}

/// A summary alert older than this isn't sent any more.
const ALERT_TTL_MS: i64 = 24 * 60 * 60 * 1000;

pub struct NightShiftService {
    path: PathBuf,
    bus: EventBus,
    file: Mutex<File>,
}

impl NightShiftService {
    pub fn new(path: PathBuf, bus: EventBus) -> Self {
        let file = read_file(&path).unwrap_or_default();
        Self {
            path,
            bus,
            file: Mutex::new(file),
        }
    }

    /// Reads the file again: the GUI and a headless pass both write it, one
    /// at a time under the app lock, so the copy in memory can be stale
    /// (D-2541). Keeps the copy when the file can't be read.
    fn reload_into(&self, file: &mut File) {
        if let Some(on_disk) = read_file(&self.path) {
            *file = on_disk;
        }
    }

    /// Reads the file again (the app lock just became this app's, or a
    /// tick starts).
    pub fn reload(&self) {
        if let Ok(mut file) = self.file.lock() {
            self.reload_into(&mut file);
        }
    }

    /// The runs due at `now`, from the file as it is now: a run another
    /// process started while this one waited for the lock isn't due again.
    pub fn due_runs(&self, now: i64) -> Vec<NightRun> {
        self.reload();
        self.list()
            .into_iter()
            .filter(|r| {
                matches!(r.state, NightState::Scheduled | NightState::Waiting) && r.run_at <= now
            })
            .collect()
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

    /// Read, change, write: never over what another process wrote since.
    fn update(&self, f: impl FnOnce(&mut File)) {
        if let Ok(mut file) = self.file.lock() {
            self.reload_into(&mut file);
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

    /// The background runner's cap passed: each started run's attempt is
    /// stopped, and the run needs the user, with the reason.
    pub async fn stop_started(&self, state: &AppState) {
        let started: Vec<NightRun> = self
            .list()
            .into_iter()
            .filter(|r| r.state == NightState::Started)
            .collect();
        for run in started {
            if let Some(id) = &run.attempt_id {
                if let Err(e) = state.task_exec.stop(id).await {
                    tracing::warn!(code = "night.stop_failed", error = %e, "couldn't stop a Night Shift run");
                }
            }
            let at = state.clock.now_ms();
            self.update(|f| {
                if let Some(r) = f.runs.iter_mut().find(|r| r.id == run.id) {
                    r.state = NightState::NeedsYou;
                    r.reason = Some(
                        "Stopped after 8 hours: the most a run gets while Vibezeo is closed."
                            .into(),
                    );
                    r.ended_at = Some(at);
                }
            });
        }
    }

    /// The summary alerts waiting for a push loop, taken out of the file
    /// (those older than a day are dropped). Only a GUI calls this.
    pub fn take_alerts(&self, now: i64) -> Vec<PushAlert> {
        let Ok(mut file) = self.file.lock() else {
            return Vec::new();
        };
        self.reload_into(&mut file);
        if file.pending_alerts.is_empty() {
            return Vec::new();
        }
        let alerts = std::mem::take(&mut file.pending_alerts);
        self.save(&file);
        alerts
            .into_iter()
            .filter(|a| now - a.at < ALERT_TTL_MS)
            .collect()
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
        // Due runs: read the account's usage again first.
        for run in self.due_runs(now).iter() {
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
                        supervision: None,
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
                let Some((next, reason)) = follow(a.attempt.state, a.attempt.error.as_deref())
                else {
                    continue;
                };
                let at = state.clock.now_ms();
                self.update(|f| {
                    if let Some(r) = f.runs.iter_mut().find(|r| r.id == run.id) {
                        r.state = next;
                        r.reason = reason;
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
        let mut pending = None;
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
            // Kept in the file with the day it marks sent, so a headless
            // pass's summary still reaches the phones when the app opens.
            pending = Some(PushAlert {
                kind: "night_shift".into(),
                id: format!("night:{today}"),
                title,
                body: phone_body(&body),
                at: sent_at,
            });
        }
        if let Ok(mut f) = self.file.lock() {
            self.reload_into(&mut f);
            f.last_summary_day = Some(today);
            f.last_summary_at = sent_at;
            f.pending_alerts.extend(pending);
            self.save(&f);
        }
    }
}

fn read_file(path: &std::path::Path) -> Option<File> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<File>(&b).ok())
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
    fn f_rt_06_8_a_dead_run_ends_as_needs_you_with_its_reason() {
        // The GUI settles attempts a closed process left (`settle_interrupted`):
        // they read Stopped with "Vibezeo closed during the run."
        assert_eq!(
            follow(
                AttemptState::Stopped,
                Some("Vibezeo closed during the run.")
            ),
            Some((
                NightState::NeedsYou,
                Some("Vibezeo closed during the run.".to_string())
            ))
        );
        assert_eq!(
            follow(AttemptState::Failed, None).map(|(s, _)| s),
            Some(NightState::NeedsYou)
        );
        assert_eq!(
            follow(AttemptState::InReview, None),
            Some((NightState::Done, None))
        );
        assert_eq!(follow(AttemptState::Running, None), None);
        // Ended that way, the summary counts it.
        let mut r = run(NightState::NeedsYou, T0);
        r.ended_at = Some(T0 + MIN);
        assert_eq!(summary(&[r], T0).map(|(d, n, _)| (d, n)), Some((0, 1)));
    }

    #[test]
    fn f_rt_06_7_the_background_runner_waits_for_a_started_run_until_its_cap() {
        let cap = i64::try_from(NIGHT_CAP.as_millis()).expect("cap");
        let started = [run(NightState::Started, T0)];
        // Well past routines' 50-minute cap, still waiting.
        assert!(keep_waiting(&started, T0 + 3 * 60 * MIN, T0 + cap));
        assert!(
            !keep_waiting(&started, T0 + cap, T0 + cap),
            "the cap ends it"
        );
        assert!(!keep_waiting(&[run(NightState::Done, T0)], T0, T0 + cap));
        assert_eq!(cap, 8 * 60 * MIN);
    }

    #[test]
    fn f_rt_06_11_a_run_the_headless_pass_started_isnt_due_again_in_the_gui() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("night-shift.json");
        let headless = NightShiftService::new(path.clone(), EventBus::new());
        headless.add(run(NightState::Scheduled, T0));
        // The GUI launches just before the boundary and reads the run as
        // scheduled; it loses the lock race to the pass.
        let gui = NightShiftService::new(path.clone(), EventBus::new());
        assert_eq!(gui.list()[0].state, NightState::Scheduled);
        // The pass starts the run, finishes it and exits.
        headless.update(|f| {
            f.runs[0].state = NightState::Done;
            f.runs[0].attempt_id = Some(AttemptId::from_db("att_1".into()));
            f.runs[0].ended_at = Some(T0 + MIN);
        });
        // The GUI gets the lock: nothing is due, and its next change keeps
        // the pass's record.
        assert!(gui.due_runs(T0 + 2 * MIN).is_empty());
        let mut other = run(NightState::Scheduled, T0 + 600 * MIN);
        other.id = "nr_2".into();
        gui.add(other);
        let on_disk = NightShiftService::new(path, EventBus::new()).list();
        assert_eq!(on_disk.len(), 2);
        assert_eq!(on_disk[0].state, NightState::Done);
        assert_eq!(
            on_disk[0].attempt_id.as_ref().map(|a| a.as_str()),
            Some("att_1")
        );
    }

    #[test]
    fn f_tk_08_1_night_shifts_phone_alert_hides_a_credential_behind_what_is_removable() {
        // The runs' titles name issues a tracker holds, and the phone is off
        // this machine: redacted as written and as read, then cut, so a cut
        // can't leave a token's start that no rule knows (D-3173).
        let z = '\u{200b}';
        let token = format!("ghp_0123456789{z}abcdefghijABCDEFGHIJ012345");
        for body in [
            format!("Rotate abc{z}{token}, Write tests for billing"),
            format!("{} {token}", "x".repeat(140)),
        ] {
            let shown = phone_body(&body);
            assert!(!shown.contains("0123456"), "{shown:?}");
            assert!(shown.chars().count() <= 160, "{shown:?}");
        }
        assert_eq!(
            phone_body("Write tests for billing"),
            "Write tests for billing"
        );
    }

    #[test]
    fn f_rt_06_12_a_headless_summarys_phone_alert_waits_in_the_file_for_the_app() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("night-shift.json");
        let alert = |id: &str, at: i64| PushAlert {
            kind: "night_shift".into(),
            id: id.into(),
            title: "Night Shift: 1 done".into(),
            body: "Write tests for billing".into(),
            at,
        };
        // The headless pass summarizes and exits: no push loop ran.
        let headless = NightShiftService::new(path.clone(), EventBus::new());
        headless.update(|f| {
            f.last_summary_day = Some("2026-10-16".into());
            f.pending_alerts
                .push(alert("night:2026-10-15", T0 - 25 * 60 * MIN));
            f.pending_alerts.push(alert("night:2026-10-16", T0));
        });
        drop(headless);
        // The app opens later that morning: it takes today's alert once
        // (yesterday's is too old), and the day stays marked sent.
        let gui = NightShiftService::new(path.clone(), EventBus::new());
        let got = gui.take_alerts(T0 + 120 * MIN);
        assert_eq!(
            got.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            ["night:2026-10-16"]
        );
        assert!(gui.take_alerts(T0 + 121 * MIN).is_empty(), "sent once");
        let again = NightShiftService::new(path, EventBus::new());
        assert!(again.take_alerts(T0 + 122 * MIN).is_empty());
        assert_eq!(
            again.file.lock().expect("lock").last_summary_day.as_deref(),
            Some("2026-10-16")
        );
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
