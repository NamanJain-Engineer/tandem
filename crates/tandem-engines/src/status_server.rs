//! The status server (ARCHITECTURE §6.6, F-EN-05): where `tandem-hook`
//! reports. HTTP/1.1 on a Unix socket (`$DATA/run/status.sock`, 0600), or
//! loopback TCP on a random port on Windows. Each pane gets a fresh secret
//! per launch; a report is applied only when its HMAC matches that pane's.

use std::collections::HashMap;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rand::RngCore;
use tandem_core::status_report::{
    accept, claimed_pane, ReportError, StatusReport, MAX_REPORT_BYTES, SIGNATURE_HEADER,
};
use tokio::task::JoinHandle;

/// Receives every accepted report.
pub trait ReportSink: Send + Sync {
    fn report(&self, report: StatusReport);
}

impl<F: Fn(StatusReport) + Send + Sync> ReportSink for F {
    fn report(&self, report: StatusReport) {
        self(report)
    }
}

type Secrets = Arc<Mutex<HashMap<String, String>>>;

pub struct StatusServer {
    url: String,
    socket: Option<PathBuf>,
    secrets: Secrets,
    task: JoinHandle<()>,
}

impl StatusServer {
    /// Starts serving in `run_dir`.
    pub async fn start(run_dir: &Path, sink: Arc<dyn ReportSink>) -> std::io::Result<Self> {
        std::fs::create_dir_all(run_dir)?;
        let secrets: Secrets = Arc::default();
        #[cfg(unix)]
        {
            let path = run_dir.join("status.sock");
            // A socket left by a crashed run.
            let _ = std::fs::remove_file(&path);
            let listener = tokio::net::UnixListener::bind(&path)?;
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            }
            let state = (secrets.clone(), sink, false);
            let task = tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        continue;
                    };
                    serve(TokioIo::new(stream), state.clone());
                }
            });
            Ok(Self {
                url: format!("unix://{}", path.display()),
                socket: Some(path),
                secrets,
                task,
            })
        }
        #[cfg(not(unix))]
        {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
            let port = listener.local_addr()?.port();
            let state = (secrets.clone(), sink, true);
            let task = tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        continue;
                    };
                    serve(TokioIo::new(stream), state.clone());
                }
            });
            Ok(Self {
                url: format!("http://127.0.0.1:{port}"),
                socket: None,
                secrets,
                task,
            })
        }
    }

    /// `TANDEM_STATUS_URL` for panes.
    pub fn url(&self) -> String {
        self.url.clone()
    }

    /// A new secret for a pane's launch (32 random bytes, hex); the previous
    /// launch's secret stops working.
    pub fn register(&self, pane_id: &str) -> String {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let secret = hex::encode(bytes);
        if let Ok(mut s) = self.secrets.lock() {
            s.insert(pane_id.to_string(), secret.clone());
        }
        secret
    }

    pub fn unregister(&self, pane_id: &str) {
        if let Ok(mut s) = self.secrets.lock() {
            s.remove(pane_id);
        }
    }

    pub fn shutdown(&self) {
        self.task.abort();
        if let Some(p) = &self.socket {
            let _ = std::fs::remove_file(p);
        }
    }
}

impl Drop for StatusServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

type State = (Secrets, Arc<dyn ReportSink>, bool);

fn serve<I>(io: TokioIo<I>, state: State)
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let svc = hyper::service::service_fn(move |req| handle(req, state.clone()));
        let _ = hyper::server::conn::http1::Builder::new()
            .serve_connection(io, svc)
            .await;
    });
}

fn reply(code: StatusCode, body: &'static str) -> Response<Full<Bytes>> {
    let mut r = Response::new(Full::new(Bytes::from_static(body.as_bytes())));
    *r.status_mut() = code;
    r
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Loopback TCP is reachable from web pages: only a loopback Host header
/// (no DNS rebinding) is served.
fn host_ok(req: &Request<Incoming>, tcp: bool) -> bool {
    if !tcp {
        return true;
    }
    let host = req
        .headers()
        .get(hyper::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    let name = host.rsplit_once(':').map_or(host, |(h, _)| h);
    matches!(name, "127.0.0.1" | "localhost")
}

async fn handle(
    req: Request<Incoming>,
    (secrets, sink, tcp): State,
) -> Result<Response<Full<Bytes>>, Infallible> {
    if !host_ok(&req, tcp) {
        return Ok(reply(StatusCode::FORBIDDEN, ""));
    }
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/v1/health") => Ok(reply(StatusCode::OK, r#"{"ok":true}"#)),
        (&Method::POST, "/v1/status") => {
            let sig = req
                .headers()
                .get(SIGNATURE_HEADER)
                .and_then(|h| h.to_str().ok())
                .map(str::to_string);
            let body = match Limited::new(req.into_body(), MAX_REPORT_BYTES)
                .collect()
                .await
            {
                Ok(b) => b.to_bytes(),
                Err(_) => return Ok(reply(StatusCode::PAYLOAD_TOO_LARGE, "")),
            };
            Ok(
                match apply(&secrets, sink.as_ref(), &body, sig.as_deref()) {
                    Ok(()) => reply(StatusCode::NO_CONTENT, ""),
                    Err(e) => {
                        tracing::debug!(
                            code = "status.refused",
                            status = e.http_status(),
                            "status report refused"
                        );
                        reply(
                            StatusCode::from_u16(e.http_status())
                                .unwrap_or(StatusCode::BAD_REQUEST),
                            "",
                        )
                    }
                },
            )
        }
        _ => Ok(reply(StatusCode::NOT_FOUND, "")),
    }
}

fn apply(
    secrets: &Secrets,
    sink: &dyn ReportSink,
    body: &[u8],
    sig: Option<&str>,
) -> Result<(), ReportError> {
    // An unknown pane and a bad signature look the same from outside.
    let pane = claimed_pane(body).map_err(|e| match e {
        ReportError::TooLarge => e,
        _ => ReportError::Unauthorized,
    })?;
    let secret = secrets
        .lock()
        .ok()
        .and_then(|s| s.get(&pane).cloned())
        .ok_or(ReportError::Unauthorized)?;
    let report = accept(&secret, body, sig, now_ms())?;
    sink.report(report);
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::hook;
    use std::time::Duration;
    use tandem_core::status_report::{sign, ReportEvent};

    async fn server() -> (
        tempfile::TempDir,
        StatusServer,
        Arc<Mutex<Vec<StatusReport>>>,
    ) {
        let dir = tempfile::tempdir().expect("dir");
        let got: Arc<Mutex<Vec<StatusReport>>> = Arc::default();
        let g = got.clone();
        let sink: Arc<dyn ReportSink> =
            Arc::new(move |r: StatusReport| g.lock().expect("lock").push(r));
        let s = StatusServer::start(dir.path(), sink).await.expect("start");
        (dir, s, got)
    }

    fn raw(body: &str, sig: Option<&str>) -> Vec<u8> {
        let sig = sig
            .map(|s| format!("{SIGNATURE_HEADER}: {s}\r\n"))
            .unwrap_or_default();
        format!(
            "POST /v1/status HTTP/1.1\r\nHost: localhost\r\n{sig}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    async fn post(url: &str, req: Vec<u8>) -> u16 {
        let url = url.to_string();
        tokio::task::spawn_blocking(move || hook::post(&url, &req, Duration::from_secs(2)))
            .await
            .expect("join")
            .expect("post")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_en_05_1_the_status_socket_rejects_a_bad_signature_with_401() {
        let (_d, s, got) = server().await;
        let secret = s.register("p1");
        let body = format!(r#"{{"pane_id":"p1","event":"done","ts":{}}}"#, now_ms());
        assert_eq!(post(&s.url(), raw(&body, Some("deadbeef"))).await, 401);
        assert_eq!(post(&s.url(), raw(&body, None)).await, 401);
        // The verification command's body (ts 1) is refused for its signature first.
        assert_eq!(
            post(
                &s.url(),
                raw(
                    r#"{"pane_id":"p1","event":"done","ts":1}"#,
                    Some("deadbeef")
                )
            )
            .await,
            401
        );
        assert!(got.lock().expect("lock").is_empty(), "no state change");
        assert_eq!(
            post(&s.url(), raw(&body, Some(&sign(&secret, body.as_bytes())))).await,
            204
        );
        assert_eq!(got.lock().expect("lock")[0].event, ReportEvent::Done);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = s.url().trim_start_matches("unix://").to_string();
            assert_eq!(
                std::fs::metadata(path).expect("sock").permissions().mode() & 0o777,
                0o600
            );
        }
    }

    // F-TL-06 rule 2 (status reports) is tests/invariants/tests/rule_2.rs.

    #[tokio::test(flavor = "multi_thread")]
    async fn secrets_rotate_per_launch_and_unknown_panes_are_refused() {
        let (_d, s, got) = server().await;
        let old = s.register("p1");
        let new = s.register("p1");
        assert_ne!(old, new);
        let body = format!(r#"{{"pane_id":"p1","event":"working","ts":{}}}"#, now_ms());
        assert_eq!(
            post(&s.url(), raw(&body, Some(&sign(&old, body.as_bytes())))).await,
            401
        );
        let other = format!(r#"{{"pane_id":"p2","event":"working","ts":{}}}"#, now_ms());
        assert_eq!(
            post(&s.url(), raw(&other, Some(&sign(&new, other.as_bytes())))).await,
            401
        );
        s.unregister("p1");
        assert_eq!(
            post(&s.url(), raw(&body, Some(&sign(&new, body.as_bytes())))).await,
            401
        );
        assert!(got.lock().expect("lock").is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_hook_reports_end_to_end() {
        let (_d, s, got) = server().await;
        let secret = s.register("pn_9");
        let report = hook::claude_report(
            "Notification",
            br#"{"notification_type":"permission_prompt"}"#,
            "pn_9",
            now_ms(),
        )
        .expect("report");
        let req = hook::request(&report, &secret).expect("request");
        assert_eq!(post(&s.url(), req).await, 204);
        assert_eq!(got.lock().expect("lock")[0].event, ReportEvent::Waiting);
        let health = "GET /v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            .as_bytes()
            .to_vec();
        assert_eq!(post(&s.url(), health).await, 200);
    }
}
