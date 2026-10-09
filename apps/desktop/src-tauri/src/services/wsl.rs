//! WSL workspaces (F-RM-02, ARCHITECTURE §13.2). Windows only.

use tandem_core::{ErrorCode, IpcError};

/// The distro's folder for a new workspace, checked inside the distro.
pub async fn checked_root(distro: &str, root: &str) -> Result<String, IpcError> {
    let _ = (distro, root);
    Err(IpcError::new(ErrorCode::Unavailable, "WSL folders open on Windows only").with_stage("wsl"))
}
