//! The resource governor (F-EX-06): what this system can do, each pane's
//! usage and chip, and Freeze/Thaw.

use tauri::State;

use tandem_core::ids::PaneId;
use tandem_core::{ErrorCode, IpcError};
use tandem_pty::governor::GovernorStatus;

use crate::services::resources::{PaneResources, PaneUsage};
use crate::state::AppState;

#[tauri::command]
pub async fn resources_status(state: State<'_, AppState>) -> Result<GovernorStatus, IpcError> {
    Ok(state.terminals.resources.status().await)
}

#[tauri::command]
pub async fn resources_list(state: State<'_, AppState>) -> Result<Vec<PaneUsage>, IpcError> {
    Ok(state.terminals.resources.list(&state.terminals.pty).await)
}

#[tauri::command]
pub fn resources_notes(state: State<'_, AppState>) -> Vec<PaneResources> {
    state.terminals.resources.notes()
}

#[tauri::command]
pub async fn resources_freeze(state: State<'_, AppState>, pane_id: PaneId) -> Result<(), IpcError> {
    state
        .terminals
        .resources
        .freeze(&state.terminals.pty, pane_id.as_str())
        .await
}

#[tauri::command]
pub async fn resources_thaw(state: State<'_, AppState>, pane_id: PaneId) -> Result<(), IpcError> {
    state.terminals.resources.thaw(pane_id.as_str()).await
}

/// `agent_memory_cap` other than `off` only where a cap can apply: the
/// error says why (F-EX-06.2).
pub fn may_cap(status: &GovernorStatus, value: &serde_json::Value) -> Result<(), IpcError> {
    if value.as_str().is_none_or(|v| v == "off") || status.capping {
        return Ok(());
    }
    Err(IpcError::new(
        ErrorCode::Invalid,
        status
            .reason
            .clone()
            .unwrap_or_else(|| tandem_pty::governor::NOT_YET.into()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn f_ex_06_2_a_cap_turns_on_only_where_it_applies() {
        let none = GovernorStatus {
            grouping: false,
            capping: false,
            freezing: true,
            reason: Some(tandem_pty::governor::NO_SESSION.into()),
        };
        let err = may_cap(&none, &json!("512")).expect_err("refused");
        assert_eq!(err.message, tandem_pty::governor::NO_SESSION);
        assert!(may_cap(&none, &json!("off")).is_ok());
        let yes = GovernorStatus {
            capping: true,
            grouping: true,
            reason: None,
            ..none
        };
        assert!(may_cap(&yes, &json!("512")).is_ok());
    }
}
