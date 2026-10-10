//! F-PL-09 AC1: a fixture contract test per ready module. Each vendor's
//! upstream is a local mock (wiremock) answering with the tool list and
//! replies its docs describe; the test checks the module signs in with the
//! credential only at its pinned upstream, names the account, classes every
//! tool, calls one and passes back what the vendor said, with any echo of
//! the credential redacted.
//!
//! The fixtures are written from each vendor's published tool list and API
//! reference (BUILD_LOG M12 records the check); the smoke test against a
//! real account is manual (HUMAN_TODO).

use std::sync::Arc;

use serde_json::{json, Value};
use tandem_core::ids::ConnectionId;
use tandem_gateway::pinned::PinnedClient;
use tandem_gateway::vendors::catalog;
use tandem_gateway::vendors::web_fetch::WebFetch;
use tandem_gateway::vendors::{CallCtx, ConnectionInfo, RiskClass, VendorModule};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const TOKEN: &str = "fixture-token-1234567890";

/// A remote MCP server over Streamable HTTP, as far as the gateway uses it.
struct McpFixture {
    tools: Value,
    /// Tool name → the text it answers (the token is echoed when the text
    /// holds `{token}`).
    replies: Vec<(&'static str, String)>,
}

impl Respond for McpFixture {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let auth = req
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if auth != format!("Bearer {TOKEN}") {
            return ResponseTemplate::new(401).insert_header(
                "www-authenticate",
                "Bearer resource_metadata=\"http://127.0.0.1/.well-known/oauth-protected-resource\"",
            );
        }
        let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        let id = body.get("id").cloned().unwrap_or(Value::Null);
        match body.get("method").and_then(Value::as_str) {
            Some("initialize") => ResponseTemplate::new(200)
                .insert_header("mcp-session-id", "fixture-session")
                .set_body_json(json!({"jsonrpc": "2.0", "id": id, "result": {
                    "protocolVersion": "2025-03-26", "capabilities": {"tools": {}},
                    "serverInfo": {"name": "fixture", "version": "1"}}})),
            Some("notifications/initialized") => ResponseTemplate::new(202),
            Some("tools/list") => {
                // Answer as an SSE stream, as many servers do.
                let msg = json!({"jsonrpc": "2.0", "id": id, "result": {"tools": self.tools}});
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(format!("event: message\ndata: {msg}\n\n"))
            }
            Some("tools/call") => {
                let name = body
                    .pointer("/params/name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let text = self
                    .replies
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, t)| t.replace("{token}", TOKEN))
                    .unwrap_or_else(|| "{}".into());
                ResponseTemplate::new(200)
                    .set_body_json(json!({"jsonrpc": "2.0", "id": id, "result": {
                    "content": [{"type": "text", "text": text}], "isError": false}}))
            }
            _ => ResponseTemplate::new(400),
        }
    }
}

fn tools(list: &[&str]) -> Value {
    Value::Array(
        list.iter()
            .map(|n| json!({"name": n, "description": format!("{n} (fixture)"), "inputSchema": {"type": "object", "properties": {}}}))
            .collect(),
    )
}

fn module(base: &str, id: &str) -> Arc<dyn VendorModule> {
    catalog::modules(Some(base))
        .into_iter()
        .find(|m| m.manifest().id == id)
        .expect("module")
}

struct Case {
    id: &'static str,
    /// (tool, expected class); the first read tool is called.
    tools: Vec<(&'static str, RiskClass)>,
    replies: Vec<(&'static str, String)>,
    meta: Value,
    account: Option<&'static str>,
}

/// Mounts the fixture at every one of the module's upstream URLs, then
/// runs the contract.
async fn contract(case: Case) {
    let server = MockServer::start().await;
    let m = module(&server.uri(), case.id);
    let names: Vec<&str> = case.tools.iter().map(|(n, _)| *n).collect();
    Mock::given(method("POST"))
        .and(path_regex(format!("^/{}/", case.id)))
        .respond_with(McpFixture {
            tools: tools(&names),
            replies: case.replies.clone(),
        })
        .mount(&server)
        .await;
    let mut conn = ConnectionInfo::new(ConnectionId::new(), case.id);
    conn.meta = case.meta.clone();
    let http = PinnedClient::with_bearer(m.manifest().pinned.clone(), TOKEN);
    let cx = CallCtx {
        conn: &conn,
        http: &http,
    };
    let account = m.verify(&cx).await.expect("verify");
    assert_eq!(
        account.label.as_deref(),
        case.account,
        "{}: account label",
        case.id
    );
    let found = m.tools(&cx).await.expect("tools");
    for (name, class) in &case.tools {
        let t = found
            .iter()
            .find(|t| t.name == *name)
            .unwrap_or_else(|| panic!("{}: {name} missing", case.id));
        assert_eq!(t.risk_class, *class, "{}: {name}", case.id);
    }
    let read = case
        .tools
        .iter()
        .find(|(_, c)| *c == RiskClass::Read)
        .expect("a read tool")
        .0;
    let out = m.call(&cx, read, json!({})).await.expect("call");
    assert!(!out.is_error, "{}: {}", case.id, out.text);
    assert!(
        !http.redact(&out.text).contains(TOKEN),
        "{}: token redacted",
        case.id
    );
    // The credential went only to the module's pinned upstream.
    for r in server.received_requests().await.expect("requests") {
        assert!(
            r.url.path().starts_with(&format!("/{}/", case.id)),
            "{}: {}",
            case.id,
            r.url.path()
        );
        assert_eq!(
            r.headers.get("authorization").and_then(|v| v.to_str().ok()),
            Some(format!("Bearer {TOKEN}").as_str())
        );
    }
    // Without the credential the vendor refuses, and the module says so.
    let anon = PinnedClient::anonymous(m.manifest().pinned.clone());
    let fresh = module(&server.uri(), case.id);
    let cx = CallCtx {
        conn: &conn,
        http: &anon,
    };
    assert!(
        fresh.verify(&cx).await.is_err(),
        "{}: no credential, no access",
        case.id
    );
}

use RiskClass::*;

#[tokio::test]
async fn f_pl_09_1_github_fixture() {
    contract(Case {
        id: "github",
        tools: vec![
            ("get_me", Read),
            ("list_issues", Read),
            ("search_code", Read),
            ("issue_read", Read),
            ("issue_write", Write),
            ("create_pull_request", Write),
            ("merge_pull_request", Destructive),
            ("delete_file", Destructive),
            ("run_secret_scanning", Unclassified),
        ],
        replies: vec![
            ("get_me", json!({"login": "octo-shop"}).to_string()),
            (
                "list_issues",
                "[{\"number\":1,\"body\":\"token {token}\"}]".into(),
            ),
        ],
        meta: json!({}),
        account: Some("octo-shop"),
    })
    .await;
}

#[tokio::test]
async fn f_pl_09_1_linear_fixture() {
    contract(Case {
        id: "linear",
        tools: vec![
            ("list_issues", Read),
            ("get_issue", Read),
            ("create_issue", Write),
            ("update_issue", Write),
        ],
        replies: vec![("list_issues", "[]".into())],
        meta: json!({}),
        account: None,
    })
    .await;
}

#[tokio::test]
async fn f_pl_09_1_notion_fixture() {
    contract(Case {
        id: "notion",
        tools: vec![
            ("notion-search", Read),
            ("notion-fetch", Read),
            ("notion-create-pages", Write),
            ("notion-update-page", Write),
        ],
        replies: vec![("notion-search", "{\"results\":[]}".into())],
        meta: json!({}),
        account: None,
    })
    .await;
}

#[tokio::test]
async fn f_pl_09_1_sentry_fixture() {
    contract(Case {
        id: "sentry",
        tools: vec![
            ("whoami", Read),
            ("find_issues", Read),
            ("get_issue_details", Read),
            ("update_issue", Write),
        ],
        replies: vec![(
            "whoami",
            json!({"name": "Ada", "email": "ada@example.com"}).to_string(),
        )],
        meta: json!({}),
        account: Some("Ada"),
    })
    .await;
}

#[tokio::test]
async fn f_pl_09_1_vercel_fixture() {
    contract(Case {
        id: "vercel",
        tools: vec![
            ("list_projects", Read),
            ("get_deployment", Read),
            ("deploy_to_vercel", Write),
        ],
        replies: vec![("list_projects", "[]".into())],
        meta: json!({}),
        account: None,
    })
    .await;
}

#[tokio::test]
async fn f_pl_09_1_cloudflare_fixture() {
    // Two servers (bindings, observability) under one module.
    contract(Case {
        id: "cloudflare",
        tools: vec![
            ("accounts_list", Read),
            ("kv_namespace_create", Write),
            ("kv_namespace_delete", Destructive),
            ("query_worker_observability", Read),
        ],
        replies: vec![("accounts_list", "[]".into())],
        meta: json!({}),
        account: None,
    })
    .await;
}

#[tokio::test]
async fn f_pl_09_1_supabase_fixture() {
    contract(Case {
        id: "supabase",
        tools: vec![
            ("list_projects", Read),
            ("execute_sql", Destructive),
            ("apply_migration", Destructive),
            ("create_project", Spend),
            ("deploy_edge_function", Write),
        ],
        replies: vec![("list_projects", "[]".into())],
        meta: json!({}),
        account: None,
    })
    .await;
}

#[tokio::test]
async fn f_pl_09_1_stripe_fixture() {
    // A live-mode connection: refunds are destructive, payment links spend.
    contract(Case {
        id: "stripe",
        tools: vec![
            ("list_customers", Read),
            ("retrieve_balance", Read),
            ("create_refund", Destructive),
            ("create_payment_link", Spend),
            ("cancel_subscription", Destructive),
        ],
        replies: vec![("list_customers", "{\"data\":[]}".into())],
        meta: json!({"mode": "live"}),
        account: None,
    })
    .await;
    contract(Case {
        id: "stripe",
        tools: vec![
            ("list_customers", Read),
            ("create_refund", Write),
            ("create_payment_link", Write),
        ],
        replies: vec![("list_customers", "{\"data\":[]}".into())],
        meta: json!({"mode": "test"}),
        account: None,
    })
    .await;
}

#[tokio::test]
async fn f_pl_09_1_posthog_fixture() {
    contract(Case {
        id: "posthog",
        tools: vec![
            ("projects-get", Read),
            ("insights-get-all", Read),
            ("feature-flag-get-all", Read),
            ("create-feature-flag", Write),
            ("delete-feature-flag", Destructive),
        ],
        replies: vec![("projects-get", "[]".into())],
        meta: json!({}),
        account: None,
    })
    .await;
}

/// Slack's Web API: `{"ok": …}` bodies, bot token as Bearer.
#[tokio::test]
async fn f_pl_09_1_slack_fixture() {
    let server = MockServer::start().await;
    let authed = |body: Value| {
        move |r: &Request| {
            if r.headers.get("authorization").and_then(|v| v.to_str().ok())
                == Some(&format!("Bearer {TOKEN}"))
            {
                ResponseTemplate::new(200).set_body_json(body.clone())
            } else {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"ok": false, "error": "not_authed"}))
            }
        }
    };
    Mock::given(method("POST"))
        .and(path("/slack/slack.com/api/auth.test"))
        .respond_with(authed(
            json!({"ok": true, "team": "Octo Shop", "user": "vibezeo"}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/slack/slack.com/api/conversations.list"))
        .respond_with(authed(
            json!({"ok": true, "channels": [{"id": "C1", "name": "general"}]}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/slack/slack.com/api/chat.postMessage"))
        .respond_with(authed(json!({"ok": false, "error": "channel_not_found"})))
        .mount(&server)
        .await;
    let m = module(&server.uri(), "slack");
    let conn = ConnectionInfo::new(ConnectionId::new(), "slack");
    let http = PinnedClient::with_bearer(m.manifest().pinned.clone(), TOKEN);
    let cx = CallCtx {
        conn: &conn,
        http: &http,
    };
    assert_eq!(
        m.verify(&cx).await.expect("verify").label.as_deref(),
        Some("Octo Shop")
    );
    let tools = m.tools(&cx).await.expect("tools");
    let class = |n: &str| tools.iter().find(|t| t.name == n).map(|t| t.risk_class);
    assert_eq!(class("list_channels"), Some(Read));
    assert_eq!(class("channel_history"), Some(Read));
    assert_eq!(class("post_message"), Some(ExternalPublish));
    let out = m
        .call(&cx, "list_channels", json!({"limit": 5}))
        .await
        .expect("call");
    assert!(
        !out.is_error && out.text.contains("general"),
        "{}",
        out.text
    );
    let sent = server.received_requests().await.expect("seen");
    assert!(sent.iter().any(|r| r.url.query() == Some("limit=5")));
    // Slack's own error words reach the agent untouched.
    let out = m
        .call(
            &cx,
            "post_message",
            json!({"channel": "nope", "text": "hi"}),
        )
        .await
        .expect("call");
    assert!(out.is_error);
    assert_eq!(out.text, "channel_not_found");
    let anon = PinnedClient::anonymous(m.manifest().pinned.clone());
    assert!(m
        .verify(&CallCtx {
            conn: &conn,
            http: &anon
        })
        .await
        .is_err());
}

/// Gmail's REST API: the profile names the account, send is publishing.
#[tokio::test]
async fn f_pl_09_1_gmail_fixture() {
    let server = MockServer::start().await;
    let base = "/gmail/gmail.googleapis.com/gmail/v1/users/me";
    Mock::given(method("GET"))
        .and(path(format!("{base}/profile")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"emailAddress": "ada@example.com", "messagesTotal": 3})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{base}/messages")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"messages": [{"id": "m1"}]})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{base}/drafts")))
        .respond_with(|r: &Request| {
            let b: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
            let raw = b
                .pointer("/message/raw")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            ResponseTemplate::new(200).set_body_json(json!({"id": "d1", "raw_len": raw.len()}))
        })
        .mount(&server)
        .await;
    let m = module(&server.uri(), "gmail");
    let conn = ConnectionInfo::new(ConnectionId::new(), "gmail");
    let http = PinnedClient::with_bearer(m.manifest().pinned.clone(), TOKEN);
    let cx = CallCtx {
        conn: &conn,
        http: &http,
    };
    assert_eq!(
        m.verify(&cx).await.expect("verify").label.as_deref(),
        Some("ada@example.com")
    );
    let tools = m.tools(&cx).await.expect("tools");
    let class = |n: &str| tools.iter().find(|t| t.name == n).map(|t| t.risk_class);
    assert_eq!(class("list_messages"), Some(Read));
    assert_eq!(class("create_draft"), Some(Write));
    assert_eq!(class("send_message"), Some(ExternalPublish));
    let out = m
        .call(&cx, "list_messages", json!({"q": "from:me"}))
        .await
        .expect("list");
    assert!(out.text.contains("m1"));
    let out = m
        .call(
            &cx,
            "create_draft",
            json!({"to": "bo@example.com", "subject": "Hi", "body": "Hello"}),
        )
        .await
        .expect("draft");
    assert!(!out.is_error, "{}", out.text);
    // A header injection in the subject is refused before anything is sent.
    let err = m
        .call(
            &cx,
            "create_draft",
            json!({"to": "bo@example.com\r\nBcc: x@evil.com", "subject": "Hi"}),
        )
        .await;
    assert!(err.is_err());
    for r in server.received_requests().await.expect("seen") {
        assert!(r.url.path().starts_with("/gmail/"));
    }
}

/// The built-in Web fetch respects robots.txt.
#[tokio::test]
async fn f_pl_09_web_fetch_fixture() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("User-agent: *\nDisallow: /private\n"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/page"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<h1>Hello</h1>"))
        .mount(&server)
        .await;
    let m = WebFetch::allowing_private_for_tests();
    let conn = ConnectionInfo::new(ConnectionId::new(), "web_fetch");
    let http = PinnedClient::anonymous(Vec::new());
    let cx = CallCtx {
        conn: &conn,
        http: &http,
    };
    let out = m
        .call(
            &cx,
            "fetch",
            json!({"url": format!("{}/page", server.uri())}),
        )
        .await
        .expect("fetch");
    assert_eq!(out.text, "<h1>Hello</h1>");
    let out = m
        .call(
            &cx,
            "fetch",
            json!({"url": format!("{}/private/x", server.uri())}),
        )
        .await
        .expect("fetch");
    assert!(
        out.is_error && out.text.contains("robots.txt"),
        "{}",
        out.text
    );
}

#[test]
fn f_pl_09_1_every_ready_module_has_a_fixture_test() {
    let src = include_str!("vendors.rs");
    for id in catalog::READY {
        assert!(
            src.contains(&format!("fn f_pl_09_1_{id}_fixture")),
            "{id} has no fixture test"
        );
    }
}
