//! Session restore (F-SH-07, ARCHITECTURE §1.3 step 7): paused agent cards,
//! the queue that relaunches them with at most two spawning at once, and
//! "Resume agents automatically".
//!
//! An agent counts as spawning from its launch until it's ready (§6.5) or
//! [`READY_WAIT`] passes; only then does the next one start.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use ts_rs::TS;

use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::{PaneId, WorkspaceId};
use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_pty::PtyInfo;
use tandem_store::repo::restore::{self, Settled};
use tandem_store::repo::{panes, settings};
use tandem_store::Store;

use crate::services::terminal::{LaunchMode, TerminalService};

/// Engine processes allowed to be starting at the same time.
pub const MAX_SPAWNING: usize = 2;
/// A starting agent holds its slot until ready, at most this long.
pub const READY_WAIT: Duration = Duration::from_secs(10);
/// Size of a PTY started before any view measured it; the view resizes it.
const DEFAULT_SIZE: (u16, u16) = (80, 24);

/// An agent pane's card (UI-132).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaneRestore {
    pub pane_id: PaneId,
    pub workspace_id: WorkspaceId,
    pub engine_id: Option<String>,
    /// `paused` | `queued` | `cancelled` | `live`
    pub state: String,
    /// Resume is offered; otherwise only Start fresh.
    pub can_resume: bool,
    /// Why Resume isn't offered ("Launch configuration unavailable", …).
    pub reason: Option<String>,
    /// A scrollback snapshot from the last run can be shown.
    pub has_scrollback: bool,
}

/// `restore_get_snapshot`: what the last run left to restore. Workspaces,
/// tabs and layouts come from their own commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RestoreSnapshot {
    pub panes: Vec<PaneRestore>,
    /// Threads settled to Interrupted at this launch.
    pub interrupted_threads: u32,
    pub auto_resume: bool,
}

pub struct RestoreService {
    slots: Arc<Semaphore>,
    /// Panes waiting for a slot; Ctrl+C or closing the pane takes them out.
    queued: Mutex<HashSet<String>>,
    spawning: AtomicUsize,
    /// The most ever spawning at once (instrumented, F-SH-07 AC3).
    max_spawning: AtomicUsize,
    settled: Mutex<Settled>,
    /// Auto-resume runs once per launch, however often the UI asks.
    auto_started: std::sync::atomic::AtomicBool,
    store: Store,
    bus: EventBus,
}

impl RestoreService {
    pub fn new(store: Store, bus: EventBus) -> Arc<Self> {
        Arc::new(Self {
            slots: Arc::new(Semaphore::new(MAX_SPAWNING)),
            queued: Mutex::default(),
            spawning: AtomicUsize::new(0),
            max_spawning: AtomicUsize::new(0),
            settled: Mutex::default(),
            auto_started: std::sync::atomic::AtomicBool::new(false),
            store,
            bus,
        })
    }

    /// At launch, before the UI lists panes: agents pause, interrupted
    /// threads settle.
    pub fn settle_at_launch(&self) {
        match restore::settle_blocking(&self.store) {
            Ok(s) => {
                tracing::info!(
                    code = "restore.settled",
                    paused = s.paused_panes,
                    interrupted = s.interrupted_threads,
                    "last run settled"
                );
                if let Ok(mut settled) = self.settled.lock() {
                    *settled = s;
                }
            }
            Err(err) => {
                tracing::warn!(code = "restore.settle_failed", error = %err, "couldn't settle the last run")
            }
        }
    }

    /// The most agents ever spawning at once; `reset` starts counting again.
    pub fn max_spawning(&self, reset: bool) -> usize {
        if reset {
            self.max_spawning
                .swap(self.spawning.load(Ordering::SeqCst), Ordering::SeqCst)
        } else {
            self.max_spawning.load(Ordering::SeqCst)
        }
    }

    fn is_queued(&self, pane: &str) -> bool {
        self.queued
            .lock()
            .map(|q| q.contains(pane))
            .unwrap_or(false)
    }

    fn publish(&self, pane: &PaneId, state: &str) {
        self.bus.publish(BusEvent::new(
            Topic::App,
            "restore:changed",
            serde_json::json!({ "paneId": pane, "state": state }),
        ));
    }

    pub async fn snapshot(
        &self,
        terminals: &TerminalService,
        scrollback_dir: &std::path::Path,
    ) -> Result<RestoreSnapshot, IpcError> {
        let mut out = Vec::new();
        for w in restore::waiting(&self.store).await? {
            let record = panes::get_open(&self.store, &w.pane_id).await?;
            let plan = terminals.resume_plan(&record).await;
            let state = if self.is_queued(w.pane_id.as_str()) {
                "queued".to_string()
            } else {
                w.state.clone()
            };
            out.push(PaneRestore {
                has_scrollback: scrollback_dir
                    .join(format!("{}.bin", w.pane_id.as_str()))
                    .is_file(),
                pane_id: w.pane_id,
                workspace_id: w.workspace_id,
                engine_id: w.engine_id,
                state,
                can_resume: plan.is_ok(),
                reason: plan.err().map(|e| e.message),
            });
        }
        let auto_resume = settings::get(&self.store, SettingKey::ResumeAgentsAutomatically)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let interrupted = self
            .settled
            .lock()
            .map(|s| s.interrupted_threads)
            .unwrap_or(0);
        Ok(RestoreSnapshot {
            panes: out,
            interrupted_threads: interrupted as u32,
            auto_resume,
        })
    }

    /// Resume or Start fresh from a card: waits for a slot, then starts the
    /// pane with no view attached (the view attaches and replays).
    pub async fn launch(
        self: &Arc<Self>,
        terminals: &Arc<TerminalService>,
        pane: &PaneId,
        size: Option<(u16, u16)>,
        max_live: Option<u32>,
        mode: LaunchMode,
    ) -> Result<PtyInfo, IpcError> {
        let record = panes::get_open(&self.store, pane).await?;
        if record.kind != "agent" {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Only agent panes restore",
            ));
        }
        if !matches!(record.restore_state.as_str(), "paused" | "cancelled") {
            return Err(IpcError::new(
                ErrorCode::Conflict,
                "This agent is already running",
            ));
        }
        if mode == LaunchMode::Resume {
            // Refused before queueing: the card says why.
            terminals.resume_plan(&record).await?;
        }
        if !self.enqueue(pane) {
            return Err(IpcError::new(
                ErrorCode::Conflict,
                "This agent is already starting",
            ));
        }
        let permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| IpcError::new(ErrorCode::Internal, "The restore queue closed"))?;
        self.run(terminals, pane, size, max_live, mode, permit)
            .await
    }

    /// Takes `pane` into the queue; false when it's already there.
    fn enqueue(&self, pane: &PaneId) -> bool {
        let added = self
            .queued
            .lock()
            .map(|mut q| q.insert(pane.as_str().to_string()))
            .unwrap_or(false);
        if added {
            self.publish(pane, "queued");
        }
        added
    }

    async fn run(
        self: &Arc<Self>,
        terminals: &Arc<TerminalService>,
        pane: &PaneId,
        size: Option<(u16, u16)>,
        max_live: Option<u32>,
        mode: LaunchMode,
        permit: OwnedSemaphorePermit,
    ) -> Result<PtyInfo, IpcError> {
        let still_queued = self
            .queued
            .lock()
            .map(|mut q| q.remove(pane.as_str()))
            .unwrap_or(false);
        if !still_queued {
            return Err(IpcError::new(
                ErrorCode::Cancelled,
                "This pane's restore was cancelled",
            ));
        }
        let now = self.spawning.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_spawning.fetch_max(now, Ordering::SeqCst);
        let (cols, rows) = size.unwrap_or(DEFAULT_SIZE);
        let started = terminals
            .spawn_with(pane, cols, rows, max_live, None, mode)
            .await;
        match started {
            Ok(info) => {
                self.publish(pane, "live");
                let (me, t, pane) = (self.clone(), terminals.clone(), pane.clone());
                tauri::async_runtime::spawn(async move {
                    let engine = panes::get(&me.store, &pane)
                        .await
                        .ok()
                        .and_then(|r| r.engine_id)
                        .and_then(|id| t.engines.get(&id));
                    if let Some(engine) = engine {
                        t.wait_ready(&pane, &engine, READY_WAIT).await;
                    }
                    me.spawning.fetch_sub(1, Ordering::SeqCst);
                    drop(permit);
                });
                Ok(info)
            }
            Err(err) => {
                self.spawning.fetch_sub(1, Ordering::SeqCst);
                drop(permit);
                let state = panes::get(&self.store, pane)
                    .await
                    .map(|r| r.restore_state)
                    .unwrap_or_else(|_| "paused".into());
                self.publish(pane, &state);
                Err(err)
            }
        }
    }

    /// Ctrl+C on a card: out of the queue, and its saved output stays
    /// viewable (F-SH-07 AC5). A running agent is left alone.
    pub async fn cancel(&self, pane: &PaneId) -> Result<(), IpcError> {
        let record = panes::get_open(&self.store, pane).await?;
        if let Ok(mut q) = self.queued.lock() {
            q.remove(pane.as_str());
        }
        if !matches!(record.restore_state.as_str(), "paused" | "cancelled") {
            return Ok(());
        }
        restore::set_state(&self.store, pane, "cancelled").await?;
        self.publish(pane, "cancelled");
        Ok(())
    }

    /// The pane closed: its queued launch never runs.
    pub fn forget(&self, pane: &PaneId) {
        if let Ok(mut q) = self.queued.lock() {
            q.remove(pane.as_str());
        }
    }

    /// "Resume agents automatically" (off by default): every paused agent
    /// that can resume does, newest first, two at a time. The UI asks once
    /// the app shell shows (never behind the sign-in screen); later calls in
    /// the same launch do nothing.
    pub async fn auto_resume(
        self: &Arc<Self>,
        terminals: &Arc<TerminalService>,
        max_live: Option<u32>,
    ) {
        if self.auto_started.swap(true, Ordering::SeqCst) {
            return;
        }
        let on = settings::get(&self.store, SettingKey::ResumeAgentsAutomatically)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !on {
            return;
        }
        let Ok(waiting) = restore::waiting(&self.store).await else {
            return;
        };
        let mut order = Vec::new();
        for w in waiting.into_iter().filter(|w| w.state == "paused") {
            let Ok(record) = panes::get_open(&self.store, &w.pane_id).await else {
                continue;
            };
            if terminals.resume_plan(&record).await.is_ok() && self.enqueue(&w.pane_id) {
                order.push(w.pane_id);
            }
        }
        tracing::info!(
            code = "restore.auto",
            count = order.len(),
            "resuming agents"
        );
        for pane in order {
            // Slots are taken in order here, so the newest starts first.
            let Ok(permit) = self.slots.clone().acquire_owned().await else {
                return;
            };
            if !self.is_queued(pane.as_str()) {
                continue;
            }
            let (me, t) = (self.clone(), terminals.clone());
            tauri::async_runtime::spawn(async move {
                if let Err(err) = me
                    .run(&t, &pane, None, max_live, LaunchMode::Resume, permit)
                    .await
                {
                    tracing::warn!(code = "restore.auto_failed", error = ?err.code, "an agent didn't resume");
                }
            });
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use tandem_core::DataDir;
    use tandem_pty::env::LoginEnv;
    use tandem_pty::{wait_until, PtyManager};
    use tandem_store::repo::sessions::{self, LaunchConfig};
    use tandem_store::repo::tabs;

    /// A `claude` that shows its args and home, gets ready after a moment
    /// (the `> ` prompt) and stays up.
    const FAKE: &str = "#!/bin/sh\nprintf 'args: %s\\r\\n' \"$*\"\nprintf 'home: %s\\r\\n' \"$CLAUDE_CONFIG_DIR\"\nsleep 0.6\nprintf '> '\nsleep 30\n";

    struct World {
        _dir: tempfile::TempDir,
        root: PathBuf,
        store: Store,
        terminals: Arc<TerminalService>,
        restore: Arc<RestoreService>,
        bus: EventBus,
        panes: Vec<(PaneId, PathBuf)>,
    }

    async fn world(agents: usize) -> World {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let root = dir.path().join("ws");
        std::fs::create_dir_all(&root).expect("ws");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let fake = bin.join("claude");
        std::fs::write(&fake, FAKE).expect("fake");
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        settings::set(
            &store,
            SettingKey::DefaultShell,
            serde_json::json!("/bin/sh"),
        )
        .await
        .expect("shell");
        let added = crate::services::workspaces::add(&store, root.to_str().expect("utf8"), None)
            .await
            .expect("ws");
        let tab = tabs::list(&store, &added.workspace.id)
            .await
            .expect("tabs")
            .remove(0);
        let mut made = Vec::new();
        for i in 0..agents {
            let p = panes::create_with(&store, &tab.id, "agent", None, Some("claude".into()))
                .await
                .expect("pane");
            let home = dir.path().join(format!("home-{i}"));
            std::fs::create_dir_all(&home).expect("home");
            let config = LaunchConfig {
                extra_args: vec![],
                home: Some(home.to_string_lossy().into_owned()),
                own_home: false,
            };
            sessions::start_with_config(
                &store,
                &p.id,
                &added.workspace.id,
                "claude",
                None,
                &config,
            )
            .await
            .expect("session");
            sessions::set_engine_session(&store, &p.id, &format!("sid-{i}"))
                .await
                .expect("sid");
            // Distinct start times: newest first is well defined.
            tokio::time::sleep(Duration::from_millis(3)).await;
            made.push((p.id, home));
        }
        let bus = EventBus::new();
        let restore = RestoreService::new(store.clone(), bus.clone());
        restore.settle_at_launch();
        let terminals = TerminalService::new(PtyManager::new(), store.clone());
        let mut vars = std::collections::BTreeMap::new();
        vars.insert(
            "PATH".to_string(),
            format!("{}:/usr/bin:/bin", bin.display()),
        );
        vars.insert("HOME".to_string(), dir.path().display().to_string());
        terminals.set_login_env(LoginEnv {
            vars,
            shell: "/bin/sh".into(),
            degraded: false,
        });
        World {
            _dir: dir,
            root,
            store,
            terminals,
            restore,
            bus,
            panes: made,
        }
    }

    fn screen(t: &TerminalService, pane: &PaneId) -> String {
        String::from_utf8_lossy(&t.pty.tail(pane.as_str(), 50).unwrap_or_default()).into_owned()
    }

    fn kill_all(w: &World) {
        for (p, _) in &w.panes {
            let _ = w.terminals.pty.kill(p.as_str());
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_sh_07_paused_agents_never_start_from_a_plain_spawn() {
        let w = world(1).await;
        let (pane, _) = &w.panes[0];
        assert_eq!(
            panes::get(&w.store, pane).await.expect("p").restore_state,
            "paused"
        );
        let err = w
            .terminals
            .spawn(pane, 80, 24, None, Arc::new(tandem_pty::VecSink::default()))
            .await
            .expect_err("paused");
        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(w.terminals.pty.live_count(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_sh_07_2_resume_spawns_one_process_in_the_saved_home_with_the_session_id() {
        let w = world(2).await;
        let (pane, home) = &w.panes[1];
        w.restore
            .launch(&w.terminals, pane, None, None, LaunchMode::Resume)
            .await
            .expect("resume");
        assert!(
            wait_until(Duration::from_secs(10), || screen(&w.terminals, pane)
                .contains("home:")),
            "{}",
            screen(&w.terminals, pane)
        );
        let text = screen(&w.terminals, pane);
        assert!(text.contains("args: --resume sid-1"), "{text}");
        assert!(
            text.contains(&format!("home: {}", home.display())),
            "{text}"
        );
        // Exactly one process: the other card is still paused.
        assert_eq!(w.terminals.pty.live_count(), 1);
        assert_eq!(
            panes::get(&w.store, pane).await.expect("p").restore_state,
            "live"
        );
        assert_eq!(
            panes::get(&w.store, &w.panes[0].0)
                .await
                .expect("p")
                .restore_state,
            "paused"
        );
        // The same session row runs again; no new one was made.
        let s = sessions::latest_for_pane(&w.store, pane)
            .await
            .expect("s")
            .expect("one");
        assert_eq!(s.engine_session_id.as_deref(), Some("sid-1"));
        assert_eq!(s.ended_at, None);
        kill_all(&w);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_sh_07_4_a_missing_launch_configuration_never_spawns() {
        let w = world(1).await;
        let (pane, home) = &w.panes[0];
        std::fs::remove_dir_all(home).expect("rm home");
        let snap = w
            .restore
            .snapshot(&w.terminals, &w.root.join("no-scrollback"))
            .await
            .expect("snap");
        assert_eq!(snap.panes.len(), 1);
        assert!(!snap.panes[0].can_resume);
        assert_eq!(
            snap.panes[0].reason.as_deref(),
            Some(LAUNCH_CONFIG_UNAVAILABLE_TEXT)
        );
        let err = w
            .restore
            .launch(&w.terminals, pane, None, None, LaunchMode::Resume)
            .await
            .expect_err("unavailable");
        assert_eq!(err.code, ErrorCode::Unavailable);
        assert_eq!(err.message, LAUNCH_CONFIG_UNAVAILABLE_TEXT);
        assert_eq!(w.terminals.pty.live_count(), 0);
        // Start fresh is still offered, and works.
        w.restore
            .launch(&w.terminals, pane, None, None, LaunchMode::Fresh)
            .await
            .expect("fresh");
        assert!(wait_until(Duration::from_secs(10), || screen(
            &w.terminals,
            pane
        )
        .contains("args:")));
        assert!(!screen(&w.terminals, pane).contains("--resume"));
        kill_all(&w);
    }

    const LAUNCH_CONFIG_UNAVAILABLE_TEXT: &str =
        crate::services::engines::LAUNCH_CONFIG_UNAVAILABLE;

    #[tokio::test(flavor = "multi_thread")]
    async fn f_sh_07_5_cancel_takes_a_queued_card_out_of_the_queue() {
        let w = world(1).await;
        let (pane, _) = w.panes[0].clone();
        // Both slots busy: the launch waits in the queue.
        let a = w.restore.slots.clone().acquire_owned().await.expect("a");
        let b = w.restore.slots.clone().acquire_owned().await.expect("b");
        let (r, t, p) = (w.restore.clone(), w.terminals.clone(), pane.clone());
        let launch =
            tokio::spawn(async move { r.launch(&t, &p, None, None, LaunchMode::Resume).await });
        let queued = || w.restore.is_queued(pane.as_str());
        for _ in 0..100 {
            if queued() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(queued());
        w.restore.cancel(&pane).await.expect("cancel");
        drop((a, b));
        let err = launch.await.expect("join").expect_err("cancelled");
        assert_eq!(err.code, ErrorCode::Cancelled);
        assert_eq!(w.terminals.pty.live_count(), 0);
        assert_eq!(
            panes::get(&w.store, &pane).await.expect("p").restore_state,
            "cancelled"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_sh_07_closing_a_pane_drops_its_queued_launch() {
        let w = world(1).await;
        let (pane, _) = w.panes[0].clone();
        let a = w.restore.slots.clone().acquire_owned().await.expect("a");
        let b = w.restore.slots.clone().acquire_owned().await.expect("b");
        let (r, t, p) = (w.restore.clone(), w.terminals.clone(), pane.clone());
        let launch =
            tokio::spawn(async move { r.launch(&t, &p, None, None, LaunchMode::Resume).await });
        while !w.restore.is_queued(pane.as_str()) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        w.restore.forget(&pane);
        panes::close(&w.store, &pane).await.expect("close");
        drop((a, b));
        assert!(launch.await.expect("join").is_err());
        assert_eq!(w.terminals.pty.live_count(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_sh_07_3_auto_resume_runs_newest_first_never_more_than_two_spawning() {
        let w = world(5).await;
        settings::set(
            &w.store,
            SettingKey::ResumeAgentsAutomatically,
            serde_json::json!(true),
        )
        .await
        .expect("setting");
        let mut events = w.bus.subscribe(Topic::App);
        w.restore.auto_resume(&w.terminals, None).await;
        let all_live = || {
            w.panes
                .iter()
                .all(|(p, _)| w.terminals.pty.is_live(p.as_str()))
        };
        assert!(wait_until(Duration::from_secs(20), all_live));
        assert!(wait_until(Duration::from_secs(15), || w
            .restore
            .spawning
            .load(Ordering::SeqCst)
            == 0));
        assert_eq!(w.restore.max_spawning(false), MAX_SPAWNING);
        let mut live_order = Vec::new();
        while let Ok(e) = events.try_recv() {
            if e.payload["state"] == "live" {
                live_order.push(e.payload["paneId"].as_str().unwrap_or_default().to_string());
            }
        }
        let newest_first: Vec<String> = w
            .panes
            .iter()
            .rev()
            .map(|(p, _)| p.as_str().to_string())
            .collect();
        // Only two slots: the two newest start first, the oldest last of all
        // (later ones may finish starting in either order).
        assert_eq!(live_order.len(), 5);
        let mut first_two = live_order[..2].to_vec();
        first_two.sort();
        let mut want = newest_first[..2].to_vec();
        want.sort();
        assert_eq!(first_two, want);
        assert!(live_order[3..].contains(&newest_first[4]), "{live_order:?}");
        kill_all(&w);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_sh_07_3_auto_resume_is_off_by_default() {
        let w = world(2).await;
        w.restore.auto_resume(&w.terminals, None).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(w.terminals.pty.live_count(), 0);
    }
}
