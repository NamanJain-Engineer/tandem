//! The status server (ARCHITECTURE §6.6, F-EN-05): where `tandem-hook`
//! reports. HTTP/1.1 on a Unix socket (`$DATA/run/status.sock`, 0600), or
//! loopback TCP on a random port on Windows. Each pane gets a fresh secret
//! per launch; a report is applied only when its HMAC matches that pane's.

use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rand::RngCore;
use tandem_core::run_request::{self, RunReply, RunRequest, MAX_RUN_BYTES, RUN_PATH};
use tandem_core::status_report::{
    accept, claimed_pane, ReportError, StatusReport, MAX_AGE_MS, MAX_REPORT_BYTES, SIGNATURE_HEADER,
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

/// Runs a `vibezeo run` request (F-TL-12): the app's approval, the run and
/// its masked output. The connection stays open until it resolves.
pub trait RunHandler: Send + Sync {
    fn run(&self, req: RunRequest) -> Pin<Box<dyn Future<Output = RunReply> + Send>>;
}

type Handler = Arc<Mutex<Option<Arc<dyn RunHandler>>>>;

/// Nonces seen in the last [`MAX_AGE_MS`]: a replayed run is refused.
type Nonces = Arc<Mutex<HashMap<String, i64>>>;

pub struct StatusServer {
    url: String,
    socket: Option<PathBuf>,
    secrets: Secrets,
    handler: Handler,
    task: JoinHandle<()>,
}

impl StatusServer {
    /// Starts serving in `run_dir`.
    pub async fn start(run_dir: &Path, sink: Arc<dyn ReportSink>) -> std::io::Result<Self> {
        std::fs::create_dir_all(run_dir)?;
        let secrets: Secrets = Arc::default();
        let handler: Handler = Arc::default();
        let base = State {
            secrets: secrets.clone(),
            sink,
            handler: handler.clone(),
            nonces: Arc::default(),
            tcp: false,
        };
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
            let state = base;
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
                handler,
                task,
            })
        }
        #[cfg(not(unix))]
        {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
            let port = listener.local_addr()?.port();
            let state = State { tcp: true, ..base };
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
                handler,
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

    /// Checks a report another machine relayed (an SSH host's status
    /// socket, F-RM-01) exactly as one posted here: the pane's secret signs
    /// it, its timestamp is recent.
    pub fn accept_relayed(
        &self,
        body: &[u8],
        signature: Option<&str>,
    ) -> Result<StatusReport, ReportError> {
        let pane = claimed_pane(body)?;
        let secret = self
            .secrets
            .lock()
            .ok()
            .and_then(|s| s.get(&pane).cloned())
            .ok_or(ReportError::Unauthorized)?;
        accept(&secret, body, signature, now_ms())
    }

    /// Where `vibezeo run` requests go (F-TL-12); none answers "not ready".
    pub fn set_run_handler(&self, handler: Arc<dyn RunHandler>) {
        if let Ok(mut h) = self.handler.lock() {
            *h = Some(handler);
        }
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

#[derive(Clone)]
struct State {
    secrets: Secrets,
    sink: Arc<dyn ReportSink>,
    handler: Handler,
    nonces: Nonces,
    tcp: bool,
}

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

fn json_reply(reply: &RunReply) -> Response<Full<Bytes>> {
    let body = serde_json::to_vec(reply).unwrap_or_default();
    let mut r = Response::new(Full::new(Bytes::from(body)));
    r.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    r
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

async fn handle(req: Request<Incoming>, state: State) -> Result<Response<Full<Bytes>>, Infallible> {
    let State {
        secrets, sink, tcp, ..
    } = state.clone();
    if !host_ok(&req, tcp) {
        return Ok(reply(StatusCode::FORBIDDEN, ""));
    }
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/v1/health") => Ok(reply(StatusCode::OK, r#"{"ok":true}"#)),
        (&Method::POST, RUN_PATH) => Ok(run(req, state).await),
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

/// `vibezeo run`: signed by the pane's secret, recent, never seen before.
async fn run(req: Request<Incoming>, state: State) -> Response<Full<Bytes>> {
    let sig = req
        .headers()
        .get(SIGNATURE_HEADER)
        .and_then(|h| h.to_str().ok())
        .map(str::to_string);
    let body = match Limited::new(req.into_body(), MAX_RUN_BYTES).collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return reply(StatusCode::PAYLOAD_TOO_LARGE, ""),
    };
    let secret = claimed_pane(&body).ok().and_then(|pane| {
        state
            .secrets
            .lock()
            .ok()
            .and_then(|s| s.get(&pane).cloned())
    });
    let Some(secret) = secret else {
        return reply(StatusCode::UNAUTHORIZED, "");
    };
    let now = now_ms();
    let request = match run_request::accept(&secret, &body, sig.as_deref(), now) {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(
                code = "run.refused",
                status = e.http_status(),
                "run request refused"
            );
            return reply(
                StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::BAD_REQUEST),
                "",
            );
        }
    };
    {
        let Ok(mut seen) = state.nonces.lock() else {
            return reply(StatusCode::INTERNAL_SERVER_ERROR, "");
        };
        seen.retain(|_, at| *at >= now - MAX_AGE_MS);
        if seen.insert(request.nonce.clone(), now).is_some() {
            return reply(StatusCode::CONFLICT, "");
        }
    }
    let handler = state.handler.lock().ok().and_then(|h| h.clone());
    let out = match handler {
        Some(h) => h.run(request).await,
        None => RunReply::refused("Vibezeo isn't ready to run commands yet"),
    };
    json_reply(&out)
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

    /// Echoes the command back as its output, exit 7.
    struct Echo;
    impl RunHandler for Echo {
        fn run(&self, req: RunRequest) -> Pin<Box<dyn Future<Output = RunReply> + Send>> {
            Box::pin(async move {
                RunReply {
                    exit_code: Some(7),
                    output: format!("ran {} in {}", req.argv.join(" "), req.cwd),
                    ..RunReply::default()
                }
            })
        }
    }

    fn run_req(pane: &str, nonce: &str) -> RunRequest {
        RunRequest {
            pane_id: pane.into(),
            cwd: "/w".into(),
            argv: vec!["pnpm".into(), "deploy".into()],
            ts: now_ms(),
            nonce: nonce.into(),
        }
    }

    async fn exchange(url: &str, req: Vec<u8>) -> (u16, Vec<u8>) {
        let url = url.to_string();
        tokio::task::spawn_blocking(move || {
            hook::exchange(&url, &req, Duration::from_secs(5), 1 << 20)
        })
        .await
        .expect("join")
        .expect("exchange")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tl_12_7_a_signed_run_reaches_the_handler_and_its_reply_comes_back() {
        let (_d, s, _got) = server().await;
        let secret = s.register("p1");
        let req = hook::run_request(&run_req("p1", "0123456789abcdef"), &secret).expect("req");
        // No handler yet: refused with a reason, not dropped.
        let (code, body) = exchange(&s.url(), req.clone()).await;
        assert_eq!(code, 200);
        let reply: RunReply = serde_json::from_slice(&body).expect("json");
        assert!(reply.error.is_some() && reply.exit_code.is_none());

        s.set_run_handler(Arc::new(Echo));
        let fresh = hook::run_request(&run_req("p1", "fedcba9876543210"), &secret).expect("req");
        let (code, body) = exchange(&s.url(), fresh.clone()).await;
        assert_eq!(code, 200);
        let reply: RunReply = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            (reply.exit_code, reply.output.as_str()),
            (Some(7), "ran pnpm deploy in /w")
        );
        // The same body again is a replay.
        assert_eq!(exchange(&s.url(), fresh).await.0, 409);
        // Another pane's secret, or an unknown pane, isn't accepted.
        let wrong =
            hook::run_request(&run_req("p1", "aaaaaaaaaaaaaaaa"), "not-the-secret").expect("req");
        assert_eq!(exchange(&s.url(), wrong).await.0, 401);
        let unknown = hook::run_request(&run_req("p9", "bbbbbbbbbbbbbbbb"), &secret).expect("req");
        assert_eq!(exchange(&s.url(), unknown).await.0, 401);
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

    #[tokio::test(flavor = "multi_thread")]
    async fn f_rm_01_a_relayed_report_needs_the_panes_secret() {
        let (_d, s, _got) = server().await;
        let secret = s.register("pn_r");
        let body = format!(
            r#"{{"v":1,"pane_id":"pn_r","event":"working","ts":{},"payload":{{}}}}"#,
            now_ms()
        );
        let report = s
            .accept_relayed(body.as_bytes(), Some(&sign(&secret, body.as_bytes())))
            .expect("signed by the pane");
        assert_eq!(report.event, ReportEvent::Working);
        assert_eq!(
            s.accept_relayed(body.as_bytes(), Some(&sign("other", body.as_bytes()))),
            Err(ReportError::Unauthorized)
        );
        assert_eq!(
            s.accept_relayed(body.as_bytes(), None),
            Err(ReportError::Unauthorized)
        );
        s.unregister("pn_r");
        assert!(s
            .accept_relayed(body.as_bytes(), Some(&sign(&secret, body.as_bytes())))
            .is_err());
    }
}
