//! Remote hosts (ARCHITECTURE §3.2 "Remote", F-RM-01, F-RM-02).

use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

use tandem_core::ids::HostId;
use tandem_core::{ErrorCode, IpcError};
use tandem_host::ssh::SshTarget;
use tandem_store::repo::hosts::{self, Host, NewSshHost};

use crate::commands::account::OpHandle;
use crate::services::remote::HostView;
use crate::state::AppState;

/// `ssh_list_dir`: a folder on the host and its sub-folders (the remote
/// folder picker).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemoteDir {
    pub path: String,
    pub parent: Option<String>,
    pub dirs: Vec<String>,
}

#[tauri::command]
pub async fn ssh_hosts_list(state: State<'_, AppState>) -> Result<Vec<HostView>, IpcError> {
    state.remote.list().await
}

/// `target` is an alias from `~/.ssh/config`, `host`, `user@host` or
/// `user@host:port`; `port` and `user` override what it says.
#[tauri::command]
pub async fn ssh_host_add(
    state: State<'_, AppState>,
    target: String,
    port: Option<u16>,
    user: Option<String>,
    identity_file: Option<String>,
    label: Option<String>,
) -> Result<Host, IpcError> {
    let mut t = SshTarget::parse(&target).ok_or_else(|| {
        IpcError::new(
            ErrorCode::Invalid,
            "Use a host name, an alias from your SSH config, or user@host:port",
        )
    })?;
    if let Some(u) = user.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()) {
        if u.starts_with('-') || u.contains(char::is_whitespace) || u.contains('@') {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "That user name can't be used",
            ));
        }
        t.user = Some(u);
    }
    if port.is_some() {
        t.port = port;
    }
    let identity_file = identity_file
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    if let Some(k) = &identity_file {
        if !std::path::Path::new(k).is_absolute() || k.starts_with('-') {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Choose the key file with its full path",
            ));
        }
    }
    let stored = match &t.user {
        Some(u) => format!("{u}@{}", t.target),
        None => t.target.clone(),
    };
    let label = label
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| t.label());
    Ok(hosts::add_ssh(
        &state.store,
        NewSshHost {
            label,
            target: stored,
            port: t.port,
            identity_file,
        },
    )
    .await?)
}

#[tauri::command]
pub async fn ssh_host_remove(state: State<'_, AppState>, host_id: HostId) -> Result<(), IpcError> {
    state.remote.disconnect(&host_id).await?;
    Ok(hosts::remove(&state.store, &host_id).await?)
}

#[tauri::command]
pub async fn ssh_connect(
    state: State<'_, AppState>,
    host_id: HostId,
) -> Result<OpHandle, IpcError> {
    hosts::get(&state.store, &host_id).await?;
    Ok(OpHandle {
        op_id: state.remote.connect(host_id),
    })
}

#[tauri::command]
pub fn ssh_hostkey_decide(state: State<'_, AppState>, op_id: String, accept: bool) {
    state.remote.decide_host_key(&op_id, accept);
}

#[tauri::command]
pub fn ssh_askpass_reply(state: State<'_, AppState>, op_id: String, secret: Option<String>) {
    state.remote.reply_secret(&op_id, secret);
}

#[tauri::command]
pub async fn ssh_disconnect(state: State<'_, AppState>, host_id: HostId) -> Result<(), IpcError> {
    state.remote.disconnect(&host_id).await
}

#[tauri::command]
pub async fn ssh_list_dir(
    state: State<'_, AppState>,
    host_id: HostId,
    path: String,
) -> Result<RemoteDir, IpcError> {
    let l = state.remote.list_dir(&host_id, &path).await?;
    Ok(RemoteDir {
        path: l.path,
        parent: l.parent,
        dirs: l.dirs,
    })
}

/// "Trust new key…" (UI-14, a danger confirm): the pinned key is forgotten
/// and the next connection shows the new fingerprint for approval.
#[tauri::command]
pub async fn ssh_host_reset_key(
    state: State<'_, AppState>,
    host_id: HostId,
) -> Result<(), IpcError> {
    state.remote.disconnect(&host_id).await?;
    state.remote.reset_key(&host_id).await
}
