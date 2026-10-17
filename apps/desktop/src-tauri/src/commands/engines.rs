//! Engines (ARCHITECTURE §3.2, §6.1–6.3; F-EN-01, F-EN-02, F-EN-03): the
//! registry joined with detection, Check again, install commands and the
//! user's extra launch args.

use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

use tandem_engines::detect::{DetectionReport, FoundSource};
use tandem_engines::EngineSpec;

use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineInstall {
    pub path: String,
    pub version: Option<String>,
    pub source: FoundSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineView {
    pub spec: EngineSpec,
    /// Where detection found it; absent when not installed.
    pub install: Option<EngineInstall>,
    pub chat_capable: bool,
    pub unsupported_reason: Option<String>,
    /// The user's extra launch args (Settings › Engines).
    pub extra_args: Vec<String>,
}

/// The install command for this OS, shown verbatim and typed into a visible
/// terminal by "Run in terminal" (F-EN-03).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstallCommand {
    pub display: String,
    pub shell: String,
}

fn views(
    spec: &[EngineSpec],
    report: Option<&DetectionReport>,
    extra: &serde_json::Value,
) -> Vec<EngineView> {
    spec.iter()
        .map(|s| EngineView {
            install: report.and_then(|r| r.get(&s.id)).map(|f| EngineInstall {
                path: f.path.clone(),
                version: f.version.clone(),
                source: f.source,
            }),
            chat_capable: crate::services::threads::has_chat_adapter(s),
            unsupported_reason: None,
            extra_args: extra
                .get(&s.id)
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
            spec: s.clone(),
        })
        .collect()
}

/// Installed engines first, in registry order otherwise (UI-10).
#[tauri::command]
pub async fn engines_list(
    state: State<'_, AppState>,
    host_id: Option<String>,
) -> Result<Vec<EngineView>, tandem_core::IpcError> {
    let engines = &state.terminals.engines;
    if let Some(host) = host_id.filter(|h| h != "local") {
        // The host's own engines (F-RM-01 "the + menu lists engines
        // installed on the host"); nothing until it's been asked.
        let host = tandem_core::ids::HostId::from_db(host);
        let report = state.remote.report(&host).await;
        let extra = tandem_store::repo::settings::get(
            &state.store,
            tandem_core::settings::SettingKey::EngineExtraArgs,
        )
        .await
        .unwrap_or_default();
        let mut list = views(engines.registry().all(), report.as_ref(), &extra);
        list.sort_by_key(|v| v.install.is_none());
        return Ok(list);
    }
    let report = match engines.report() {
        Some(r) => r,
        // First run, nothing cached: wait for detection (≤ 8 s + versions).
        None => engines.detect(&*state.terminals.login_env().await).await,
    };
    let extra = tandem_store::repo::settings::get(
        &state.store,
        tandem_core::settings::SettingKey::EngineExtraArgs,
    )
    .await
    .unwrap_or_default();
    let mut list = views(engines.registry().all(), Some(&report), &extra);
    list.sort_by_key(|v| v.install.is_none());
    Ok(list)
}

/// Check again: re-reads the remote registry and detects (F-EN-01 AC3).
#[tauri::command]
pub async fn engines_detect(
    state: State<'_, AppState>,
    host_id: Option<String>,
) -> Result<DetectionReport, tandem_core::IpcError> {
    if let Some(host) = host_id.filter(|h| h != "local") {
        let host = tandem_core::ids::HostId::from_db(host);
        let engines = state.terminals.engines.registry().all().to_vec();
        let row = tandem_store::repo::hosts::get(&state.store, &host).await?;
        if row.kind == "wsl" {
            // Inside the distro's login shell (F-RM-02 "Detection runs
            // inside each distro").
            let distro = row.wsl_distro.unwrap_or_default();
            let report = state.wsl.detect_report(&host, &distro, &engines).await?;
            state.remote.publish_report(&host, &report).await?;
            return Ok(report);
        }
        return state.remote.detect(&host, &engines).await;
    }
    let config = state.config.refresh().await;
    state.terminals.engines.apply_config(&config);
    let login = state.terminals.refresh_login_env().await;
    Ok(state.terminals.engines.detect(&login).await)
}

#[tauri::command]
pub async fn engines_install_command(
    state: State<'_, AppState>,
    engine_id: String,
) -> Result<InstallCommand, tandem_core::IpcError> {
    let spec = state.terminals.engines.get(&engine_id).ok_or_else(|| {
        tandem_core::IpcError::new(tandem_core::ErrorCode::NotFound, "That engine isn't known")
    })?;
    let display = if cfg!(target_os = "macos") {
        spec.install.mac
    } else if cfg!(windows) {
        spec.install.win
    } else {
        spec.install.lin
    }
    .ok_or_else(|| {
        tandem_core::IpcError::new(
            tandem_core::ErrorCode::NotFound,
            format!("{} has no install command for this system", spec.label),
        )
    })?;
    Ok(InstallCommand {
        display,
        shell: if cfg!(windows) { "powershell" } else { "sh" }.into(),
    })
}

#[tauri::command]
pub async fn engines_set_extra_args(
    state: State<'_, AppState>,
    engine_id: String,
    args: Vec<String>,
) -> Result<(), tandem_core::IpcError> {
    if state.terminals.engines.get(&engine_id).is_none() {
        return Err(tandem_core::IpcError::new(
            tandem_core::ErrorCode::NotFound,
            "That engine isn't known",
        ));
    }
    state
        .terminals
        .engines
        .set_extra_args(&engine_id, args)
        .await
}

/// Engine accounts and their provider homes (F-AC-01, F-AC-02).
#[tauri::command]
pub async fn accounts_list(
    state: State<'_, AppState>,
) -> Result<Vec<tandem_store::repo::accounts::Account>, tandem_core::IpcError> {
    Ok(tandem_store::repo::accounts::list(&state.store).await?)
}

/// The last detection report (F-EN-01 diagnostics after a webview reload).
#[tauri::command]
pub async fn engines_report(
    state: State<'_, AppState>,
    host_id: Option<String>,
) -> Result<Option<DetectionReport>, tandem_core::IpcError> {
    if let Some(host) = host_id.filter(|h| h != "local") {
        return Ok(state
            .remote
            .report(&tandem_core::ids::HostId::from_db(host))
            .await);
    }
    Ok(state.terminals.engines.report())
}

#[cfg(test)]
mod chat_tests {
    use super::*;

    #[test]
    fn f_th_01_engines_with_a_thread_adapter_are_chat_capable() {
        let registry = tandem_engines::registry::EngineRegistry::bundled();
        let list = views(registry.all(), None, &serde_json::json!({}));
        let capable = |id: &str| {
            list.iter()
                .find(|v| v.spec.id == id)
                .map(|v| v.chat_capable)
        };
        assert_eq!(capable("claude"), Some(true));
        assert_eq!(capable("codex"), Some(true));
        // ACP agents (F-TH-06).
        for id in ["cursor-agent", "grok", "opencode"] {
            assert_eq!(capable(id), Some(true), "{id}");
        }
        // Terminal-only engines stay in Code mode.
        assert!(list
            .iter()
            .filter(|v| v.spec.chat_adapter.is_none())
            .all(|v| !v.chat_capable));
    }

    #[test]
    fn f_en_09_3_antigravity_and_kiro_are_never_thread_or_teammate_engines() {
        let registry = tandem_engines::registry::EngineRegistry::bundled();
        let list = views(registry.all(), None, &serde_json::json!({}));
        for id in ["antigravity", "kiro"] {
            let v = list.iter().find(|v| v.spec.id == id).expect(id);
            assert!(!v.chat_capable, "{id}");
            assert!(!crate::services::threads::has_chat_adapter(&v.spec), "{id}");
            assert!(v.spec.headless_cmd.is_none(), "{id} never runs headless");
        }
    }
}
