//! The local MCP gateway (ARCHITECTURE §2.7, §11, F-PL-01): one Streamable
//! HTTP server on `127.0.0.1:<random>/mcp` for every engine session of this
//! app. Each session gets a lease: a 32-byte bearer token, of which only the
//! hash is kept. A request without a live lease gets 401, an MCP session is
//! bound to the lease that opened it, and only loopback `Host`/`Origin`
//! values are accepted (DNS rebinding).
//!
//! M7 slice: the server, leases and the built-in
//! `tandem_approvals__request` (Claude's permission prompt tool, F-TH-04).
//! Plugins, OAuth and the rest of `tandem__*` arrive with M10.

pub mod approvals;
pub mod lease;

use std::sync::Arc;

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
pub use lease::{Attended, BuiltinSet, Caller, Lease, LeaseContext, SessionDescriptor};

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("couldn't open the gateway's port: {0}")]
    Bind(std::io::Error),
}

/// What the gateway serves with.
pub struct GatewayDeps {
    pub approvals: Arc<dyn ApprovalAsker>,
    /// Where a gateway that keeps failing is reported (`app:subsystem-failed`).
    pub bus: Option<tandem_core::bus::EventBus>,
}

/// The running gateway. Cloning shares it.
#[derive(Clone)]
pub struct GatewayHandle {
    /// `http://127.0.0.1:<port>/mcp`
    pub url: String,
    leases: Arc<lease::Leases>,
    shutdown: CancellationToken,
    /// Ends the current run as a failure would (tests of the watchdog).
    #[cfg_attr(not(test), allow(dead_code))]
    fail_run: Arc<std::sync::Mutex<CancellationToken>>,
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

    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }

    /// Stops the server as if it had failed; the watchdog brings it back on
    /// the same address.
    #[cfg(test)]
    pub(crate) fn fail_for_test(&self) {
        let mut current = self.fail_run.lock().unwrap_or_else(|e| e.into_inner());
        current.cancel();
        *current = CancellationToken::new();
    }
}

pub struct Gateway;

impl Gateway {
    /// Binds `127.0.0.1:0` and serves until [`GatewayHandle::shutdown`].
    pub async fn start(deps: GatewayDeps) -> Result<GatewayHandle, GatewayError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(GatewayError::Bind)?;
        let addr = listener.local_addr().map_err(GatewayError::Bind)?;
        let leases = Arc::new(lease::Leases::default());
        let shutdown = CancellationToken::new();
        let server = GatewayServer {
            approvals: deps.approvals,
        };
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
        config.cancellation_token = shutdown.child_token();
        let mcp: StreamableHttpService<GatewayServer, LocalSessionManager> =
            StreamableHttpService::new(move || Ok(server.clone()), Default::default(), config);
        let router = axum::Router::new()
            .nest_service("/mcp", mcp)
            .layer(axum::middleware::from_fn_with_state(leases.clone(), guard));
        // The server runs under the watchdog (§5.2): if it stops without a
        // shutdown it's served again on the same address, at most 3 times
        // per 10 minutes, then reported.
        let fail_run = Arc::new(std::sync::Mutex::new(CancellationToken::new()));
        let first = Arc::new(std::sync::Mutex::new(Some(listener)));
        let sup = tandem_core::supervisor::Supervision {
            clock: Arc::new(tandem_core::clock::SystemClock),
            bus: deps.bus.unwrap_or_default(),
            watchdog: tandem_core::supervisor::watchdog(),
        };
        let (stop, failing) = (shutdown.clone(), fail_run.clone());
        tokio::spawn(async move {
            tandem_core::supervisor::supervise_task("gateway", sup, None, move || {
                let (router, stop) = (router.clone(), stop.clone());
                let fail = failing.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let first = first.lock().unwrap_or_else(|e| e.into_inner()).take();
                async move {
                    let listener = match first {
                        Some(l) => l,
                        None => tokio::net::TcpListener::bind(addr)
                            .await
                            .map_err(|e| format!("couldn't listen again: {e}"))?,
                    };
                    let ended = stop.clone();
                    let served = axum::serve(listener, router)
                        .with_graceful_shutdown(async move {
                            tokio::select! {
                                _ = ended.cancelled() => {}
                                _ = fail.cancelled() => {}
                            }
                        })
                        .await;
                    if stop.is_cancelled() {
                        return Ok(());
                    }
                    tracing::warn!(code = "gateway.stopped", "the gateway stopped");
                    Err(match served {
                        Err(e) => e.to_string(),
                        Ok(()) => "the server stopped".to_string(),
                    })
                }
            })
            .await;
        });
        tracing::info!(
            code = "gateway.started",
            "MCP gateway listening on loopback"
        );
        Ok(GatewayHandle {
            url: format!("http://{addr}/mcp"),
            leases,
            shutdown,
            fail_run,
        })
    }
}

/// Bearer lease check, and MCP sessions kept to the lease that opened them.
async fn guard(State(leases): State<Arc<lease::Leases>>, mut req: Request, next: Next) -> Response {
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
        let mut tools: Vec<Tool> = Vec::new();
        if lease.session.builtins.approvals {
            tools.push(approvals::tool());
        }
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let lease = lease_of(&ctx)?;
        if request.name == APPROVALS_TOOL && lease.session.builtins.approvals {
            let args = serde_json::Value::Object(request.arguments.unwrap_or_default());
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
        Ok(CallToolResult::error(vec![ContentBlock::text(format!(
            "Unknown tool: {}",
            request.name
        ))])
        .into())
    }
}

#[cfg(test)]
mod tests;
