//! The engine process a chat session owns (ARCHITECTURE §6.8): its own
//! process group, a stdout line reader (16 MB per line; longer lines are
//! dropped and reported), a stdin writer whose writes time out after 3 s, and
//! the stop policy of F-TH-03: the engine's interrupt, then SIGINT at 3 s,
//! then the whole tree, all within 3.5 s. A blocked stdin never holds up a
//! stop.

use std::ffi::OsString;
use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{timeout, Instant};

use super::{AdapterError, InterruptOutcome};

/// Longest stdout line read; longer ones are dropped.
pub const MAX_LINE: usize = 16 * 1024 * 1024;
/// A stdin write that takes longer means the engine stopped reading.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(3);
/// How long the engine gets to acknowledge an interrupt.
pub const ACK_WAIT: Duration = Duration::from_secs(3);
/// When the process tree is stopped, counted from the stop: early enough
/// that the stop is over by 3.5 s (F-TH-03 AC2).
pub const KILL_AT: Duration = Duration::from_millis(3_400);
/// Stderr kept for the error card (never logged).
const STDERR_TAIL: usize = 4 * 1024;

#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// The whole environment: the app's own is never inherited.
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    pub max_line: usize,
    pub write_timeout: Duration,
}

impl ProcessSpec {
    pub fn new(
        program: PathBuf,
        args: Vec<OsString>,
        env: Vec<(String, String)>,
        cwd: PathBuf,
    ) -> Self {
        Self {
            program,
            args,
            env,
            cwd,
            max_line: MAX_LINE,
            write_timeout: WRITE_TIMEOUT,
        }
    }
}

/// Debug builds with `TANDEM_SPAWN_LOG`: one JSON line per engine spawn,
/// as the terminal service writes for panes (argv, env keys, cwd; values of
/// secret-shaped variables never). The E2E harness reads it.
fn log_spawn(spec: &ProcessSpec) {
    let args: Vec<String> = spec
        .args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    log_spawn_of(&spec.program, &args, &spec.env, &spec.cwd);
}

/// One `TANDEM_SPAWN_LOG` line for any engine run (chat or headless).
pub fn log_spawn_of(
    program: &std::path::Path,
    args: &[String],
    env: &[(String, String)],
    cwd: &std::path::Path,
) {
    if !cfg!(debug_assertions) {
        return;
    }
    let Some(path) = std::env::var_os("TANDEM_SPAWN_LOG") else {
        return;
    };
    let spec = (program, args, env, cwd);
    let mut argv = vec![spec.0.to_string_lossy().into_owned()];
    argv.extend(spec.1.iter().cloned());
    let home = spec
        .2
        .iter()
        .find(|(k, _)| matches!(k.as_str(), "CLAUDE_CONFIG_DIR" | "CODEX_HOME"))
        .map(|(_, v)| v.clone());
    let record = serde_json::json!({
        "argv": argv,
        "envKeys": spec.2.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>(),
        "secretEnvKeys": spec
            .2
            .iter()
            .filter(|(_, v)| crate::launch::looks_secret(v))
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>(),
        "providerHome": home,
        "cwd": spec.3.to_string_lossy(),
    });
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{record}");
    }
}

/// One line of the engine's stdout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessLine {
    Line(String),
    /// A line over the limit was dropped.
    TooLong,
}

type Write = (Vec<u8>, oneshot::Sender<std::io::Result<()>>);

pub struct ChatProcess {
    pid: Option<u32>,
    writer: mpsc::Sender<Write>,
    exit: watch::Receiver<Option<Option<i32>>>,
    kill: mpsc::Sender<()>,
    stderr: Arc<Mutex<Vec<u8>>>,
    write_timeout: Duration,
}

impl ChatProcess {
    /// Starts the engine; its stdout lines arrive on the returned receiver,
    /// which closes when stdout does.
    pub fn spawn(spec: ProcessSpec) -> Result<(Self, mpsc::Receiver<ProcessLine>), AdapterError> {
        log_spawn(&spec);
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .env_clear()
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&spec.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        #[cfg(windows)]
        cmd.creation_flags(0x0000_0200); // CREATE_NEW_PROCESS_GROUP
        let mut child = cmd.spawn().map_err(AdapterError::Spawn)?;
        let pid = child.id();
        let (stdin, stdout, stderr) =
            match (child.stdin.take(), child.stdout.take(), child.stderr.take()) {
                (Some(i), Some(o), Some(e)) => (i, o, e),
                _ => return Err(AdapterError::Closed),
            };

        let (line_tx, line_rx) = mpsc::channel(256);
        tokio::spawn(read_lines(stdout, spec.max_line, line_tx));
        let tail = Arc::new(Mutex::new(Vec::new()));
        tokio::spawn(keep_tail(stderr, tail.clone()));
        let (writer, writes) = mpsc::channel(16);
        tokio::spawn(write_lines(stdin, writes, spec.write_timeout));

        let (exit_tx, exit) = watch::channel(None);
        let (kill, mut kill_rx) = mpsc::channel::<()>(1);
        tokio::spawn(async move {
            // A kill request, or the handle going away, stops the child.
            let status = tokio::select! {
                s = child.wait() => s,
                _ = kill_rx.recv() => {
                    let _ = child.start_kill();
                    child.wait().await
                }
            };
            let _ = exit_tx.send(Some(status.ok().and_then(|s| s.code())));
        });
        Ok((
            Self {
                pid,
                writer,
                exit,
                kill,
                stderr: tail,
                write_timeout: spec.write_timeout,
            },
            line_rx,
        ))
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Writes one line (a newline is added). Fails with `StdinBlocked` when
    /// the engine doesn't take it within the write timeout.
    pub async fn write_line(&self, line: &str) -> Result<(), AdapterError> {
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(b'\n');
        let (reply, answer) = oneshot::channel();
        self.writer
            .send((bytes, reply))
            .await
            .map_err(|_| AdapterError::Closed)?;
        match timeout(self.write_timeout + Duration::from_millis(100), answer).await {
            Err(_) => Err(AdapterError::StdinBlocked),
            Ok(Err(_)) => Err(AdapterError::Closed),
            Ok(Ok(Err(e))) if e.kind() == std::io::ErrorKind::TimedOut => {
                Err(AdapterError::StdinBlocked)
            }
            Ok(Ok(Err(_))) => Err(AdapterError::Closed),
            Ok(Ok(Ok(()))) => Ok(()),
        }
    }

    /// The exit code once the process ended (`Some(None)`: killed by a signal).
    pub fn exit_code(&self) -> Option<Option<i32>> {
        *self.exit.borrow()
    }

    pub fn exited(&self) -> bool {
        self.exit.borrow().is_some()
    }

    /// Waits up to `limit` for the process to end; true if it did.
    pub async fn wait_exit(&self, limit: Duration) -> bool {
        let mut rx = self.exit.clone();
        let ended = timeout(limit, async { rx.wait_for(Option::is_some).await.is_ok() }).await;
        matches!(ended, Ok(true))
    }

    /// The end of stderr, for an error card.
    pub fn stderr_tail(&self) -> String {
        self.stderr
            .lock()
            .map(|t| String::from_utf8_lossy(&t).trim().to_string())
            .unwrap_or_default()
    }

    /// SIGINT to the process group (unix); elsewhere engines get their own
    /// interrupt message only.
    pub fn signal_interrupt(&self) {
        #[cfg(unix)]
        self.signal_group(libc::SIGINT);
    }

    /// Stops the whole tree and waits up to a second for the exit.
    pub async fn kill_tree(&self) {
        #[cfg(unix)]
        self.signal_group(libc::SIGKILL);
        let _ = self.kill.try_send(());
        self.wait_exit(Duration::from_secs(1)).await;
    }

    #[cfg(unix)]
    fn signal_group(&self, sig: libc::c_int) {
        let Some(pid) = self.pid.and_then(|p| i32::try_from(p).ok()) else {
            return;
        };
        if self.exited() {
            return;
        }
        // SAFETY: kill(2) on the process group of our own child.
        unsafe {
            libc::kill(-pid, sig);
        }
    }

    /// F-TH-03 Stop: `ask` sends the engine's interrupt and resolves to
    /// whether the engine acknowledged it. Without an answer in 3 s the group
    /// gets SIGINT, and the tree is killed in time to be over by 3.5 s.
    pub async fn stop<F>(&self, ask: F) -> InterruptOutcome
    where
        F: Future<Output = bool>,
    {
        let start = Instant::now();
        if matches!(timeout(ACK_WAIT, ask).await, Ok(true)) {
            return InterruptOutcome {
                acked: true,
                killed: false,
            };
        }
        if !self.exited() {
            self.signal_interrupt();
            let left = KILL_AT.saturating_sub(start.elapsed());
            if !self.wait_exit(left).await {
                self.kill_tree().await;
            }
        }
        InterruptOutcome {
            acked: false,
            killed: true,
        }
    }
}

impl Drop for ChatProcess {
    fn drop(&mut self) {
        // The waiter task kills the child when `kill` closes; the group goes
        // too, so nothing the engine started outlives the session.
        #[cfg(unix)]
        self.signal_group(libc::SIGKILL);
    }
}

async fn read_lines<R: AsyncRead + Unpin>(out: R, max: usize, tx: mpsc::Sender<ProcessLine>) {
    let mut reader = BufReader::with_capacity(64 * 1024, out);
    let mut line: Vec<u8> = Vec::new();
    let mut skipping = false;
    loop {
        let chunk = match reader.fill_buf().await {
            Ok([]) => break,
            Ok(c) => c,
            Err(_) => break,
        };
        let (take, ends) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        if !skipping {
            let body = if ends {
                &chunk[..take - 1]
            } else {
                &chunk[..take]
            };
            if line.len() + body.len() > max {
                skipping = true;
                line.clear();
            } else {
                line.extend_from_slice(body);
            }
        }
        reader.consume(take);
        if ends {
            let item = if skipping {
                ProcessLine::TooLong
            } else {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                ProcessLine::Line(String::from_utf8_lossy(&line).into_owned())
            };
            skipping = false;
            line.clear();
            if tx.send(item).await.is_err() {
                return;
            }
        }
    }
    if skipping {
        let _ = tx.send(ProcessLine::TooLong).await;
    } else if !line.is_empty() {
        let _ = tx
            .send(ProcessLine::Line(
                String::from_utf8_lossy(&line).into_owned(),
            ))
            .await;
    }
}

async fn keep_tail<R: AsyncRead + Unpin>(mut err: R, tail: Arc<Mutex<Vec<u8>>>) {
    let mut buf = [0u8; 4096];
    while let Ok(n) = err.read(&mut buf).await {
        if n == 0 {
            break;
        }
        if let Ok(mut t) = tail.lock() {
            t.extend_from_slice(&buf[..n]);
            let extra = t.len().saturating_sub(STDERR_TAIL);
            t.drain(..extra);
        }
    }
}

async fn write_lines(mut stdin: ChildStdin, mut writes: mpsc::Receiver<Write>, limit: Duration) {
    let mut stuck = false;
    while let Some((bytes, reply)) = writes.recv().await {
        if stuck {
            let _ = reply.send(Err(std::io::ErrorKind::TimedOut.into()));
            continue;
        }
        let done = timeout(limit, async {
            stdin.write_all(&bytes).await?;
            stdin.flush().await
        })
        .await;
        let result = match done {
            Ok(r) => r,
            Err(_) => {
                // Part of a line may be in the pipe: nothing more is sent.
                stuck = true;
                Err(std::io::ErrorKind::TimedOut.into())
            }
        };
        let _ = reply.send(result);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> ProcessSpec {
        ProcessSpec::new(
            PathBuf::from("/bin/sh"),
            vec!["-c".into(), script.into()],
            vec![("PATH".into(), "/usr/bin:/bin".into())],
            std::env::temp_dir(),
        )
    }

    /// Running (a zombie counts as gone: it only waits to be reaped).
    fn alive(pid: u32) -> bool {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat
                .rfind(')')
                .and_then(|i| stat[i + 1..].split_whitespace().next())
                .is_some_and(|state| state != "Z"),
            // SAFETY: kill(2) with signal 0 only checks the pid.
            Err(_) => cfg!(not(target_os = "linux")) && unsafe { libc::kill(pid as i32, 0) } == 0,
        }
    }

    #[tokio::test]
    async fn f_th_03_2_stop_ends_an_engine_that_ignores_stdin_and_sigint_within_3_5_s() {
        // Ignores SIGINT and never reads stdin; its child reports its pid.
        let (proc, mut lines) =
            ChatProcess::spawn(sh("trap '' INT; sleep 60 & echo $!; wait")).expect("spawn");
        let Some(ProcessLine::Line(child)) = lines.recv().await else {
            panic!("no child pid");
        };
        let child: u32 = child.trim().parse().expect("pid");
        let root = proc.pid().expect("pid");
        assert!(alive(root) && alive(child));

        let started = std::time::Instant::now();
        let outcome = proc
            .stop(async {
                // The interrupt is written, and never answered.
                let _ = proc.write_line(r#"{"type":"control_request"}"#).await;
                std::future::pending::<bool>().await
            })
            .await;
        let took = started.elapsed();
        assert_eq!(
            outcome,
            InterruptOutcome {
                acked: false,
                killed: true
            }
        );
        assert!(took >= ACK_WAIT, "waited for the ack first ({took:?})");
        assert!(
            took <= Duration::from_millis(3_500),
            "over by 3.5 s ({took:?})"
        );
        assert!(proc.exited());
        for _ in 0..50 {
            if !alive(child) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!alive(root), "the engine is gone");
        assert!(!alive(child), "its children are gone");
    }

    #[tokio::test]
    async fn f_th_03_an_acknowledged_interrupt_leaves_the_engine_running() {
        let (proc, _lines) = ChatProcess::spawn(sh("cat >/dev/null")).expect("spawn");
        let outcome = proc
            .stop(async { proc.write_line("interrupt").await.is_ok() })
            .await;
        assert_eq!(
            outcome,
            InterruptOutcome {
                acked: true,
                killed: false
            }
        );
        assert!(!proc.exited());
        proc.kill_tree().await;
        assert!(proc.exited());
    }

    #[tokio::test]
    async fn f_th_03_an_overlong_line_is_dropped_and_reading_goes_on() {
        let mut spec =
            sh("head -c 5000 /dev/zero | tr '\\0' a; echo; printf 'ok\\r\\n'; printf tail");
        spec.max_line = 1024;
        let (_proc, mut lines) = ChatProcess::spawn(spec).expect("spawn");
        assert_eq!(lines.recv().await, Some(ProcessLine::TooLong));
        assert_eq!(lines.recv().await, Some(ProcessLine::Line("ok".into())));
        assert_eq!(lines.recv().await, Some(ProcessLine::Line("tail".into())));
        assert_eq!(lines.recv().await, None);
    }

    #[tokio::test]
    async fn f_th_03_a_write_to_an_engine_that_stopped_reading_times_out() {
        let mut spec = sh("echo $$; sleep 60");
        spec.write_timeout = Duration::from_millis(300);
        let (proc, mut lines) = ChatProcess::spawn(spec).expect("spawn");
        lines.recv().await;
        let started = std::time::Instant::now();
        // More than a pipe holds.
        let big = "x".repeat(1024 * 1024);
        assert!(matches!(
            proc.write_line(&big).await,
            Err(AdapterError::StdinBlocked)
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
        // Later writes fail at once.
        assert!(matches!(
            proc.write_line("next").await,
            Err(AdapterError::StdinBlocked)
        ));
        proc.kill_tree().await;
    }

    #[tokio::test]
    async fn f_th_03_4_the_exit_code_and_stderr_tail_are_reported() {
        let (proc, _lines) =
            ChatProcess::spawn(sh("echo 'boom: bad flag' >&2; exit 3")).expect("spawn");
        assert!(proc.wait_exit(Duration::from_secs(5)).await);
        assert_eq!(proc.exit_code(), Some(Some(3)));
        // The tail reader may finish just after the exit.
        for _ in 0..50 {
            if !proc.stderr_tail().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(proc.stderr_tail(), "boom: bad flag");
    }
}
