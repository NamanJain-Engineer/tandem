//! Engine accounts (ARCHITECTURE §3.2 "Engines and accounts"; F-AC-02,
//! F-AC-03, UI-35). `accounts_list` lives with the engines.

use tauri::State;

use tandem_core::ids::{AccountId, WorkspaceId};
use tandem_core::IpcError;
use tandem_store::repo::accounts::{self, Account, AccountOverride};

use crate::services::accounts::{self as service, LoginAction, LoginSession, SignInState};
use crate::state::AppState;

/// Starts a new account's sign-in in a mini terminal (`pty_attach` its
/// `paneId`); `accounts:login-finished` says how it ended.
#[tauri::command]
pub async fn accounts_begin_add(
    state: State<'_, AppState>,
    engine_id: String,
    label: String,
) -> Result<LoginSession, IpcError> {
    service::begin_add(&state, &engine_id, &label).await
}

/// Signs an existing account in again, or out.
#[tauri::command]
pub async fn accounts_begin_session(
    state: State<'_, AppState>,
    account_id: AccountId,
    action: LoginAction,
) -> Result<LoginSession, IpcError> {
    service::begin_session(&state, &account_id, action).await
}

/// "Done" for engines that can't report their sign-in.
#[tauri::command]
pub async fn accounts_finish_login(
    state: State<'_, AppState>,
    op_id: String,
) -> Result<(), IpcError> {
    service::finish_manually(&state, &op_id).await
}

/// Cancel: kills the login process; a new account is never created.
#[tauri::command]
pub async fn accounts_cancel_login(
    state: State<'_, AppState>,
    op_id: String,
) -> Result<(), IpcError> {
    service::cancel(&state, &op_id).await
}

#[tauri::command]
pub async fn accounts_update(
    state: State<'_, AppState>,
    account_id: AccountId,
    label: String,
) -> Result<Account, IpcError> {
    Ok(accounts::rename(&state.store, &account_id, &label).await?)
}

#[tauri::command]
pub async fn accounts_set_default(
    state: State<'_, AppState>,
    account_id: AccountId,
) -> Result<(), IpcError> {
    Ok(accounts::set_default(&state.store, &account_id).await?)
}

/// Removes the account and deletes its provider home.
#[tauri::command]
pub async fn accounts_remove(
    state: State<'_, AppState>,
    account_id: AccountId,
) -> Result<(), IpcError> {
    service::remove(&state, &account_id).await
}

#[tauri::command]
pub async fn accounts_status(
    state: State<'_, AppState>,
    account_id: AccountId,
) -> Result<SignInState, IpcError> {
    service::status(&state, &account_id).await
}

#[tauri::command]
pub async fn accounts_overrides(
    state: State<'_, AppState>,
) -> Result<Vec<AccountOverride>, IpcError> {
    Ok(accounts::overrides(&state.store).await?)
}

/// The account new `engine_id` sessions in this workspace use; `None` goes
/// back to the engine's default (F-AC-03).
#[tauri::command]
pub async fn workspace_set_account_override(
    state: State<'_, AppState>,
    id: WorkspaceId,
    engine_id: String,
    account_id: Option<AccountId>,
) -> Result<(), IpcError> {
    Ok(
        accounts::set_workspace_override(&state.store, &id, &engine_id, account_id.as_ref())
            .await?,
    )
}
