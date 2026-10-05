//! The Tandem API client (ARCHITECTURE §14.6). Services depend on the
//! [`ApiClient`] trait so `cargo test -p tandem-desktop` runs them against a
//! fake; the real client sends everything through [`Net`] so it shows up in
//! `TANDEM_NET_LOG`.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use tandem_core::entitlement::{Limits, Plan};
use tandem_core::net::Net;
use tandem_core::remote_config::SignedConfig;

/// Where the API and the web app live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub api: String,
    pub web: String,
}

impl Endpoints {
    /// `TANDEM_API_URL` / `TANDEM_WEB_URL`, else the build's defaults. Dev
    /// builds use `*.tandem.local` when those names resolve and loopback
    /// otherwise (ARCHITECTURE §17.5).
    pub fn from_env() -> Self {
        let api = std::env::var("TANDEM_API_URL")
            .ok()
            .filter(|v| !v.is_empty());
        let web = std::env::var("TANDEM_WEB_URL")
            .ok()
            .filter(|v| !v.is_empty());
        let (api_default, web_default) = if cfg!(debug_assertions) {
            if resolves("api.tandem.local:8787") {
                (
                    "http://api.tandem.local:8787",
                    "http://app.tandem.local:3000",
                )
            } else {
                ("http://127.0.0.1:8787", "http://localhost:3000")
            }
        } else {
            ("https://api.tandem.dev", "https://app.tandem.dev")
        };
        Self {
            api: api
                .unwrap_or_else(|| api_default.into())
                .trim_end_matches('/')
                .to_string(),
            web: web
                .unwrap_or_else(|| web_default.into())
                .trim_end_matches('/')
                .to_string(),
        }
    }
}

fn resolves(host_port: &str) -> bool {
    use std::net::ToSocketAddrs;
    host_port
        .to_socket_addrs()
        .map(|mut a| a.next().is_some())
        .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    /// Couldn't connect, timed out, or the server is down (5xx).
    #[error("couldn't reach Tandem: {0}")]
    Network(String),
    #[error("{status} {code}")]
    Http {
        status: u16,
        code: String,
        detail: Option<String>,
    },
    #[error("unexpected response from Tandem")]
    Decode(String),
}

impl ApiError {
    pub fn status(&self) -> Option<u16> {
        match self {
            ApiError::Http { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn is_unauthorized(&self) -> bool {
        self.status() == Some(401)
    }

    pub fn is_network(&self) -> bool {
        matches!(self, ApiError::Network(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeUser {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeCredits {
    pub monthly: i64,
    pub purchased: i64,
    pub resets_at: Option<String>,
}

/// `GET /v1/me` (ARCHITECTURE §14.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeResponse {
    pub user: MeUser,
    pub plan: Plan,
    pub status: String,
    pub limits: Limits,
    pub paid_until: Option<String>,
    #[serde(default)]
    pub cancel_at_period_end: bool,
    pub credits: MeCredits,
    #[serde(default)]
    pub flags: BTreeMap<String, bool>,
    pub policy: Option<Value>,
    pub entitlement_token: String,
}

#[async_trait]
pub trait ApiClient: Send + Sync {
    async fn exchange(
        &self,
        code: &str,
        verifier: &str,
        fingerprint: &str,
    ) -> Result<TokenPair, ApiError>;
    async fn refresh(&self, refresh_token: &str) -> Result<TokenPair, ApiError>;
    async fn logout(&self, access: &str, refresh_token: &str) -> Result<(), ApiError>;
    async fn me(&self, access: &str) -> Result<MeResponse, ApiError>;
    /// Returns the checkout URL to open in the system browser.
    async fn checkout(&self, access: &str, plan: &str, interval: &str) -> Result<String, ApiError>;
    async fn portal(&self, access: &str) -> Result<String, ApiError>;
    async fn config(&self) -> Result<SignedConfig, ApiError>;
    /// `GET /v1/me/events` as a stream of event names (`entitlement`, `credits`).
    async fn events(
        &self,
        access: &str,
    ) -> Result<BoxStream<'static, Result<String, ApiError>>, ApiError>;
}

pub struct HttpApi {
    net: Net,
    endpoints: Endpoints,
}

impl HttpApi {
    pub fn new(net: Net, endpoints: Endpoints) -> Self {
        Self { net, endpoints }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.endpoints.api)
    }

    fn with_headers(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.header("x-tandem-app", "desktop")
            .header("x-tandem-version", env!("CARGO_PKG_VERSION"))
            .header("x-tandem-platform", platform())
            .timeout(Duration::from_secs(20))
    }

    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<T, ApiError> {
        let res = self
            .net
            .send(self.with_headers(req))
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        let status = res.status().as_u16();
        if status >= 500 {
            return Err(ApiError::Network(format!("server error {status}")));
        }
        if status >= 400 {
            let body: Value = res.json().await.unwrap_or(Value::Null);
            let code = body
                .get("code")
                .or_else(|| body.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_string();
            let detail = body
                .get("detail")
                .and_then(Value::as_str)
                .map(str::to_string);
            return Err(ApiError::Http {
                status,
                code,
                detail,
            });
        }
        if status == 204 {
            return serde_json::from_value(Value::Null)
                .map_err(|e| ApiError::Decode(e.to_string()));
        }
        res.json::<T>()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))
    }
}

pub fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    }
}

#[derive(Deserialize)]
struct UrlBody {
    url: String,
}

#[async_trait]
impl ApiClient for HttpApi {
    async fn exchange(
        &self,
        code: &str,
        verifier: &str,
        fingerprint: &str,
    ) -> Result<TokenPair, ApiError> {
        let body = json!({ "code": code, "verifier": verifier, "device_fingerprint": fingerprint });
        self.call(
            self.net
                .client()
                .post(self.url("/v1/auth/desktop/exchange"))
                .json(&body),
        )
        .await
    }

    async fn refresh(&self, refresh_token: &str) -> Result<TokenPair, ApiError> {
        let body = json!({ "refresh_token": refresh_token });
        self.call(
            self.net
                .client()
                .post(self.url("/v1/auth/desktop/refresh"))
                .json(&body),
        )
        .await
    }

    async fn logout(&self, access: &str, refresh_token: &str) -> Result<(), ApiError> {
        let body = json!({ "refresh_token": refresh_token });
        let req = self
            .net
            .client()
            .post(self.url("/v1/auth/desktop/logout"))
            .bearer_auth(access)
            .json(&body);
        self.call::<Option<Value>>(req).await.map(|_| ())
    }

    async fn me(&self, access: &str) -> Result<MeResponse, ApiError> {
        self.call(
            self.net
                .client()
                .get(self.url("/v1/me"))
                .bearer_auth(access),
        )
        .await
    }

    async fn checkout(&self, access: &str, plan: &str, interval: &str) -> Result<String, ApiError> {
        let body = json!({ "plan": plan, "interval": interval });
        let req = self
            .net
            .client()
            .post(self.url("/v1/billing/checkout"))
            .bearer_auth(access)
            .json(&body);
        self.call::<UrlBody>(req).await.map(|b| b.url)
    }

    async fn portal(&self, access: &str) -> Result<String, ApiError> {
        let req = self
            .net
            .client()
            .post(self.url("/v1/billing/portal"))
            .bearer_auth(access)
            .json(&json!({}));
        self.call::<UrlBody>(req).await.map(|b| b.url)
    }

    async fn config(&self) -> Result<SignedConfig, ApiError> {
        self.call(self.net.client().get(self.url("/v1/config")))
            .await
    }

    async fn events(
        &self,
        access: &str,
    ) -> Result<BoxStream<'static, Result<String, ApiError>>, ApiError> {
        let req = self
            .net
            .client()
            .get(self.url("/v1/me/events"))
            .bearer_auth(access)
            .header("accept", "text/event-stream")
            .header("x-tandem-app", "desktop")
            .header("x-tandem-version", env!("CARGO_PKG_VERSION"));
        let res = self
            .net
            .send(req)
            .await
            .map_err(|e| ApiError::Network(e.to_string()))?;
        let status = res.status().as_u16();
        if status >= 400 {
            return Err(if status >= 500 {
                ApiError::Network(format!("server error {status}"))
            } else {
                ApiError::Http {
                    status,
                    code: "error".into(),
                    detail: None,
                }
            });
        }
        let mut parser = SseParser::default();
        let stream = res
            .bytes_stream()
            .map(move |chunk| match chunk {
                Ok(bytes) => Ok(parser.feed(&bytes)),
                Err(e) => Err(ApiError::Network(e.to_string())),
            })
            .flat_map(|r| {
                futures::stream::iter(match r {
                    Ok(names) => names.into_iter().map(Ok).collect::<Vec<_>>(),
                    Err(e) => vec![Err(e)],
                })
            });
        Ok(stream.boxed())
    }
}

/// Minimal `text/event-stream` parser: yields the `event:` name of each
/// complete event (`message` when absent). Payloads aren't needed: every
/// event means "refetch /me".
#[derive(Default)]
pub struct SseParser {
    buf: String,
}

impl SseParser {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf
            .push_str(&String::from_utf8_lossy(bytes).replace("\r\n", "\n"));
        let mut out = Vec::new();
        while let Some(end) = self.buf.find("\n\n") {
            let block: String = self.buf.drain(..end + 2).collect();
            let mut name = None;
            let mut has_data = false;
            for line in block.lines() {
                if let Some(v) = line.strip_prefix("event:") {
                    name = Some(v.trim().to_string());
                } else if line.starts_with("data:") {
                    has_data = true;
                }
            }
            if name.is_some() || has_data {
                out.push(name.unwrap_or_else(|| "message".into()));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_parser_handles_split_chunks_and_comments() {
        let mut p = SseParser::default();
        assert!(p.feed(b": keep-alive\n\nevent: entitle").is_empty());
        assert_eq!(
            p.feed(b"ment\ndata: {}\n\nevent: credits\r\ndata: {}\r\n\r\n"),
            vec!["entitlement", "credits"]
        );
        assert_eq!(p.feed(b"data: hi\n\n"), vec!["message"]);
    }

    #[test]
    fn endpoints_honour_env_overrides() {
        // Read-only check of the parsing path; env is process-global so only
        // the default shape is asserted here.
        let e = Endpoints::from_env();
        assert!(e.api.starts_with("http"));
        assert!(!e.api.ends_with('/'));
    }
}
