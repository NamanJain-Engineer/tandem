//! The built-in Web fetch (F-PL-09): read-only, no sign-in. It fetches one
//! public http(s) page, at most 2 MB, after checking the site's robots.txt;
//! private, loopback and link-local addresses are refused, also when a
//! redirect leads there.

use async_trait::async_trait;
use serde_json::{json, Value};
use url::Url;

use super::{
    AuthKind, CallCtx, Category, ModuleStatus, RiskClass, ToolDef, ToolOutput, Upstream,
    VendorError, VendorManifest, VendorModule,
};
use crate::netguard::is_private_url;
use crate::pinned::{Outgoing, PinnedClient};

pub const MAX_PAGE: usize = 2 * 1024 * 1024;
const AGENT: &str = "vibezeo";

pub struct WebFetch {
    manifest: VendorManifest,
    /// Tests reach a local server; the app never sets this.
    allow_private: bool,
}

impl WebFetch {
    pub fn new() -> Self {
        Self {
            manifest: VendorManifest {
                id: "web_fetch",
                label: "Web fetch",
                category: Category::Research,
                logo: "Wf",
                description: "Reads public web pages. Respects robots.txt.",
                auth: AuthKind::None,
                alt_auth: None,
                upstream: Upstream::Builtin,
                // No credential ever goes anywhere; nothing is pinned.
                pinned: Vec::new(),
                auth_hosts: Vec::new(),
                status: ModuleStatus::Ready,
                risk_rules: Vec::new(),
                target_keys: vec!["url"],
                key_hint: None,
                scopes: Vec::new(),
            },
            allow_private: false,
        }
    }

    /// For tests against a local server only.
    pub fn allowing_private_for_tests() -> Self {
        Self {
            allow_private: true,
            ..Self::new()
        }
    }
}

impl Default for WebFetch {
    fn default() -> Self {
        Self::new()
    }
}

/// A robots.txt group: its user agents and its (allow, prefix) rules.
type Group = (Vec<String>, Vec<(bool, String)>);

/// Whether robots.txt lets `agent` fetch `path` (longest matching rule
/// wins; Allow wins a tie; the agent's own group beats `*`).
pub fn robots_allows(robots: &str, agent: &str, path: &str) -> bool {
    let mut groups: Vec<Group> = Vec::new();
    let mut in_agents = false;
    for raw in robots.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_string());
        match k.as_str() {
            "user-agent" => {
                if !in_agents || groups.is_empty() {
                    groups.push((Vec::new(), Vec::new()));
                }
                if let Some(g) = groups.last_mut() {
                    g.0.push(v.to_ascii_lowercase());
                }
                in_agents = true;
            }
            "allow" | "disallow" => {
                in_agents = false;
                if let Some(g) = groups.last_mut() {
                    if !v.is_empty() {
                        g.1.push((k == "allow", v));
                    }
                }
            }
            _ => {}
        }
    }
    let agent = agent.to_ascii_lowercase();
    let pick = groups
        .iter()
        .find(|g| g.0.iter().any(|a| a == &agent))
        .or_else(|| groups.iter().find(|g| g.0.iter().any(|a| a == "*")));
    let Some((_, rules)) = pick else {
        return true;
    };
    let mut best: Option<(usize, bool)> = None;
    for (allow, prefix) in rules {
        let p = prefix.trim_end_matches('$');
        if path.starts_with(p) {
            let len = p.len();
            best = match best {
                Some((l, a)) if l > len || (l == len && a) => Some((l, a)),
                _ => Some((len, *allow)),
            };
        }
    }
    best.is_none_or(|(_, allow)| allow)
}

#[async_trait]
impl VendorModule for WebFetch {
    fn manifest(&self) -> &VendorManifest {
        &self.manifest
    }

    async fn tools(&self, _cx: &CallCtx<'_>) -> Result<Vec<ToolDef>, VendorError> {
        Ok(vec![ToolDef {
            name: "fetch".into(),
            description: "Fetches a public web page (http or https) as text, up to 2 MB.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"url": {"type": "string"}},
                "required": ["url"]
            }),
            risk_class: RiskClass::Read,
        }])
    }

    async fn call(
        &self,
        _cx: &CallCtx<'_>,
        tool: &str,
        args: Value,
    ) -> Result<ToolOutput, VendorError> {
        if tool != "fetch" {
            return Err(VendorError::UnknownTool(tool.into()));
        }
        let raw = args
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| VendorError::Invalid("url is required".into()))?;
        let url = Url::parse(raw).map_err(|_| VendorError::Invalid("That isn't a URL".into()))?;
        if url.scheme() != "http" && url.scheme() != "https" {
            return Ok(ToolOutput::error(
                "Only http and https pages can be fetched",
            ));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Ok(ToolOutput::error("URLs with credentials can't be fetched"));
        }
        if !self.allow_private && is_private_url(&url).await {
            return Ok(ToolOutput::error(
                "Private and local addresses can't be fetched",
            ));
        }
        let mut client = PinnedClient::anonymous(Vec::new());
        if self.allow_private {
            if let Some(p) =
                crate::netguard::PinnedHost::from_url(url.origin().ascii_serialization().as_str())
            {
                client = client.reachable(crate::netguard::PinnedHost {
                    path_prefix: "/".into(),
                    ..p
                });
            }
        }
        if let Ok(robots_url) = url.join("/robots.txt") {
            if let Ok(r) = client.send(Outgoing::get(robots_url)).await {
                if r.status.is_success() && !robots_allows(&r.body, AGENT, url.path()) {
                    return Ok(ToolOutput::error(format!(
                        "{} doesn't allow fetching this page (robots.txt)",
                        url.host_str().unwrap_or_default()
                    )));
                }
            }
        }
        let resp = client.send(Outgoing::get(url)).await?;
        if !resp.status.is_success() {
            return Ok(ToolOutput::error(format!(
                "The page answered {}",
                resp.status.as_u16()
            )));
        }
        let mut body = resp.body;
        if body.len() > MAX_PAGE {
            let mut cut = MAX_PAGE;
            while !body.is_char_boundary(cut) {
                cut -= 1;
            }
            body.truncate(cut);
        }
        Ok(ToolOutput::ok(body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_pl_09_robots_rules_pick_the_longest_match_for_the_agent() {
        let robots = "User-agent: *\nDisallow: /private\nAllow: /private/ok\n\nUser-agent: other\nDisallow: /";
        assert!(robots_allows(robots, AGENT, "/"));
        assert!(!robots_allows(robots, AGENT, "/private/x"));
        assert!(robots_allows(robots, AGENT, "/private/ok/page"));
        assert!(!robots_allows(robots, "other", "/anything"));
        assert!(robots_allows("", AGENT, "/x"));
        assert!(!robots_allows(
            "User-agent: vibezeo\nDisallow: /\nUser-agent: *\nAllow: /",
            AGENT,
            "/a"
        ));
    }

    #[tokio::test]
    async fn f_pl_09_web_fetch_refuses_private_addresses() {
        let m = WebFetch::new();
        let conn =
            super::super::ConnectionInfo::new(tandem_core::ids::ConnectionId::new(), "web_fetch");
        let http = PinnedClient::anonymous(Vec::new());
        let cx = CallCtx {
            conn: &conn,
            http: &http,
        };
        for u in [
            "http://169.254.169.254/latest",
            "http://127.0.0.1:1/",
            "http://10.1.2.3/",
            "file:///etc/passwd",
        ] {
            let out = m.call(&cx, "fetch", json!({"url": u})).await.expect("call");
            assert!(out.is_error, "{u}");
        }
    }
}
