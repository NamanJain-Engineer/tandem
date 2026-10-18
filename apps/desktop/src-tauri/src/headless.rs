//! `tandem --run-routines` (F-RT-03, ARCHITECTURE §10.4): what the OS
//! launcher runs every 5 minutes. No window: it takes the app lock, fires the
//! due background routines (and a missed run within 24 h, once), waits for
//! them and exits 0. The GUI holds the same lock while it runs, so a pass
//! started then exits at once and the GUI's own scheduler keeps the job.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// `$DATA/run/app.lock`.
pub fn lock_path(run: &Path) -> PathBuf {
    run.join("app.lock")
}

/// Who holds the lock (`app.lock.json`, diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    /// `gui` or `headless`.
    pub mode: String,
}

/// The app lock, held until dropped (or the process exits).
#[derive(Debug)]
pub struct AppLock {
    file: File,
    info: PathBuf,
}

impl Drop for AppLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.info);
        let _ = self.file.unlock();
    }
}

/// Takes the app lock without waiting; `None` when another process (the
/// GUI, or another headless pass) has it.
pub fn try_lock(run: &Path, mode: &str) -> std::io::Result<Option<AppLock>> {
    std::fs::create_dir_all(run)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path(run))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(e)) => return Err(e),
    }
    let info = run.join("app.lock.json");
    let body = serde_json::to_vec(&LockInfo {
        pid: std::process::id(),
        mode: mode.to_string(),
    })
    .unwrap_or_default();
    let _ = tandem_core::atomic::atomic_write(&info, &body, Some(0o600));
    Ok(Some(AppLock { file, info }))
}

/// What a pass did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pass {
    /// The lock was held: nothing ran.
    Busy,
    /// The plan doesn't include background routines: nothing ran.
    NotOnPlan,
    /// These runs started (and finished, or hit the cap).
    Ran(usize),
}

/// One headless pass over `routines`: takes the lock in `run`, then
/// [`run_locked`].
pub async fn pass(
    run: &Path,
    background_allowed: bool,
    routines: &Arc<crate::services::routines::RoutinesService>,
    cap: Duration,
) -> std::io::Result<Pass> {
    let Some(_lock) = try_lock(run, "headless")? else {
        tracing::info!(
            code = "routines.headless_busy",
            "the app is running; its scheduler runs routines"
        );
        return Ok(Pass::Busy);
    };
    Ok(run_locked(background_allowed, routines, cap).await)
}

/// The pass itself, with the lock already held.
pub async fn run_locked(
    background_allowed: bool,
    routines: &Arc<crate::services::routines::RoutinesService>,
    cap: Duration,
) -> Pass {
    if !background_allowed {
        tracing::info!(
            code = "routines.headless_plan",
            "background routines aren't on this plan"
        );
        return Pass::NotOnPlan;
    }
    routines.set_runner(tandem_store::repo::routines::Runner::Headless);
    routines.settle().await;
    let started = routines.tick().await;
    routines.wait_idle(cap).await;
    Pass::Ran(started.len())
}

/// Starts due Night Shift runs and keeps this process (and the app lock)
/// while one runs, at most `cap`; then it stops them and reports them
/// (F-RT-06). With the lock held no GUI is running, so attempts still marked
/// active were left by a process that died: they're settled first, and the
/// runs they belong to end as needing the user.
pub async fn night_pass(state: &crate::state::AppState, cap: Duration) {
    use crate::services::night_shift::keep_waiting;
    state.task_exec.settle().await;
    let until = state.clock.now_ms() + i64::try_from(cap.as_millis()).unwrap_or(i64::MAX);
    loop {
        state.night_shift.tick(state).await;
        let now = state.clock.now_ms();
        let runs = state.night_shift.list();
        if !keep_waiting(&runs, now, until) {
            if now >= until {
                state.night_shift.stop_started(state).await;
            }
            break;
        }
        state.clock.sleep(Duration::from_secs(5)).await;
    }
}

/// OS notifications without a window.
struct DesktopNotifier;

impl crate::services::notifier::Notifier for DesktopNotifier {
    fn notify(&self, n: &crate::services::notifier::OsNotification) {
        if let Err(e) = notify_rust::Notification::new()
            .summary(&n.title)
            .body(&n.body)
            .appname("Vibezeo")
            .show()
        {
            tracing::warn!(code = "notify.os_failed", error = %e, "couldn't show a system notification");
        }
    }

    fn attention(&self) {}
}

/// `tandem --run-routines`: exits 0 whatever the routines did (their
/// results are on their rows); 1 only when the app's data can't be opened.
pub fn main() -> i32 {
    let data = match tandem_core::DataDir::resolve() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Vibezeo couldn't find its data: {e}");
            return 1;
        }
    };
    // The lock first: with the GUI running, nothing else is touched.
    let lock = match try_lock(&data.run(), "headless") {
        Ok(Some(lock)) => lock,
        Ok(None) => return 0,
        Err(e) => {
            eprintln!("Vibezeo couldn't take its lock: {e}");
            return 1;
        }
    };
    let state = match crate::state::AppState::init() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Vibezeo couldn't start: {e}");
            return 1;
        }
    };
    let state = Arc::new(state);
    tracing::info!(code = "routines.headless_start", "background routine pass");
    let pass = tauri::async_runtime::block_on(async move {
        if state.e2e {
            state.notifications.set_notifier(Arc::new(
                crate::services::notifier::RecordingNotifier::default(),
            ));
        } else {
            state.notifications.set_notifier(Arc::new(DesktopNotifier));
        }
        // This process holds the lock: what the last run left working is
        // stale (D-2540).
        state.restore.settle_threads().await;
        // Which engines are here, as the GUI would know them.
        state.terminals.engines.load_cached().await;
        let login = state.terminals.login_env().await;
        state.terminals.engines.detect(&login).await;
        let allowed = state.entitlements.limits().background_routines;
        let routines = state.routines.clone();
        let pass = run_locked(allowed, &routines, crate::services::routines::HEADLESS_CAP).await;
        // The Night Shift's due runs start here too, and the pass waits for
        // them within the same cap (F-RT-06).
        if allowed {
            night_pass(&state, crate::services::night_shift::NIGHT_CAP).await;
        }
        pass
    });
    tracing::info!(code = "routines.headless_done", pass = ?pass, "background routine pass finished");
    drop(lock);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::night_shift::{NightRun, NightState};
    use crate::services::tasks_exec::tests::FakeHost;
    use crate::services::tasks_exec::Isolation;
    use crate::state::AppState;
    use std::sync::atomic::Ordering;
    use tandem_core::access::Access;
    use tandem_core::clock::{Clock, MockClock};
    use tandem_core::ids::{AccountId, AttemptId, WorkspaceId};
    use tandem_core::DataDir;
    use tandem_store::repo::task_attempts::{self, AttemptState, NewAttempt};
    use tandem_store::Store;
    use tandem_tasks::{SaveIssue, TaskStore, Writer};

    struct World {
        _dir: tempfile::TempDir,
        state: Arc<AppState>,
        clock: MockClock,
        host: Arc<FakeHost>,
        issue: String,
    }

    /// A headless pass's state: a mock clock, and a fake engine host whose
    /// agents keep working until released.
    fn world() -> World {
        let dir = tempfile::tempdir().expect("dir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let tracker = TaskStore::open(data.tasks()).expect("tracker");
        let issue = tracker
            .save_issue(
                SaveIssue {
                    title: Some("Write tests for billing".into()),
                    ..Default::default()
                },
                &Writer::human(),
            )
            .expect("issue")
            .id;
        let host = Arc::new(FakeHost {
            pro: true,
            ..Default::default()
        });
        let _ = host.tracker.set(tracker);
        let clock = MockClock::new(chrono::Utc::now());
        let state = Arc::new(AppState::for_tests_with(
            data,
            store,
            clock.clone(),
            host.clone(),
        ));
        World {
            _dir: dir,
            state,
            clock,
            host,
            issue,
        }
    }

    fn night_run(w: &World, state: NightState, attempt: Option<AttemptId>) -> NightRun {
        let now = w.clock.now_ms();
        NightRun {
            id: "nr_1".into(),
            issue_id: w.issue.clone(),
            title: "Write tests for billing".into(),
            workspace_id: WorkspaceId::from_db("ws_1".into()),
            account_id: AccountId::from_db("acc_1".into()),
            engine_id: "claude".into(),
            isolation: Isolation::Lane,
            access: Access::Edits,
            full_confirmed: false,
            review: false,
            run_at: now,
            window: "5h".into(),
            state,
            attempt_id: attempt,
            reason: None,
            created_at: now,
            ended_at: None,
        }
    }

    /// Runs the pass, moving the mock clock 5 s at a time until it ends.
    async fn run_pass(w: &World, cap: Duration) -> Duration {
        let pass = {
            let state = w.state.clone();
            tokio::spawn(async move { night_pass(&state, cap).await })
        };
        let mut moved = Duration::ZERO;
        for _ in 0..200 {
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }
            if pass.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
            w.clock.advance(Duration::from_secs(5));
            moved += Duration::from_secs(5);
        }
        pass.await.expect("pass");
        moved
    }

    #[tokio::test]
    async fn f_rt_06_7_the_pass_starts_a_due_run_keeps_it_until_the_cap_then_stops_it() {
        let w = world();
        w.state
            .night_shift
            .add(night_run(&w, NightState::Scheduled, None));
        let cap = Duration::from_secs(60);
        let moved = run_pass(&w, cap).await;
        assert!(
            moved >= cap,
            "the pass waited for the working run until its cap"
        );
        // The fake agent started, then was stopped at the cap.
        assert_eq!(w.host.spawns.load(Ordering::SeqCst), 1);
        assert_eq!(w.host.stopped.lock().expect("lock").len(), 1);
        let runs = w.state.night_shift.list();
        assert_eq!(runs[0].state, NightState::NeedsYou);
        assert!(runs[0]
            .reason
            .as_deref()
            .is_some_and(|r| r.starts_with("Stopped after 8 hours")));
        let attempt = runs[0].attempt_id.clone().expect("attempt");
        let a = task_attempts::get(&w.state.store, &attempt)
            .await
            .expect("attempt row");
        assert!(!a.state.active(), "its attempt ended: {:?}", a.state);
    }

    #[tokio::test]
    async fn f_rt_06_8_a_run_whose_process_died_ends_as_needs_you_without_waiting() {
        let w = world();
        // The last pass started the run, then its process died: the attempt
        // is still marked active.
        let attempt = task_attempts::insert(
            &w.state.store,
            NewAttempt {
                id: AttemptId::new(),
                issue_id: w.issue.clone(),
                workspace_id: WorkspaceId::from_db("ws_1".into()),
                teammate_id: None,
                engine_id: "claude".into(),
                account_id: None,
                access: Access::Edits,
                review: false,
            },
            w.clock.now_ms(),
        )
        .await
        .expect("attempt");
        let attempt = task_attempts::update(
            &w.state.store,
            &attempt.id,
            task_attempts::AttemptPatch {
                state: Some(AttemptState::Running),
                started_at: Some(w.clock.now_ms()),
                ..Default::default()
            },
        )
        .await
        .expect("running");
        assert!(attempt.state.active());
        w.state
            .night_shift
            .add(night_run(&w, NightState::Started, Some(attempt.id.clone())));
        let moved = run_pass(&w, Duration::from_secs(8 * 3600)).await;
        assert!(
            moved < Duration::from_secs(60),
            "no wait for the cap: {moved:?}"
        );
        let runs = w.state.night_shift.list();
        assert_eq!(runs[0].state, NightState::NeedsYou);
        assert_eq!(
            runs[0].reason.as_deref(),
            Some("Vibezeo closed during the run.")
        );
        assert_eq!(
            w.host.spawns.load(Ordering::SeqCst),
            0,
            "nothing started again"
        );
        let a = task_attempts::get(&w.state.store, &attempt.id)
            .await
            .expect("row");
        assert_eq!(a.state, AttemptState::Stopped);
    }
}
