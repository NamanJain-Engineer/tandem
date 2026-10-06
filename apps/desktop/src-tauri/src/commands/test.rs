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

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FixtureAgent {
    pub id: String,
    pub title: String,
    pub status: tandem_core::status::AgentStatus,
    pub engine_id: Option<String>,
    pub workspace_id: Option<String>,
    pub preview: Option<String>,
    /// Asks for approval with this summary (the agent waits until answered).
    pub approval: Option<String>,
}

/// A scripted agent for the Dashboard and notification E2E (the "fixture
/// source"): it shows in the status bus like any agent; with `approval` it
/// waits on the broker, and once allowed it goes back to work.
#[tauri::command]
pub async fn test_fixture_agent(
    state: State<'_, AppState>,
    agent: FixtureAgent,
) -> Result<Option<tandem_core::approvals::ApprovalView>, IpcError> {
    use tandem_core::approvals::{ApprovalKind, ApprovalRequest, ApprovalScope};
    use tandem_core::source::SourceRef;
    use tandem_core::status::{AgentStatus, StatusUpdate, WaitingReason};
    guard(&state)?;
    let source = SourceRef::Fixture {
        fixture_id: agent.id.clone(),
    };
    let meta = crate::services::status::ExternalMeta {
        source: source.clone(),
        title: agent.title.clone(),
        engine_id: agent.engine_id.clone(),
        workspace_id: agent.workspace_id.clone(),
    };
    let status = state.terminals.status.clone();
    let update = |status: AgentStatus, reason: Option<WaitingReason>, preview: Option<String>| {
        StatusUpdate {
            status,
            reason,
            ts: 0,
            tool: None,
            title: Some(agent.title.clone()),
            preview,
        }
    };
    let Some(summary) = agent.approval.clone() else {
        status.set_external(meta, update(agent.status, None, agent.preview.clone()));
        return Ok(None);
    };
    status.set_external(
        meta.clone(),
        update(
            AgentStatus::Waiting,
            Some(WaitingReason::Approval),
            Some(format!("Wants to run: {summary}")),
        ),
    );
    let (view, ticket) = state.approvals.broker.request(ApprovalRequest {
        source,
        kind: ApprovalKind::Command,
        summary: format!("Run `{summary}`"),
        detail: summary.clone(),
        options: vec![ApprovalScope::Once],
    });
    let title = agent.title.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = ticket.outcome().await;
        let (next, preview) = if outcome.allowed() {
            (AgentStatus::Working, format!("Running {summary}"))
        } else {
            (AgentStatus::Idle, "Denied".to_string())
        };
        status.set_external(
            meta,
            StatusUpdate {
                status: next,
                reason: None,
                ts: 0,
                tool: None,
                title: Some(title),
                preview: Some(preview),
            },
        );
    });
    Ok(Some(view))
}

/// What the recording notifier was asked to show (F-DB-03 AC1).
#[tauri::command]
pub fn test_notifier_calls(
    state: State<'_, AppState>,
    clear: Option<bool>,
) -> Result<Vec<crate::services::notifier::OsNotification>, IpcError> {
    guard(&state)?;
    let rec = state
        .notifications
        .recording
        .get()
        .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "No recording notifier"))?;
    let calls = rec.calls();
    if clear.unwrap_or(false) {
        rec.clear();
    }
    Ok(calls)
}

/// Clicks a recorded OS notification: focuses the app and its source.
#[tauri::command]
pub async fn test_notifier_click(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    index: usize,
) -> Result<(), IpcError> {
    guard(&state)?;
    let source = state
        .notifications
        .recording
        .get()
        .and_then(|r| r.calls().get(index).map(|c| c.source.clone()))
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "No such notification"))?;
    crate::services::notifications::NotificationService::click(&app, source).await;
    Ok(())
}

/// Pretends the window is focused or not (`None`: the real state).
#[tauri::command]
pub fn test_set_window_focused(
    state: State<'_, AppState>,
    focused: Option<bool>,
) -> Result<(), IpcError> {
    guard(&state)?;
    state.notifications.override_focus(focused);
    Ok(())
}
