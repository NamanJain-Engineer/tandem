//! Panes and PTYs (ARCHITECTURE §3.2). Output streams as binary frames over
//! a per-pane `Channel` (§3.4).

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;
use ts_rs::TS;

use tandem_core::ids::{PaneId, TabId};
use tandem_core::{ErrorCode, IpcError};
use tandem_pty::{ExitInfo, OutputSink, PtyFrame, PtyInfo, SinkClosed, TermColors};
use tandem_store::repo::panes::{self, PaneRecord};

use crate::state::AppState;

struct ChannelSink(Channel<InvokeResponseBody>);

impl OutputSink for ChannelSink {
    fn send(&self, frame: PtyFrame) -> Result<(), SinkClosed> {
        self.0
            .send(InvokeResponseBody::Raw(frame.encode()))
            .map_err(|_| SinkClosed)
    }
}

#[tauri::command]
pub async fn pane_create(
    state: State<'_, AppState>,
    tab_id: TabId,
    kind: String,
    cwd: Option<String>,
) -> Result<PaneRecord, IpcError> {
    if kind != "terminal" {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Only terminal panes are available yet",
        ));
    }
    state
        .terminals
        .check_live_limit(state.entitlements.limits().panes)?;
    Ok(panes::create(&state.store, &tab_id, &kind, cwd).await?)
}

#[tauri::command]
pub async fn pane_list(
    state: State<'_, AppState>,
    tab_id: TabId,
) -> Result<Vec<PaneRecord>, IpcError> {
    Ok(panes::list_open(&state.store, &tab_id).await?)
}

#[tauri::command]
pub async fn pane_close(state: State<'_, AppState>, pane_id: PaneId) -> Result<(), IpcError> {
    let _ = state.terminals.pty.kill(pane_id.as_str());
    Ok(panes::close(&state.store, &pane_id).await?)
}

/// Moves a pane to another tab; its process keeps running (F-CD-02).
#[tauri::command]
pub async fn pane_move(
    state: State<'_, AppState>,
    pane_id: PaneId,
    tab_id: TabId,
) -> Result<PaneRecord, IpcError> {
    Ok(panes::move_to_tab(&state.store, &pane_id, &tab_id).await?)
}

#[tauri::command]
pub async fn pty_spawn(
    state: State<'_, AppState>,
    pane_id: PaneId,
    cols: u16,
    rows: u16,
    channel: Channel<InvokeResponseBody>,
) -> Result<PtyInfo, IpcError> {
    let max_live = state.entitlements.limits().panes;
    state
        .terminals
        .spawn(
            &pane_id,
            cols,
            rows,
            max_live,
            Arc::new(ChannelSink(channel)),
        )
        .await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AttachSnapshot {
    /// Ring contents, base64.
    pub replay: String,
    pub live: bool,
}

/// Re-attaches after a webview reload: returns the scrollback to replay.
#[tauri::command]
pub fn pty_attach(
    state: State<'_, AppState>,
    pane_id: PaneId,
    channel: Channel<InvokeResponseBody>,
) -> Result<AttachSnapshot, IpcError> {
    let pty = &state.terminals.pty;
    let replay = pty.attach(pane_id.as_str(), Arc::new(ChannelSink(channel)))?;
    Ok(AttachSnapshot {
        replay: STANDARD.encode(replay),
        live: pty.is_live(pane_id.as_str()),
    })
}

#[tauri::command]
pub fn pty_write(
    state: State<'_, AppState>,
    pane_id: PaneId,
    data: String,
) -> Result<(), IpcError> {
    Ok(state
        .terminals
        .pty
        .write(pane_id.as_str(), data.as_bytes())?)
}

#[tauri::command]
pub fn pty_ack(state: State<'_, AppState>, pane_id: PaneId, bytes: u64) {
    state.terminals.pty.ack(pane_id.as_str(), bytes);
}

#[tauri::command]
pub fn pty_resize(
    state: State<'_, AppState>,
    pane_id: PaneId,
    cols: u16,
    rows: u16,
) -> Result<(), IpcError> {
    crate::commands::app::count_resize();
    Ok(state.terminals.pty.resize(pane_id.as_str(), cols, rows)?)
}

#[tauri::command]
pub fn pty_kill(state: State<'_, AppState>, pane_id: PaneId) -> Result<ExitInfo, IpcError> {
    Ok(state.terminals.pty.kill(pane_id.as_str())?)
}

/// Theme colours for OSC 10/11/12 answers (F-SH-05 AC2).
#[tauri::command]
pub fn term_set_theme_colors(state: State<'_, AppState>, colors: TermColors) {
    state.terminals.pty.set_theme_colors(colors);
}
