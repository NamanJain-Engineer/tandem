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
use tandem_pty::orphans::{self, OrphanInfo, SystemTable};
use tandem_pty::{ExitInfo, OutputSink, PtyFrame, PtyInfo, PtyState, SinkClosed, TermColors};
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
    engine_id: Option<String>,
    account_id: Option<tandem_core::ids::AccountId>,
    url: Option<String>,
) -> Result<PaneRecord, IpcError> {
    // A browser pane runs no process: the live-pane limit doesn't count it.
    if kind == "browser" {
        return create_browser_pane(&state, &tab_id, url).await;
    }
    state
        .terminals
        .check_live_limit(state.entitlements.limits().panes)?;
    create_pane(&state, &tab_id, &kind, cwd, engine_id, account_id).await
}

/// A browser pane (F-CD-17). `url`, read like the address bar, is its first
/// page; without one it starts empty.
pub(crate) async fn create_browser_pane(
    state: &AppState,
    tab_id: &TabId,
    url: Option<String>,
) -> Result<PaneRecord, IpcError> {
    let url = match url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
        Some(input) => Some(crate::commands::browser::resolve(state, input).await?),
        None => None,
    };
    let mut pane = panes::create(&state.store, tab_id, "browser", None).await?;
    if let Some(url) = url {
        panes::set_browser_url(&state.store, &pane.id, Some(url.clone())).await?;
        pane.browser_url = Some(url);
    }
    Ok(pane)
}

/// A pane record in `tab` (the view starts its process). Callers check the
/// plan's live-pane cap first: one pane (`pane_create`) or a whole lineup.
pub(crate) async fn create_pane(
    state: &AppState,
    tab_id: &TabId,
    kind: &str,
    cwd: Option<String>,
    engine_id: Option<String>,
    account_id: Option<tandem_core::ids::AccountId>,
) -> Result<PaneRecord, IpcError> {
    match kind {
        "terminal" => {}
        "agent" => {
            let known = engine_id
                .as_deref()
                .is_some_and(|id| state.terminals.engines.get(id).is_some());
            if !known {
                return Err(IpcError::new(ErrorCode::NotFound, "No engine with that id"));
            }
        }
        "browser" => return create_browser_pane(state, tab_id, None).await,
        _ => {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Simulator panes aren't available yet",
            ))
        }
    }
    let engine_id = engine_id.filter(|_| kind == "agent");
    // An account must belong to the pane's engine (F-AC-01).
    if let Some(account) = &account_id {
        let a = tandem_store::repo::accounts::get(&state.store, account).await?;
        if Some(a.engine_id.as_str()) != engine_id.as_deref() {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "That account is for another engine",
            ));
        }
    }
    let mut pane = panes::create_with(&state.store, tab_id, kind, cwd, engine_id.clone()).await?;
    // An agent pane is pinned to the account it starts on (F-AC-03).
    let engine = engine_id
        .as_deref()
        .and_then(|id| state.terminals.engines.get(id));
    let account_id = match &engine {
        Some(engine) => {
            state
                .terminals
                .engines
                .account_for_new_session(engine, &pane.workspace_id, account_id.as_ref())
                .await?
        }
        None => account_id,
    };
    if let Some(account) = account_id {
        panes::set_account(&state.store, &pane.id, &account).await?;
        pane.account_id = Some(account);
    }
    // Agent panes get a first name, unique in the workspace (F-CD-12).
    if kind == "agent" {
        let name = panes::assign_human_name(
            &state.store,
            &pane.id,
            crate::services::names::names(),
            |n| rand::random::<u32>() as usize % n.max(1),
        )
        .await?;
        pane.human_name = Some(name);
    }
    Ok(pane)
}

/// Renames a pane (F-CD-12). Conflict when another open pane of the
/// workspace already has the name.
#[tauri::command]
pub async fn pane_rename(
    state: State<'_, AppState>,
    pane_id: PaneId,
    human_name: String,
) -> Result<String, IpcError> {
    Ok(panes::rename(&state.store, &pane_id, &human_name).await?)
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
    close_pane(&state, &pane_id).await
}

/// Ends a pane's process tree, its status record and any approval it asked
/// for, then closes it in the store.
pub async fn close_pane(state: &AppState, pane_id: &PaneId) -> Result<(), IpcError> {
    // A queued restore never starts once its pane is gone (F-SH-07).
    state.restore.forget(pane_id);
    let _ = state.terminals.pty.kill(pane_id.as_str());
    state.terminals.status.unwatch(pane_id.as_str());
    // A browser pane's page goes with it (one moved to the sidebar has
    // another id by now).
    if let Some(browser) = state.browser.get() {
        browser.close(pane_id.as_str());
    }
    state
        .approvals
        .broker
        .cancel_source(&tandem_core::source::SourceRef::Pane {
            pane_id: pane_id.as_str().to_string(),
        });
    Ok(panes::close(&state.store, pane_id).await?)
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
    /// Modes and title as of the attach (later changes arrive as `pty:state`).
    pub state: Option<PtyState>,
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
        state: pty.state(pane_id.as_str()),
    })
}

#[tauri::command]
pub fn pty_write(
    state: State<'_, AppState>,
    pane_id: PaneId,
    data: String,
) -> Result<(), IpcError> {
    // The echo of typing is never the agent working (F-EN-05 AC3).
    state.terminals.status.quiet_window(pane_id.as_str());
    Ok(state
        .terminals
        .pty
        .write(pane_id.as_str(), data.as_bytes())?)
}

/// Types a prompt into an agent's TUI once it's ready (F-EN-06): one paste,
/// then Enter as its own write. Refused while the agent waits for approval.
#[tauri::command]
pub async fn pty_type_prompt(
    state: State<'_, AppState>,
    pane_id: PaneId,
    text: String,
) -> Result<(), IpcError> {
    state.terminals.type_prompt(&pane_id, &text).await
}

/// Every known agent status (rail, pane headers; F-EN-05).
#[tauri::command]
pub fn status_list(state: State<'_, AppState>) -> Vec<tandem_core::status::StatusRecord> {
    state.terminals.status.snapshot()
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
    crate::commands::app::count_resize(pane_id.as_str());
    // A resize's repaint is never the agent working (F-EN-05 AC3).
    state.terminals.status.quiet_window(pane_id.as_str());
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

/// The last finished command's plain-text output in a pane, from its shell
/// integration marks ("Copy last command output", F-CD-10 AC3).
#[tauri::command]
pub fn term_last_output(
    state: State<'_, AppState>,
    pane_id: tandem_core::ids::PaneId,
) -> Option<tandem_pty::marks::LastOutput> {
    state
        .terminals
        .pty
        .last_output(pane_id.as_str())
        .map(|(text, exit_code)| tandem_pty::marks::LastOutput { text, exit_code })
}

/// Processes a crashed run left behind (UI-132).
#[tauri::command]
pub async fn orphans_list(state: State<'_, AppState>) -> Result<Vec<OrphanInfo>, IpcError> {
    Ok(state.orphans.list().await)
}

/// "Stop them" ends every listed process (terminate, then kill after 2 s);
/// "Keep them" forgets the list.
#[tauri::command]
pub async fn orphans_resolve(state: State<'_, AppState>, action: String) -> Result<(), IpcError> {
    if action != "stop" && action != "keep" {
        return Err(IpcError::new(ErrorCode::Invalid, "Choose stop or keep"));
    }
    let list = state.orphans.take().await;
    if action == "stop" && !list.is_empty() {
        tracing::info!(
            code = "orphans.stop",
            count = list.len(),
            "stopping orphaned processes"
        );
        tokio::task::spawn_blocking(move || {
            orphans::stop(
                &list,
                &mut SystemTable::default(),
                std::time::Duration::from_secs(2),
            );
        })
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    }
    Ok(())
}

/// A workspace's Saved sessions, newest first (F-CD-03).
#[tauri::command]
pub async fn sessions_saved_list(
    state: State<'_, AppState>,
    workspace_id: tandem_core::ids::WorkspaceId,
) -> Result<Vec<tandem_store::repo::sessions::SavedSession>, IpcError> {
    Ok(tandem_store::repo::sessions::saved_list(&state.store, &workspace_id).await?)
}

/// Shell-quoted paths for a drop into this pane (F-CD-06).
#[tauri::command]
pub fn term_quote_paths(
    state: State<'_, AppState>,
    pane_id: PaneId,
    paths: Vec<String>,
) -> Result<String, IpcError> {
    state.terminals.quote_paths(&pane_id, &paths)
}

/// What a link a pane printed opens (F-CD-09): a URL, or a file that exists,
/// with relative paths taken from the pane's folder (its shell's last OSC 7,
/// so they follow `cd`), read-only outside every workspace.
#[tauri::command]
pub async fn pane_resolve_link(
    state: State<'_, AppState>,
    pane_id: PaneId,
    text: String,
) -> Result<Option<tandem_core::files::links::ResolvedLink>, IpcError> {
    let cwd = state.terminals.link_cwd(&pane_id).await;
    let roots = state.files.workspace_roots().await?;
    tokio::task::spawn_blocking(move || {
        tandem_core::files::links::resolve(&text, cwd.as_deref(), &roots)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))
}

/// Saves the clipboard image as a PNG under `$DATA/staging/<pane>/` and
/// returns its quoted path, for an agent to read (F-CD-05 AC3).
#[tauri::command]
pub async fn term_stage_clipboard_image(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    pane_id: PaneId,
) -> Result<String, IpcError> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let image = app
        .clipboard()
        .read_image()
        .map_err(|_| IpcError::new(ErrorCode::NotFound, "There's no image on the clipboard"))?;
    let (width, height, rgba) = (image.width(), image.height(), image.rgba().to_vec());
    let dir = state.data.staging().join(pane_id.as_str());
    let path = tokio::task::spawn_blocking(move || {
        crate::services::staging::write_png(&dir, width, height, &rgba)
    })
    .await
    .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))??;
    state
        .terminals
        .quote_paths(&pane_id, &[path.to_string_lossy().into_owned()])
}
