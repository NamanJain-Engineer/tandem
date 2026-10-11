//! The real Simulator host (macOS only): `xcrun simctl` and, when installed,
//! `idb`. Kept small; all parsing and policy live in the shared module.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;

use super::{classify_failure, SimError, SimHost, CALL_TIMEOUT};

pub struct XcrunHost {
    idb: Option<PathBuf>,
    scratch: PathBuf,
}

impl XcrunHost {
    pub fn new() -> Self {
        Self {
            idb: find_on_path("idb"),
            scratch: std::env::temp_dir().join("tandem-simulator"),
        }
    }
}

/// The first executable `name` on PATH.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

async fn run(mut cmd: Command, timeout: Duration) -> Result<Vec<u8>, SimError> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let out = match tokio::time::timeout(timeout, cmd.output()).await {
        Err(_) => return Err(SimError::Timeout),
        Ok(Err(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(SimError::NoXcode)
        }
        Ok(Err(err)) => return Err(SimError::Failed(err.to_string())),
        Ok(Ok(out)) => out,
    };
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(classify_failure(&String::from_utf8_lossy(&out.stderr)))
    }
}

#[async_trait]
impl SimHost for XcrunHost {
    async fn simctl(&self, args: &[String], timeout: Duration) -> Result<Vec<u8>, SimError> {
        let mut cmd = Command::new("xcrun");
        cmd.arg("simctl").args(args);
        run(cmd, timeout).await
    }

    async fn idb(&self, args: &[String]) -> Result<Vec<u8>, SimError> {
        let Some(idb) = &self.idb else {
            return Err(SimError::NoIdb);
        };
        let mut cmd = Command::new(idb);
        cmd.args(args);
        run(cmd, CALL_TIMEOUT).await
    }

    fn has_idb(&self) -> bool {
        self.idb.is_some()
    }

    fn scratch_dir(&self) -> PathBuf {
        self.scratch.clone()
    }
}
