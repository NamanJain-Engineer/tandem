//! F-PL-01 AC2's memory check runs alone in this binary: it reads the
//! process's resident memory, which other tests in the same process (mock
//! upstreams, OAuth servers) would add to.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};
use tandem_core::access::Access;
use tandem_core::ids::{ConnectionId, TeammateId, ThreadId};
use tandem_gateway::*;

/// Allows `npm test`, denies anything else with a reason.
struct Fake;

#[async_trait]
impl ApprovalAsker for Fake {
    async fn ask(&self, _caller: &Caller, ask: ToolApproval) -> ApprovalAnswer {
        if ask.input.get("command").and_then(Value::as_str) == Some("npm test") {
            ApprovalAnswer::Allow
        } else {
            ApprovalAnswer::Deny {
                message: Some("Not on this branch".into()),
            }
        }
    }
}

/// One JSON-RPC exchange: the response headers and the message with an id
/// (from a JSON body or an SSE stream).
async fn rpc(
    url: &str,
    token: Option<&str>,
    session: Option<&str>,
    body: Value,
) -> (u16, HeaderMap, Option<Value>) {
    let client = reqwest::Client::new();
    let mut req = client
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .json(&body);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    if let Some(s) = session {
        req = req
            .header("mcp-session-id", s)
            .header("mcp-protocol-version", "2025-03-26");
    }
    let resp = req.send().await.expect("send");
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    let text = resp.text().await.unwrap_or_default();
    let message = serde_json::from_str::<Value>(&text).ok().or_else(|| {
        text.lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
            .find(|m| m.get("id").is_some())
    });
    (status, headers, message)
}

fn initialize() -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-03-26",
        "capabilities": {},
        "clientInfo": {"name": "test", "version": "1"}
    }})
}

/// Initializes a session with `token`; returns its session id.
async fn open(url: &str, token: &str) -> String {
    let (status, headers, message) = rpc(url, Some(token), None, initialize()).await;
    assert_eq!(status, 200, "initialize: {message:?}");
    let session = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .expect("session id")
        .to_string();
    let (status, _, _) = rpc(
        url,
        Some(token),
        Some(&session),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await;
    assert!(status == 202 || status == 200, "initialized: {status}");
    session
}

/// Answers each built-in with its name and the lease's teammate.
struct EchoBuiltins;

#[async_trait]
impl BuiltinHandler for EchoBuiltins {
    async fn call(&self, lease: &LeaseContext, builtin: Builtin, args: Value) -> ToolOutput {
        ToolOutput {
            text: json!({
                "builtin": format!("{builtin:?}"),
                "teammate": lease.session.caller.teammate().map(|t| t.to_string()),
                "args": args,
            })
            .to_string(),
            is_error: false,
        }
    }
}

async fn gateway_with(run_dir: Option<std::path::PathBuf>, every: Duration) -> GatewayHandle {
    let mut deps = GatewayDeps::new(Arc::new(Fake));
    deps.builtins = Arc::new(EchoBuiltins);
    deps.run_dir = run_dir;
    deps.watchdog_every = every;
    Gateway::start(deps).await.expect("gateway")
}

fn connection(plugin: &str) -> ConnectionInfo {
    ConnectionInfo::new(ConnectionId::new(), plugin)
}

fn teammate_session(granted: Vec<ConnectionId>, may_message: bool) -> SessionDescriptor {
    let mut s = SessionDescriptor::new(
        Caller::TeammateChat {
            teammate_id: TeammateId::new(),
            thread_id: ThreadId::new(),
        },
        Attended::Interactive,
        Access::Edits,
        BuiltinSet::for_teammate(may_message, false),
    );
    s.granted = granted;
    s
}

async fn list_names(url: &str, token: &str, session: &str) -> Vec<String> {
    let (_, _, listed) = rpc(
        url,
        Some(token),
        Some(session),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .await;
    listed
        .as_ref()
        .and_then(|m| m.pointer("/result/tools"))
        .and_then(Value::as_array)
        .map(|t| {
            t.iter()
                .filter_map(|t| t["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// A tools/call: (is_error, text).
async fn tool(url: &str, token: &str, session: &str, name: &str, args: Value) -> (bool, String) {
    let (status, _, message) = rpc(
        url,
        Some(token),
        Some(session),
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {
            "name": name, "arguments": args
        }}),
    )
    .await;
    assert_eq!(status, 200);
    let result = message
        .as_ref()
        .and_then(|m| m.get("result"))
        .unwrap_or_else(|| panic!("no result: {message:?}"));
    (
        result["isError"].as_bool().unwrap_or(false),
        result
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f_pl_01_2_ten_sessions_share_one_listener_and_memory_stays_under_80_mb() {
    let gw = gateway_with(None, WATCHDOG_EVERY).await;
    let echo = connection("test_echo");
    gw.set_connections(vec![echo.clone()]);
    let mut sessions = Vec::new();
    for _ in 0..10 {
        let lease = gw.prepare(teammate_session(vec![echo.id.clone()], true));
        let sid = open(&gw.url, &lease.token).await;
        sessions.push((lease, sid));
    }
    for round in 0..20 {
        for (lease, sid) in &sessions {
            let (err, _) = tool(
                &gw.url,
                &lease.token,
                sid,
                "tandem__call",
                json!({"tool": "test_echo__echo", "args": {"text": format!("round {round}")}}),
            )
            .await;
            assert!(!err);
            list_names(&gw.url, &lease.token, sid).await;
        }
    }
    let m = gw.metrics();
    assert_eq!(m.listeners, 1, "one listener for every session");
    assert_eq!(m.leases, 10);
    assert_eq!(m.mcp_sessions, 10);
    assert!(m.requests >= 400, "{m:?}");
    #[cfg(target_os = "linux")]
    {
        let rss = m.rss_bytes.expect("rss on Linux");
        assert!(
            rss <= 80 * 1024 * 1024,
            "gateway process memory {} MB",
            rss / (1024 * 1024)
        );
    }
    gw.shutdown();
}
