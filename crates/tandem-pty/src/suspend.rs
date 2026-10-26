//! Pausing a pane's foreground job (F-AC-09): stop and continue a process
//! group. Unix sends SIGSTOP and SIGCONT; Windows has no equivalent, so it
//! reports that pausing isn't available. Behind a trait so the runaway guard
//! is tested with a fake.

use std::time::Duration;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SuspendError {
    #[error("pausing a program isn't available on this system")]
    Unsupported,
    #[error("the program isn't running")]
    Gone,
    #[error("the program didn't stop")]
    NotStopped,
    #[error("couldn't signal the program: {0}")]
    Os(String),
}

pub trait ProcessControl: Send + Sync {
    /// Stops every process in group `pgid`.
    fn stop(&self, pgid: i32) -> Result<(), SuspendError>;
    /// Continues every process in group `pgid`.
    fn cont(&self, pgid: i32) -> Result<(), SuspendError>;
    /// True once process `pid` has stopped, false if it hasn't within
    /// `limit` or is gone. A stop signal is only queued when `stop` returns;
    /// the process stops when it next runs.
    fn wait_stopped(&self, _pid: i32, _limit: Duration) -> bool {
        true
    }
}

/// How long a pause waits for the pane's shell to stop (D-860).
pub const SHELL_STOP_LIMIT: Duration = Duration::from_millis(500);

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
/// outlives the pane. A stopped shell can't react, so the agent is stopped
/// only once the shell really has (D-860): until then the shell's wait for
/// its job can still return the job's stop, and it acts on that when it's
/// continued. A shell that doesn't stop is continued and nothing is paused,
/// as when the agent can't be stopped.
pub fn pause(
    control: &dyn ProcessControl,
    agent: i32,
    shell: Option<i32>,
) -> Result<PausedJob, SuspendError> {
    pause_with(control, agent, shell, false)
}

/// [`pause`] for Freeze (F-EX-06): when the foreground job ended between
/// reading it and stopping it, its stopped shell is the whole job. It can't
/// start the next command, so the pane is frozen, and Thaw continues it.
pub fn pause_or_hold_shell(
    control: &dyn ProcessControl,
    agent: i32,
    shell: Option<i32>,
) -> Result<PausedJob, SuspendError> {
    pause_with(control, agent, shell, true)
}

fn pause_with(
    control: &dyn ProcessControl,
    agent: i32,
    shell: Option<i32>,
    hold_shell: bool,
) -> Result<PausedJob, SuspendError> {
    let shell = shell.filter(|&s| s > 1 && s != agent);
    if let Some(s) = shell {
        control.stop(s)?;
        if !control.wait_stopped(s, SHELL_STOP_LIMIT) {
            let _ = control.cont(s);
            return Err(SuspendError::NotStopped);
        }
    }
    if let Err(e) = control.stop(agent) {
        if let (true, SuspendError::Gone, Some(s)) = (hold_shell, &e, shell) {
            return Ok(PausedJob {
                agent: s,
                shell: None,
            });
        }
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

/// Whether `pid` is stopped; `None` when it's gone or exiting.
#[cfg(unix)]
fn is_stopped(sys: &mut sysinfo::System, pid: i32) -> Option<bool> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate};
    let pid = Pid::from_u32(u32::try_from(pid).ok()?);
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    match sys.process(pid)?.status() {
        ProcessStatus::Stop | ProcessStatus::Tracing => Some(true),
        ProcessStatus::Zombie | ProcessStatus::Dead => None,
        _ => Some(false),
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
    fn wait_stopped(&self, pid: i32, limit: Duration) -> bool {
        let mut sys = sysinfo::System::new();
        let start = std::time::Instant::now();
        loop {
            match is_stopped(&mut sys, pid) {
                Some(true) => return true,
                None => return false,
                Some(false) if start.elapsed() >= limit => return false,
                Some(false) => std::thread::sleep(Duration::from_millis(1)),
            }
        }
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

    /// Records each signal and wait; `fail` names a group whose stop fails,
    /// `stuck` a process that never stops.
    #[derive(Default)]
    struct Log {
        calls: Mutex<Vec<(&'static str, i32)>>,
        fail: Option<i32>,
        stuck: Option<i32>,
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
        fn wait_stopped(&self, pid: i32, _limit: Duration) -> bool {
            self.calls.lock().expect("lock").push(("wait", pid));
            self.stuck != Some(pid)
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
        // The job is stopped only once the shell has stopped (D-860).
        assert_eq!(
            log.calls.lock().expect("lock").as_slice(),
            &[
                ("stop", 100),
                ("wait", 100),
                ("stop", 200),
                ("cont", 200),
                ("cont", 100)
            ]
        );
    }

    #[test]
    fn f_ac_09_8_a_shell_that_doesnt_stop_leaves_its_job_running() {
        // Stopping the job then could still reach the shell's wait (D-860).
        let log = Log {
            stuck: Some(100),
            ..Log::default()
        };
        assert_eq!(pause(&log, 200, Some(100)), Err(SuspendError::NotStopped));
        assert_eq!(
            log.calls.lock().expect("lock").as_slice(),
            &[("stop", 100), ("wait", 100), ("cont", 100)]
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
            &[("stop", 100), ("wait", 100), ("stop", 200), ("cont", 100)]
        );
    }

    #[test]
    fn f_ex_06_4_freeze_holds_the_shell_when_its_job_ended_first() {
        let log = Log {
            fail: Some(200),
            ..Log::default()
        };
        let job = pause_or_hold_shell(&log, 200, Some(100)).expect("frozen");
        assert_eq!(
            job,
            PausedJob {
                agent: 100,
                shell: None
            }
        );
        // The shell stays stopped until Thaw.
        assert_eq!(
            log.calls.lock().expect("lock").as_slice(),
            &[("stop", 100), ("wait", 100), ("stop", 200)]
        );
        resume(&log, &job).expect("thaw");
        assert_eq!(log.calls.lock().expect("lock").last(), Some(&("cont", 100)));
        // Any other failure still leaves nothing stopped.
        let stuck = Log {
            stuck: Some(100),
            ..Log::default()
        };
        assert_eq!(
            pause_or_hold_shell(&stuck, 200, Some(100)),
            Err(SuspendError::NotStopped)
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

    #[test]
    fn f_ac_09_8_waits_until_a_stopped_process_has_really_stopped() {
        use std::os::unix::process::CommandExt;
        use std::time::Duration;
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .expect("sleep");
        let pid = child.id() as i32;
        let c = SignalControl;
        // Running, it never stops by itself.
        assert!(!c.wait_stopped(pid, Duration::from_millis(50)));
        c.stop(pid).expect("stop");
        assert!(c.wait_stopped(pid, Duration::from_secs(2)));
        assert_eq!(state(pid as u32), "T");
        c.cont(pid).expect("cont");
        let _ = child.kill();
        let _ = child.wait();
        // Gone: nothing to wait for.
        assert!(!c.wait_stopped(pid, Duration::from_millis(50)));
    }
}
