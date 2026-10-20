//! Engine versions and updates under the user's control (ARCHITECTURE §3
//! "Engines, accounts, usage", F-EN-08).

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{State, WebviewWindow};
use ts_rs::TS;

use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::registry::InstallMethod;
use tandem_engines::versions;
use tandem_store::repo::settings;

use crate::commands::engines::InstallCommand;
use crate::services::engine_processes::{self, BackgroundProcess, EnginePaths};
use crate::services::engine_versions::{self as svc, EngineVersionView};
use crate::state::AppState;

/// How long `npm view` may take.
const CHECK_WAIT: Duration = Duration::from_secs(30);

/// What Check for updates found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateCheck {
    /// The newest version, when Vibezeo asked (an npm install).
    pub latest: Option<String>,
    /// Otherwise, the command that updates it.
    pub how: Option<String>,
}

fn not_found() -> IpcError {
    IpcError::new(ErrorCode::NotFound, "That engine isn't installed")
}

async fn pins(state: &AppState) -> serde_json::Value {
    settings::get(&state.store, SettingKey::EnginePins)
        .await
        .unwrap_or_else(|_| serde_json::json!({}))
}

async fn home(state: &AppState) -> PathBuf {
    let login = state.terminals.login_env().await;
    login
        .vars
        .get("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// The table's rows: never the network.
#[tauri::command]
pub async fn engine_versions(
    state: State<'_, AppState>,
) -> Result<Vec<EngineVersionView>, IpcError> {
    let engines = &state.terminals.engines;
    let report = engines.report();
    let book = engines.versions.sync(report.as_ref());
    let pins = pins(&state).await;
    let home = home(&state).await;
    Ok(svc::views(
        engines.registry().all(),
        report.as_ref(),
        &book,
        &home,
        |id| {
            pins.get(id)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        },
    ))
}

/// Check for updates, only when clicked: an npm install asks the npm
/// registry for its newest version; any other install method answers with
/// the command that updates it.
#[tauri::command]
pub async fn engine_check_update(
    state: State<'_, AppState>,
    engine_id: String,
) -> Result<UpdateCheck, IpcError> {
    let engines = &state.terminals.engines;
    let spec = engines.get(&engine_id).ok_or_else(not_found)?;
    let found = engines.installed(&engine_id).ok_or_else(not_found)?;
    let home = home(&state).await;
    let method = versions::install_method(
        &svc::resolved(&found.to_string_lossy()),
        &home,
        spec.updates.as_ref(),
    );
    let package = spec.updates.as_ref().and_then(|u| u.npm.clone());
    let (InstallMethod::Npm, Some(package)) = (method, package) else {
        return Ok(UpdateCheck {
            latest: None,
            how: versions::update_hint(&spec, method),
        });
    };
    let login = state.terminals.login_env().await;
    let mut cmd = tokio::process::Command::new("npm");
    // From the data folder, so no project's `.npmrc` steers it.
    cmd.args(["view", &package, "version"])
        .current_dir(state.data.root())
        .env_clear()
        .envs(login.vars.iter())
        .kill_on_drop(true);
    let unavailable = |why: String| IpcError::new(ErrorCode::Unavailable, why).with_stage("npm");
    let out = tokio::time::timeout(CHECK_WAIT, cmd.output())
        .await
        .map_err(|_| unavailable("npm didn't answer in 30 seconds".into()))?
        .map_err(|e| unavailable(format!("Couldn't run npm: {e}")))?;
    if !out.status.success() {
        return Err(unavailable("npm couldn't read the newest version".into()));
    }
    let latest = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !versions::valid_version(&latest) {
        return Err(unavailable(
            "npm answered with something that isn't a version".into(),
        ));
    }
    tracing::info!(
        code = "engine_versions.checked",
        "checked an engine for updates"
    );
    Ok(UpdateCheck {
        latest: Some(latest),
        how: None,
    })
}

/// Pin: new panes of the engine start with its documented switch.
#[tauri::command]
pub async fn engine_pin(
    state: State<'_, AppState>,
    engine_id: String,
    on: bool,
) -> Result<(), IpcError> {
    let spec = state
        .terminals
        .engines
        .get(&engine_id)
        .ok_or_else(not_found)?;
    if spec.updates.as_ref().and_then(|u| u.pin.as_ref()).is_none() {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            format!(
                "{} has no documented way to stop it updating itself",
                spec.label
            ),
        ));
    }
    let mut pins = pins(&state).await;
    if !pins.is_object() {
        pins = serde_json::json!({});
    }
    if let Some(map) = pins.as_object_mut() {
        if on {
            map.insert(engine_id, serde_json::Value::Bool(true));
        } else {
            map.remove(&engine_id);
        }
    }
    settings::set(&state.store, SettingKey::EnginePins, pins).await?;
    Ok(())
}

/// The command that rolls the engine back to `version`, to show and then
/// type into a visible pane (like Install, F-EN-03).
#[tauri::command]
pub async fn engine_rollback_command(
    state: State<'_, AppState>,
    engine_id: String,
    version: String,
) -> Result<InstallCommand, IpcError> {
    let engines = &state.terminals.engines;
    let spec = engines.get(&engine_id).ok_or_else(not_found)?;
    let found = engines.installed(&engine_id).ok_or_else(not_found)?;
    let home = home(&state).await;
    let method = versions::install_method(
        &svc::resolved(&found.to_string_lossy()),
        &home,
        spec.updates.as_ref(),
    );
    let display = versions::rollback_command(&spec, method, &version).ok_or_else(|| {
        IpcError::new(
            ErrorCode::Invalid,
            format!(
                "Vibezeo can't roll {} back when it's installed this way",
                spec.label
            ),
        )
    })?;
    Ok(InstallCommand {
        display,
        shell: if cfg!(windows) { "powershell" } else { "sh" }.into(),
    })
}

fn engine_paths(state: &AppState) -> Vec<EnginePaths> {
    let engines = &state.terminals.engines;
    engines
        .registry()
        .all()
        .iter()
        .filter_map(|spec| {
            let found = engines.installed(&spec.id)?;
            let resolved = svc::resolved(&found.to_string_lossy());
            let mut paths = vec![found];
            if !paths.contains(&resolved) {
                paths.push(resolved);
            }
            Some(EnginePaths {
                id: spec.id.clone(),
                paths,
            })
        })
        .collect()
}

async fn background_now(state: &AppState) -> Result<Vec<BackgroundProcess>, IpcError> {
    let engines = engine_paths(state);
    let table = engine_processes::system();
    tokio::task::spawn_blocking(move || {
        engine_processes::list(table.as_ref(), &engines, std::process::id())
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
}

/// Engine processes with no terminal, outside the app's own.
#[tauri::command]
pub async fn engine_background(
    state: State<'_, AppState>,
) -> Result<Vec<BackgroundProcess>, IpcError> {
    background_now(&state).await
}

/// Stop, after the window asked: only a process still in that list with
/// the same engine and start time, so a reused pid is never signalled.
#[tauri::command]
pub async fn engine_background_stop(
    window: WebviewWindow,
    state: State<'_, AppState>,
    pid: u32,
    engine_id: String,
    started: u64,
) -> Result<(), IpcError> {
    if window.label() != "main" {
        return Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "Stopping a process needs the main window",
        ));
    }
    let engines = engine_paths(&state);
    let table = engine_processes::system();
    tokio::task::spawn_blocking(move || {
        engine_processes::stop(
            table.as_ref(),
            &engines,
            std::process::id(),
            pid,
            &engine_id,
            started,
        )
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))??;
    tracing::info!(
        code = "engine_versions.stopped",
        "stopped a background engine process"
    );
    Ok(())
}
