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
        // Which engines are here, as the GUI would know them.
        state.terminals.engines.load_cached().await;
        let login = state.terminals.login_env().await;
        state.terminals.engines.detect(&login).await;
        let allowed = state.entitlements.limits().background_routines;
        let routines = state.routines.clone();
        run_locked(allowed, &routines, crate::services::routines::HEADLESS_CAP).await
    });
    tracing::info!(code = "routines.headless_done", pass = ?pass, "background routine pass finished");
    drop(lock);
    0
}
