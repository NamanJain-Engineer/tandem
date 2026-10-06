//! The E2E test harness (ARCHITECTURE §3.2): debug builds only, and only
//! when `TANDEM_E2E=1`. Release builds don't compile this module.

use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

use tandem_core::{ErrorCode, IpcError};

use crate::services::auth::AuthState;
use crate::services::terminal::SpawnRecord;
use crate::state::AppState;

fn guard(state: &AppState) -> Result<(), IpcError> {
    if state.e2e {
        Ok(())
    } else {
        Err(IpcError::new(
            ErrorCode::PermissionDenied,
            "The test harness is off",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TestCounters {
    pub pty_resizes: u64,
    /// `pty_resize` calls per pane.
    pub pty_resizes_by_pane: std::collections::BTreeMap<String, u32>,
    pub webgl_contexts: u64,
    pub dom_renderers: u64,
    pub reflows: u64,
}

#[tauri::command]
pub fn test_counters(
    state: State<'_, AppState>,
    reset: Option<bool>,
) -> Result<TestCounters, IpcError> {
    guard(&state)?;
    let reset = reset.unwrap_or(false);
    Ok(TestCounters {
        pty_resizes_by_pane: crate::commands::app::resize_counts_by_pane(reset),
        pty_resizes: crate::commands::app::resize_count(reset),
        webgl_contexts: 0,
        dom_renderers: 0,
        reflows: 0,
    })
}

#[tauri::command]
pub fn test_spawn_log(state: State<'_, AppState>) -> Result<Vec<SpawnRecord>, IpcError> {
    guard(&state)?;
    Ok(state.terminals.spawn_records())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ClockNow {
    pub now: i64,
}

#[tauri::command]
pub fn test_clock_advance(state: State<'_, AppState>, ms: u64) -> Result<ClockNow, IpcError> {
    guard(&state)?;
    let clock = state.mock_clock.as_ref().ok_or_else(|| {
        IpcError::new(
            ErrorCode::Unavailable,
            "Start with TANDEM_FAKE_CLOCK to move time",
        )
    })?;
    clock.advance(std::time::Duration::from_millis(ms));
    Ok(ClockNow {
        now: state.clock.now_ms(),
    })
}

/// Forces the refresh path, e.g. to observe a revoked device (E2E-M2-6).
#[tauri::command]
pub async fn test_auth_refresh(state: State<'_, AppState>) -> Result<AuthState, IpcError> {
    guard(&state)?;
    let _ = state.auth.force_refresh().await;
    state.entitlements.refresh().await;
    Ok(state.auth.state())
}

/// A second account for an engine, with its own home (E2E-M4-4). The real
/// sign-in flow is `accounts_begin_add` (F-AC-02).
#[tauri::command]
pub async fn test_account_create(
    state: State<'_, AppState>,
    engine_id: String,
    label: String,
) -> Result<tandem_store::repo::accounts::Account, IpcError> {
    guard(&state)?;
    let engine = state
        .terminals
        .engines
        .get(&engine_id)
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "No engine with that id"))?;
    state
        .terminals
        .engines
        .create_account(&engine, &label)
        .await
}
