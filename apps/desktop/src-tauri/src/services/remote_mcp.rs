//! Plugins and task tools for engines on SSH hosts (D-1604). An engine on a
//! host starts `tandem-host mcp --channel <id>` as a stdio MCP server; the
//! helper relays its lines over the host's connection, and here each
//! channel is answered by what a local engine would reach: the gateway
//! (served over a stream, bound to the session's lease held on this
//! machine) or the tasks server (the same local `tandem-tasks` process).
//! No port is forwarded and no token, credential or task file goes to the
//! host; approvals are asked here.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::services::remote::{McpEvent, RemoteService};

pub type Reader = Pin<Box<dyn AsyncRead + Send>>;
pub type Writer = Pin<Box<dyn AsyncWrite + Send>>;

/// One MCP session on this machine: lines in, lines out, and whatever must
/// live as long as it (a task, a process), ended when dropped.
pub struct Session {
    pub input: Writer,
    pub output: Reader,
    pub guard: Box<dyn Send>,
}

/// What answers a channel. Each time the engine starts its server (the
/// first line after a close) gets a fresh session.
#[async_trait]
pub trait Backend: Send + Sync + 'static {
    async fn open(&self) -> Result<Session, String>;
}

/// Where the bridge sends what the session answers.
pub trait Sink: Send + Sync + 'static {
    fn send(&self, line: String) -> bool;
}

impl Sink for (Arc<RemoteService>, String) {
    fn send(&self, line: String) -> bool {
        self.0.mcp_send(&self.1, line)
    }
}

/// Passes lines between a channel and its backend until the channel ends.
pub async fn bridge(
    mut events: mpsc::UnboundedReceiver<McpEvent>,
    sink: impl Sink,
    backend: Arc<dyn Backend>,
) {
    loop {
        // The engine speaks first (`initialize`): no session before it.
        let first = match events.recv().await {
            Some(McpEvent::Line(l)) => l,
            Some(McpEvent::Closed) => continue,
            None => return,
        };
        let session = match backend.open().await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(code = "remote.mcp_open_failed", error = %e, "couldn't start an MCP session for a host");
                continue;
            }
        };
        let Session {
            mut input,
            output,
            guard,
        } = session;
        let mut lines = BufReader::new(output).lines();
        if write_line(&mut input, &first).await.is_err() {
            continue;
        }
        loop {
            tokio::select! {
                ev = events.recv() => match ev {
                    Some(McpEvent::Line(l)) => {
                        if write_line(&mut input, &l).await.is_err() {
                            break;
                        }
                    }
                    // The engine's server went: so does this session.
                    Some(McpEvent::Closed) => break,
                    None => return,
                },
                out = lines.next_line() => match out {
                    Ok(Some(l)) => {
                        if !l.trim().is_empty() && !sink.send(l) {
                            break;
                        }
                    }
                    // The session ended on this side; the next start opens another.
                    _ => break,
                },
            }
        }
        drop(guard);
    }
}

async fn write_line(w: &mut Writer, line: &str) -> std::io::Result<()> {
    w.write_all(line.as_bytes()).await?;
    w.write_all(b"\n").await?;
    w.flush().await
}

/// The gateway, bound to one lease.
pub struct GatewayBackend {
    pub gateway: tandem_gateway::GatewayHandle,
    pub lease: tandem_gateway::Lease,
}

#[async_trait]
impl Backend for GatewayBackend {
    async fn open(&self) -> Result<Session, String> {
        let (ours, theirs) = tokio::io::duplex(256 * 1024);
        let (read, write) = tokio::io::split(theirs);
        let gateway = self.gateway.clone();
        let lease = tandem_gateway::Lease {
            id: self.lease.id.clone(),
            token: self.lease.token.clone(),
        };
        let task = tokio::spawn(async move {
            if let Err(e) = gateway.serve_io(&lease, read, write).await {
                tracing::info!(code = "remote.mcp_session_ended", error = %e, "a host's gateway session ended");
            }
        });
        let (output, input) = tokio::io::split(ours);
        Ok(Session {
            input: Box::pin(input),
            output: Box::pin(output),
            guard: Box::new(AbortOnDrop(task)),
        })
    }
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A local stdio MCP server (the tasks server), started per session.
pub struct ProcessBackend {
    pub server: tandem_engines::chat::StdioServer,
    /// Values for the server's `env_vars`, from this machine.
    pub env: Vec<(String, String)>,
}

#[async_trait]
impl Backend for ProcessBackend {
    async fn open(&self) -> Result<Session, String> {
        let mut cmd = tokio::process::Command::new(&self.server.command);
        cmd.args(&self.server.args)
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let input = child.stdin.take().ok_or("no stdin")?;
        let output = child.stdout.take().ok_or("no stdout")?;
        Ok(Session {
            input: Box::pin(input),
            output: Box::pin(output),
            guard: Box::new(child),
        })
    }
}

/// A session's channels on a host (a thread's, or a terminal agent's);
/// dropping it closes them (the engine's servers there end) and stops their
/// bridges.
pub struct RemoteMcp {
    remote: Arc<RemoteService>,
    channels: Vec<(String, tokio::task::JoinHandle<()>)>,
    keep: bool,
}

impl RemoteMcp {
    pub fn new(remote: Arc<RemoteService>) -> Self {
        Self {
            remote,
            channels: Vec::new(),
            keep: false,
        }
    }

    /// For a terminal agent: its channels outlive a dropped connection, as
    /// its PTY does, so its servers keep their sessions.
    pub fn kept(remote: Arc<RemoteService>) -> Self {
        Self {
            remote,
            channels: Vec::new(),
            keep: true,
        }
    }

    /// Opens a channel on `host` answered by `backend`; returns the stdio
    /// server an engine on that host starts to reach it.
    pub async fn add(
        &mut self,
        host: &tandem_core::ids::HostId,
        name: &str,
        backend: Arc<dyn Backend>,
    ) -> Result<tandem_engines::chat::StdioServer, tandem_core::IpcError> {
        let bin = self.remote.host_bin(host).ok_or_else(|| {
            tandem_core::IpcError::new(
                tandem_core::ErrorCode::Unavailable,
                "The host isn't connected",
            )
        })?;
        let (channel, events) = self.remote.mcp_open_with(host, self.keep).await?;
        let sink = (self.remote.clone(), channel.clone());
        let task = tokio::spawn(bridge(events, sink, backend));
        self.channels.push((channel.clone(), task));
        Ok(tandem_engines::chat::StdioServer {
            name: name.to_string(),
            command: std::path::PathBuf::from(bin),
            args: vec!["mcp".into(), "--channel".into(), channel],
            env_vars: Vec::new(),
        })
    }
}

impl Drop for RemoteMcp {
    fn drop(&mut self) {
        for (channel, task) in self.channels.drain(..) {
            task.abort();
            self.remote.mcp_close(&channel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct Collect(Arc<Mutex<Vec<String>>>);

    impl Sink for Collect {
        fn send(&self, line: String) -> bool {
            self.0.lock().map(|mut v| v.push(line)).is_ok()
        }
    }

    impl Collect {
        async fn wait_for(&self, n: usize) -> Vec<String> {
            for _ in 0..200 {
                let got = self.0.lock().map(|v| v.clone()).unwrap_or_default();
                if got.len() >= n {
                    return got;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            panic!("only {:?}", self.0.lock().map(|v| v.clone()));
        }
    }

    /// Echoes each line back with the number of sessions opened so far.
    #[derive(Default)]
    struct Echo(std::sync::atomic::AtomicUsize);

    #[async_trait]
    impl Backend for Echo {
        async fn open(&self) -> Result<Session, String> {
            let n = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            let (ours, theirs) = tokio::io::duplex(4096);
            let task = tokio::spawn(async move {
                let (r, mut w) = tokio::io::split(theirs);
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    let _ = w.write_all(format!("{n}:{l}\n").as_bytes()).await;
                }
            });
            let (output, input) = tokio::io::split(ours);
            Ok(Session {
                input: Box::pin(input),
                output: Box::pin(output),
                guard: Box::new(AbortOnDrop(task)),
            })
        }
    }

    #[tokio::test]
    async fn f_rm_01_a_channel_gets_a_fresh_session_each_time_the_engine_starts_its_server() {
        let (tx, rx) = mpsc::unbounded_channel();
        let out = Collect::default();
        let echo = Arc::new(Echo::default());
        let task = tokio::spawn(bridge(rx, out.clone(), echo.clone()));
        // Nothing starts before the engine speaks.
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(echo.0.load(std::sync::atomic::Ordering::SeqCst), 0);
        tx.send(McpEvent::Line("a".into())).expect("send");
        tx.send(McpEvent::Line("b".into())).expect("send");
        assert_eq!(out.wait_for(2).await, ["1:a", "1:b"]);
        // The server restarted on the host: a new session answers.
        tx.send(McpEvent::Closed).expect("send");
        tx.send(McpEvent::Line("c".into())).expect("send");
        assert_eq!(out.wait_for(3).await[2], "2:c");
        drop(tx);
        task.await.expect("the bridge ends with its channel");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_rm_01_f_tk_02_a_process_backend_runs_the_local_server_per_session() {
        let backend = ProcessBackend {
            server: tandem_engines::chat::StdioServer {
                name: "tandem_tasks".into(),
                command: "sh".into(),
                args: vec![
                    "-c".into(),
                    "while read l; do echo \"$TANDEM_TASKS_CONNECTION $l\"; done".into(),
                ],
                env_vars: vec!["TANDEM_TASKS_CONNECTION".into()],
            },
            env: vec![("TANDEM_TASKS_CONNECTION".into(), "conn_1".into())],
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let out = Collect::default();
        let task = tokio::spawn(bridge(rx, out.clone(), Arc::new(backend)));
        tx.send(McpEvent::Line(r#"{"id":1}"#.into())).expect("send");
        assert_eq!(out.wait_for(1).await, [r#"conn_1 {"id":1}"#]);
        drop(tx);
        task.await.expect("ends");
    }
}
