//! The initial catalog (F-PL-09, ARCHITECTURE §11.8): 11 ready modules, 9
//! previews and the built-in Web fetch. Upstreams and auth kinds were checked
//! against the vendors' docs when this was written (BUILD_LOG M12).
//!
//! Debug builds started with `TANDEM_E2E=1` and `TANDEM_E2E_PLUGIN_BASE`
//! point every upstream and sign-in host at that one local mock (the E2E
//! suite's `scripts/mock-plugins.ts`); release builds never read it.

use std::sync::Arc;

use serde_json::{json, Value};

use super::mcp::{McpModule, WhoAmI};
use super::rest::{arg, check_status, seg, RestModule, RestRequest, RestTool};
use super::web_fetch::WebFetch;
use super::{
    AuthKind, Category, ModuleStatus, PinnedHost, RiskClass, RiskRule, Upstream, VendorError,
    VendorManifest, VendorModule,
};

use RiskClass::*;

/// The E2E mock origin, when this is a debug E2E run.
pub fn override_base() -> Option<String> {
    if !cfg!(debug_assertions) || std::env::var("TANDEM_E2E").as_deref() != Ok("1") {
        return None;
    }
    std::env::var("TANDEM_E2E_PLUGIN_BASE")
        .ok()
        .filter(|v| v.starts_with("http://127.0.0.1:"))
}

/// Points a manifest at a local mock: each upstream URL `https://h/p`
/// becomes `<base>/<id>/h/p`; the credential is pinned to `<base>/<id>/`
/// and sign-in may use the whole mock origin.
pub fn rebase(mut m: VendorManifest, base: &str) -> VendorManifest {
    let base = base.trim_end_matches('/');
    let map = |u: &str| -> String {
        let rest = u.split_once("://").map(|(_, r)| r).unwrap_or(u);
        format!("{base}/{}/{rest}", m.id)
    };
    m.upstream = match &m.upstream {
        Upstream::RemoteMcp { urls } => Upstream::RemoteMcp {
            urls: urls.iter().map(|u| map(u)).collect(),
        },
        Upstream::Rest { base: b } => Upstream::Rest { base: map(b) },
        other => other.clone(),
    };
    if let Some(origin) = PinnedHost::from_url(base) {
        let id = m.id;
        if !m.pinned.is_empty() {
            m.pinned = vec![PinnedHost {
                path_prefix: format!("/{id}/"),
                ..origin.clone()
            }];
        }
        if !m.auth_hosts.is_empty() {
            m.auth_hosts = vec![PinnedHost {
                path_prefix: "/".into(),
                ..origin
            }];
        }
    }
    m
}

fn pin(url: &str) -> PinnedHost {
    PinnedHost::from_url(url).unwrap_or_else(|| PinnedHost::new("https", "invalid", 443, "/"))
}

fn host(h: &str) -> PinnedHost {
    PinnedHost::new("https", h, 443, "/")
}

#[allow(clippy::too_many_arguments)]
fn manifest(
    id: &'static str,
    label: &'static str,
    category: Category,
    logo: &'static str,
    description: &'static str,
    auth: AuthKind,
    upstream: Upstream,
    pinned: Vec<PinnedHost>,
    auth_hosts: Vec<PinnedHost>,
    risk_rules: Vec<RiskRule>,
) -> VendorManifest {
    VendorManifest {
        id,
        label,
        category,
        logo,
        description,
        auth,
        alt_auth: None,
        upstream,
        pinned,
        auth_hosts,
        status: ModuleStatus::Ready,
        risk_rules,
        target_keys: Vec::new(),
        key_hint: None,
        scopes: Vec::new(),
    }
}

fn mcp(urls: &[&str]) -> (Upstream, Vec<PinnedHost>) {
    (
        Upstream::RemoteMcp {
            urls: urls.iter().map(|u| (*u).to_string()).collect(),
        },
        urls.iter().map(|u| pin(u)).collect(),
    )
}

// ── ready ─────────────────────────────────────────────────────────────

pub fn github() -> VendorManifest {
    let (up, pins) = mcp(&["https://api.githubcopilot.com/mcp/"]);
    VendorManifest {
        alt_auth: Some(AuthKind::ApiKey),
        target_keys: vec!["owner", "repo"],
        key_hint: Some("Create a fine-grained personal access token at GitHub › Settings › Developer settings › Personal access tokens."),
        scopes: vec!["repo", "read:org"],
        ..manifest(
            "github",
            "GitHub",
            Category::Dev,
            "Gh",
            "Issues, pull requests and code search.",
            AuthKind::OauthStatic,
            up,
            pins,
            vec![host("github.com"), host("api.githubcopilot.com")],
            vec![
                RiskRule::new("get_*", Read),
                RiskRule::new("list_*", Read),
                RiskRule::new("search_*", Read),
                RiskRule::new("*_read", Read),
                RiskRule::new("merge_*", Destructive),
                RiskRule::new("delete_*", Destructive),
                RiskRule::new("create_*", Write),
                RiskRule::new("update_*", Write),
                RiskRule::new("add_*", Write),
                RiskRule::new("*_write", Write),
            ],
        )
    }
}

pub fn linear() -> VendorManifest {
    let (up, pins) = mcp(&["https://mcp.linear.app/mcp"]);
    VendorManifest {
        alt_auth: Some(AuthKind::ApiKey),
        key_hint: Some("Create a personal API key at Linear › Settings › Security & access."),
        target_keys: vec!["team", "teamId", "issueId", "id"],
        ..manifest(
            "linear",
            "Linear",
            Category::Dev,
            "Li",
            "Issues, projects and cycles.",
            AuthKind::OauthDcr,
            up,
            pins,
            vec![
                host("mcp.linear.app"),
                host("linear.app"),
                host("api.linear.app"),
            ],
            vec![
                RiskRule::new("list_*", Read),
                RiskRule::new("get_*", Read),
                RiskRule::new("search_*", Read),
                RiskRule::new("delete_*", Destructive),
                RiskRule::new("create_*", Write),
                RiskRule::new("update_*", Write),
            ],
        )
    }
}

pub fn notion() -> VendorManifest {
    let (up, pins) = mcp(&["https://mcp.notion.com/mcp"]);
    VendorManifest {
        target_keys: vec!["page_id", "database_id"],
        ..manifest(
            "notion",
            "Notion",
            Category::Business,
            "No",
            "Pages and databases.",
            AuthKind::OauthDcr,
            up,
            pins,
            vec![
                host("mcp.notion.com"),
                host("api.notion.com"),
                host("www.notion.so"),
            ],
            vec![
                RiskRule::new("search", Read),
                RiskRule::new("fetch", Read),
                RiskRule::new("notion-search", Read),
                RiskRule::new("notion-fetch", Read),
                RiskRule::new("notion-get-*", Read),
                RiskRule::new("get_*", Read),
                RiskRule::new("*create*", Write),
                RiskRule::new("*update*", Write),
                RiskRule::new("*move*", Write),
                RiskRule::new("*duplicate*", Write),
            ],
        )
    }
}

pub fn sentry() -> VendorManifest {
    let (up, pins) = mcp(&["https://mcp.sentry.dev/mcp"]);
    VendorManifest {
        target_keys: vec!["organizationSlug", "projectSlug", "issueId"],
        ..manifest(
            "sentry",
            "Sentry",
            Category::Dev,
            "Se",
            "Errors, issues and releases.",
            AuthKind::OauthDcr,
            up,
            pins,
            vec![host("mcp.sentry.dev"), host("sentry.io")],
            vec![
                RiskRule::new("find_*", Read),
                RiskRule::new("get_*", Read),
                RiskRule::new("search_*", Read),
                RiskRule::new("whoami", Read),
                RiskRule::new("analyze_*", Read),
                RiskRule::new("update_*", Write),
                RiskRule::new("create_*", Write),
            ],
        )
    }
}

pub fn vercel() -> VendorManifest {
    let (up, pins) = mcp(&["https://mcp.vercel.com/"]);
    VendorManifest {
        target_keys: vec!["teamId", "projectId"],
        ..manifest(
            "vercel",
            "Vercel",
            Category::Dev,
            "Ve",
            "Projects, deployments and logs.",
            AuthKind::OauthDcr,
            up,
            pins,
            vec![
                host("mcp.vercel.com"),
                host("vercel.com"),
                host("api.vercel.com"),
            ],
            vec![
                RiskRule::new("list_*", Read),
                RiskRule::new("get_*", Read),
                RiskRule::new("search_*", Read),
                RiskRule::new("check_*", Read),
                RiskRule::new("delete_*", Destructive),
                RiskRule::new("deploy*", Write),
                RiskRule::new("create_*", Write),
                RiskRule::new("update_*", Write),
            ],
        )
    }
}

pub fn cloudflare() -> VendorManifest {
    let (up, pins) = mcp(&[
        "https://bindings.mcp.cloudflare.com/mcp",
        "https://observability.mcp.cloudflare.com/mcp",
    ]);
    VendorManifest {
        target_keys: vec!["account_id", "accountId"],
        ..manifest(
            "cloudflare",
            "Cloudflare",
            Category::Dev,
            "Cf",
            "Workers, storage bindings and observability.",
            AuthKind::OauthDcr,
            up,
            pins,
            vec![
                host("bindings.mcp.cloudflare.com"),
                host("observability.mcp.cloudflare.com"),
                host("dash.cloudflare.com"),
            ],
            vec![
                RiskRule::new("*_list", Read),
                RiskRule::new("*_get", Read),
                RiskRule::new("*list*", Read),
                RiskRule::new("*get*", Read),
                RiskRule::new("query_*", Read),
                RiskRule::new("observability_*", Read),
                RiskRule::new("search_*", Read),
                RiskRule::new("*delete*", Destructive),
                RiskRule::new("*create*", Write),
                RiskRule::new("*update*", Write),
                RiskRule::new("set_active_account", Write),
            ],
        )
    }
}

pub fn supabase() -> VendorManifest {
    let (up, pins) = mcp(&["https://mcp.supabase.com/mcp"]);
    VendorManifest {
        alt_auth: Some(AuthKind::ApiKey),
        key_hint: Some("Create a personal access token at Supabase › Account › Access tokens."),
        target_keys: vec!["project_id"],
        ..manifest(
            "supabase",
            "Supabase",
            Category::Dev,
            "Sb",
            "Projects, databases and edge functions.",
            AuthKind::OauthDcr,
            up,
            pins,
            vec![host("mcp.supabase.com"), host("api.supabase.com")],
            vec![
                RiskRule::new("execute_sql", Destructive),
                RiskRule::new("apply_migration", Destructive),
                RiskRule::new("create_project", Spend),
                RiskRule::new("create_branch", Spend),
                RiskRule::new("delete_*", Destructive),
                RiskRule::new("pause_project", Destructive),
                RiskRule::new("list_*", Read),
                RiskRule::new("get_*", Read),
                RiskRule::new("search_*", Read),
                RiskRule::new("generate_*", Read),
                RiskRule::new("deploy_*", Write),
                RiskRule::new("restore_project", Write),
                RiskRule::new("confirm_cost", Read),
            ],
        )
    }
}

pub fn stripe() -> VendorManifest {
    let (up, pins) = mcp(&["https://mcp.stripe.com/"]);
    VendorManifest {
        alt_auth: Some(AuthKind::ApiKey),
        key_hint: Some("Create an agent API key at Stripe › Developers › API keys. A test-mode key (rk_test_ or sk_test_) keeps writes in test mode."),
        target_keys: vec!["customer", "payment_intent", "product"],
        ..manifest(
            "stripe",
            "Stripe",
            Category::Business,
            "St",
            "Customers, payments and products.",
            AuthKind::OauthDcr,
            up,
            pins,
            vec![host("mcp.stripe.com"), host("access.stripe.com")],
            vec![
                RiskRule::new("list_*", Read),
                RiskRule::new("retrieve_*", Read),
                RiskRule::new("search_*", Read),
                RiskRule::new("get_*", Read),
                RiskRule::new("fetch_*", Read),
                // Test mode: every write is an ordinary write.
                RiskRule::in_mode("create_*", Write, "test"),
                RiskRule::in_mode("update_*", Write, "test"),
                RiskRule::in_mode("cancel_*", Write, "test"),
                RiskRule::in_mode("finalize_*", Write, "test"),
                // Live mode: refunds and cancellations undo money; charges,
                // invoices and payment links take it.
                RiskRule::in_mode("create_refund", Destructive, "live"),
                RiskRule::in_mode("cancel_*", Destructive, "live"),
                RiskRule::in_mode("create_payment_link", Spend, "live"),
                RiskRule::in_mode("create_invoice*", Spend, "live"),
                RiskRule::in_mode("finalize_*", Spend, "live"),
                RiskRule::in_mode("create_price", Spend, "live"),
                RiskRule::in_mode("create_customer", Write, "live"),
                RiskRule::in_mode("create_product", Write, "live"),
                RiskRule::in_mode("create_coupon", Spend, "live"),
                RiskRule::in_mode("update_*", Write, "live"),
            ],
        )
    }
}

pub fn posthog() -> VendorManifest {
    let (up, pins) = mcp(&["https://mcp.posthog.com/mcp"]);
    VendorManifest {
        alt_auth: Some(AuthKind::ApiKey),
        key_hint: Some("Create a personal API key at PostHog › Settings › Personal API keys, with the MCP Server preset."),
        target_keys: vec!["projectId"],
        ..manifest(
            "posthog",
            "PostHog",
            Category::Growth,
            "Ph",
            "Product analytics, flags and experiments.",
            AuthKind::OauthDcr,
            up,
            pins,
            vec![host("mcp.posthog.com"), host("oauth.posthog.com")],
            vec![
                RiskRule::new("*-get*", Read),
                RiskRule::new("*-list*", Read),
                RiskRule::new("*get*", Read),
                RiskRule::new("*list*", Read),
                RiskRule::new("*query*", Read),
                RiskRule::new("*search*", Read),
                RiskRule::new("docs-*", Read),
                RiskRule::new("*delete*", Destructive),
                // "All writes ask" (ARCHITECTURE §11.8).
                RiskRule::new("*", Write),
            ],
        )
    }
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

/// Slack answers 200 with `{"ok": false, "error": …}` when a call fails.
fn slack_check(status: u16, body: &Value) -> Result<(), String> {
    check_status(status, body)?;
    match body.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(()),
        _ => Err(body
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Slack said the call failed")
            .to_string()),
    }
}

pub fn slack_manifest() -> VendorManifest {
    VendorManifest {
        alt_auth: Some(AuthKind::ApiKey),
        key_hint: Some("Install a Slack app to your workspace and copy its Bot User OAuth Token (xoxb-) from OAuth & Permissions."),
        target_keys: vec!["channel"],
        scopes: vec!["channels:read", "channels:history", "chat:write"],
        ..manifest(
            "slack",
            "Slack",
            Category::Communication,
            "Sl",
            "Channels and messages, as your bot.",
            AuthKind::OauthStatic,
            Upstream::Rest {
                base: "https://slack.com/api/".into(),
            },
            vec![pin("https://slack.com/api/")],
            vec![host("slack.com")],
            Vec::new(),
        )
    }
}

pub fn slack_tools() -> Vec<RestTool> {
    vec![
        RestTool {
            name: "list_channels",
            description: "Lists the workspace's public channels.",
            schema: || {
                schema(
                    json!({"limit": {"type": "integer"}, "cursor": {"type": "string"}}),
                    &[],
                )
            },
            class: Read,
            build: |a| {
                Ok(RestRequest::get("conversations.list").query_from(a, &["limit", "cursor"]))
            },
        },
        RestTool {
            name: "channel_history",
            description: "Reads a channel's recent messages.",
            schema: || {
                schema(
                    json!({"channel": {"type": "string"}, "limit": {"type": "integer"}}),
                    &["channel"],
                )
            },
            class: Read,
            build: |a| {
                arg(a, "channel")?;
                Ok(RestRequest::get("conversations.history").query_from(a, &["channel", "limit"]))
            },
        },
        RestTool {
            name: "post_message",
            description: "Posts a message to a channel, where everyone in it reads it.",
            schema: || {
                schema(
                    json!({"channel": {"type": "string"}, "text": {"type": "string"}}),
                    &["channel", "text"],
                )
            },
            class: ExternalPublish,
            build: |a| {
                Ok(RestRequest::post(
                    "chat.postMessage",
                    json!({"channel": arg(a, "channel")?, "text": arg(a, "text")?}),
                ))
            },
        },
    ]
}

pub fn gmail_manifest() -> VendorManifest {
    VendorManifest {
        target_keys: vec!["to"],
        scopes: vec![
            "https://www.googleapis.com/auth/gmail.readonly",
            "https://www.googleapis.com/auth/gmail.compose",
            "https://www.googleapis.com/auth/gmail.send",
        ],
        ..manifest(
            "gmail",
            "Gmail",
            Category::Communication,
            "Gm",
            "Read, draft and send email.",
            AuthKind::OauthStatic,
            Upstream::Rest {
                base: "https://gmail.googleapis.com/gmail/v1/".into(),
            },
            vec![pin("https://gmail.googleapis.com/gmail/v1/")],
            vec![host("accounts.google.com"), host("oauth2.googleapis.com")],
            Vec::new(),
        )
    }
}

/// An RFC 2822 message as Gmail's base64url `raw`.
fn gmail_raw(a: &Value) -> Result<String, VendorError> {
    use base64::Engine;
    let to = arg(a, "to")?;
    let subject = arg(a, "subject")?;
    let body = a.get("body").and_then(Value::as_str).unwrap_or_default();
    if to.contains(['\r', '\n']) || subject.contains(['\r', '\n']) {
        return Err(VendorError::Invalid(
            "to and subject must be one line".into(),
        ));
    }
    let msg = format!(
        "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{body}"
    );
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(msg))
}

fn mail_schema() -> Value {
    schema(
        json!({"to": {"type": "string"}, "subject": {"type": "string"}, "body": {"type": "string"}}),
        &["to", "subject"],
    )
}

pub fn gmail_tools() -> Vec<RestTool> {
    vec![
        RestTool {
            name: "list_messages",
            description: "Lists messages matching a Gmail search.",
            schema: || {
                schema(
                    json!({"q": {"type": "string"}, "maxResults": {"type": "integer"}}),
                    &[],
                )
            },
            class: Read,
            build: |a| Ok(RestRequest::get("users/me/messages").query_from(a, &["q", "maxResults"])),
        },
        RestTool {
            name: "get_message",
            description: "Reads one message.",
            schema: || schema(json!({"id": {"type": "string"}}), &["id"]),
            class: Read,
            build: |a| {
                Ok(RestRequest::get(format!(
                    "users/me/messages/{}",
                    seg(arg(a, "id")?)
                )))
            },
        },
        RestTool {
            name: "create_draft",
            description: "Saves a draft (nothing is sent).",
            schema: mail_schema,
            class: Write,
            build: |a| {
                Ok(RestRequest::post(
                    "users/me/drafts",
                    json!({"message": {"raw": gmail_raw(a)?}}),
                ))
            },
        },
        RestTool {
            name: "send_message",
            description: "Sends an email to its recipients.",
            schema: mail_schema,
            class: ExternalPublish,
            build: |a| {
                Ok(RestRequest::post(
                    "users/me/messages/send",
                    json!({"raw": gmail_raw(a)?}),
                ))
            },
        },
    ]
}

// ── preview ───────────────────────────────────────────────────────────

fn preview(
    id: &'static str,
    label: &'static str,
    category: Category,
    logo: &'static str,
    description: &'static str,
    api: &str,
) -> VendorManifest {
    VendorManifest {
        status: ModuleStatus::Preview,
        ..manifest(
            id,
            label,
            category,
            logo,
            description,
            AuthKind::OauthStatic,
            Upstream::Rest {
                base: api.to_string(),
            },
            vec![pin(api)],
            Vec::new(),
            Vec::new(),
        )
    }
}

pub fn previews() -> Vec<VendorManifest> {
    vec![
        preview(
            "google_ads",
            "Google Ads",
            Category::Growth,
            "Ga",
            "Campaigns and budgets.",
            "https://googleads.googleapis.com/",
        ),
        preview(
            "meta_ads",
            "Meta Ads",
            Category::Growth,
            "Ma",
            "Ad campaigns on Facebook and Instagram.",
            "https://graph.facebook.com/",
        ),
        preview(
            "x",
            "X",
            Category::Communication,
            "Xx",
            "Posts and mentions.",
            "https://api.x.com/2/",
        ),
        preview(
            "youtube",
            "YouTube",
            Category::Media,
            "Yt",
            "Channels, videos and comments.",
            "https://www.googleapis.com/youtube/v3/",
        ),
        preview(
            "shopify",
            "Shopify",
            Category::Business,
            "Sh",
            "Orders, products and customers.",
            "https://shopify.dev/",
        ),
        preview(
            "quickbooks",
            "QuickBooks",
            Category::Business,
            "Qb",
            "Invoices and accounting.",
            "https://quickbooks.api.intuit.com/",
        ),
        preview(
            "revenuecat",
            "RevenueCat",
            Category::Business,
            "Rc",
            "Subscriptions and customers.",
            "https://api.revenuecat.com/v2/",
        ),
        preview(
            "resend",
            "Resend",
            Category::Communication,
            "Re",
            "Transactional email.",
            "https://api.resend.com/",
        ),
        preview(
            "figma",
            "Figma",
            Category::Media,
            "Fi",
            "Files, comments and components.",
            "https://api.figma.com/v1/",
        ),
    ]
}

/// A module that is only a card (preview): no tools, can't be connected.
pub struct PreviewModule(pub VendorManifest);

#[async_trait::async_trait]
impl VendorModule for PreviewModule {
    fn manifest(&self) -> &VendorManifest {
        &self.0
    }

    async fn tools(&self, _cx: &super::CallCtx<'_>) -> Result<Vec<super::ToolDef>, VendorError> {
        Err(VendorError::Unavailable(format!(
            "{} isn't ready yet",
            self.0.label
        )))
    }

    async fn call(
        &self,
        _cx: &super::CallCtx<'_>,
        _tool: &str,
        _args: Value,
    ) -> Result<super::ToolOutput, VendorError> {
        Err(VendorError::Unavailable(format!(
            "{} isn't ready yet",
            self.0.label
        )))
    }
}

/// The catalog, pointed at `base` when given (fixture tests, E2E).
pub fn modules(base: Option<&str>) -> Vec<Arc<dyn VendorModule>> {
    let at = |m: VendorManifest| match base {
        Some(b) => rebase(m, b),
        None => m,
    };
    let whoami_github = WhoAmI {
        tool: "get_me",
        args: "{}",
        pointer: "/login",
    };
    let mut out: Vec<Arc<dyn VendorModule>> = vec![
        Arc::new(McpModule::new(at(github()), Some(whoami_github))),
        Arc::new(McpModule::new(at(linear()), None)),
        Arc::new(McpModule::new(at(notion()), None)),
        Arc::new(RestModule::new(
            at(slack_manifest()),
            slack_tools(),
            (|_| Ok(RestRequest::post("auth.test", json!({}))), "/team"),
            slack_check,
        )),
        Arc::new(McpModule::new(
            at(sentry()),
            Some(WhoAmI {
                tool: "whoami",
                args: "{}",
                pointer: "/name",
            }),
        )),
        Arc::new(McpModule::new(at(vercel()), None)),
        Arc::new(McpModule::new(at(cloudflare()), None)),
        Arc::new(McpModule::new(at(supabase()), None)),
        Arc::new(McpModule::new(at(stripe()), None)),
        Arc::new(McpModule::new(at(posthog()), None)),
        Arc::new(RestModule::new(
            at(gmail_manifest()),
            gmail_tools(),
            (
                |_| Ok(RestRequest::get("users/me/profile")),
                "/emailAddress",
            ),
            check_status,
        )),
        Arc::new(WebFetch::new()),
    ];
    for p in previews() {
        out.push(Arc::new(PreviewModule(p)));
    }
    out
}

/// The ready modules' ids, in catalog order.
pub const READY: [&str; 11] = [
    "github",
    "linear",
    "notion",
    "slack",
    "sentry",
    "vercel",
    "cloudflare",
    "supabase",
    "stripe",
    "posthog",
    "gmail",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_pl_09_the_catalog_has_11_ready_modules_9_previews_and_web_fetch() {
        let all = modules(None);
        let ready: Vec<_> = all
            .iter()
            .map(|m| m.manifest())
            .filter(|m| m.status == ModuleStatus::Ready && m.id != "web_fetch")
            .map(|m| m.id)
            .collect();
        assert_eq!(ready, READY.to_vec());
        let previews: Vec<_> = all
            .iter()
            .map(|m| m.manifest())
            .filter(|m| m.status == ModuleStatus::Preview)
            .collect();
        assert_eq!(previews.len(), 9);
        let web = all
            .iter()
            .find(|m| m.manifest().id == "web_fetch")
            .expect("web fetch");
        assert_eq!(web.manifest().auth, AuthKind::None);
        let gmail = gmail_manifest();
        let send = gmail_tools()
            .into_iter()
            .find(|t| t.name == "send_message")
            .expect("send");
        assert_eq!(send.class, ExternalPublish);
        assert_eq!(gmail.status, ModuleStatus::Ready);
    }

    #[test]
    fn f_pl_09_stripe_live_writes_are_spend_or_destructive_and_test_writes_are_writes() {
        let rules = stripe().risk_rules;
        use super::super::classify;
        assert_eq!(classify(&rules, "create_refund", Some("live")), Destructive);
        assert_eq!(classify(&rules, "create_payment_link", Some("live")), Spend);
        assert_eq!(classify(&rules, "create_refund", Some("test")), Write);
        assert_eq!(classify(&rules, "list_customers", Some("live")), Read);
        // No mode recorded: nothing but reads is classified, so writes ask
        // every time.
        assert_eq!(classify(&rules, "create_refund", None), Unclassified);
    }

    #[test]
    fn rebasing_points_upstreams_and_pins_at_the_mock() {
        let m = rebase(github(), "http://127.0.0.1:8791");
        assert_eq!(
            m.upstream,
            Upstream::RemoteMcp {
                urls: vec!["http://127.0.0.1:8791/github/api.githubcopilot.com/mcp/".into()]
            }
        );
        assert_eq!(
            m.pinned,
            vec![PinnedHost::new("http", "127.0.0.1", 8791, "/github/")]
        );
        assert_eq!(
            m.auth_hosts,
            vec![PinnedHost::new("http", "127.0.0.1", 8791, "/")]
        );
    }
}
