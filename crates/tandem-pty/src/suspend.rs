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

/// A job paused for the runaway guard: the terminal's foreground group (the
/// agent) and, when the agent runs as a job of the pane's own shell, that
/// shell's group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PausedJob {
    pub agent: i32,
    pub shell: Option<i32>,
}

/// Stops `agent`, its `shell` first (D-857). A job-control shell that sees
/// its foreground job stop takes the terminal back and runs the rest of its
/// command: in the agent wrapper that hands the pane back to a new shell
/// (F-CD-03) and leaves the agent behind as a background job, which then
/// outlives the pane. A stopped shell can't react. If the agent can't be
/// stopped, the shell is continued again.
pub fn pause(
    control: &dyn ProcessControl,
    agent: i32,
    shell: Option<i32>,
) -> Result<PausedJob, SuspendError> {
    let shell = shell.filter(|&s| s > 1 && s != agent);
    if let Some(s) = shell {
        control.stop(s)?;
    }
    if let Err(e) = control.stop(agent) {
        if let Some(s) = shell {
            let _ = control.cont(s);
        }
        return Err(e);
    }
    Ok(PausedJob { agent, shell })
}

/// Continues a paused job: the agent first, so its shell never sees it
/// stopped, then the shell, even when the agent has gone.
pub fn resume(control: &dyn ProcessControl, job: &PausedJob) -> Result<(), SuspendError> {
    let agent = control.cont(job.agent);
    let shell = job.shell.map_or(Ok(()), |s| control.cont(s));
    agent.and(shell)
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

#[cfg(test)]
mod order_tests {
    use super::*;
    use std::sync::Mutex;

    /// Records each signal; `fail` names a group whose stop fails.
    #[derive(Default)]
    struct Log {
        calls: Mutex<Vec<(&'static str, i32)>>,
        fail: Option<i32>,
    }

    impl ProcessControl for Log {
        fn stop(&self, pgid: i32) -> Result<(), SuspendError> {
            self.calls.lock().expect("lock").push(("stop", pgid));
            if self.fail == Some(pgid) {
                return Err(SuspendError::Gone);
            }
            Ok(())
        }
        fn cont(&self, pgid: i32) -> Result<(), SuspendError> {
            self.calls.lock().expect("lock").push(("cont", pgid));
            Ok(())
        }
    }

    #[test]
    fn f_ac_09_8_the_shell_stops_before_its_job_and_continues_after_it() {
        let log = Log::default();
        let job = pause(&log, 200, Some(100)).expect("pause");
        assert_eq!(
            job,
            PausedJob {
                agent: 200,
                shell: Some(100)
            }
        );
        resume(&log, &job).expect("resume");
        assert_eq!(
            log.calls.lock().expect("lock").as_slice(),
            &[("stop", 100), ("stop", 200), ("cont", 200), ("cont", 100)]
        );
    }

    #[test]
    fn f_ac_09_8_an_agent_that_is_the_pane_itself_or_wont_stop_leaves_nothing_stopped() {
        // The agent is the pane's own process: no shell to stop.
        let log = Log::default();
        assert_eq!(
            pause(&log, 300, Some(300)).expect("pause"),
            PausedJob {
                agent: 300,
                shell: None
            }
        );
        assert_eq!(log.calls.lock().expect("lock").as_slice(), &[("stop", 300)]);
        // The agent can't be stopped: its shell goes on again.
        let log = Log {
            fail: Some(200),
            ..Log::default()
        };
        assert_eq!(pause(&log, 200, Some(100)), Err(SuspendError::Gone));
        assert_eq!(
            log.calls.lock().expect("lock").as_slice(),
            &[("stop", 100), ("stop", 200), ("cont", 100)]
        );
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
