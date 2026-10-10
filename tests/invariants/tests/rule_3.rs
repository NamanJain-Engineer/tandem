//! F-TL-06 rule 3: credentials go only to pinned hosts (F-PL-04). Today the
//! only credential Vibezeo hands out is a gateway lease, and the gateway
//! serves it on loopback only: no web page (Origin) and no rebound name
//! (Host) can use it, and plugin credentials go only to their pinned
//! scheme, host, port and path prefix (M12).

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
    let gw = Gateway::start(GatewayDeps::new(Arc::new(Deny)))
        .await
        .expect("gateway");
    assert!(gw.url.starts_with("http://127.0.0.1:"), "{}", gw.url);
    let lease = gw.prepare(SessionDescriptor::new(
        Caller::Thread {
            thread_id: ThreadId::new(),
        },
        Attended::Interactive,
        Access::Edits,
        BuiltinSet::for_thread(),
    ));
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

/// F-TL-06 rule 3, F-PL-04: a plugin credential goes only to its pinned
/// scheme + host + port + path prefix. Near misses and redirects off the pin
/// get the request without it; nothing the caller sets can add it.
#[tokio::test]
async fn invariant_rule_3_plugin_credentials_go_only_to_pinned_hosts() {
    use tandem_gateway::netguard::PinnedHost;
    use tandem_gateway::pinned::{Outgoing, PinnedClient};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    let pinned = MockServer::start().await;
    let other = MockServer::start().await;
    let echo = |r: &Request| {
        ResponseTemplate::new(200).set_body_string(if r.headers.contains_key("authorization") {
            "CRED"
        } else {
            "none"
        })
    };
    Mock::given(method("GET"))
        .and(path("/v1/away"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/v1/x", other.uri()).as_str()),
        )
        .mount(&pinned)
        .await;
    Mock::given(method("GET"))
        .respond_with(echo)
        .mount(&pinned)
        .await;
    Mock::given(method("GET"))
        .respond_with(echo)
        .mount(&other)
        .await;
    let pin_of = |s: &MockServer, prefix: &str| {
        let u = url::Url::parse(&s.uri()).expect("uri");
        PinnedHost::new(
            "http",
            u.host_str().expect("host"),
            u.port().expect("port"),
            prefix,
        )
    };
    let client = || {
        PinnedClient::with_bearer(vec![pin_of(&pinned, "/v1/")], "rule-3-secret-token")
            .reachable(pin_of(&pinned, "/"))
            .reachable(pin_of(&other, "/"))
    };
    let get = |c: PinnedClient, u: String| async move {
        c.send(Outgoing::get(url::Url::parse(&u).expect("url")))
            .await
            .expect("send")
            .body
    };
    assert_eq!(
        get(client(), format!("{}/v1/issues", pinned.uri())).await,
        "CRED"
    );
    assert_eq!(
        get(client(), format!("{}/v2/issues", pinned.uri())).await,
        "none"
    );
    assert_eq!(
        get(client(), format!("{}/v1/issues", other.uri())).await,
        "none"
    );
    assert_eq!(
        get(client(), format!("{}/v1/away", pinned.uri())).await,
        "none"
    );
    // A redirect to a private address that isn't allowed is refused.
    let strict = PinnedClient::with_bearer(vec![pin_of(&pinned, "/v1/")], "rule-3-secret-token");
    assert!(strict
        .send(Outgoing::get(
            url::Url::parse(&format!("{}/v1/away", pinned.uri())).expect("url")
        ))
        .await
        .is_err());
    for r in other.received_requests().await.expect("seen") {
        assert!(
            !r.headers.contains_key("authorization"),
            "credential sent off the pin: {}",
            r.url.path()
        );
    }
}
