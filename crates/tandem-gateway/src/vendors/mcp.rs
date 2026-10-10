//! A vendor whose tools are a remote MCP server (Streamable HTTP): the
//! gateway is its MCP client. Tools are discovered lazily per connection,
//! cached for 10 minutes and classed by the module's rules (F-PL-02); calls
//! go to the server that listed the tool, always through the pinned client.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{json, Value};
use tandem_core::ids::ConnectionId;
use url::Url;

use super::{
    classify, AccountInfo, CallCtx, ToolDef, ToolOutput, Upstream, VendorError, VendorManifest,
    VendorModule,
};
use crate::pinned::{Outgoing, PinnedClient};

/// How long discovered tools are kept (ARCHITECTURE §11.3).
pub const DISCOVERY_TTL: Duration = Duration::from_secs(600);
const PROTOCOL: &str = "2025-03-26";

/// One MCP exchange over Streamable HTTP. Returns the JSON-RPC message
/// with an id (from a JSON body or an SSE stream) and the session id.
pub async fn rpc(
    http: &PinnedClient,
    url: &Url,
    session: Option<&str>,
    body: Value,
) -> Result<(Option<Value>, Option<String>), VendorError> {
    let mut req = Outgoing::json(Method::POST, url.clone(), &body)
        .header("accept", "application/json, text/event-stream");
    if let Some(s) = session {
        req = req
            .header("mcp-session-id", s)
            .header("mcp-protocol-version", PROTOCOL);
    }
    let resp = http.send(req).await?;
    if resp.status.as_u16() == 401 || resp.status.as_u16() == 403 {
        return Err(VendorError::Unauthorized(format!(
            "The server refused the sign-in ({})",
            resp.status.as_u16()
        )));
    }
    if resp.status.as_u16() == 404 && session.is_some() {
        return Err(VendorError::Unavailable("session expired".into()));
    }
    if !resp.status.is_success() {
        return Err(VendorError::Unavailable(format!(
            "The server answered {}: {}",
            resp.status.as_u16(),
            resp.body.trim()
        )));
    }
    let sid = resp
        .headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let message = serde_json::from_str::<Value>(&resp.body).ok().or_else(|| {
        resp.body
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
            .find(|m| m.get("id").is_some())
    });
    if let Some(err) = message.as_ref().and_then(|m| m.get("error")) {
        let text = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("error")
            .to_string();
        return Err(VendorError::Invalid(text));
    }
    Ok((message, sid))
}

/// Opens an MCP session: `initialize` then `notifications/initialized`.
pub async fn open(http: &PinnedClient, url: &Url) -> Result<Option<String>, VendorError> {
    let (_, sid) = rpc(
        http,
        url,
        None,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": PROTOCOL,
            "capabilities": {},
            "clientInfo": {"name": "vibezeo-gateway", "version": env!("CARGO_PKG_VERSION")}
        }}),
    )
    .await?;
    let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    let mut req = Outgoing::json(Method::POST, url.clone(), &note)
        .header("accept", "application/json, text/event-stream");
    if let Some(s) = &sid {
        req = req
            .header("mcp-session-id", s)
            .header("mcp-protocol-version", PROTOCOL);
    }
    http.send(req).await?;
    Ok(sid)
}

/// The text of a `tools/call` result, and whether it's an error.
pub fn result_text(result: &Value) -> ToolOutput {
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    ToolOutput { text, is_error }
}

struct Discovered {
    at: Instant,
    /// Tool name → (definition, the server that listed it).
    tools: Vec<(ToolDef, usize)>,
}

/// How a module learns who signed in: call `tool` and read `pointer`.
#[derive(Debug, Clone, Copy)]
pub struct WhoAmI {
    pub tool: &'static str,
    pub args: &'static str,
    pub pointer: &'static str,
}

pub struct McpModule {
    manifest: VendorManifest,
    urls: Vec<Url>,
    whoami: Option<WhoAmI>,
    cache: Mutex<HashMap<ConnectionId, Discovered>>,
    sessions: Mutex<HashMap<(ConnectionId, usize), String>>,
}

impl McpModule {
    pub fn new(manifest: VendorManifest, whoami: Option<WhoAmI>) -> Self {
        let urls = match &manifest.upstream {
            Upstream::RemoteMcp { urls } => {
                urls.iter().filter_map(|u| Url::parse(u).ok()).collect()
            }
            _ => Vec::new(),
        };
        Self {
            manifest,
            urls,
            whoami,
            cache: Mutex::default(),
            sessions: Mutex::default(),
        }
    }

    fn session(&self, conn: &ConnectionId, idx: usize) -> Option<String> {
        self.sessions
            .lock()
            .ok()
            .and_then(|s| s.get(&(conn.clone(), idx)).cloned())
    }

    async fn ensure_session(
        &self,
        cx: &CallCtx<'_>,
        idx: usize,
    ) -> Result<Option<String>, VendorError> {
        if let Some(s) = self.session(&cx.conn.id, idx) {
            return Ok(Some(s));
        }
        let url = self
            .urls
            .get(idx)
            .ok_or_else(|| VendorError::Unavailable("no server".into()))?;
        let sid = open(cx.http, url).await?;
        if let (Some(s), Ok(mut map)) = (&sid, self.sessions.lock()) {
            map.insert((cx.conn.id.clone(), idx), s.clone());
        }
        Ok(sid)
    }

    fn forget_session(&self, conn: &ConnectionId, idx: usize) {
        if let Ok(mut map) = self.sessions.lock() {
            map.remove(&(conn.clone(), idx));
        }
    }

    /// One request on the server's session, reopening it once if expired.
    async fn request(
        &self,
        cx: &CallCtx<'_>,
        idx: usize,
        body: Value,
    ) -> Result<Option<Value>, VendorError> {
        let url = self
            .urls
            .get(idx)
            .ok_or_else(|| VendorError::Unavailable("no server".into()))?;
        let sid = self.ensure_session(cx, idx).await?;
        match rpc(cx.http, url, sid.as_deref(), body.clone()).await {
            Err(VendorError::Unavailable(m)) if m == "session expired" => {
                self.forget_session(&cx.conn.id, idx);
                let sid = self.ensure_session(cx, idx).await?;
                Ok(rpc(cx.http, url, sid.as_deref(), body).await?.0)
            }
            other => Ok(other?.0),
        }
    }

    async fn discover(&self, cx: &CallCtx<'_>) -> Result<Vec<(ToolDef, usize)>, VendorError> {
        if let Ok(cache) = self.cache.lock() {
            if let Some(d) = cache.get(&cx.conn.id) {
                if d.at.elapsed() < DISCOVERY_TTL {
                    return Ok(d.tools.clone());
                }
            }
        }
        let mut tools = Vec::new();
        for idx in 0..self.urls.len() {
            let mut cursor: Option<String> = None;
            for _page in 0..20 {
                let params = match &cursor {
                    Some(c) => json!({"cursor": c}),
                    None => json!({}),
                };
                let msg = self
                    .request(
                        cx,
                        idx,
                        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": params}),
                    )
                    .await?;
                let result = msg.as_ref().and_then(|m| m.get("result"));
                for t in result
                    .and_then(|r| r.get("tools"))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let Some(name) = t.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    if name.contains("__")
                        || tools.iter().any(|(d, _): &(ToolDef, usize)| d.name == name)
                    {
                        continue;
                    }
                    tools.push((
                        ToolDef {
                            name: name.to_string(),
                            description: t
                                .get("description")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            input_schema: t
                                .get("inputSchema")
                                .cloned()
                                .filter(Value::is_object)
                                .unwrap_or_else(|| json!({"type": "object"})),
                            risk_class: classify(&self.manifest.risk_rules, name, cx.conn.mode()),
                        },
                        idx,
                    ));
                }
                cursor = result
                    .and_then(|r| r.get("nextCursor"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if cursor.is_none() {
                    break;
                }
            }
        }
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(
                cx.conn.id.clone(),
                Discovered {
                    at: Instant::now(),
                    tools: tools.clone(),
                },
            );
        }
        Ok(tools)
    }
}

#[async_trait]
impl VendorModule for McpModule {
    fn manifest(&self) -> &VendorManifest {
        &self.manifest
    }

    async fn tools(&self, cx: &CallCtx<'_>) -> Result<Vec<ToolDef>, VendorError> {
        Ok(self
            .discover(cx)
            .await?
            .into_iter()
            .map(|(d, _)| d)
            .collect())
    }

    async fn call(
        &self,
        cx: &CallCtx<'_>,
        tool: &str,
        args: Value,
    ) -> Result<ToolOutput, VendorError> {
        let idx = self
            .discover(cx)
            .await?
            .into_iter()
            .find(|(d, _)| d.name == tool)
            .map(|(_, i)| i)
            .ok_or_else(|| VendorError::UnknownTool(tool.to_string()))?;
        let msg = self
            .request(
                cx,
                idx,
                json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                       "params": {"name": tool, "arguments": args}}),
            )
            .await?;
        let result = msg
            .as_ref()
            .and_then(|m| m.get("result"))
            .ok_or_else(|| VendorError::Unavailable("The server sent no result".into()))?;
        Ok(result_text(result))
    }

    async fn verify(&self, cx: &CallCtx<'_>) -> Result<AccountInfo, VendorError> {
        // A fresh look, not the cache: the credential is what's on trial.
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(&cx.conn.id);
        }
        for idx in 0..self.urls.len() {
            self.forget_session(&cx.conn.id, idx);
        }
        self.discover(cx).await?;
        let mut info = AccountInfo::default();
        if let Some(w) = self.whoami {
            let args: Value = serde_json::from_str(w.args).unwrap_or_else(|_| json!({}));
            let out = self.call(cx, w.tool, args).await?;
            if out.is_error {
                return Err(VendorError::Invalid(out.text));
            }
            info.label = serde_json::from_str::<Value>(&out.text)
                .ok()
                .and_then(|v| v.pointer(w.pointer).cloned())
                .and_then(|v| v.as_str().map(str::to_string));
        }
        Ok(info)
    }
}
