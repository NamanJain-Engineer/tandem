//! A tiny MCP client for the gateway cases (rules 4 and 8).
#![allow(dead_code)]

use serde_json::{json, Value};

/// One JSON-RPC exchange; the message with an id (JSON body or SSE).
pub async fn rpc(
    url: &str,
    token: &str,
    session: Option<&str>,
    body: Value,
) -> (Option<String>, Option<Value>) {
    let mut req = reqwest::Client::new()
        .post(url)
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .json(&body);
    if let Some(s) = session {
        req = req
            .header("mcp-session-id", s)
            .header("mcp-protocol-version", "2025-03-26");
    }
    let resp = req.send().await.expect("send");
    let sid = resp
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let text = resp.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<Value>(&text).ok().or_else(|| {
        text.lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
            .find(|m| m.get("id").is_some())
    });
    (sid, msg)
}

pub async fn open(url: &str, token: &str) -> String {
    let (sid, _) = rpc(url, token, None, json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "invariants", "version": "1"}}})).await;
    let sid = sid.expect("session");
    rpc(
        url,
        token,
        Some(&sid),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await;
    sid
}

/// Calls `tandem__call` with a plugin tool; returns (is_error, text).
pub async fn call(url: &str, token: &str, sid: &str, tool: &str, args: Value) -> (bool, String) {
    let (_, msg) = rpc(
        url,
        token,
        Some(sid),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "tandem__call", "arguments": {"tool": tool, "args": args}}}),
    )
    .await;
    let msg = msg.expect("reply");
    let text = msg
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let err = msg
        .pointer("/result/isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    (err, text)
}
