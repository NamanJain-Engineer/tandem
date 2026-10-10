//! The SSH askpass helper (ARCHITECTURE §13.1, F-RM-01 "an optional
//! password prompt handled through an askpass helper dialog").
//!
//! ssh runs `SSH_ASKPASS` (the app's own executable) with the prompt as its
//! argument. In that mode the executable connects to this server on the
//! loopback interface, names the connection attempt and its one-time token,
//! and prints the secret the user typed in the dialog (or exits 1 when they
//! cancelled). The secret goes from the dialog to ssh's pipe and is never
//! stored or logged.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Mutex, Weak};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use tandem_core::ids::HostId;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

use crate::services::remote::RemoteService;

pub const ADDR_VAR: &str = "TANDEM_ASKPASS_ADDR";
pub const OP_VAR: &str = "TANDEM_ASKPASS_OP";
pub const TOKEN_VAR: &str = "TANDEM_ASKPASS_TOKEN";
const MAX_REQUEST: u64 = 4096;
/// Connection attempts remembered at once (older ones are forgotten).
const MAX_OPS: usize = 64;

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    op: String,
    token: String,
    prompt: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Response {
    secret: Option<String>,
}

struct Entry {
    token: String,
    host: HostId,
    service: Weak<RemoteService>,
    order: u64,
}

pub struct AskpassServer {
    addr: SocketAddr,
    exe: PathBuf,
    ops: Mutex<HashMap<String, Entry>>,
    next: std::sync::atomic::AtomicU64,
}

/// What ssh's prompt says, safe to show: printable, one line, short.
pub fn label(prompt: &str) -> String {
    let clean: String = prompt
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let clean = clean.trim();
    let mut out: String = clean.chars().take(200).collect();
    if out.is_empty() {
        out = "Password".into();
    }
    out
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

impl AskpassServer {
    pub async fn start() -> std::io::Result<std::sync::Arc<Self>> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let server = std::sync::Arc::new(Self {
            addr: listener.local_addr()?,
            exe: std::env::current_exe()?,
            ops: Mutex::default(),
            next: std::sync::atomic::AtomicU64::new(0),
        });
        let me = std::sync::Arc::downgrade(&server);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let Some(server) = me.upgrade() else { return };
                tokio::spawn(async move { server.handle(stream).await });
            }
        });
        Ok(server)
    }

    /// The askpass program and its variables for connection attempt `op`.
    pub fn register(
        &self,
        op: &str,
        host: HostId,
        service: Weak<RemoteService>,
    ) -> Option<(PathBuf, Vec<(String, String)>)> {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = hex::encode(bytes);
        let order = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut ops = self.ops.lock().ok()?;
        if ops.len() >= MAX_OPS {
            if let Some(oldest) = ops
                .iter()
                .min_by_key(|(_, e)| e.order)
                .map(|(k, _)| k.clone())
            {
                ops.remove(&oldest);
            }
        }
        ops.insert(
            op.to_string(),
            Entry {
                token: token.clone(),
                host,
                service,
                order,
            },
        );
        Some((
            self.exe.clone(),
            vec![
                (ADDR_VAR.into(), self.addr.to_string()),
                (OP_VAR.into(), op.into()),
                (TOKEN_VAR.into(), token),
            ],
        ))
    }

    async fn handle(&self, mut stream: tokio::net::TcpStream) {
        let (read, mut write) = stream.split();
        let mut line = String::new();
        let mut reader = tokio::io::BufReader::new(read.take(MAX_REQUEST));
        if reader.read_line(&mut line).await.is_err() {
            return;
        }
        let secret = match serde_json::from_str::<Request>(&line) {
            Ok(req) => self.answer(req).await,
            Err(_) => None,
        };
        let mut body = serde_json::to_vec(&Response { secret }).unwrap_or_default();
        body.push(b'\n');
        let _ = write.write_all(&body).await;
    }

    async fn answer(&self, req: Request) -> Option<String> {
        let (host, service) = {
            let ops = self.ops.lock().ok()?;
            let e = ops.get(&req.op)?;
            if !same(&e.token, &req.token) {
                return None;
            }
            (e.host.clone(), e.service.upgrade()?)
        };
        service.askpass(&req.op, &host, label(&req.prompt)).await
    }
}

/// The executable in askpass mode (`main` calls this first): prints the
/// secret and returns the exit code, or `None` when not started by ssh.
pub fn client_main() -> Option<i32> {
    let addr = std::env::var(ADDR_VAR).ok()?;
    let op = std::env::var(OP_VAR).ok()?;
    let token = std::env::var(TOKEN_VAR).ok()?;
    let prompt = std::env::args().nth(1).unwrap_or_default();
    let run = || -> std::io::Result<Option<String>> {
        let addr: SocketAddr = addr
            .parse()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "addr"))?;
        if !addr.ip().is_loopback() {
            return Ok(None);
        }
        let mut s = std::net::TcpStream::connect(addr)?;
        let mut req = serde_json::to_vec(&Request { op, token, prompt }).unwrap_or_default();
        req.push(b'\n');
        s.write_all(&req)?;
        let mut line = String::new();
        BufReader::new(s.take(64 * 1024)).read_line(&mut line)?;
        Ok(serde_json::from_str::<Response>(&line)
            .ok()
            .and_then(|r| r.secret))
    };
    Some(match run() {
        Ok(Some(secret)) => {
            println!("{secret}");
            0
        }
        _ => 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_rm_01_askpass_labels_are_one_clean_line() {
        assert_eq!(label("tandem@h's password: "), "tandem@h's password:");
        assert_eq!(
            label("Enter passphrase\nfor key\u{1b}[31m"),
            "Enter passphrase for key [31m"
        );
        assert_eq!(label(""), "Password");
        assert_eq!(label(&"x".repeat(500)).len(), 200);
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
    }

    #[tokio::test]
    async fn f_rm_01_askpass_refuses_a_wrong_token() {
        let server = AskpassServer::start().await.expect("start");
        let (_, env) = server
            .register("op_1", HostId::local(), Weak::new())
            .expect("registered");
        let addr = env
            .iter()
            .find(|(k, _)| k == ADDR_VAR)
            .map(|(_, v)| v.clone())
            .expect("addr");
        let ask = |token: &str| {
            let addr = addr.clone();
            let token = token.to_string();
            tokio::task::spawn_blocking(move || {
                let mut s = std::net::TcpStream::connect(addr).expect("connect");
                let mut req = serde_json::to_vec(&Request {
                    op: "op_1".into(),
                    token,
                    prompt: "pw".into(),
                })
                .expect("json");
                req.push(b'\n');
                s.write_all(&req).expect("write");
                let mut line = String::new();
                BufReader::new(s).read_line(&mut line).expect("read");
                serde_json::from_str::<Response>(&line)
                    .expect("json")
                    .secret
            })
        };
        // A wrong token, and a right one whose service is gone, get nothing.
        assert_eq!(ask("bad").await.expect("join"), None);
        let token = env
            .iter()
            .find(|(k, _)| k == TOKEN_VAR)
            .map(|(_, v)| v.clone())
            .expect("token");
        assert_eq!(ask(&token).await.expect("join"), None);
    }
}
