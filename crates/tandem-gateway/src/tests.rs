//! The gateway over real HTTP (F-PL-01 AC1/AC2, F-TH-04 AC4).

use super::*;
use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};
use tandem_core::access::Access;
use tandem_core::ids::ThreadId;

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

fn thread_session() -> SessionDescriptor {
    SessionDescriptor {
        caller: Caller::Thread {
            thread_id: ThreadId::new(),
        },
        attended: Attended::Interactive,
        access: Access::Edits,
        builtins: BuiltinSet::for_thread(),
    }
}

async fn gateway() -> GatewayHandle {
    Gateway::start(GatewayDeps {
        approvals: Arc::new(Fake),
    })
    .await
    .expect("gateway")
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

async fn call(url: &str, token: &str, session: &str, args: Value) -> Value {
    let (status, _, message) = rpc(
        url,
        Some(token),
        Some(session),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
            "name": APPROVALS_TOOL, "arguments": args
        }}),
    )
    .await;
    assert_eq!(status, 200);
    let text = message
        .as_ref()
        .and_then(|m| m.pointer("/result/content/0/text"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no tool text: {message:?}"));
    serde_json::from_str(text).expect("reply json")
}

#[tokio::test]
async fn f_pl_01_1_a_request_without_a_live_lease_gets_401() {
    let gw = gateway().await;
    assert!(gw.url.starts_with("http://127.0.0.1:"));
    assert!(gw.url.ends_with("/mcp"));
    assert_eq!(rpc(&gw.url, None, None, initialize()).await.0, 401);
    assert_eq!(
        rpc(&gw.url, Some("not-a-lease"), None, initialize())
            .await
            .0,
        401
    );
    let lease = gw.prepare(thread_session());
    assert_eq!(lease.token.len(), 43, "32 bytes, base64url");
    assert!(
        !format!("{lease:?}").contains(&lease.token),
        "Debug hides the token"
    );
    open(&gw.url, &lease.token).await;
    gw.revoke(&lease.id);
    assert_eq!(
        rpc(&gw.url, Some(&lease.token), None, initialize()).await.0,
        401
    );
    gw.shutdown();
}

#[tokio::test]
async fn f_th_04_4_the_approvals_tool_answers_allow_with_the_input_or_deny_with_a_message() {
    let gw = gateway().await;
    let lease = gw.prepare(thread_session());
    let session = open(&gw.url, &lease.token).await;
    let (_, _, listed) = rpc(
        &gw.url,
        Some(&lease.token),
        Some(&session),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .await;
    let names: Vec<&str> = listed
        .as_ref()
        .and_then(|m| m.pointer("/result/tools"))
        .and_then(Value::as_array)
        .map(|t| t.iter().filter_map(|t| t["name"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(names, vec![APPROVALS_TOOL]);

    let allowed = call(
        &gw.url,
        &lease.token,
        &session,
        json!({"tool_name": "Bash", "input": {"command": "npm test"}, "tool_use_id": "toolu_1"}),
    )
    .await;
    assert_eq!(
        allowed,
        json!({"behavior": "allow", "updatedInput": {"command": "npm test"}})
    );
    let denied = call(
        &gw.url,
        &lease.token,
        &session,
        json!({"tool_name": "Bash", "input": {"command": "rm -rf build"}}),
    )
    .await;
    assert_eq!(
        denied,
        json!({"behavior": "deny", "message": "Not on this branch"})
    );
    gw.shutdown();
}

#[tokio::test]
async fn f_pl_01_2_one_gateway_serves_ten_sessions_and_keeps_each_to_its_lease() {
    let gw = gateway().await;
    let leases: Vec<Lease> = (0..10).map(|_| gw.prepare(thread_session())).collect();
    let mut sessions = Vec::new();
    for lease in &leases {
        sessions.push(open(&gw.url, &lease.token).await);
    }
    // Another lease can't use a session it didn't open.
    let (status, _, _) = rpc(
        &gw.url,
        Some(&leases[1].token),
        Some(&sessions[0]),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .await;
    assert_eq!(status, 401);
    gw.shutdown();
}

#[tokio::test]
async fn f_pl_01_a_session_without_built_ins_lists_no_tools() {
    let gw = gateway().await;
    let mut session = thread_session();
    session.builtins = BuiltinSet::default();
    let lease = gw.prepare(session);
    let sid = open(&gw.url, &lease.token).await;
    let (_, _, listed) = rpc(
        &gw.url,
        Some(&lease.token),
        Some(&sid),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .await;
    assert_eq!(
        listed.as_ref().and_then(|m| m.pointer("/result/tools")),
        Some(&json!([]))
    );
    gw.shutdown();
}

#[tokio::test]
async fn f_pl_01_only_loopback_host_and_origin_are_served() {
    let gw = gateway().await;
    let lease = gw.prepare(thread_session());
    let client = reqwest::Client::new();
    let from_web = client
        .post(&gw.url)
        .bearer_auth(&lease.token)
        .header("accept", "application/json, text/event-stream")
        .header("origin", "https://evil.example")
        .json(&initialize())
        .send()
        .await
        .expect("send");
    assert_eq!(from_web.status().as_u16(), 403);
    let rebound = client
        .post(&gw.url)
        .bearer_auth(&lease.token)
        .header("accept", "application/json, text/event-stream")
        .header("host", "evil.example")
        .json(&initialize())
        .send()
        .await
        .expect("send");
    assert!(rebound.status().is_client_error());
    gw.shutdown();
}
