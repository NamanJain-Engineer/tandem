//! The Dashboard's cards (F-DB-01): every agent the status bus knows, with
//! what a card shows (engine, workspace, branch, account, a redacted
//! preview and any pending approval).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::approvals::ApprovalView;
use tandem_core::ids::{PaneId, WorkspaceId};
use tandem_core::redact::Redactor;
use tandem_core::source::SourceRef;
use tandem_core::status::{AgentStatus, SessionKey, StatusRecord, WaitingReason};
use tandem_store::repo::{accounts, panes, workspaces};

use crate::services::workspaces::git_branch;
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DashboardItem {
    pub key: SessionKey,
    pub source: SourceRef,
    pub status: AgentStatus,
    pub reason: Option<WaitingReason>,
    pub since: i64,
    /// The work title (hook or terminal title); the webview cleans it.
    pub title: Option<String>,
    pub human_name: Option<String>,
    pub engine_id: Option<String>,
    pub engine_label: Option<String>,
    pub engine_logo: Option<String>,
    pub account_label: Option<String>,
    pub workspace_id: Option<String>,
    pub workspace: Option<String>,
    pub branch: Option<String>,
    /// The latest activity line, through the secret redactor.
    pub preview: Option<String>,
    pub pending_approval: Option<ApprovalView>,
}

fn redact(s: Option<String>) -> Option<String> {
    s.map(|p| Redactor::global().redact(&p).into_owned())
        .filter(|p| !p.trim().is_empty())
}

/// One card per live status record whose source still exists.
pub async fn snapshot(state: &AppState) -> Vec<DashboardItem> {
    let mut items = Vec::new();
    for rec in state.terminals.status.snapshot() {
        if let Some(item) = item_for(state, rec).await {
            items.push(item);
        }
    }
    items.sort_by(|a, b| b.since.cmp(&a.since).then_with(|| a.key.0.cmp(&b.key.0)));
    items
}

async fn item_for(state: &AppState, rec: StatusRecord) -> Option<DashboardItem> {
    let engines = &state.terminals.engines;
    if let Some(pane_id) = rec.key.0.strip_prefix("pane:") {
        let pane = panes::get_open(&state.store, &PaneId::from_db(pane_id.to_string()))
            .await
            .ok()?;
        let ws = workspaces::get(&state.store, &pane.workspace_id).await.ok();
        let spec = pane.engine_id.as_deref().and_then(|id| engines.get(id));
        let account = match &pane.account_id {
            Some(id) => accounts::get(&state.store, id).await.ok(),
            None => match &pane.engine_id {
                Some(engine) => accounts::default_for(&state.store, engine)
                    .await
                    .ok()
                    .flatten(),
                None => None,
            },
        };
        let title = rec
            .title
            .clone()
            .or_else(|| state.terminals.pty.state(pane_id).and_then(|s| s.title));
        let source = SourceRef::Pane {
            pane_id: pane_id.to_string(),
        };
        return Some(DashboardItem {
            key: rec.key.clone(),
            pending_approval: state.approvals.broker.pending_for(&source),
            source,
            status: rec.status,
            reason: rec.reason,
            since: rec.since,
            title,
            human_name: pane.human_name,
            engine_label: spec.as_ref().map(|s| s.label.clone()),
            engine_logo: spec.as_ref().map(|s| s.logo.clone()),
            engine_id: pane.engine_id,
            account_label: account.map(|a| a.label),
            workspace_id: Some(pane.workspace_id.as_str().to_string()),
            branch: ws
                .as_ref()
                .and_then(|w| git_branch(std::path::Path::new(&w.root))),
            workspace: ws.map(|w| w.name),
            preview: redact(rec.preview.or(rec.tool)),
        });
    }
    let meta = state.terminals.status.external(&rec.key)?;
    let spec = meta.engine_id.as_deref().and_then(|id| engines.get(id));
    let ws = match &meta.workspace_id {
        Some(id) => workspaces::get(&state.store, &WorkspaceId::from_db(id.clone()))
            .await
            .ok(),
        None => None,
    };
    Some(DashboardItem {
        key: rec.key.clone(),
        pending_approval: state.approvals.broker.pending_for(&meta.source),
        source: meta.source,
        status: rec.status,
        reason: rec.reason,
        since: rec.since,
        title: rec.title.or(Some(meta.title)),
        human_name: None,
        engine_label: spec.as_ref().map(|s| s.label.clone()),
        engine_logo: spec.as_ref().map(|s| s.logo.clone()),
        engine_id: meta.engine_id,
        account_label: None,
        workspace_id: meta.workspace_id,
        branch: ws
            .as_ref()
            .and_then(|w| git_branch(std::path::Path::new(&w.root))),
        workspace: ws.map(|w| w.name),
        preview: redact(rec.preview.or(rec.tool)),
    })
}
