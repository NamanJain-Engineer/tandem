//! The Night Shift (F-RT-06, UI-171): "Run at reset…" on a task.

use tandem_core::{ErrorCode, IpcError};
use tauri::State;

use crate::services::night_shift::{self, NightAccount, NightInput, NightRun, NightState};
use crate::state::AppState;

/// Every account's next reset, for the dialog.
#[tauri::command]
pub async fn night_accounts(state: State<'_, AppState>) -> Result<Vec<NightAccount>, IpcError> {
    Ok(night_shift::accounts(&state).await)
}

/// The runs that are scheduled, waiting, started or ended.
#[tauri::command]
pub fn night_list(state: State<'_, AppState>) -> Vec<NightRun> {
    state.night_shift.list()
}

/// Saves a run the user approved: it starts at the account's next reset,
/// on that account only.
#[tauri::command]
pub async fn night_schedule(
    state: State<'_, AppState>,
    input: NightInput,
) -> Result<NightRun, IpcError> {
    if !state.auth.is_signed_in() {
        return Err(IpcError::new(
            ErrorCode::Unauthenticated,
            "Sign in to run tasks. Your tasks stay available offline.",
        ));
    }
    if !state.entitlements.limits().task_execution {
        return Err(IpcError::new(
            ErrorCode::ForbiddenPlan,
            "Running tasks is part of Pro. You can still track tasks for free.",
        ));
    }
    if input.access == tandem_core::access::Access::Full && !input.full_confirmed {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Full access must be chosen for this task.",
        ));
    }
    let account = night_shift::accounts(&state)
        .await
        .into_iter()
        .find(|a| a.account_id == input.account_id)
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That account doesn't exist"))?;
    let (Some(run_at), Some(window)) = (account.resets_at, account.window) else {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "No reset time is known for that account yet.",
        ));
    };
    let title = state
        .tasks
        .tracker
        .as_ref()
        .and_then(|t| t.get(&input.issue_id).ok())
        .map(|i| i.title)
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That task doesn't exist"))?;
    let run = NightRun {
        id: format!("nr_{}", ulid::Ulid::new()),
        issue_id: input.issue_id,
        title,
        workspace_id: input.workspace_id,
        account_id: input.account_id,
        engine_id: account.engine_id,
        isolation: input.isolation,
        access: input.access,
        full_confirmed: input.full_confirmed,
        review: input.review,
        run_at,
        window,
        state: NightState::Scheduled,
        attempt_id: None,
        reason: None,
        created_at: state.clock.now_ms(),
        ended_at: None,
    };
    Ok(state.night_shift.add(run))
}

/// Cancel a run before it starts.
#[tauri::command]
pub fn night_cancel(state: State<'_, AppState>, id: String) -> Result<(), IpcError> {
    state.night_shift.cancel(&id)
}
