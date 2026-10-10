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
    state: State<'_, AppState>,
    input: RoutineInput,
) -> Result<Routine, IpcError> {
    let background = input.run_mode == tandem_routines::RunMode::Background;
    let saved = state.routines.save(input, None).await?;
    // A routine that runs while Vibezeo is closed needs the launcher.
    if background && !launcher().status().installed {
        if let Err(e) = set_background(&state, true) {
            tracing::warn!(code = "routines.launcher_failed", error = %e.message, "couldn't install the background runner");
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
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Ok(appimage.into());
    }
    std::env::current_exe().map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))
}

fn set_background(state: &AppState, enabled: bool) -> Result<LauncherStatus, IpcError> {
    let l = launcher();
    if enabled {
        if !state.entitlements.limits().background_routines {
            return Err(IpcError::new(
                ErrorCode::ForbiddenPlan,
                "Background routines are part of Pro.",
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
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<LauncherStatus, IpcError> {
    set_background(&state, enabled)
}

#[tauri::command]
pub async fn routines_background_status() -> Result<LauncherStatus, IpcError> {
    tokio::task::spawn_blocking(|| launcher().status())
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))
}
