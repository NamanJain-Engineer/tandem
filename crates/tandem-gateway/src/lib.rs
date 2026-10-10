//! The local MCP gateway (ARCHITECTURE §2.7, §11, F-PL-01): one Streamable
//! HTTP server on `127.0.0.1:<random>/mcp` for every engine session of this
//! app. Each session gets a lease: a 32-byte bearer token, of which only the
//! hash is kept. A request without a live lease gets 401, an MCP session is
//! bound to the lease that opened it, and only loopback `Host`/`Origin`
//! values are accepted (DNS rebinding).
//!
//! A lease records its caller, access, granted plugin connections and
//! built-ins. `tools/list` shows the built-ins it has (`tandem__*`, the
//! approvals tool) and its plugins: `tandem__search_tools` +
//! `tandem__call` by default, every granted tool when "Expose all plugin
//! tools directly" is on. A watchdog brings the listener back on its port if
//! it stops answering.

pub mod approvals;
pub mod builtins;
pub mod lease;
pub mod metrics;
mod plugins;
pub mod vendors;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use tokio_util::sync::CancellationToken;

pub use approvals::{ApprovalAnswer, ApprovalAsker, ToolApproval, APPROVALS_TOOL};
pub use builtins::{Builtin, BuiltinHandler, NoBuiltins, BUILTIN_PLUGIN, CALL, SEARCH_TOOLS};
pub use lease::{Attended, BuiltinSet, Caller, Lease, LeaseContext, SessionDescriptor};
pub use metrics::GatewayMetrics;
pub use vendors::{ConnectionInfo, ToolOutput, VendorModule};

/// How often the watchdog checks that the listener answers.
pub const WATCHDOG_EVERY: Duration = Duration::from_secs(30);
/// How long a probe may take before the listener counts as stuck.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("couldn't open the gateway's port: {0}")]
    Bind(std::io::Error),
}

/// What the gateway serves with.
pub struct GatewayDeps {
    pub approvals: Arc<dyn ApprovalAsker>,
    /// Answers the `tandem__*` built-ins.
    pub builtins: Arc<dyn BuiltinHandler>,
    /// The vendor modules this build has.
    pub modules: Vec<Arc<dyn VendorModule>>,
    /// `$DATA/run`: the port goes to `gateway.json` (diagnostics).
    pub run_dir: Option<PathBuf>,
    pub watchdog_every: Duration,
    /// Where a gateway that keeps failing is reported (`app:subsystem-failed`,
    /// PRODUCT_SPEC §5.2): after 3 restarts in 10 minutes the watchdog stops.
    pub bus: Option<tandem_core::bus::EventBus>,
}

impl GatewayDeps {
    pub fn new(approvals: Arc<dyn ApprovalAsker>) -> Self {
        Self {
            approvals,
            builtins: Arc::new(NoBuiltins),
            modules: vendors::registry(),
            run_dir: None,
            watchdog_every: WATCHDOG_EVERY,
            bus: None,
        }
    }
}

#[derive(Default)]
struct Counters {
    requests: AtomicU64,
    restarts: AtomicU32,
}

/// The running gateway. Cloning shares it.
#[derive(Clone)]
pub struct GatewayHandle {
    /// `http://127.0.0.1:<port>/mcp`
    pub url: String,
    addr: SocketAddr,
    leases: Arc<lease::Leases>,
    plugins: Arc<plugins::Plugins>,
    counters: Arc<Counters>,
    serving: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    shutdown: CancellationToken,
}

impl GatewayHandle {
    /// A lease for one engine session; its token goes in that session's MCP
    /// config and nowhere else.
    pub fn prepare(&self, session: SessionDescriptor) -> Lease {
        self.leases.issue(session)
    }

    /// Ends a lease (session over, quit, sign-out): its token gets 401.
    pub fn revoke(&self, lease: &tandem_core::ids::LeaseId) {
        self.leases.revoke(lease);
    }

    /// The connections plugins run on (no secrets). Leases see the change
    /// on their next call (F-PL-01 AC4).
    pub fn set_connections(&self, connections: Vec<ConnectionInfo>) {
        self.plugins.connections.replace(connections);
    }

    /// A connection went away: its tools stop at once for every lease.
    pub fn invalidate_connection(&self, connection: &tandem_core::ids::ConnectionId) {
        self.plugins.connections.remove(connection);
    }

    /// "Expose all plugin tools directly" (default off).
    pub fn set_expose_all(&self, on: bool) {
        self.plugins.expose_all.store(on, Ordering::SeqCst);
    }

    /// Bumped whenever connections change.
    pub fn generation(&self) -> u64 {
        self.plugins.connections.generation()
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn metrics(&self) -> GatewayMetrics {
        let listening = match self.serving.lock() {
            Ok(s) => s.as_ref().is_some_and(|t| !t.is_finished()),
            Err(_) => false,
        };
        GatewayMetrics {
            port: self.addr.port(),
            listeners: u32::from(listening),
            leases: self.leases.count() as u32,
            mcp_sessions: self.leases.session_count() as u32,
            requests: self.counters.requests.load(Ordering::SeqCst),
            restarts: self.counters.restarts.load(Ordering::SeqCst),
            rss_bytes: metrics::probe().rss_bytes(),
        }
    }

    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }

    /// Stops the listener as a crash would (tests of the watchdog).
    #[cfg(test)]
    fn kill_listener(&self) {
        if let Ok(s) = self.serving.lock() {
            if let Some(t) = s.as_ref() {
                t.abort();
            }
        }
    }
}

pub struct Gateway;

/// Everything one listener serves with.
#[derive(Clone)]
struct Service {
    leases: Arc<lease::Leases>,
    server: GatewayServer,
    counters: Arc<Counters>,
    shutdown: CancellationToken,
}

impl Service {
    fn router(&self) -> axum::Router {
        let server = self.server.clone();
        let mut config = StreamableHttpServerConfig::default().with_allowed_hosts([
            "127.0.0.1",
            "localhost",
            "::1",
        ]);
        // A browser page may not call it: `Origin` must be absent, "null" or
        // loopback.
        config.allowed_origins = vec![
            "null".into(),
            "http://127.0.0.1:*".into(),
            "http://localhost:*".into(),
        ];
        config.cancellation_token = self.shutdown.child_token();
        let mcp: StreamableHttpService<GatewayServer, LocalSessionManager> =
            StreamableHttpService::new(move || Ok(server.clone()), Default::default(), config);
        axum::Router::new()
            .nest_service("/mcp", mcp)
            .layer(axum::middleware::from_fn_with_state(
                (self.leases.clone(), self.counters.clone()),
                guard,
            ))
    }

    fn serve(&self, listener: tokio::net::TcpListener) -> tokio::task::JoinHandle<()> {
        let router = self.router();
        let stop = self.shutdown.clone();
        tokio::spawn(async move {
            let served = axum::serve(listener, router)
                .with_graceful_shutdown(async move { stop.cancelled_owned().await })
                .await;
            if let Err(e) = served {
                tracing::warn!(code = "gateway.stopped", error = %e, "the gateway stopped");
            }
        })
    }
}

/// Whether the listener answers: an unauthenticated request must get 401.
async fn probe(addr: SocketAddr) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let attempt = async {
        let mut stream = tokio::net::TcpStream::connect(addr).await.ok()?;
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).await.ok()?;
        let mut buf = [0u8; 16];
        let n = stream.read(&mut buf).await.ok()?;
        Some(buf[..n].starts_with(b"HTTP/1.1 401"))
    };
    matches!(
        tokio::time::timeout(PROBE_TIMEOUT, attempt).await,
        Ok(Some(true))
    )
}

/// `$DATA/run/gateway.json`: `{port}`, owner-only.
fn write_run_file(dir: &std::path::Path, port: u16) {
    let path = dir.join("gateway.json");
    let body = serde_json::json!({ "port": port }).to_string();
    if let Err(e) = std::fs::create_dir_all(dir)
        .map_err(|e| e.to_string())
        .and_then(|_| {
            tandem_core::atomic::atomic_write(&path, body.as_bytes(), Some(0o600))
                .map_err(|e| e.to_string())
        })
    {
        tracing::warn!(code = "gateway.run_file_failed", error = %e, "couldn't write gateway.json");
    }
}

impl Gateway {
    /// Binds `127.0.0.1:0` and serves until [`GatewayHandle::shutdown`].
    pub async fn start(deps: GatewayDeps) -> Result<GatewayHandle, GatewayError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(GatewayError::Bind)?;
        let addr = listener.local_addr().map_err(GatewayError::Bind)?;
        let leases = Arc::new(lease::Leases::default());
        let plugins = Arc::new(plugins::Plugins::new(deps.modules));
        let counters = Arc::new(Counters::default());
        let shutdown = CancellationToken::new();
        let service = Service {
            leases: leases.clone(),
            server: GatewayServer {
                approvals: deps.approvals,
                builtins: deps.builtins,
                plugins: plugins.clone(),
            },
            counters: counters.clone(),
            shutdown: shutdown.clone(),
        };
        let serving = Arc::new(Mutex::new(Some(service.serve(listener))));
        if let Some(dir) = &deps.run_dir {
            write_run_file(dir, addr.port());
        }
        tokio::spawn(watchdog(
            service,
            addr,
            serving.clone(),
            deps.watchdog_every,
            deps.bus.unwrap_or_default(),
        ));
        tracing::info!(
            code = "gateway.started",
            "MCP gateway listening on loopback"
        );
        Ok(GatewayHandle {
            url: format!("http://{addr}/mcp"),
            addr,
            leases,
            plugins,
            counters,
            serving,
            shutdown,
        })
    }
}

/// The gateway's name in the watchdog and in `app:subsystem-failed`.
const NAME: &str = "gateway";

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Brings the listener back on the same port when it stopped or stopped
/// answering, so every session's MCP config stays valid.
async fn watchdog(
    service: Service,
    addr: SocketAddr,
    serving: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    every: Duration,
    bus: tandem_core::bus::EventBus,
) {
    let stop = service.shutdown.clone();
    // Shared with every supervised subsystem (Help › Diagnostics, §5.7).
    let dog = tandem_core::supervisor::watchdog();
    let mut budget = tandem_core::supervisor::RestartBudget::default();
    dog.beat(NAME);
    loop {
        tokio::select! {
            _ = stop.cancelled() => return,
            _ = tokio::time::sleep(every) => {}
        }
        let finished = match serving.lock() {
            Ok(s) => s.as_ref().is_none_or(|t| t.is_finished()),
            Err(_) => true,
        };
        if !finished && probe(addr).await {
            dog.beat(NAME);
            continue;
        }
        if stop.is_cancelled() {
            return;
        }
        // At most 3 restarts in 10 minutes, then it's reported and left down.
        if !budget.try_restart(now_ms()) {
            dog.record(NAME, budget.recent(), "stopped answering", true);
            tandem_core::supervisor::report_failed(
                &bus,
                NAME,
                budget.recent(),
                "stopped answering".into(),
            );
            return;
        }
        dog.record(NAME, budget.recent(), "stopped answering", false);
        tracing::warn!(
            code = "gateway.restarting",
            "the gateway stopped answering; restarting it"
        );
        if let Ok(mut s) = serving.lock() {
            if let Some(t) = s.take() {
                t.abort();
            }
        }
        // The port frees once the old task is gone; try for a few seconds.
        let mut listener = None;
        for _ in 0..50 {
            match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => {
                    listener = Some(l);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
        match listener {
            Some(l) => {
                if let Ok(mut s) = serving.lock() {
                    *s = Some(service.serve(l));
                }
                service.counters.restarts.fetch_add(1, Ordering::SeqCst);
            }
            None => {
                tracing::warn!(
                    code = "gateway.restart_failed",
                    "couldn't reopen the gateway's port"
                );
            }
        }
    }
}

/// Bearer lease check, and MCP sessions kept to the lease that opened them.
async fn guard(
    State((leases, counters)): State<(Arc<lease::Leases>, Arc<Counters>)>,
    mut req: Request,
    next: Next,
) -> Response {
    counters.requests.fetch_add(1, Ordering::Relaxed);
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let Some(ctx) = token.and_then(|t| leases.find(t)) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let session = req
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    if let Some(sid) = &session {
        if leases
            .session_owner(sid)
            .is_some_and(|owner| owner != ctx.id)
        {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    req.extensions_mut().insert(ctx.clone());
    let resp = next.run(req).await;
    if session.is_none() {
        if let Some(sid) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            leases.bind_session(sid, &ctx.id);
        }
    }
    resp
}

/// The caller's lease, as the guard attached it to the HTTP request.
fn lease_of(ctx: &RequestContext<RoleServer>) -> Result<LeaseContext, ErrorData> {
    ctx.extensions
        .get::<axum::http::request::Parts>()
        .and_then(|p| p.extensions.get::<LeaseContext>())
        .cloned()
        .ok_or_else(|| ErrorData::invalid_request("no lease", None))
}

#[derive(Clone)]
struct GatewayServer {
    approvals: Arc<dyn ApprovalAsker>,
    builtins: Arc<dyn BuiltinHandler>,
    plugins: Arc<plugins::Plugins>,
}

fn reply(out: ToolOutput) -> CallToolResponse {
    if out.is_error {
        CallToolResult::error(vec![ContentBlock::text(out.text)]).into()
    } else {
        CallToolResult::success(vec![ContentBlock::text(out.text)]).into()
    }
}

impl ServerHandler for GatewayServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("vibezeo", env!("CARGO_PKG_VERSION")))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let lease = lease_of(&ctx)?;
        let set = lease.session.builtins;
        let mut tools: Vec<Tool> = Vec::new();
        if set.approvals {
            tools.push(approvals::tool());
        }
        tools.extend(
            builtins::BUILTINS
                .iter()
                .filter(|b| b.enabled(&set))
                .map(|b| b.tool()),
        );
        tools.extend(self.plugins.list(&lease).await);
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let lease = lease_of(&ctx)?;
        let args = serde_json::Value::Object(request.arguments.unwrap_or_default());
        let name = request.name.as_ref();
        if name == APPROVALS_TOOL && lease.session.builtins.approvals {
            let Some(ask) = ToolApproval::from_args(&args) else {
                return Ok(CallToolResult::error(vec![ContentBlock::text(
                    "tool_name and input are required",
                )])
                .into());
            };
            let answer = tokio::select! {
                a = self.approvals.ask(&lease.session.caller, ask.clone()) => a,
                // The engine gave up on the request: nobody is asked any more.
                _ = ctx.ct.cancelled() => ApprovalAnswer::Deny { message: None },
            };
            return Ok(
                CallToolResult::success(vec![ContentBlock::text(answer.reply(&ask))]).into(),
            );
        }
        if let Some(b) = builtins::find(name, &lease.session.builtins) {
            return Ok(reply(self.builtins.call(&lease, b.builtin, args).await));
        }
        if name == SEARCH_TOOLS {
            return Ok(reply(self.plugins.search(&lease, &args).await));
        }
        if name == CALL {
            let Some(tool) = args.get("tool").and_then(serde_json::Value::as_str) else {
                return Ok(reply(ToolOutput {
                    text: "tool is required".into(),
                    is_error: true,
                }));
            };
            let inner = args
                .get("args")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            return Ok(reply(self.plugins.call(&lease, tool, inner).await));
        }
        if !name.starts_with("tandem__") && !name.starts_with("tandem_approvals__") {
            // A plugin tool by its own name (listed when "Expose all plugin
            // tools directly" is on).
            return Ok(reply(self.plugins.call(&lease, name, args).await));
        }
        Ok(CallToolResult::error(vec![ContentBlock::text(format!("Unknown tool: {name}"))]).into())
    }
}

#[cfg(test)]
mod tests;
