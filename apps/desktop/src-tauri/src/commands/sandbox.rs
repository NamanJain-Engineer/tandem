//! The agent sandbox (F-TL-11): whether it can run here, and each agent
//! pane's note.

use tauri::State;

use crate::services::terminal::PaneSandbox;
use crate::state::AppState;
use tandem_engines::sandbox::SandboxStatus;

/// Whether agent panes can be sandboxed on this computer, and if not, why.
#[tauri::command]
pub async fn sandbox_status(
    state: State<'_, AppState>,
) -> Result<SandboxStatus, tandem_core::IpcError> {
    Ok(state.terminals.sandboxer().await.status())
}

/// Each agent pane started while the setting was on: sandboxed, or why not.
#[tauri::command]
pub fn sandbox_panes(state: State<'_, AppState>) -> Vec<PaneSandbox> {
    state.terminals.sandbox_panes()
}

/// `agent_sandbox = "workspace"` only where the sandbox can run: the error
/// says why (F-TL-11.4).
pub fn may_turn_on(
    sandbox: &dyn tandem_engines::sandbox::Sandboxer,
    value: &serde_json::Value,
) -> Result<(), tandem_core::IpcError> {
    if value.as_str() != Some("workspace") {
        return Ok(());
    }
    let status = sandbox.status();
    if status.available {
        return Ok(());
    }
    Err(tandem_core::IpcError::new(
        tandem_core::ErrorCode::Invalid,
        status
            .reason
            .unwrap_or_else(|| tandem_engines::sandbox::NOT_YET.into()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tandem_engines::sandbox::NotYet;

    #[test]
    fn f_tl_11_4_the_setting_cant_turn_on_where_the_sandbox_cant_run() {
        let err = may_turn_on(&NotYet, &json!("workspace")).expect_err("refused");
        assert_eq!(err.message, "Not available on this system yet.");
        assert!(
            may_turn_on(&NotYet, &json!("off")).is_ok(),
            "off always works"
        );
    }
}
