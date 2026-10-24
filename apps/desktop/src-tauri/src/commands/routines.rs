//! Routines (ARCHITECTURE §3.2 "Routines"; F-RT-01..04).

use std::sync::Arc;

use tandem_core::ids::{RoutineId, RunId};
use tandem_core::{ErrorCode, IpcError};
use tandem_routines::launcher::{
    launcher_home, platform_launcher, LauncherSpec, LauncherStatus, OsLauncher, SystemRunner,
};
use tandem_routines::{RoutineInput, ScheduleSpec};
use tandem_store::repo::routines::Routine;
use tauri::State;

use crate::services::routines::{RoutineView, RunPage, SchedulePreview};
use crate::state::AppState;

#[tauri::command]
pub async fn routines_list(state: State<'_, AppState>) -> Result<Vec<RoutineView>, IpcError> {
    state.routines.list().await
}

#[tauri::command]
pub async fn routine_get(state: State<'_, AppState>, id: RoutineId) -> Result<Routine, IpcError> {
    state.routines.get(&id).await
}

#[tauri::command]
pub async fn routine_preview_schedule(
    state: State<'_, AppState>,
    spec: ScheduleSpec,
) -> Result<SchedulePreview, IpcError> {
    state.routines.preview(&spec)
}

#[tauri::command]
pub async fn routine_save(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: RoutineInput,
) -> Result<Routine, IpcError> {
    let background = input.run_mode == tandem_routines::RunMode::Background;
    let saved = state.routines.save(input, None).await?;
    // A routine that runs while Vibezeo is closed needs the launcher.
    if background && !launcher().status().installed {
        match set_background(&state, true) {
            Ok(s) => crate::tray::sync(&app, s.installed).await,
            Err(e) => {
                tracing::warn!(code = "routines.launcher_failed", error = %e.message, "couldn't install the background runner")
            }
        }
    }
    Ok(saved)
}

#[tauri::command]
pub async fn routine_delete(state: State<'_, AppState>, id: RoutineId) -> Result<(), IpcError> {
    state.routines.delete(&id).await
}

#[tauri::command]
pub async fn routine_pause(
    state: State<'_, AppState>,
    id: RoutineId,
    paused: bool,
) -> Result<(), IpcError> {
    state.routines.set_paused(&id, paused).await
}

#[tauri::command]
pub async fn routine_run_now(state: State<'_, AppState>, id: RoutineId) -> Result<RunId, IpcError> {
    state.routines.run_now(&id).await
}

#[tauri::command]
pub async fn routine_runs(
    state: State<'_, AppState>,
    id: RoutineId,
    before: Option<i64>,
) -> Result<RunPage, IpcError> {
    state.routines.runs(&id, before).await
}

fn launcher() -> Box<dyn OsLauncher> {
    let home = launcher_home().unwrap_or_else(std::env::temp_dir);
    platform_launcher(Arc::new(SystemRunner), &home)
}

/// What the launcher runs: the AppImage when there is one (its mount path
/// changes every run), else this executable.
fn launcher_exe() -> Result<std::path::PathBuf, IpcError> {
    let exe = std::env::current_exe().map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?;
    Ok(tandem_routines::launcher::launcher_program(
        exe,
        std::env::var_os("APPIMAGE").map(Into::into),
        tandem_core::install::managed_by_store(
            tandem_core::install::windows_package_full_name().as_deref(),
        ),
        std::env::var_os("LOCALAPPDATA").map(Into::into),
    ))
}

fn set_background(state: &AppState, enabled: bool) -> Result<LauncherStatus, IpcError> {
    let l = launcher();
    if enabled {
        if !state.entitlements.limits().background_routines {
            return Err(IpcError::new(
                ErrorCode::ForbiddenPlan,
                "Background routines are turned off for this account.",
            ));
        }
        let spec = LauncherSpec {
            exe: launcher_exe()?,
            tandem_home: std::env::var_os("TANDEM_HOME").map(Into::into),
            logs: state.data.logs(),
        };
        l.install(&spec).map_err(|e| {
            IpcError::new(
                ErrorCode::Io,
                format!("Couldn't install the background runner: {e}"),
            )
        })
    } else {
        l.uninstall().map_err(|e| {
            IpcError::new(
                ErrorCode::Io,
                format!("Couldn't remove the background runner: {e}"),
            )
        })?;
        Ok(l.status())
    }
}

/// "Run even when Vibezeo is closed": installs or removes the OS entry at
/// once (F-RT-03 AC1).
#[tauri::command]
pub async fn routines_background_set(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<LauncherStatus, IpcError> {
    let status = set_background(&state, enabled)?;
    crate::tray::sync(&app, status.installed).await;
    Ok(status)
}

#[tauri::command]
pub async fn routines_background_status() -> Result<LauncherStatus, IpcError> {
    tokio::task::spawn_blocking(|| launcher().status())
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))
}

/// Whether the background runner is installed (the tray shows then).
pub async fn background_on() -> bool {
    routines_background_status()
        .await
        .map(|s| s.installed)
        .unwrap_or(false)
}
