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
use crate::services::deeplink::DeepLinkQueue;
use crate::services::entitlement::EntitlementService;
use crate::services::opener::{self, UrlOpener};
use crate::services::telemetry::TelemetryService;
use crate::services::terminal::TerminalService;

pub const BUNDLED_KEYS: &str = include_str!("../resources/keys.json");

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
    pub telemetry: Arc<TelemetryService>,
    pub deeplinks: DeepLinkQueue,
    /// Processes a crashed run left behind, until the user decides (UI-132).
    pub orphans: Mutex<Vec<OrphanInfo>>,
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
        let api: Arc<dyn ApiClient> = Arc::new(HttpApi::new(net.clone(), endpoints.clone()));
        Ok(Self::with(
            data,
            store,
            log,
            first_run,
            net,
            endpoints,
            api,
            opener::default_opener(),
        ))
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
        let (entitlement_keys, _config_keys) = bundled_keys(BUNDLED_KEYS)
            .unwrap_or((PublicKeySet::default(), PublicKeySet::default()));
        let entitlements = EntitlementService::new(
            auth.clone(),
            api.clone(),
            entitlement_keys,
            data.clone(),
            clock.clone(),
            bus.clone(),
        );
        // Scan last run's children before this run's registry replaces the file.
        let children = data.run().join("children.json");
        let mut table = SystemTable::default();
        let found = orphans::scan(&orphans::read_records(&children), &mut table);
        let registry = Arc::new(ChildRegistry::new(children, Box::new(table)));
        let pty_bus = bus.clone();
        let pty = PtyManager::new()
            .with_registry(registry)
            .with_state_sink(Arc::new(move |state| {
                pty_bus.publish(BusEvent::new(Topic::Pty, "pty:state", state));
            }));
        let terminals = TerminalService::new(pty, store.clone());
        let telemetry = TelemetryService::new(&data, net.clone());
        let e2e = cfg!(debug_assertions) && std::env::var("TANDEM_E2E").as_deref() == Ok("1");
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
            telemetry,
            deeplinks: DeepLinkQueue::default(),
            orphans: Mutex::new(found),
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
        let list = self
            .orphans
            .lock()
            .map(|mut o| std::mem::take(&mut *o))
            .unwrap_or_default();
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
        let found = state.orphans.lock().expect("orphans").clone();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pids, vec![pid]);
        // This run's registry starts empty.
        assert_eq!(
            orphans::read_records(&state.data.run().join("children.json")),
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
        assert!(state.orphans.lock().expect("orphans").is_empty());
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
        assert!(state.orphans.lock().expect("orphans").is_empty());
    }
}
