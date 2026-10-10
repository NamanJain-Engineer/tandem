//! F-TL-06 rule 3: credentials go only to pinned hosts (F-PL-04). Today the
//! only credential Vibezeo hands out is a gateway lease, and the gateway
//! serves it on loopback only: no web page (Origin) and no rebound name
//! (Host) can use it. Plugin credentials and their pinned hosts join this
//! file when M12 builds F-PL-04.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tandem_core::access::Access;
use tandem_core::ids::ThreadId;
use tandem_gateway::{
    ApprovalAnswer, ApprovalAsker, Attended, BuiltinSet, Caller, Gateway, GatewayDeps,
    SessionDescriptor, ToolApproval,
};

struct Deny;

#[async_trait]
impl ApprovalAsker for Deny {
    async fn ask(&self, _caller: &Caller, _ask: ToolApproval) -> ApprovalAnswer {
        ApprovalAnswer::Deny { message: None }
    }
}

#[tokio::test]
async fn invariant_rule_3_the_gateway_lease_is_served_on_loopback_only() {
    let gw = Gateway::start(GatewayDeps {
        approvals: Arc::new(Deny),
        bus: None,
    })
    .await
    .expect("gateway");
    assert!(gw.url.starts_with("http://127.0.0.1:"), "{}", gw.url);
    let lease = gw.prepare(SessionDescriptor {
        caller: Caller::Thread {
            thread_id: ThreadId::new(),
        },
        attended: Attended::Interactive,
        access: Access::Edits,
        builtins: BuiltinSet::for_thread(),
    });
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "rule-3", "version": "1"}
    }});
    let client = reqwest::Client::new();
    let send = |header: (&'static str, &'static str)| {
        client
            .post(&gw.url)
            .bearer_auth(&lease.token)
            .header("accept", "application/json, text/event-stream")
            .header(header.0, header.1)
            .json(&init)
            .send()
    };
    let from_web = send(("origin", "https://evil.example"))
        .await
        .expect("send");
    assert_eq!(from_web.status().as_u16(), 403);
    let rebound = send(("host", "evil.example")).await.expect("send");
    assert!(rebound.status().is_client_error(), "{}", rebound.status());
    let local = send(("origin", "http://127.0.0.1")).await.expect("send");
    assert!(local.status().is_success(), "{}", local.status());
    gw.shutdown();
}
