//! Pausing a pane's foreground job (F-AC-09): stop and continue a process
//! group. Unix sends SIGSTOP and SIGCONT; Windows has no equivalent, so it
//! reports that pausing isn't available. Behind a trait so the runaway guard
//! is tested with a fake.

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SuspendError {
    #[error("pausing a program isn't available on this system")]
    Unsupported,
    #[error("the program isn't running")]
    Gone,
    #[error("couldn't signal the program: {0}")]
    Os(String),
}

pub trait ProcessControl: Send + Sync {
    /// Stops every process in group `pgid`.
    fn stop(&self, pgid: i32) -> Result<(), SuspendError>;
    /// Continues every process in group `pgid`.
    fn cont(&self, pgid: i32) -> Result<(), SuspendError>;
}

/// The real signals.
#[derive(Debug, Default, Clone, Copy)]
pub struct SignalControl;

#[cfg(unix)]
fn signal(pgid: i32, sig: libc::c_int) -> Result<(), SuspendError> {
    if pgid <= 1 {
        return Err(SuspendError::Gone);
    }
    // SAFETY: killpg(2) on the foreground group of a pane this app spawned.
    let rc = unsafe { libc::killpg(pgid, sig) };
    if rc == 0 {
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            Err(SuspendError::Gone)
        } else {
            Err(SuspendError::Os(err.to_string()))
        }
    }
}

#[cfg(unix)]
impl ProcessControl for SignalControl {
    fn stop(&self, pgid: i32) -> Result<(), SuspendError> {
        signal(pgid, libc::SIGSTOP)
    }
    fn cont(&self, pgid: i32) -> Result<(), SuspendError> {
        signal(pgid, libc::SIGCONT)
    }
}

#[cfg(windows)]
impl ProcessControl for SignalControl {
    fn stop(&self, _pgid: i32) -> Result<(), SuspendError> {
        Err(SuspendError::Unsupported)
    }
    fn cont(&self, _pgid: i32) -> Result<(), SuspendError> {
        Err(SuspendError::Unsupported)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn state(pid: u32) -> String {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|s| {
                s.rsplit(')')
                    .next()
                    .map(|r| r.trim().chars().next().unwrap_or('?').to_string())
            })
            .unwrap_or_default()
    }

    #[test]
    fn f_ac_09_2_stop_and_continue_a_process_group() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .expect("sleep");
        let pid = child.id();
        let c = SignalControl;
        c.stop(pid as i32).expect("stop");
        let stopped = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            state(pid) == "T"
        });
        assert!(stopped, "stopped: {}", state(pid));
        c.cont(pid as i32).expect("cont");
        let running = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            state(pid) == "S" || state(pid) == "R"
        });
        assert!(running, "continued: {}", state(pid));
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(c.stop(pid as i32), Err(SuspendError::Gone));
        assert_eq!(c.stop(1), Err(SuspendError::Gone), "never init");
    }
}
