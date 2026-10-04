use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::redact::Redactor;
use tandem_core::settings::SettingKey;
use tandem_store::repo::settings;

use crate::state::AppState;

/// What the UI needs on its first render (`app_ready`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppBoot {
    pub version: String,
    pub build: String,
    pub platform: String,
    pub dev: bool,
    pub first_run: bool,
    pub last_mode: String,
    pub flags: Vec<String>,
    /// The data folder with the home directory shown as `~`.
    pub data_dir_display: String,
}

pub fn build_id() -> String {
    option_env!("TANDEM_BUILD").unwrap_or("dev").to_string()
}

pub async fn app_boot(state: &AppState) -> AppBoot {
    let last_mode = settings::get(&state.store, SettingKey::LastMode)
        .await
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "code".to_string());
    let mut flags = Vec::new();
    if cfg!(debug_assertions) && std::env::var("TANDEM_E2E").as_deref() == Ok("1") {
        flags.push("e2e".to_string());
    }
    AppBoot {
        version: env!("CARGO_PKG_VERSION").to_string(),
        build: build_id(),
        platform: std::env::consts::OS.to_string(),
        dev: cfg!(debug_assertions),
        first_run: state.first_run,
        last_mode,
        flags,
        data_dir_display: Redactor::global().redact_home(&state.data.root().to_string_lossy()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::DataDir;
    use tandem_store::Store;

    #[tokio::test]
    async fn boot_reports_version_and_default_mode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let state = AppState::with(data, store, None, true);
        let boot = app_boot(&state).await;
        assert_eq!(boot.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(boot.last_mode, "code");
        assert!(boot.first_run);
    }
}
