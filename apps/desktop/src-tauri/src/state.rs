use std::sync::Arc;

use tandem_core::bus::EventBus;
use tandem_core::clock::{Clock, MockClock, SystemClock};
use tandem_core::keychain::{self, Keychain};
use tandem_core::logging::{self, RedactingFile};
use tandem_core::{CoreError, DataDir};
use tandem_store::Store;

/// Everything commands need, built once at startup and shared.
pub struct AppState {
    pub data: DataDir,
    pub store: Store,
    pub bus: EventBus,
    pub clock: Arc<dyn Clock>,
    pub keychain: Arc<dyn Keychain>,
    pub log: Option<RedactingFile>,
    pub first_run: bool,
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
        Ok(Self::with(data, store, log, first_run))
    }

    pub fn with(data: DataDir, store: Store, log: Option<RedactingFile>, first_run: bool) -> Self {
        let clock: Arc<dyn Clock> = match MockClock::from_env().filter(|_| cfg!(debug_assertions)) {
            Some(mock) => Arc::new(mock),
            None => Arc::new(SystemClock),
        };
        Self {
            data,
            store,
            bus: EventBus::new(),
            clock,
            keychain: keychain::open_keychain(),
            log,
            first_run,
        }
    }
}
