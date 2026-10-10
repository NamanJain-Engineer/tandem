//! Vendor modules (ARCHITECTURE §11.4, F-PL-02): plugins are code, one module
//! per vendor under `vendors/<id>/`. Each declares what it is, how it signs
//! in, the hosts its credentials may go to and its tools with their risk
//! classes. Remote-MCP vendors discover their tools, which are classed by the
//! module's rules; a tool no rule matches is `unclassified` and asks every
//! time.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

pub use crate::netguard::PinnedHost;
use crate::pinned::PinnedClient;

pub mod catalog;
pub mod mcp;
pub mod rest;
pub mod web_fetch;

#[cfg(any(test, feature = "test-vendors"))]
pub mod test_echo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Dev,
    Business,
    Communication,
    Research,
    Media,
    Growth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AuthKind {
    OauthDcr,
    OauthStatic,
    ApiKey,
    LocalPort,
    None,
}

impl AuthKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthKind::OauthDcr => "oauth_dcr",
            AuthKind::OauthStatic => "oauth_static",
            AuthKind::ApiKey => "api_key",
            AuthKind::LocalPort => "local_port",
            AuthKind::None => "none",
        }
    }

    pub fn is_oauth(self) -> bool {
        matches!(self, AuthKind::OauthDcr | AuthKind::OauthStatic)
    }
}

/// What a tool may do (F-PL-06); decides whether a call asks first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RiskClass {
    Read,
    Write,
    ExternalPublish,
    Spend,
    CreditConsuming,
    Destructive,
    Unclassified,
}

impl RiskClass {
    pub const ALL: [RiskClass; 7] = [
        RiskClass::Read,
        RiskClass::Write,
        RiskClass::ExternalPublish,
        RiskClass::Spend,
        RiskClass::CreditConsuming,
        RiskClass::Destructive,
        RiskClass::Unclassified,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            RiskClass::Read => "read",
            RiskClass::Write => "write",
            RiskClass::ExternalPublish => "external_publish",
            RiskClass::Spend => "spend",
            RiskClass::CreditConsuming => "credit_consuming",
            RiskClass::Destructive => "destructive",
            RiskClass::Unclassified => "unclassified",
        }
    }

    pub fn parse(s: &str) -> RiskClass {
        RiskClass::ALL
            .into_iter()
            .find(|r| r.as_str() == s)
            .unwrap_or(RiskClass::Unclassified)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleStatus {
    Ready,
    /// Listed, but can't be connected.
    Preview,
}

/// Where a module's tools live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upstream {
    /// One or more remote MCP servers (Streamable HTTP); tools are
    /// discovered and classed by `risk_rules`.
    RemoteMcp { urls: Vec<String> },
    /// A REST API; tools are declared by the module.
    Rest { base: String },
    /// A desktop app's local MCP add-on on `127.0.0.1:<port>`.
    LocalPort {
        default_port: u16,
        probe_path: String,
    },
    /// Answered inside the gateway (web fetch).
    Builtin,
}

/// `pattern` is a glob on the tool's name (`*` any run, `?` one char).
/// `mode`, when set, must equal the connection's `meta.mode` (Stripe's test
/// and live keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskRule {
    pub pattern: &'static str,
    pub class: RiskClass,
    pub mode: Option<&'static str>,
}

impl RiskRule {
    pub const fn new(pattern: &'static str, class: RiskClass) -> Self {
        Self {
            pattern,
            class,
            mode: None,
        }
    }

    pub const fn in_mode(pattern: &'static str, class: RiskClass, mode: &'static str) -> Self {
        Self {
            pattern,
            class,
            mode: Some(mode),
        }
    }
}

/// Glob match: `*` matches any run of characters, `?` one.
pub fn glob(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The class of a discovered tool: the first rule that matches (in the
/// connection's mode), else `unclassified` (F-PL-02 AC2).
pub fn classify(rules: &[RiskRule], tool: &str, mode: Option<&str>) -> RiskClass {
    rules
        .iter()
        .find(|r| r.mode.is_none_or(|m| Some(m) == mode) && glob(r.pattern, tool))
        .map(|r| r.class)
        .unwrap_or(RiskClass::Unclassified)
}

#[derive(Debug, Clone)]
pub struct VendorManifest {
    pub id: &'static str,
    pub label: &'static str,
    pub category: Category,
    /// The card's letter tile (two letters, no vendor artwork).
    pub logo: &'static str,
    pub description: &'static str,
    pub auth: AuthKind,
    /// A second way in (GitHub and Slack take a token instead of OAuth).
    pub alt_auth: Option<AuthKind>,
    pub upstream: Upstream,
    /// Where the credential may be sent (F-PL-04).
    pub pinned: Vec<PinnedHost>,
    /// Where discovery, registration, sign-in and token exchange may go
    /// (F-PL-03); matched on scheme, host and port.
    pub auth_hosts: Vec<PinnedHost>,
    pub status: ModuleStatus,
    pub risk_rules: Vec<RiskRule>,
    /// Argument keys that name the target of an action (`owner`, `repo`):
    /// shown on the card and part of the "for this conversation" key.
    pub target_keys: Vec<&'static str>,
    /// Where to get an API key, for the connect sheet.
    pub key_hint: Option<&'static str>,
    /// OAuth scopes to ask for.
    pub scopes: Vec<&'static str>,
}

/// One tool of a module, before the gateway names it `<plugin>__<tool>`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub risk_class: RiskClass,
}

/// What a call returns: text for the engine, or an error it can read.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VendorError {
    /// The vendor can't be reached or the connection needs attention; the
    /// other plugins keep working (F-PL-01).
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    UnknownTool(String),
    #[error("{0}")]
    Invalid(String),
    /// The upstream refused the credential (401): refresh, or reconnect.
    #[error("{0}")]
    Unauthorized(String),
}

/// Money or credits an action costs (F-PL-06 spend, credit_consuming).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Cost {
    /// As the vendor counts it, in minor units for money ("500" = 5.00 USD).
    pub amount: i64,
    /// ISO 4217 code in lower case, or `credits`.
    pub currency: String,
}

impl Cost {
    /// "5.00 USD" or "120 credits".
    pub fn display(&self) -> String {
        if self.currency == "credits" {
            return format!("{} credits", self.amount);
        }
        let sign = if self.amount < 0 { "-" } else { "" };
        let a = self.amount.unsigned_abs();
        format!(
            "{sign}{}.{:02} {}",
            a / 100,
            a % 100,
            self.currency.to_uppercase()
        )
    }
}

/// The card's plain words for a call (F-PL-07).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActionDescription {
    /// "Create issue in tandem/app".
    pub action: String,
    /// "tandem/app".
    pub target: Option<String>,
    pub cost: Option<Cost>,
}

/// "create_issue" → "Create issue".
pub fn humanize(tool: &str) -> String {
    let words = tool.replace(['_', '.', '-'], " ");
    let mut c = words.trim().chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The card's description from the tool's name and the module's target
/// keys; spend tools with `amount` + `currency` arguments get a cost.
pub fn describe_default(manifest: &VendorManifest, tool: &str, args: &Value) -> ActionDescription {
    let parts: Vec<String> = manifest
        .target_keys
        .iter()
        .filter_map(|k| args.get(*k))
        .filter_map(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .collect();
    let target = (!parts.is_empty()).then(|| parts.join("/"));
    let action = match &target {
        Some(t) => format!("{} in {t}", humanize(tool)),
        None => humanize(tool),
    };
    let cost = match (
        args.get("amount").and_then(Value::as_i64),
        args.get("currency").and_then(Value::as_str),
    ) {
        (Some(amount), Some(currency)) => Some(Cost {
            amount,
            currency: currency.to_ascii_lowercase(),
        }),
        _ => args
            .get("credits")
            .and_then(Value::as_i64)
            .map(|amount| Cost {
                amount,
                currency: "credits".into(),
            }),
    };
    ActionDescription {
        action,
        target,
        cost,
    }
}

/// Who a connection signed in as.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountInfo {
    pub label: Option<String>,
    /// Non-secret facts to keep in `meta_json` (Stripe's `mode`).
    pub meta: serde_json::Map<String, Value>,
}

/// Per-connection options (`options_json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PluginOptions {
    /// "Let Full access chats publish without asking" (F-PL-06; off).
    pub allow_publish_in_full: bool,
    /// "Let Full access chats run writes without asking" (UI-93; on).
    pub allow_writes_in_full: bool,
}

impl Default for PluginOptions {
    fn default() -> Self {
        Self {
            allow_publish_in_full: false,
            allow_writes_in_full: true,
        }
    }
}

/// One connected account of a module, as the gateway knows it (no secrets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub id: tandem_core::ids::ConnectionId,
    pub plugin_id: String,
    pub account_label: Option<String>,
    pub connected: bool,
    /// Non-secret facts: client id, token endpoint, mode, port.
    pub meta: Value,
    pub options: PluginOptions,
}

impl ConnectionInfo {
    pub fn new(id: tandem_core::ids::ConnectionId, plugin_id: &str) -> Self {
        Self {
            id,
            plugin_id: plugin_id.to_string(),
            account_label: None,
            connected: true,
            meta: Value::Object(Default::default()),
            options: PluginOptions::default(),
        }
    }

    pub fn mode(&self) -> Option<&str> {
        self.meta.get("mode").and_then(Value::as_str)
    }
}

/// What a module gets for a call: the connection and its pinned client.
pub struct CallCtx<'a> {
    pub conn: &'a ConnectionInfo,
    pub http: &'a PinnedClient,
}

#[async_trait]
pub trait VendorModule: Send + Sync {
    fn manifest(&self) -> &VendorManifest;

    /// The module's tools for a connection (a remote MCP upstream discovers
    /// them).
    async fn tools(&self, cx: &CallCtx<'_>) -> Result<Vec<ToolDef>, VendorError>;

    async fn call(
        &self,
        cx: &CallCtx<'_>,
        tool: &str,
        args: Value,
    ) -> Result<ToolOutput, VendorError>;

    /// A cheap read that proves the credential works, naming the account.
    async fn verify(&self, cx: &CallCtx<'_>) -> Result<AccountInfo, VendorError> {
        self.tools(cx).await.map(|_| AccountInfo::default())
    }

    /// The card's plain words for a call.
    fn describe(&self, tool: &str, args: &Value) -> ActionDescription {
        describe_default(self.manifest(), tool, args)
    }

    /// Re-reads what a call will cost right before it runs (F-PL-06 spend:
    /// "re-verifies the amount and budget at execution"). The default is
    /// what the arguments say.
    async fn pre_execute(
        &self,
        _cx: &CallCtx<'_>,
        tool: &str,
        args: &Value,
    ) -> Result<Option<Cost>, VendorError> {
        Ok(self.describe(tool, args).cost)
    }

    /// Revokes the credential upstream on disconnect, where supported.
    async fn revoke(&self, _cx: &CallCtx<'_>) -> Result<(), VendorError> {
        Ok(())
    }
}

/// Every module this build has: the catalog (F-PL-09) and, in test builds,
/// the test modules.
pub fn registry() -> Vec<Arc<dyn VendorModule>> {
    #[allow(unused_mut)]
    let mut modules = catalog::modules(catalog::override_base().as_deref());
    #[cfg(any(test, feature = "test-vendors"))]
    modules.extend(test_echo::modules());
    modules
}

/// The module for a plugin id.
pub fn find(modules: &[Arc<dyn VendorModule>], plugin_id: &str) -> Option<Arc<dyn VendorModule>> {
    modules
        .iter()
        .find(|m| m.manifest().id == plugin_id)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_pl_02_1_every_module_pins_its_hosts_and_classes_each_static_tool() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let mut ids = std::collections::HashSet::new();
        for module in registry() {
            let m = module.manifest();
            assert!(ids.insert(m.id), "{} is declared twice", m.id);
            // A built-in with no sign-in (web fetch) sends no credential.
            if m.upstream != Upstream::Builtin {
                assert!(!m.pinned.is_empty(), "{} pins no host", m.id);
            }
            assert!(
                m.id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "{}: plugin ids are snake_case",
                m.id
            );
            assert_ne!(m.id, crate::BUILTIN_PLUGIN, "the tandem id is reserved");
            assert!(!m.id.contains("__"), "{}", m.id);
            assert_eq!(m.logo.chars().count(), 2, "{}: a two-letter tile", m.id);
            assert!(!m.label.is_empty() && !m.description.is_empty(), "{}", m.id);
            if m.auth.is_oauth() && m.status == ModuleStatus::Ready {
                assert!(!m.auth_hosts.is_empty(), "{}: OAuth needs auth hosts", m.id);
            }
            if let Upstream::RemoteMcp { urls } = &m.upstream {
                assert!(!urls.is_empty(), "{}", m.id);
                for u in urls {
                    let url = url::Url::parse(u).expect("upstream url");
                    assert!(
                        m.pinned.iter().any(|p| p.matches(&url)),
                        "{}: upstream {u} isn't pinned",
                        m.id
                    );
                }
                if m.status == ModuleStatus::Ready {
                    assert!(!m.risk_rules.is_empty(), "{}: no risk rules", m.id);
                }
            }
            if let Upstream::Rest { base } = &m.upstream {
                let url = url::Url::parse(base).expect("rest base");
                assert!(m.pinned.iter().any(|p| p.matches(&url)), "{}", m.id);
            }
            if m.auth == AuthKind::ApiKey || m.alt_auth == Some(AuthKind::ApiKey) {
                assert!(m.key_hint.is_some(), "{}: say where to get a key", m.id);
            }
            // Static tools: each has a class (the type makes that true) and
            // a well-formed name and schema.
            let conn = ConnectionInfo::new(tandem_core::ids::ConnectionId::new(), m.id);
            let http = PinnedClient::anonymous(m.pinned.clone());
            let cx = CallCtx {
                conn: &conn,
                http: &http,
            };
            if !matches!(m.upstream, Upstream::RemoteMcp { .. }) {
                if let Ok(tools) = rt.block_on(module.tools(&cx)) {
                    for t in tools {
                        assert!(
                            !t.name.contains("__"),
                            "{}: tool names can't hold __",
                            t.name
                        );
                        assert!(t.input_schema.is_object(), "{}: schema", t.name);
                    }
                }
            }
        }
    }

    #[test]
    fn f_pl_02_2_a_discovered_tool_with_no_rule_is_unclassified() {
        let rules = [
            RiskRule::new("get_*", RiskClass::Read),
            RiskRule::new("create_*", RiskClass::Write),
            RiskRule::in_mode("create_refund", RiskClass::Destructive, "live"),
        ];
        assert_eq!(classify(&rules, "get_issue", None), RiskClass::Read);
        assert_eq!(classify(&rules, "create_issue", None), RiskClass::Write);
        assert_eq!(
            classify(&rules, "launch_rocket", None),
            RiskClass::Unclassified
        );
        assert_eq!(classify(&[], "anything", None), RiskClass::Unclassified);
        // Mode rules apply only in their mode (first match wins).
        let live = [
            RiskRule::in_mode("create_refund", RiskClass::Destructive, "live"),
            RiskRule::new("create_*", RiskClass::Write),
        ];
        assert_eq!(
            classify(&live, "create_refund", Some("live")),
            RiskClass::Destructive
        );
        assert_eq!(
            classify(&live, "create_refund", Some("test")),
            RiskClass::Write
        );
    }

    #[test]
    fn glob_matches_stars_and_marks() {
        assert!(glob("*", "x"));
        assert!(glob("list_*", "list_issues"));
        assert!(!glob("list_*", "get_list"));
        assert!(glob("*_test_*", "a_test_b"));
        assert!(glob("chat.post?essage", "chat.postMessage"));
        assert!(!glob("get", "get_x"));
    }

    #[test]
    fn f_pl_07_costs_and_actions_read_in_plain_words() {
        assert_eq!(
            Cost {
                amount: 500,
                currency: "usd".into()
            }
            .display(),
            "5.00 USD"
        );
        assert_eq!(humanize("create_issue"), "Create issue");
        assert_eq!(humanize("chat.postMessage"), "Chat postMessage");
    }
}
