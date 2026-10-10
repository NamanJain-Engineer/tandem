//! Watchdogs for the app's long-lived subsystems (PRODUCT_SPEC §5.2): the
//! status bus, the status service, the scheduler's periodic jobs, the gateway
//! and each terminal's reader. A failed subsystem (an error, a panic, or a
//! heartbeat that stopped) restarts at most 3 times per 10 minutes; after that
//! `app:subsystem-failed` is published and the UI says so. The [`Watchdog`]
//! also keeps each subsystem's health for Help › Diagnostics (§5.7).

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use ts_rs::TS;

use crate::bus::{BusEvent, EventBus, Topic};
use crate::clock::Clock;

pub const MAX_RESTARTS: usize = 3;
pub const WINDOW: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SubsystemFailed {
    pub name: String,
    pub restarts: usize,
    pub last_error: String,
}

/// Restarts allowed within a sliding window.
#[derive(Debug, Default, Clone)]
pub struct RestartBudget {
    at: Vec<i64>,
}

impl RestartBudget {
    /// Records a failure at `now_ms`. True when a restart is allowed; false
    /// when [`MAX_RESTARTS`] already happened within [`WINDOW`].
    pub fn try_restart(&mut self, now_ms: i64) -> bool {
        self.at.retain(|t| now_ms - *t < WINDOW.as_millis() as i64);
        if self.at.len() >= MAX_RESTARTS {
            return false;
        }
        self.at.push(now_ms);
        true
    }

    pub fn recent(&self) -> usize {
        self.at.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HealthState {
    Running,
    Restarting,
    Failed,
    Stopped,
}

/// One subsystem's health, as Help › Diagnostics shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SubsystemHealth {
    pub name: String,
    pub state: HealthState,
    /// Milliseconds since the last heartbeat; null for subsystems that don't beat.
    #[ts(type = "number | null")]
    pub since_beat_ms: Option<u64>,
    /// Restarts within the last 10 minutes.
    pub restarts: usize,
    pub last_error: Option<String>,
}

#[derive(Debug)]
struct Entry {
    state: HealthState,
    beat: Option<tokio::time::Instant>,
    restarts: usize,
    last_error: Option<String>,
}

/// Heartbeats and health of every supervised subsystem.
#[derive(Debug, Default)]
pub struct Watchdog {
    entries: Mutex<HashMap<String, Entry>>,
}

impl Watchdog {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn with<R>(&self, name: &str, f: impl FnOnce(&mut Entry) -> R) -> R {
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let entry = map.entry(name.to_string()).or_insert(Entry {
            state: HealthState::Running,
            beat: None,
            restarts: 0,
            last_error: None,
        });
        f(entry)
    }

    /// The subsystem is alive. Call it at least every `stale_after` / 2.
    pub fn beat(&self, name: &str) {
        self.with(name, |e| e.beat = Some(tokio::time::Instant::now()));
    }

    fn since_beat(&self, name: &str) -> Option<Duration> {
        self.with(name, |e| e.beat.map(|b| b.elapsed()))
    }

    fn set(&self, name: &str, state: HealthState, restarts: usize, error: Option<String>) {
        self.with(name, |e| {
            e.state = state;
            e.restarts = restarts;
            if error.is_some() {
                e.last_error = error;
            }
            if state == HealthState::Running {
                e.beat = Some(tokio::time::Instant::now());
            }
        });
    }

    /// Records a failure of a subsystem that restarts itself (a terminal's
    /// reader); `failed` when it gave up.
    pub fn record(&self, name: &str, restarts: usize, error: &str, failed: bool) {
        let state = if failed {
            HealthState::Failed
        } else {
            HealthState::Running
        };
        self.set(name, state, restarts, Some(error.to_string()));
    }

    pub fn forget(&self, name: &str) {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(name);
    }

    pub fn snapshot(&self) -> Vec<SubsystemHealth> {
        let map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let mut out: Vec<SubsystemHealth> = map
            .iter()
            .map(|(name, e)| SubsystemHealth {
                name: name.clone(),
                state: e.state,
                since_beat_ms: e.beat.map(|b| b.elapsed().as_millis() as u64),
                restarts: e.restarts,
                last_error: e.last_error.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

static GLOBAL: std::sync::OnceLock<Arc<Watchdog>> = std::sync::OnceLock::new();

/// The process's watchdog: every supervised subsystem reports here, and
/// Help › Diagnostics reads it.
pub fn watchdog() -> Arc<Watchdog> {
    GLOBAL.get_or_init(Watchdog::new).clone()
}

/// Publishes the give-up event (`app:subsystem-failed`).
pub fn report_failed(bus: &EventBus, name: &str, restarts: usize, last_error: String) {
    tracing::error!(subsystem = name, "subsystem failed after restarts");
    bus.publish(BusEvent::new(
        Topic::App,
        "app:subsystem-failed",
        SubsystemFailed {
            name: name.to_string(),
            restarts,
            last_error,
        },
    ));
}

/// Runs `make()` until it returns `Ok(())`. Each `Err` restarts it unless the
/// restart budget for the window is spent, in which case `app:subsystem-failed`
/// is published and supervision stops.
pub async fn supervise<F, Fut>(name: &str, clock: Arc<dyn Clock>, bus: EventBus, mut make: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let mut budget = RestartBudget::default();
    loop {
        match make().await {
            Ok(()) => return,
            Err(err) => {
                if !budget.try_restart(clock.now_ms()) {
                    report_failed(&bus, name, budget.recent(), err);
                    return;
                }
                tracing::warn!(
                    subsystem = name,
                    attempt = budget.recent(),
                    "restarting subsystem"
                );
            }
        }
    }
}

/// Where a supervised task runs and how it's watched.
#[derive(Clone)]
pub struct Supervision {
    pub clock: Arc<dyn Clock>,
    pub bus: EventBus,
    pub watchdog: Arc<Watchdog>,
}

/// Like [`supervise`], but each run is its own task, so a panic is a failure
/// too, and with `stale_after` set a run whose heartbeat
/// ([`Watchdog::beat`]) is older than that is aborted and counts as a failure
/// ("stopped responding"). Returns when a run ends with `Ok(())` or the
/// budget is spent.
pub async fn supervise_task<F, Fut>(
    name: &str,
    sup: Supervision,
    stale_after: Option<Duration>,
    mut make: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), String>> + Send + 'static,
{
    let mut budget = RestartBudget::default();
    loop {
        sup.watchdog
            .set(name, HealthState::Running, budget.recent(), None);
        let mut run = tokio::spawn(make());
        let outcome: Result<(), String> = match stale_after {
            None => joined(&mut run).await,
            Some(stale) => {
                let check = (stale / 4).max(Duration::from_millis(10));
                loop {
                    tokio::select! {
                        r = &mut run => break joined_result(r),
                        _ = tokio::time::sleep(check) => {
                            if sup.watchdog.since_beat(name).is_some_and(|d| d > stale) {
                                run.abort();
                                break Err(format!("stopped responding for over {} s", stale.as_secs()));
                            }
                        }
                    }
                }
            }
        };
        match outcome {
            Ok(()) => {
                sup.watchdog
                    .set(name, HealthState::Stopped, budget.recent(), None);
                return;
            }
            Err(err) => {
                if !budget.try_restart(sup.clock.now_ms()) {
                    sup.watchdog.set(
                        name,
                        HealthState::Failed,
                        budget.recent(),
                        Some(err.clone()),
                    );
                    report_failed(&sup.bus, name, budget.recent(), err);
                    return;
                }
                tracing::warn!(subsystem = name, attempt = budget.recent(), error = %err, "restarting subsystem");
                sup.watchdog
                    .set(name, HealthState::Restarting, budget.recent(), Some(err));
            }
        }
    }
}

async fn joined(run: &mut tokio::task::JoinHandle<Result<(), String>>) -> Result<(), String> {
    joined_result(run.await)
}

fn joined_result(r: Result<Result<(), String>, tokio::task::JoinError>) -> Result<(), String> {
    match r {
        Ok(inner) => inner,
        Err(e) if e.is_panic() => Err("panicked".to_string()),
        Err(_) => Err("was cancelled".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::MockClock;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn sup() -> (Supervision, Arc<MockClock>) {
        let clock = Arc::new(MockClock::new(chrono::Utc::now()));
        (
            Supervision {
                clock: clock.clone(),
                bus: EventBus::new(),
                watchdog: Watchdog::new(),
            },
            clock,
        )
    }

    #[tokio::test]
    async fn gives_up_after_three_restarts_and_reports() {
        let clock = Arc::new(MockClock::new(chrono::Utc::now()));
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::App);
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = runs.clone();
        supervise("gateway", clock, bus, move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Err("boom".to_string())
            }
        })
        .await;
        assert_eq!(runs.load(Ordering::SeqCst), 4);
        let event = rx.recv().await.expect("event");
        assert_eq!(event.name, "app:subsystem-failed");
    }

    #[test]
    fn nfr_5_2_the_budget_is_three_restarts_per_ten_minutes() {
        let mut b = RestartBudget::default();
        assert!(b.try_restart(0));
        assert!(b.try_restart(1_000));
        assert!(b.try_restart(2_000));
        assert!(!b.try_restart(3_000), "a 4th within 10 min");
        // Ten minutes after the first, one slot is free again.
        assert!(b.try_restart(600_001));
        assert!(!b.try_restart(600_002));
    }

    #[tokio::test]
    async fn nfr_5_2_a_panicking_subsystem_restarts_then_is_reported_failed() {
        let (s, _) = sup();
        let mut rx = s.bus.subscribe(Topic::App);
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = runs.clone();
        supervise_task("status bus", s.clone(), None, move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                if counter.load(Ordering::SeqCst) > 0 {
                    panic!("bad event");
                }
                Ok(())
            }
        })
        .await;
        assert_eq!(
            runs.load(Ordering::SeqCst),
            4,
            "the first run and 3 restarts"
        );
        let event = rx.recv().await.expect("event");
        assert_eq!(event.name, "app:subsystem-failed");
        assert_eq!(event.payload["name"], "status bus");
        assert_eq!(event.payload["lastError"], "panicked");
        let health = s.watchdog.snapshot();
        assert_eq!(health[0].state, HealthState::Failed);
        assert_eq!(health[0].restarts, 3);
    }

    #[tokio::test(start_paused = true)]
    async fn nfr_5_2_a_subsystem_whose_heartbeat_stops_is_restarted() {
        let (s, _) = sup();
        let runs = Arc::new(AtomicUsize::new(0));
        let (counter, dog) = (runs.clone(), s.watchdog.clone());
        let task = tokio::spawn(supervise_task(
            "scheduler",
            s.clone(),
            Some(Duration::from_secs(5)),
            move || {
                let run = counter.fetch_add(1, Ordering::SeqCst);
                let dog = dog.clone();
                async move {
                    if run == 0 {
                        // Hangs without beating.
                        std::future::pending::<()>().await;
                    }
                    for _ in 0..3 {
                        dog.beat("scheduler");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    Ok(())
                }
            },
        ));
        task.await.expect("supervisor");
        assert_eq!(
            runs.load(Ordering::SeqCst),
            2,
            "the hung run was replaced once"
        );
        let health = s.watchdog.snapshot();
        assert_eq!(health[0].state, HealthState::Stopped);
        assert_eq!(health[0].restarts, 1);
        assert_eq!(
            health[0].last_error.as_deref(),
            Some("stopped responding for over 5 s")
        );
    }

    #[tokio::test]
    async fn nfr_5_2_restarts_spread_over_more_than_ten_minutes_never_give_up() {
        let (s, clock) = sup();
        let runs = Arc::new(AtomicUsize::new(0));
        let (counter, c) = (runs.clone(), clock.clone());
        supervise_task("gateway", s.clone(), None, move || {
            let run = counter.fetch_add(1, Ordering::SeqCst);
            let c = c.clone();
            async move {
                c.advance(Duration::from_secs(301));
                if run < 8 {
                    Err("listener closed".to_string())
                } else {
                    Ok(())
                }
            }
        })
        .await;
        assert_eq!(runs.load(Ordering::SeqCst), 9);
        assert_eq!(s.watchdog.snapshot()[0].state, HealthState::Stopped);
    }
}
