use std::sync::Arc;

use std::sync::Mutex;

use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::clock::{Clock, MockClock, SystemClock};
use tandem_core::entitlement::{bundled_keys, PublicKeySet};
use tandem_core::keychain::{self, Keychain};
use tandem_core::logging::{self, RedactingFile};
use tandem_core::net::Net;
use tandem_core::settings::SettingKey;
use tandem_core::{CoreError, DataDir};
use tandem_pty::orphans::{self, ChildRegistry, OrphanInfo, SystemTable};
use tandem_pty::PtyManager;
use tandem_store::repo::settings;
use tandem_store::Store;

use crate::services::api::{ApiClient, Endpoints, HttpApi};
use crate::services::auth::AuthService;
use crate::services::config::ConfigService;
use crate::services::deeplink::DeepLinkQueue;
use crate::services::engines::EngineService;
use crate::services::entitlement::EntitlementService;
use crate::services::opener::{self, UrlOpener};
use crate::services::status::StatusService;
use crate::services::telemetry::TelemetryService;
use crate::services::terminal::TerminalService;

pub const BUNDLED_KEYS: &str = include_str!("../resources/keys.json");

/// How long the startup scan waits for a previous instance that is still
/// shutting down (its own stop sequence takes up to ~4 s) before treating
/// what it left as orphans.
pub const ORPHAN_SCAN_WAIT: std::time::Duration = std::time::Duration::from_secs(8);

/// Last run's leftovers (UI-132). The scan runs off the startup path; readers
/// wait for it.
pub struct OrphanScan {
    list: Arc<Mutex<Vec<OrphanInfo>>>,
    ready: tokio::sync::watch::Receiver<bool>,
}

impl OrphanScan {
    /// Collects the registry files of runs that have exited (waiting up to
    /// `wait` for one still shutting down) and finds their live processes.
    pub fn start(
        run: std::path::PathBuf,
        own: std::path::PathBuf,
        wait: std::time::Duration,
    ) -> Self {
        let list = Arc::new(Mutex::new(Vec::new()));
        let (tx, ready) = tokio::sync::watch::channel(false);
        let out = list.clone();
        std::thread::spawn(move || {
            let mut table = SystemTable::default();
            let records = orphans::collect_stale(&run, &own, &mut table, wait);
            let found = orphans::scan(&records, &mut table);
            if !found.is_empty() {
                tracing::info!(
                    code = "orphans.found",
                    count = found.len(),
                    "processes from the last run are still running"
                );
            }
            if let Ok(mut l) = out.lock() {
                *l = found;
            }
            let _ = tx.send(true);
        });
        Self { list, ready }
    }

    async fn wait(&self) {
        let mut ready = self.ready.clone();
        let _ = ready.wait_for(|r| *r).await;
    }

    pub async fn list(&self) -> Vec<OrphanInfo> {
        self.wait().await;
        self.list.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// The list, emptied: the user (or the policy) has decided.
    pub async fn take(&self) -> Vec<OrphanInfo> {
        self.wait().await;
        self.list
            .lock()
            .map(|mut l| std::mem::take(&mut *l))
            .unwrap_or_default()
    }
}

/// Everything commands need, built once at startup and shared.
pub struct AppState {
    pub data: DataDir,
    pub store: Store,
    pub bus: EventBus,
    pub clock: Arc<dyn Clock>,
    /// Set when `TANDEM_FAKE_CLOCK` drives time (debug builds).
    pub mock_clock: Option<MockClock>,
    pub keychain: Arc<dyn Keychain>,
    pub log: Option<RedactingFile>,
    pub first_run: bool,
    pub net: Net,
    pub endpoints: Endpoints,
    pub api: Arc<dyn ApiClient>,
    pub opener: Arc<dyn UrlOpener>,
    pub auth: Arc<AuthService>,
    pub entitlements: Arc<EntitlementService>,
    pub terminals: Arc<TerminalService>,
    pub approvals: Arc<crate::services::approvals::ApprovalService>,
    pub notifications: Arc<crate::services::notifications::NotificationService>,
    /// Paused agents and their relaunch queue (F-SH-07).
    pub restore: Arc<crate::services::restore::RestoreService>,
    /// Auto-update and the minimum-version notice (F-SH-06).
    pub updates: Arc<crate::services::updates::UpdateService>,
    pub config: Arc<ConfigService>,
    pub telemetry: Arc<TelemetryService>,
    /// The one MCP gateway (F-PL-01).
    pub gateway: Arc<crate::services::gateway::GatewayService>,
    pub deeplinks: DeepLinkQueue,
    /// Processes a crashed run left behind, until the user decides (UI-132).
    pub orphans: OrphanScan,
    /// Engine-account sign-ins and sign-outs in flight (F-AC-02).
    pub logins: Arc<crate::services::accounts::LoginOps>,
    /// The latest usage per engine account (F-AC-04).
    pub usage: Arc<crate::services::usage::UsageBoard>,
    /// Thread mode: turns, their events and the chat adapters (F-TH-03).
    pub threads: Arc<crate::services::threads::ThreadService>,
    /// Models engines reported at runtime (F-TH-08).
    pub models: Arc<crate::services::models::RuntimeModels>,
    /// The Files tab and the repository status it shares with the git
    /// panel (F-CD-19, F-CD-20).
    pub files: Arc<crate::services::files::FilesService>,
    /// The Changes view and the status bar's branch (F-CD-20).
    pub git_panel: Arc<crate::services::git_panel::GitPanelService>,
    /// Browser tabs and panes (F-CD-17), bound to the window in setup.
    pub browser: std::sync::OnceLock<Arc<crate::browser::BrowserService>>,
    /// Debug + `TANDEM_E2E=1`: the `test_*` harness is reachable.
    pub e2e: bool,
}

impl AppState {
    pub fn init() -> Result<Self, CoreError> {
        let data = DataDir::resolve()?;
        let first_run = !data.db_path().exists();
        let level = std::env::var("TANDEM_LOG").unwrap_or_else(|_| "info".to_string());
        // A second init (tests) keeps the first subscriber.
        let log = logging::init(&data.logs(), &level).ok();
        let store = Store::open(&data).map_err(|e| {
            CoreError::new(
                "store",
                e.to_string(),
                "Copy diagnostics and contact support",
            )
        })?;
        let net = if cfg!(debug_assertions) {
            Net::from_env()
        } else {
            Net::with_log(None)
        };
        let endpoints = Endpoints::from_env();
        let min_version = crate::services::api::MinVersionWatch::default();
        // Early-access builds have no hosted service: nothing leaves the
        // machine for it (D-105).
        let api: Arc<dyn ApiClient> = if endpoints.hosted {
            Arc::new(HttpApi::watching(
                net.clone(),
                endpoints.clone(),
                min_version.clone(),
            ))
        } else {
            Arc::new(crate::services::api::NoHostedApi)
        };
        let state = Self::with(
            data,
            store,
            log,
            first_run,
            net,
            endpoints,
            api,
            opener::default_opener(),
        );
        // API responses can name the oldest supported version (F-SH-06).
        let updates = state.updates.clone();
        let _ = min_version.set(Arc::new(move |v: &str| updates.observe_min_version(v)));
        Ok(state)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with(
        data: DataDir,
        store: Store,
        log: Option<RedactingFile>,
        first_run: bool,
        net: Net,
        endpoints: Endpoints,
        api: Arc<dyn ApiClient>,
        opener: Arc<dyn UrlOpener>,
    ) -> Self {
        let mock_clock = MockClock::from_env().filter(|_| cfg!(debug_assertions));
        let clock: Arc<dyn Clock> = match &mock_clock {
            Some(mock) => Arc::new(mock.clone()),
            None => Arc::new(SystemClock),
        };
        let keychain = keychain::open_keychain();
        let bus = EventBus::new();
        let offline_started_at = store_blocking_int(&store, SettingKey::OfflineStartedAt);
        let auth = AuthService::new(
            data.clone(),
            keychain.clone(),
            api.clone(),
            opener.clone(),
            endpoints.clone(),
            clock.clone(),
            bus.clone(),
            offline_started_at,
        );
        let (entitlement_keys, config_keys) = bundled_keys(BUNDLED_KEYS)
            .unwrap_or((PublicKeySet::default(), PublicKeySet::default()));
        let entitlements = EntitlementService::new(
            auth.clone(),
            api.clone(),
            entitlement_keys,
            data.clone(),
            clock.clone(),
            bus.clone(),
        );
        // This run records its children in its own file; earlier runs' files
        // are scanned in the background (F-CD-03 AC8).
        let registry = Arc::new(ChildRegistry::new(
            &data.run(),
            Box::new(SystemTable::default()),
        ));
        let orphans =
            OrphanScan::start(data.run(), registry.path().to_path_buf(), ORPHAN_SCAN_WAIT);
        let config = ConfigService::new(api.clone(), config_keys, data.config_cache());
        let engines = EngineService::new(store.clone(), bus.clone(), data.providers());
        engines.apply_config(&config.value());
        let status = StatusService::new(bus.clone(), clock.clone());
        // Engine session ids are saved as soon as hooks report them, so a
        // crash can still resume (F-SH-07).
        let session_store = store.clone();
        status.set_session_sink(Arc::new(move |pane, id| {
            let store = session_store.clone();
            let pane = tandem_core::ids::PaneId::from_db(pane.to_string());
            let id = id.to_string();
            tauri::async_runtime::spawn(async move {
                if let Err(err) =
                    tandem_store::repo::sessions::set_engine_session(&store, &pane, &id).await
                {
                    tracing::warn!(error = %err, "couldn't save the engine session id");
                }
            });
        }));
        let pty_bus = bus.clone();
        let mark_bus = bus.clone();
        let cwd_store = store.clone();
        // An agent exited and its pane went back to the shell: save the
        // session for the workspace's "Saved sessions" (F-CD-03 AC3).
        let handback_store = store.clone();
        let handback_bus = bus.clone();
        let handback_status = status.clone();
        // Agent output drives the status heuristics (F-EN-05); the screen is
        // read only when a waiting-pattern check is due.
        let activity_status = status.clone();
        let screen_pty: Arc<std::sync::OnceLock<PtyManager>> = Arc::new(std::sync::OnceLock::new());
        let screen_for_sink = screen_pty.clone();
        let screen_for_tick = screen_pty.clone();
        // A turn ended: the context fill from the engine's own transcript,
        // only when it's inside the pane's provider home (F-AC-06).
        let (turn_store, turn_contexts) = (store.clone(), status.contexts.clone());
        status.set_turn_sink(Arc::new(move |pane, transcript| {
            let (store, contexts) = (turn_store.clone(), turn_contexts.clone());
            let (pane, transcript) = (pane.to_string(), std::path::PathBuf::from(transcript));
            tauri::async_runtime::spawn(async move {
                let id = tandem_core::ids::PaneId::from_db(pane.clone());
                let Ok(record) = tandem_store::repo::panes::get(&store, &id).await else {
                    return;
                };
                let Some(account) = record.account_id else {
                    return;
                };
                let Ok(account) = tandem_store::repo::accounts::get(&store, &account).await else {
                    return;
                };
                let home = std::path::PathBuf::from(account.home_path);
                let fill = tokio::task::spawn_blocking(move || {
                    tandem_engines::transcript::context_fill(&transcript, &home)
                })
                .await
                .ok()
                .flatten();
                if let Some(fill) = fill {
                    contexts.set(&pane, fill);
                }
            });
        }));
        status.set_screen_source(Arc::new(move |pane| {
            screen_for_tick.get().and_then(|p| p.screen_text(pane))
        }));
        let pty = PtyManager::new()
            .with_registry(registry)
            .with_state_sink(Arc::new(move |state| {
                // The shell's cwd (OSC 7) is the pane's stored cwd: restores
                // and link resolution start there (F-CD-10 AC1).
                if let Some(cwd) = state.cwd.clone() {
                    let (store, pane) = (cwd_store.clone(), tandem_core::ids::PaneId::from_db(state.pane_id.clone()));
                    tauri::async_runtime::spawn(async move {
                        if let Err(err) = tandem_store::repo::panes::set_cwd(&store, &pane, cwd).await {
                            tracing::warn!(error = %err, "couldn't store the pane's cwd");
                        }
                    });
                }
                pty_bus.publish(BusEvent::new(Topic::Pty, "pty:state", state));
            }))
            .with_mark_sink(Arc::new(move |pane, mark| {
                if let Some(event) = mark.event(pane) {
                    mark_bus.publish(BusEvent::new(Topic::Pty, "pty:mark", event));
                }
            }))
            .with_activity_sink(Arc::new(move |pane, _bytes| {
                if !activity_status.is_watched(pane) {
                    return;
                }
                activity_status.output(pane, || screen_for_sink.get().and_then(|p| p.screen_text(pane)));
            }))
            .with_handback_sink(Arc::new(move |pane, code| {
                handback_status.unwatch(pane);
                let (store, bus) = (handback_store.clone(), handback_bus.clone());
                let pane = tandem_core::ids::PaneId::from_db(pane.to_string());
                tracing::info!(code = "pty.handback", exit = code, "agent handed the pane back");
                tauri::async_runtime::spawn(async move {
                    match tandem_store::repo::sessions::save_on_handback(&store, &pane, None).await {
                        Ok(Some(session)) => bus.publish(BusEvent::new(
                            Topic::Pty,
                            "sessions:saved",
                            serde_json::json!({ "workspaceId": session.workspace_id, "session": session }),
                        )),
                        Ok(None) => {}
                        Err(err) => tracing::warn!(error = %err, "couldn't save the session"),
                    }
                });
            }));
        let _ = screen_pty.set(pty.clone());
        let terminals = TerminalService::with_services(
            pty,
            store.clone(),
            Some(data.shell_integration()),
            engines,
            status,
        );
        let approvals = crate::services::approvals::ApprovalService::new(
            store.clone(),
            bus.clone(),
            clock.clone(),
        );
        let notifications =
            crate::services::notifications::NotificationService::new(store.clone(), bus.clone());
        // Before the UI lists anything: agents pause, crashed threads settle.
        let restore = crate::services::restore::RestoreService::new(store.clone(), bus.clone());
        restore.settle_at_launch();
        let managed = crate::services::updates::managed_by_package_manager(
            std::path::Path::new("/usr/share/doc/tandem"),
            std::env::var("APPIMAGE").ok().as_deref(),
        );
        let updates = crate::services::updates::UpdateService::new(
            managed,
            data.update_state(),
            clock.clone(),
            bus.clone(),
        );
        let telemetry = TelemetryService::new(&data, net.clone());
        let models = Arc::new(crate::services::models::RuntimeModels::default());
        let gateway = crate::services::gateway::GatewayService::new(store.clone(), data.run());
        let launcher = Arc::new(crate::services::threads::AppLauncher::new(
            terminals.clone(),
            config.clone(),
            models.clone(),
            crate::services::threads::AppLauncher::adapters(&terminals),
            data.mcp_configs(),
            gateway.clone(),
            store.clone(),
        ));
        let threads = crate::services::threads::ThreadService::new(
            store.clone(),
            bus.clone(),
            clock.clone(),
            approvals.broker.clone(),
            launcher.clone(),
        );
        gateway.bind_threads(&threads);
        // Thread titles: AI only with the Privacy opt-in (D-118).
        let titles_store = store.clone();
        threads.configure(
            Arc::new(crate::services::thread_titles::ApiTitler {
                auth: auth.clone(),
                api: api.clone(),
                enabled: Arc::new(move || {
                    tandem_store::repo::settings::get_blocking(
                        &titles_store,
                        tandem_core::settings::SettingKey::AiThreadTitles,
                    )
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                }),
            }),
            data.clone(),
        );
        threads.sweep_trash();
        // Threads are Dashboard items too (F-DB-01, D-6).
        let status = terminals.status.clone();
        threads.set_status_sink(Arc::new(move |th| {
            use tandem_core::status::{AgentStatus, StatusUpdate, WaitingReason};
            use tandem_store::repo::threads::ThreadState;
            let source = tandem_core::source::SourceRef::Thread {
                thread_id: th.id.as_str().to_string(),
            };
            if th.archived {
                if let Some(key) = source.session_key() {
                    status.remove_external(&key);
                }
                return;
            }
            let (agent, reason) = match th.state {
                ThreadState::Working => (AgentStatus::Working, None),
                ThreadState::Waiting => (AgentStatus::Waiting, Some(WaitingReason::Approval)),
                ThreadState::Done => (AgentStatus::Done, None),
                ThreadState::Failed => (AgentStatus::Failed, None),
                ThreadState::Idle | ThreadState::Interrupted => (AgentStatus::Idle, None),
            };
            let title = (!th.title.is_empty()).then(|| th.title.clone());
            status.set_external(
                crate::services::status::ExternalMeta {
                    source,
                    title: title.clone().unwrap_or_else(|| "New thread".into()),
                    engine_id: Some(th.engine_id.clone()),
                    workspace_id: th.workspace_id.as_ref().map(|w| w.as_str().to_string()),
                },
                StatusUpdate {
                    status: agent,
                    reason,
                    ts: th.updated_at,
                    tool: None,
                    title,
                    preview: None,
                },
            );
        }));
        let e2e = cfg!(debug_assertions) && std::env::var("TANDEM_E2E").as_deref() == Ok("1");
        let files = crate::services::files::FilesService::new(store.clone());
        let git_panel = crate::services::git_panel::GitPanelService::new(files.clone());
        Self {
            data,
            store,
            bus,
            clock,
            mock_clock,
            keychain,
            log,
            first_run,
            net,
            endpoints,
            api,
            opener,
            auth,
            entitlements,
            terminals,
            approvals,
            notifications,
            restore,
            updates,
            config,
            telemetry,
            deeplinks: DeepLinkQueue::default(),
            orphans,
            logins: Arc::default(),
            usage: Arc::default(),
            threads,
            models,
            gateway,
            files,
            git_panel,
            browser: std::sync::OnceLock::new(),
            e2e,
        }
    }

    /// A state with fake network, browser and keychain for unit tests.
    #[cfg(test)]
    pub fn for_tests(data: DataDir, store: Store) -> Self {
        let api: Arc<dyn ApiClient> = Arc::new(crate::services::auth::tests::FakeApi::default());
        let opener: Arc<dyn UrlOpener> =
            Arc::new(crate::services::opener::testing::RecordingOpener::default());
        let endpoints = Endpoints {
            api: "http://api.test".into(),
            web: "http://web.test".into(),
            hosted: true,
        };
        Self::with(
            data,
            store,
            None,
            true,
            Net::with_log(None),
            endpoints,
            api,
            opener,
        )
    }

    /// Settings › Advanced "orphaned processes": "always" stops last run's
    /// leftovers at once, "never" keeps them without asking; "ask" leaves
    /// them for the UI-132 dialog.
    pub async fn apply_orphan_policy(&self) {
        let policy = settings::get(&self.store, SettingKey::OrphanPolicy)
            .await
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "ask".into());
        if policy == "ask" {
            return;
        }
        let list = self.orphans.take().await;
        if policy == "always" && !list.is_empty() {
            let _ = tokio::task::spawn_blocking(move || {
                orphans::stop(
                    &list,
                    &mut SystemTable::default(),
                    std::time::Duration::from_secs(2),
                );
            })
            .await;
        }
    }

    /// A subject's plugin grants changed: its next session picks them up.
    pub async fn after_grants_changed(&self, subject: &tandem_store::repo::plugins::GrantSubject) {
        use tandem_store::repo::plugins::GrantSubject;
        if let GrantSubject::Thread { thread_id } = subject {
            self.threads
                .refresh_session(&tandem_core::ids::ThreadId::from_db(thread_id.clone()))
                .await;
        }
    }

    /// Applies settings that live outside the webview (telemetry toggles).
    pub async fn apply_settings(&self) {
        let get = |key| settings::get(&self.store, key);
        let usage = get(SettingKey::TelemetryUsage)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let crash = get(SettingKey::TelemetryCrash)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        self.telemetry.apply(usage, crash);
    }
}

fn store_blocking_int(store: &Store, key: SettingKey) -> Option<i64> {
    settings::get_blocking(store, key)
        .ok()
        .and_then(|v| v.as_i64())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use tandem_pty::orphans::{ChildRecord, ProcessTable};

    fn state_with_children(records: &[ChildRecord]) -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        std::fs::create_dir_all(data.run()).expect("run dir");
        std::fs::write(
            data.run().join("children.json"),
            serde_json::to_vec(records).expect("json"),
        )
        .expect("write");
        let store = Store::open(&data).expect("store");
        (dir, AppState::for_tests(data, store))
    }

    #[tokio::test]
    async fn f_cd_03_8_last_runs_children_are_found_and_policy_always_stops_them() {
        // A process that outlived "the last run".
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let start_time = SystemTable::default().rows(Some(&[pid]))[0].start_time;
        let record = ChildRecord {
            pane_id: "pn_old".into(),
            pid,
            start_time,
            exe: "sleep".into(),
            cwd: "/tmp".into(),
            started_at: 1,
        };
        let (_dir, state) = state_with_children(&[record]);
        let found = state.orphans.list().await;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pids, vec![pid]);
        // Last run's file was collected; this run's registry starts empty.
        assert!(!state.data.run().join("children.json").exists());
        assert_eq!(
            orphans::read_records(
                &state
                    .data
                    .run()
                    .join(format!("children-{}.json", std::process::id()))
            ),
            Vec::<ChildRecord>::new()
        );
        settings::set(
            &state.store,
            SettingKey::OrphanPolicy,
            serde_json::json!("always"),
        )
        .await
        .expect("policy");
        state.apply_orphan_policy().await;
        assert!(state.orphans.list().await.is_empty());
        let status = child.wait().expect("wait");
        assert!(!status.success(), "sleep was stopped");
    }

    #[tokio::test]
    async fn a_reused_pid_is_not_an_orphan() {
        let me = std::process::id();
        let record = ChildRecord {
            pane_id: "pn_old".into(),
            pid: me,
            start_time: 1,
            exe: "bash".into(),
            cwd: "/tmp".into(),
            started_at: 1,
        };
        let (_dir, state) = state_with_children(&[record]);
        assert!(state.orphans.list().await.is_empty());
    }
}
