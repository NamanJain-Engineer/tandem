//! Shell-level UI: the right sidebar (F-SH-10) and focusing a source
//! (F-DB-01, F-DB-02, F-CD-12).

use tauri::{AppHandle, State};

use tandem_core::approvals::{ApprovalDecision, ApprovalVia, ApprovalView};
use tandem_core::bus::{BusEvent, Topic};
use tandem_core::ids::ApprovalId;
use tandem_core::status::{AgentStatus, SessionKey};

use crate::services::dashboard::{self, DashboardItem};
use tandem_core::ids::PaneId;
use tandem_core::source::SourceRef;
use tandem_core::IpcError;
use tandem_store::repo::panes;
use tandem_store::repo::sidebar::{self, SidebarState};

use crate::state::AppState;

#[tauri::command]
pub async fn sidebar_get(state: State<'_, AppState>) -> Result<SidebarState, IpcError> {
    Ok(sidebar::get(&state.store).await?)
}

/// Saves the sidebar's tabs, order, active tab, width and full-screen.
/// Returns what was stored (width clamped, stale active tab repaired).
#[tauri::command]
pub async fn sidebar_save(
    state: State<'_, AppState>,
    sidebar: SidebarState,
) -> Result<SidebarState, IpcError> {
    Ok(sidebar::save(&state.store, sidebar).await?)
}

/// Brings the window forward and asks the webview to show and flash a
/// source (`ui:focus`): from a Dashboard card, a notification or an OS
/// notification click. A pane that's gone is NotFound.
#[tauri::command]
pub async fn focus_source(
    app: AppHandle,
    state: State<'_, AppState>,
    source: SourceRef,
) -> Result<(), IpcError> {
    focus(&app, &state, source).await
}

pub async fn focus(app: &AppHandle, state: &AppState, source: SourceRef) -> Result<(), IpcError> {
    // Where a pane lives, so the webview can open its workspace and tab
    // before that workspace's layout has loaded.
    let location = match &source {
        SourceRef::Pane { pane_id } => {
            let pane = panes::get_open(&state.store, &PaneId::from_db(pane_id.clone())).await?;
            Some(serde_json::json!({ "workspaceId": pane.workspace_id, "tabId": pane.tab_id }))
        }
        _ => None,
    };
    crate::focus_main(app);
    state.bus.publish(BusEvent::new(
        Topic::App,
        "ui:focus",
        serde_json::json!({ "source": source, "flash": true, "location": location }),
    ));
    Ok(())
}

/// Pending approvals, oldest first (Dashboard cards, inline cards).
#[tauri::command]
pub fn approvals_pending(state: State<'_, AppState>) -> Vec<ApprovalView> {
    state.approvals.broker.pending()
}

/// Answers an approval through the broker (F-TH-11, F-DB-01 AC3). One
/// answered elsewhere already is a Conflict.
#[tauri::command]
pub fn approval_resolve(
    state: State<'_, AppState>,
    approval_id: ApprovalId,
    decision: ApprovalDecision,
    via: ApprovalVia,
) -> Result<(), IpcError> {
    state
        .approvals
        .broker
        .resolve(&approval_id, decision, via)?;
    Ok(())
}

/// Every agent card (F-DB-01).
#[tauri::command]
pub async fn status_snapshot(state: State<'_, AppState>) -> Result<Vec<DashboardItem>, IpcError> {
    Ok(dashboard::snapshot(&state).await)
}

/// "Mark as seen": a done agent becomes idle.
#[tauri::command]
pub fn status_mark_seen(state: State<'_, AppState>, key: SessionKey) {
    let status = &state.terminals.status;
    let Some(rec) = status.store.get(&key) else {
        return;
    };
    if rec.status != AgentStatus::Done {
        return;
    }
    status.set_idle(&key);
}

#[derive(Debug, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CloseIdleResult {
    pub closed: Vec<SessionKey>,
}

/// Close idle (F-DB-01 AC2): closes the given agents that are idle *now*;
/// working, waiting and done ones stay, whatever the webview thought.
#[tauri::command]
pub async fn dashboard_close_idle(
    state: State<'_, AppState>,
    keys: Vec<SessionKey>,
) -> Result<CloseIdleResult, IpcError> {
    let mut closed = Vec::new();
    for key in keys {
        let idle = state
            .terminals
            .status
            .store
            .get(&key)
            .is_some_and(|r| r.status == AgentStatus::Idle);
        if !idle {
            continue;
        }
        if let Some(pane) = key.0.strip_prefix("pane:") {
            let pane = PaneId::from_db(pane.to_string());
            if crate::commands::terminal::close_pane(&state, &pane)
                .await
                .is_ok()
            {
                closed.push(key);
            }
        } else if state.terminals.status.external(&key).is_some() {
            state.terminals.status.remove_external(&key);
            closed.push(key);
        }
    }
    Ok(CloseIdleResult { closed })
}
