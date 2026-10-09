//! The desktop's side of the protocol: requests with ids and timeouts over
//! any byte stream (the `ssh … tandem-host serve` child's stdio, or a
//! socket in tests); notifications (`pty.output`, `pty.exit`,
//! `status.event`) go to the receiver `start` returns.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, watch};

use crate::protocol::{Message, RpcError, MAX_LINE};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Rpc(#[from] RpcError),
    #[error("The connection to the host closed")]
    Disconnected,
    #[error("The host didn't answer in time")]
    Timeout,
    #[error("The host sent something unexpected: {0}")]
    Decode(String),
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, RpcError>>>>>;

pub struct HostClient {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    pending: Pending,
    next: AtomicU64,
    closed: watch::Receiver<bool>,
}

impl HostClient {
    pub fn start<R, W>(reader: R, writer: W) -> (Arc<Self>, mpsc::UnboundedReceiver<Message>)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (note_tx, note_rx) = mpsc::unbounded_channel();
        let (closed_tx, closed) = watch::channel(false);
        let closed_tx = Arc::new(closed_tx);
        let pending: Pending = Arc::default();

        let mut writer = writer;
        let closed_w = closed_tx.clone();
        tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if writer.write_all(&line).await.is_err() || writer.flush().await.is_err() {
                    break;
                }
            }
            let _ = closed_w.send(true);
        });

        let pend = pending.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let mut line = Vec::new();
            loop {
                line.clear();
                match (&mut reader)
                    .take(MAX_LINE as u64 + 1)
                    .read_until(b'\n', &mut line)
                    .await
                {
                    Ok(0) | Err(_) => break,
                    Ok(n) if n > MAX_LINE => break,
                    Ok(_) => {}
                }
                let Ok(msg) = serde_json::from_slice::<Message>(&line) else {
                    continue;
                };
                match (msg.id, msg.method.is_some()) {
                    (Some(id), false) => {
                        let waiter = pend.lock().ok().and_then(|mut p| p.remove(&id));
                        if let Some(w) = waiter {
                            let out = match msg.error {
                                Some(e) => Err(e),
                                None => Ok(msg.result.unwrap_or(Value::Null)),
                            };
                            let _ = w.send(out);
                        }
                    }
                    (None, true) => {
                        let _ = note_tx.send(msg);
                    }
                    _ => {}
                }
            }
            if let Ok(mut p) = pend.lock() {
                p.clear();
            }
            let _ = closed_tx.send(true);
        });

        (
            Arc::new(Self {
                tx,
                pending,
                next: AtomicU64::new(1),
                closed,
            }),
            note_rx,
        )
    }

    pub fn is_closed(&self) -> bool {
        *self.closed.borrow()
    }

    /// Resolves when the connection is gone.
    pub async fn closed(&self) {
        let mut c = self.closed.clone();
        let _ = c.wait_for(|v| *v).await;
    }

    pub async fn call<P: Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        params: P,
    ) -> Result<T, ClientError> {
        self.call_within(method, params, DEFAULT_TIMEOUT).await
    }

    pub async fn call_within<P: Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        params: P,
        limit: Duration,
    ) -> Result<T, ClientError> {
        if self.is_closed() {
            return Err(ClientError::Disconnected);
        }
        let params =
            serde_json::to_value(params).map_err(|e| ClientError::Decode(e.to_string()))?;
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        if let Ok(mut p) = self.pending.lock() {
            p.insert(id, tx);
        }
        if self
            .tx
            .send(Message::request(id, method, params).to_line())
            .is_err()
        {
            return Err(ClientError::Disconnected);
        }
        let out = match tokio::time::timeout(limit, rx).await {
            Ok(Ok(out)) => out?,
            Ok(Err(_)) => return Err(ClientError::Disconnected),
            Err(_) => {
                if let Ok(mut p) = self.pending.lock() {
                    p.remove(&id);
                }
                return Err(ClientError::Timeout);
            }
        };
        serde_json::from_value(out).map_err(|e| ClientError::Decode(e.to_string()))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::daemon::Daemon;
    use crate::protocol::{method, ErrorKind, Hello, HelloReply, PROTOCOL, VERSION};

    #[tokio::test]
    async fn f_rm_01_client_calls_and_notifications_over_a_socket() {
        let dir = tempfile::tempdir().expect("tmp");
        let d = Daemon::new(dir.path().to_path_buf(), None, Duration::from_secs(60));
        let (a, b) = std::os::unix::net::UnixStream::pair().expect("pair");
        d.client(b);
        a.set_nonblocking(true).expect("nb");
        let s = tokio::net::UnixStream::from_std(a).expect("tokio");
        let (r, w) = s.into_split();
        let (c, mut notes) = HostClient::start(r, w);
        let hello: HelloReply = c
            .call(
                method::HELLO,
                Hello {
                    protocol: PROTOCOL,
                    client_version: VERSION.into(),
                },
            )
            .await
            .expect("hello");
        assert_eq!(hello.protocol, PROTOCOL);
        let err = c
            .call::<_, Value>(
                method::FS_READ,
                serde_json::json!({"root": dir.path(), "path": "../x"}),
            )
            .await
            .expect_err("escape");
        assert!(
            matches!(err, ClientError::Rpc(ref e) if e.kind == ErrorKind::PathEscape),
            "{err:?}"
        );
        let _: Value = c
            .call(
                method::PTY_SPAWN,
                serde_json::json!({"id": "t", "argv": ["/bin/sh", "-c", "echo out"], "cwd": dir.path(), "cols": 80, "rows": 24}),
            )
            .await
            .expect("spawn");
        let mut saw_exit = false;
        while let Some(n) = tokio::time::timeout(Duration::from_secs(10), notes.recv())
            .await
            .expect("note")
        {
            if n.method.as_deref() == Some(method::PTY_EXIT) {
                saw_exit = true;
                break;
            }
        }
        assert!(saw_exit);
    }
}
