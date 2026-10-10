//! Exclusive lock files (`locks/*.lock`), shared by the app and every MCP
//! server process. Waits up to [`ACQUIRE_TIMEOUT`], then `UNAVAILABLE`.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::error::TaskError;

pub const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// Held until dropped.
#[derive(Debug)]
pub struct LockGuard {
    file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn acquire(path: &Path) -> Result<LockGuard, TaskError> {
    acquire_within(path, ACQUIRE_TIMEOUT)
}

pub fn acquire_within(path: &Path, timeout: Duration) -> Result<LockGuard, TaskError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    let start = Instant::now();
    let mut wait = Duration::from_millis(1);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(LockGuard { file }),
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(e)) => return Err(TaskError::Io(e)),
        }
        if start.elapsed() >= timeout {
            return Err(TaskError::Unavailable(
                "the task files are busy; try again in a moment".into(),
            ));
        }
        std::thread::sleep(wait);
        wait = (wait * 2).min(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_lock_times_out_the_second_taker() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("locks/a.lock");
        let held = acquire(&path).expect("first");
        let err = acquire_within(&path, Duration::from_millis(50)).expect_err("busy");
        assert!(matches!(err, TaskError::Unavailable(_)));
        drop(held);
        acquire_within(&path, Duration::from_millis(50)).expect("free again");
    }
}
