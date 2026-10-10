//! The gateway over real HTTP (F-PL-01 AC1/AC2, F-TH-04 AC4).

use super::*;
use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};
use std::time::Duration;
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
    SessionDescriptor::new(
        Caller::Thread {
            thread_id: ThreadId::new(),
        },
        Attended::Interactive,
        Access::Edits,
        BuiltinSet::for_thread(),
    )
}

async fn gateway() -> GatewayHandle {
    Gateway::start(GatewayDeps::new(Arc::new(Fake)))
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

// ── plugins, built-ins, watchdog and metrics (M10 task 1) ────────────────

use tandem_core::ids::{ConnectionId, TeammateId};

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
    ConnectionInfo {
        id: ConnectionId::new(),
        plugin_id: plugin.into(),
        account_label: None,
        connected: true,
    }
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

#[tokio::test]
async fn f_pl_01_3_three_granted_plugins_list_search_and_call_plus_the_built_ins() {
    let gw = gateway_with(None, WATCHDOG_EVERY).await;
    let conns = vec![
        connection("test_echo"),
        connection("test_upper"),
        connection("test_down"),
    ];
    gw.set_connections(conns.clone());
    let lease = gw.prepare(teammate_session(
        conns.iter().map(|c| c.id.clone()).collect(),
        true,
    ));
    let sid = open(&gw.url, &lease.token).await;
    let mut names = list_names(&gw.url, &lease.token, &sid).await;
    names.sort();
    assert_eq!(
        names,
        vec![
            "tandem__call",
            "tandem__list_teammates",
            "tandem__memory_write",
            "tandem__read_skill",
            "tandem__search_tools",
            "tandem__send_message",
            "tandem__session_search",
        ],
        "no vendor tools up front"
    );

    // The search finds the tools; the vendor that's down is named, not fatal.
    let (err, found) = tool(
        &gw.url,
        &lease.token,
        &sid,
        "tandem__search_tools",
        json!({"query": ""}),
    )
    .await;
    assert!(!err);
    let found: Value = serde_json::from_str(&found).expect("json");
    let mut tools: Vec<&str> = found["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    tools.sort();
    assert_eq!(
        tools,
        vec!["test_echo__echo", "test_echo__reverse", "test_upper__upper"]
    );
    assert_eq!(found["tools"][0]["risk_class"], "read");
    assert_eq!(found["unavailable"][0]["plugin"], "test_down");

    let (err, out) = tool(
        &gw.url,
        &lease.token,
        &sid,
        "tandem__call",
        json!({"tool": "test_upper__upper", "args": {"text": "hi"}}),
    )
    .await;
    assert!(!err);
    assert_eq!(out, "HI");
    // F-PL-01: an unavailable plugin doesn't break the others.
    let (err, out) = tool(
        &gw.url,
        &lease.token,
        &sid,
        "tandem__call",
        json!({"tool": "test_down__anything", "args": {}}),
    )
    .await;
    assert!(err);
    assert!(out.contains("can't be reached"), "{out}");
    let (err, out) = tool(
        &gw.url,
        &lease.token,
        &sid,
        "tandem__call",
        json!({"tool": "test_echo__echo", "args": {"text": "still here"}}),
    )
    .await;
    assert!(!err);
    assert_eq!(out, "still here");
    gw.shutdown();
}

#[tokio::test]
async fn f_pl_01_1_a_lease_cant_call_a_plugin_granted_only_to_another() {
    let gw = gateway_with(None, WATCHDOG_EVERY).await;
    let echo = connection("test_echo");
    let upper = connection("test_upper");
    gw.set_connections(vec![echo.clone(), upper.clone()]);
    let a = gw.prepare(teammate_session(vec![upper.id.clone()], false));
    let b = gw.prepare(teammate_session(vec![echo.id.clone()], false));
    let sa = open(&gw.url, &a.token).await;
    let (err, out) = tool(
        &gw.url,
        &a.token,
        &sa,
        "tandem__call",
        json!({"tool": "test_echo__echo", "args": {"text": "x"}}),
    )
    .await;
    assert!(err);
    assert_eq!(out, "The test_echo plugin isn't enabled for this session");
    // By its own name too.
    let (err, _) = tool(
        &gw.url,
        &a.token,
        &sa,
        "test_echo__echo",
        json!({"text": "x"}),
    )
    .await;
    assert!(err);
    // B, which has it, can.
    let sb = open(&gw.url, &b.token).await;
    let (err, out) = tool(
        &gw.url,
        &b.token,
        &sb,
        "tandem__call",
        json!({"tool": "test_echo__echo", "args": {"text": "x"}}),
    )
    .await;
    assert!(!err);
    assert_eq!(out, "x");
    gw.shutdown();
}

#[tokio::test]
async fn f_pl_01_4_a_disconnected_plugin_stops_for_in_flight_leases_on_the_next_call() {
    let gw = gateway_with(None, WATCHDOG_EVERY).await;
    let echo = connection("test_echo");
    gw.set_connections(vec![echo.clone()]);
    let lease = gw.prepare(teammate_session(vec![echo.id.clone()], false));
    let sid = open(&gw.url, &lease.token).await;
    let args = json!({"tool": "test_echo__echo", "args": {"text": "x"}});
    assert!(
        !tool(&gw.url, &lease.token, &sid, "tandem__call", args.clone())
            .await
            .0
    );
    let before = gw.generation();
    gw.invalidate_connection(&echo.id);
    assert!(gw.generation() > before);
    let (err, out) = tool(&gw.url, &lease.token, &sid, "tandem__call", args).await;
    assert!(err);
    assert_eq!(out, "No plugin named test_echo is connected");
    assert!(
        !list_names(&gw.url, &lease.token, &sid)
            .await
            .contains(&"tandem__call".to_string()),
        "nothing granted is left to search"
    );
    gw.shutdown();
}

#[tokio::test]
async fn f_pl_01_expose_all_lists_every_granted_tool_by_its_plugin_name() {
    let gw = gateway_with(None, WATCHDOG_EVERY).await;
    let conns = vec![connection("test_echo"), connection("test_down")];
    gw.set_connections(conns.clone());
    gw.set_expose_all(true);
    let lease = gw.prepare(teammate_session(
        conns.iter().map(|c| c.id.clone()).collect(),
        false,
    ));
    let sid = open(&gw.url, &lease.token).await;
    let names = list_names(&gw.url, &lease.token, &sid).await;
    assert!(names.contains(&"test_echo__echo".to_string()), "{names:?}");
    assert!(names.contains(&"test_echo__reverse".to_string()));
    assert!(!names.contains(&"tandem__search_tools".to_string()));
    assert!(
        !names.iter().any(|n| n.starts_with("test_down")),
        "the vendor that's down lists nothing and breaks nothing"
    );
    let (err, out) = tool(
        &gw.url,
        &lease.token,
        &sid,
        "test_echo__reverse",
        json!({"text": "abc"}),
    )
    .await;
    assert!(!err);
    assert_eq!(out, "cba");
    gw.shutdown();
}

#[tokio::test]
async fn f_ag_05_2_without_may_message_the_mail_tools_are_absent_and_refused() {
    let gw = gateway_with(None, WATCHDOG_EVERY).await;
    let quiet = gw.prepare(teammate_session(Vec::new(), false));
    let sid = open(&gw.url, &quiet.token).await;
    let names = list_names(&gw.url, &quiet.token, &sid).await;
    assert!(!names.contains(&"tandem__send_message".to_string()));
    assert!(!names.contains(&"tandem__list_teammates".to_string()));
    assert!(names.contains(&"tandem__memory_write".to_string()));
    let (err, out) = tool(
        &gw.url,
        &quiet.token,
        &sid,
        "tandem__send_message",
        json!({"to": "Lead", "body": "hi"}),
    )
    .await;
    assert!(err);
    assert_eq!(out, "Unknown tool: tandem__send_message");
    // A built-in reaches the app's handler with the lease's teammate.
    let (err, out) = tool(
        &gw.url,
        &quiet.token,
        &sid,
        "tandem__session_search",
        json!({"query": "release"}),
    )
    .await;
    assert!(!err);
    let out: Value = serde_json::from_str(&out).expect("json");
    assert_eq!(out["builtin"], "SessionSearch");
    assert!(out["teammate"]
        .as_str()
        .is_some_and(|t| t.starts_with("tm_")));
    gw.shutdown();
}

#[tokio::test]
async fn f_pl_01_the_port_goes_to_gateway_json_and_the_watchdog_brings_the_listener_back() {
    let dir = tempfile::tempdir().expect("dir");
    let gw = gateway_with(Some(dir.path().to_path_buf()), Duration::from_millis(100)).await;
    let run: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("gateway.json")).expect("gateway.json"),
    )
    .expect("json");
    assert_eq!(run["port"].as_u64(), Some(u64::from(gw.port())));
    let lease = gw.prepare(thread_session());
    open(&gw.url, &lease.token).await;
    assert_eq!(gw.metrics().listeners, 1);

    gw.kill_listener();
    let mut back = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if gw.metrics().restarts >= 1 && gw.metrics().listeners == 1 {
            back = true;
            break;
        }
    }
    assert!(
        back,
        "the watchdog restarted the listener: {:?}",
        gw.metrics()
    );
    // Same port, same leases: the session's config still works.
    assert_eq!(rpc(&gw.url, None, None, initialize()).await.0, 401);
    open(&gw.url, &lease.token).await;
    gw.shutdown();
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

#[tokio::test]
async fn nfr_5_2_a_gateway_that_keeps_failing_is_reported_after_three_restarts() {
    let bus = tandem_core::bus::EventBus::new();
    let mut rx = bus.subscribe(tandem_core::bus::Topic::App);
    let mut deps = GatewayDeps::new(Arc::new(Fake));
    deps.watchdog_every = Duration::from_millis(50);
    deps.bus = Some(bus);
    let gw = Gateway::start(deps).await.expect("gateway");
    // Kill it four times: three restarts, then the report.
    for _ in 0..4 {
        let before = gw.metrics().restarts;
        gw.kill_listener();
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if gw.metrics().restarts > before || before == 3 {
                break;
            }
        }
    }
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("reported in time")
        .expect("event");
    assert_eq!(event.name, "app:subsystem-failed");
    assert_eq!(event.payload["name"], "gateway");
    assert_eq!(event.payload["restarts"], 3);
    assert_eq!(gw.metrics().restarts, 3);
    gw.shutdown();
}
