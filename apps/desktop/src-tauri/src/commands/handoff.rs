//! Handoff between engines (ARCHITECTURE §3.2 "Launch, handoff, lanes,
//! git", F-CD-16).

use tauri::State;

use tandem_core::ids::{AccountId, PaneId};
use tandem_core::IpcError;

use crate::services::handoff::{self, HandoffBrief, HandoffPlace, HandoffResult};
use crate::state::AppState;

/// Writes the brief for handing `pane_id` to `engine_id`: the source's own,
/// the target's headless one, or the fallback.
#[tauri::command]
pub async fn handoff_brief(
    state: State<'_, AppState>,
    pane_id: PaneId,
    engine_id: String,
    account_id: Option<AccountId>,
) -> Result<HandoffBrief, IpcError> {
    handoff::brief(&state, &pane_id, &engine_id, account_id.as_ref()).await
}

/// Opens the target beside the source (or in a new worktree) with `brief`.
#[tauri::command]
pub async fn handoff_start(
    state: State<'_, AppState>,
    pane_id: PaneId,
    engine_id: String,
    account_id: Option<AccountId>,
    brief: String,
    place: HandoffPlace,
    close_source: bool,
) -> Result<HandoffResult, IpcError> {
    handoff::start(
        &state,
        &pane_id,
        &engine_id,
        account_id,
        &brief,
        place,
        close_source,
    )
    .await
}
