use std::sync::Arc;

use tandem_core::bus::EventBus;
use tandem_core::clock::{Clock, MockClock, SystemClock};
use tandem_core::entitlement::{bundled_keys, PublicKeySet};
use tandem_core::keychain::{self, Keychain};
use tandem_core::logging::{self, RedactingFile};
use tandem_core::net::Net;
use tandem_core::settings::SettingKey;
use tandem_core::{CoreError, DataDir};
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
        let terminals = TerminalService::new(PtyManager::new(), store.clone());
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
