//! Remote config (ARCHITECTURE §14.10): fetched from `/v1/config`, verified
//! with the bundled keys and cached; the bundled copy is used whenever the
//! fetched one is missing or doesn't verify.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use serde_json::Value;
use tandem_core::entitlement::PublicKeySet;
use tandem_core::remote_config::{self, ConfigSource, SignedConfig};

use crate::services::api::ApiClient;

pub const BUNDLED_CONFIG: &str = include_str!("../../resources/config.json");

pub struct ConfigService {
    api: Arc<dyn ApiClient>,
    keys: PublicKeySet,
    cache: PathBuf,
    current: RwLock<(Value, ConfigSource)>,
}

impl ConfigService {
    /// Starts from the cached envelope when it still verifies.
    pub fn new(api: Arc<dyn ApiClient>, keys: PublicKeySet, cache: PathBuf) -> Arc<Self> {
        let cached = std::fs::read(&cache)
            .ok()
            .and_then(|b| serde_json::from_slice::<SignedConfig>(&b).ok());
        let current = remote_config::resolve(cached.as_ref(), &keys, BUNDLED_CONFIG);
        Arc::new(Self {
            api,
            keys,
            cache,
            current: RwLock::new(current),
        })
    }

    pub fn value(&self) -> Value {
        self.current
            .read()
            .map(|c| c.0.clone())
            .unwrap_or(Value::Null)
    }

    pub fn source(&self) -> Option<ConfigSource> {
        self.current.read().ok().map(|c| c.1)
    }

    /// Fetches and verifies; keeps the current config on any failure.
    pub async fn refresh(&self) -> Value {
        match self.api.config().await {
            Ok(envelope) => match remote_config::verify_config(&envelope, &self.keys) {
                Ok(value) => {
                    if let Ok(bytes) = serde_json::to_vec(&envelope) {
                        if let Some(dir) = self.cache.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        let _ = tandem_core::atomic::atomic_write(&self.cache, &bytes, Some(0o600));
                    }
                    if let Ok(mut c) = self.current.write() {
                        *c = (value, ConfigSource::Remote);
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, "remote config rejected; keeping the current one")
                }
            },
            Err(err) => tracing::debug!(error = %err, "remote config fetch failed"),
        }
        self.value()
    }
}
