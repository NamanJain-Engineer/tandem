//! OAuth 2.1 for plugins (ARCHITECTURE §11.6, F-PL-03): discovery (RFC 9728
//! protected-resource metadata, then RFC 8414 / OpenID metadata), dynamic
//! client registration (RFC 7591; a public client with PKCE, or confidential
//! when the server insists), the browser sign-in with a loopback callback,
//! the code exchange, refresh and revocation.
//!
//! Every URL the flow touches must be on one of the module's auth hosts (or
//! its MCP host); anything else, and above all a private address, fails the
//! step it was met in. Errors name the step and carry the provider's text
//! untruncated.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use reqwest::header::{HeaderValue, ACCEPT, CONTENT_TYPE, LOCATION, WWW_AUTHENTICATE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use ts_rs::TS;
use url::Url;

use crate::credentials::Credential;
use crate::netguard::{is_private_url, PinnedHost};
use crate::vendors::{Upstream, VendorManifest};

/// How long the user has to sign in.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Step {
    Discovery,
    Registration,
    SignIn,
    TokenExchange,
    Verification,
    Timeout,
}

impl Step {
    pub fn as_str(self) -> &'static str {
        match self {
            Step::Discovery => "discovery",
            Step::Registration => "registration",
            Step::SignIn => "sign_in",
            Step::TokenExchange => "token_exchange",
            Step::Verification => "verification",
            Step::Timeout => "timeout",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{} failed: {message}", step.as_str())]
pub struct OAuthError {
    pub step: Step,
    /// The provider's own words where it gave any, untruncated.
    pub message: String,
}

fn fail(step: Step, message: impl Into<String>) -> OAuthError {
    OAuthError {
        step,
        message: message.into(),
    }
}

/// Authorization server metadata (RFC 8414), the fields used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AsMetadata {
    #[serde(default)]
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub registration_endpoint: Option<String>,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
    #[serde(default)]
    pub token_endpoint_auth_methods_supported: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

/// Known endpoints for a vendor whose server publishes no metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticAs {
    pub authorize: &'static str,
    pub token: &'static str,
    pub revoke: Option<&'static str>,
}

/// The fixed endpoints of the static-client vendors.
pub fn static_as(plugin_id: &str) -> Option<StaticAs> {
    match plugin_id {
        "github" => Some(StaticAs {
            authorize: "https://github.com/login/oauth/authorize",
            token: "https://github.com/login/oauth/access_token",
            revoke: None,
        }),
        "slack" => Some(StaticAs {
            authorize: "https://slack.com/oauth/v2/authorize",
            token: "https://slack.com/api/oauth.v2.access",
            revoke: Some("https://slack.com/api/auth.revoke"),
        }),
        "gmail" => Some(StaticAs {
            authorize: "https://accounts.google.com/o/oauth2/v2/auth",
            token: "https://oauth2.googleapis.com/token",
            revoke: Some("https://oauth2.googleapis.com/revoke"),
        }),
        _ => None,
    }
}

/// Who the flow may talk to: the module's auth hosts and its MCP hosts.
#[derive(Debug, Clone)]
pub struct AuthScope {
    pub label: String,
    pub hosts: Vec<PinnedHost>,
}

impl AuthScope {
    pub fn of(m: &VendorManifest) -> Self {
        let mut hosts = m.auth_hosts.clone();
        if let Upstream::RemoteMcp { urls } = &m.upstream {
            hosts.extend(urls.iter().filter_map(|u| PinnedHost::from_url(u)));
        }
        Self {
            label: m.label.to_string(),
            hosts,
        }
    }

    /// Fails `step` unless `url` is on an allowed host; a private address
    /// is named as such (F-PL-03 AC2).
    pub async fn check(&self, url: &str, step: Step) -> Result<Url, OAuthError> {
        let u = Url::parse(url).map_err(|_| fail(step, format!("{url} isn't a valid address")))?;
        if self.hosts.iter().any(|h| h.same_origin(&u)) {
            return Ok(u);
        }
        let host = u.host_str().unwrap_or_default().to_string();
        if is_private_url(&u).await {
            return Err(fail(
                step,
                format!(
                    "Refused {host}: it's a private address, not one of {}'s sign-in hosts",
                    self.label
                ),
            ));
        }
        Err(fail(
            step,
            format!(
                "Refused {host}: it isn't one of {}'s sign-in hosts",
                self.label
            ),
        ))
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(HTTP_TIMEOUT)
        .user_agent(concat!("Vibezeo/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

/// One request of the flow, following redirects only within the scope.
async fn fetch(
    scope: &AuthScope,
    step: Step,
    mut req: reqwest::Request,
) -> Result<reqwest::Response, OAuthError> {
    let http = client();
    for _ in 0..=5 {
        scope.check(req.url().as_str(), step).await?;
        let retry = req.try_clone();
        let resp = http.execute(req).await.map_err(|e| {
            fail(
                if e.is_timeout() { Step::Timeout } else { step },
                format!("Couldn't reach the server: {e}"),
            )
        })?;
        if !resp.status().is_redirection() {
            return Ok(resp);
        }
        let next = resp
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|l| resp.url().join(l).ok())
            .ok_or_else(|| fail(step, "A redirect without a location"))?;
        let mut r = retry.ok_or_else(|| fail(step, "A redirect couldn't be followed"))?;
        *r.url_mut() = next;
        req = r;
    }
    Err(fail(step, "Too many redirects"))
}

/// `error` + `error_description` from an OAuth error body, or the body.
fn provider_text(body: &str) -> String {
    match serde_json::from_str::<Value>(body) {
        Ok(v) if v.get("error").is_some() => {
            let e = v
                .get("error")
                .map(|e| {
                    e.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| e.to_string())
                })
                .unwrap_or_default();
            match v.get("error_description").and_then(Value::as_str) {
                Some(d) => format!("{e}: {d}"),
                None => e,
            }
        }
        _ => body.trim().to_string(),
    }
}

async fn get_json(
    scope: &AuthScope,
    step: Step,
    url: &str,
) -> Result<(u16, Value, String), OAuthError> {
    let u = scope.check(url, step).await?;
    let mut req = reqwest::Request::new(reqwest::Method::GET, u);
    req.headers_mut()
        .insert(ACCEPT, HeaderValue::from_static("application/json"));
    let resp = fetch(scope, step, req).await?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    Ok((
        status,
        serde_json::from_str(&body).unwrap_or(Value::Null),
        body,
    ))
}

/// `resource_metadata="…"` from a `WWW-Authenticate` header.
pub fn resource_metadata_url(header: &str) -> Option<String> {
    let i = header.find("resource_metadata=")?;
    let rest = &header[i + "resource_metadata=".len()..];
    let rest = rest.trim_start_matches('"');
    let end = rest.find(['"', ',']).unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// RFC 8414 locations for an issuer: `/.well-known/oauth-authorization-server{path}`
/// and the OpenID fallback.
fn well_known(issuer: &str) -> Vec<String> {
    let Ok(u) = Url::parse(issuer) else {
        return Vec::new();
    };
    let origin = u.origin().ascii_serialization();
    let path = u.path().trim_end_matches('/');
    vec![
        format!("{origin}/.well-known/oauth-authorization-server{path}"),
        format!("{origin}/.well-known/openid-configuration{path}"),
        format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        ),
    ]
}

/// What discovery found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub metadata: AsMetadata,
    /// The MCP server's resource id (RFC 8707 `resource=`).
    pub resource: Option<String>,
}

/// Step 1: the authorization server's metadata.
pub async fn discover(m: &VendorManifest, scope: &AuthScope) -> Result<Discovery, OAuthError> {
    let step = Step::Discovery;
    let mut resource = None;
    let mut issuer: Option<String> = None;
    if let Upstream::RemoteMcp { urls } = &m.upstream {
        if let Some(mcp) = urls.first() {
            let u = scope.check(mcp, step).await?;
            let mut req = reqwest::Request::new(reqwest::Method::POST, u);
            req.headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            req.headers_mut().insert(
                ACCEPT,
                HeaderValue::from_static("application/json, text/event-stream"),
            );
            *req.body_mut() = Some(
                json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-03-26", "capabilities": {},
                "clientInfo": {"name": "vibezeo-gateway", "version": env!("CARGO_PKG_VERSION")}}})
                .to_string()
                .into(),
            );
            let resp = fetch(scope, step, req).await?;
            let prm_url = resp
                .headers()
                .get(WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok())
                .and_then(resource_metadata_url)
                .or_else(|| {
                    Url::parse(mcp).ok().map(|u| {
                        format!(
                            "{}/.well-known/oauth-protected-resource{}",
                            u.origin().ascii_serialization(),
                            u.path().trim_end_matches('/')
                        )
                    })
                });
            if let Some(prm) = prm_url {
                if let Ok((200, v, _)) = get_json(scope, step, &prm).await {
                    resource = v
                        .get("resource")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    issuer = v
                        .get("authorization_servers")
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
            }
            if resource.is_none() {
                resource = Some(mcp.clone());
            }
        }
    }
    if let Some(issuer) = &issuer {
        for wk in well_known(issuer) {
            match get_json(scope, step, &wk).await {
                Ok((200, v, _)) => {
                    let metadata: AsMetadata = serde_json::from_value(v)
                        .map_err(|_| fail(step, "The server's sign-in metadata is incomplete"))?;
                    for ep in [
                        Some(&metadata.authorization_endpoint),
                        Some(&metadata.token_endpoint),
                        metadata.registration_endpoint.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        // Registration is checked when it's used, under its own step.
                        if Some(ep) != metadata.registration_endpoint.as_ref() {
                            scope.check(ep, step).await?;
                        }
                    }
                    return Ok(Discovery { metadata, resource });
                }
                Ok(_) => continue,
                Err(e) if e.message.starts_with("Refused") => return Err(e),
                Err(_) => continue,
            }
        }
    }
    if let Some(s) = static_as(m.id) {
        scope.check(s.authorize, step).await?;
        scope.check(s.token, step).await?;
        return Ok(Discovery {
            metadata: AsMetadata {
                issuer: String::new(),
                authorization_endpoint: s.authorize.into(),
                token_endpoint: s.token.into(),
                registration_endpoint: None,
                revocation_endpoint: s.revoke.map(str::to_string),
                ..AsMetadata::default()
            },
            resource,
        });
    }
    Err(fail(
        step,
        format!("Couldn't find {}'s sign-in server", m.label),
    ))
}

/// A registered (or configured) client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientReg {
    pub client_id: String,
    pub client_secret: Option<String>,
}

/// Step 2: RFC 7591 registration, public first.
pub async fn register(
    scope: &AuthScope,
    metadata: &AsMetadata,
    redirect_uri: &str,
) -> Result<ClientReg, OAuthError> {
    let step = Step::Registration;
    let Some(endpoint) = &metadata.registration_endpoint else {
        return Err(fail(
            step,
            "The server doesn't register apps; a client id is needed",
        ));
    };
    let url = scope.check(endpoint, step).await?;
    let attempt = |method: &str| {
        json!({
            "client_name": "Vibezeo",
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": method,
        })
    };
    let public_ok = metadata.token_endpoint_auth_methods_supported.is_empty()
        || metadata
            .token_endpoint_auth_methods_supported
            .iter()
            .any(|m| m == "none");
    let methods: Vec<&str> = if public_ok {
        vec!["none", "client_secret_post"]
    } else {
        vec!["client_secret_post"]
    };
    let mut last = String::new();
    for (i, method) in methods.iter().enumerate() {
        let mut req = reqwest::Request::new(reqwest::Method::POST, url.clone());
        req.headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        *req.body_mut() = Some(attempt(method).to_string().into());
        let resp = fetch(scope, step, req).await?;
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        if (200..300).contains(&status) {
            let v: Value = serde_json::from_str(&body).map_err(|_| fail(step, body.clone()))?;
            let client_id = v
                .get("client_id")
                .and_then(Value::as_str)
                .ok_or_else(|| fail(step, "The server sent no client id"))?
                .to_string();
            return Ok(ClientReg {
                client_id,
                client_secret: v
                    .get("client_secret")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
        last = provider_text(&body);
        // A server that turns public clients away says so in
        // `invalid_client_metadata`; try once as a confidential client.
        let rejects_public =
            last.contains("invalid_client_metadata") && i == 0 && methods.len() > 1;
        if !rejects_public {
            break;
        }
    }
    Err(fail(step, last))
}

/// PKCE (S256) and the state of one attempt.
#[derive(Clone)]
pub struct Attempt {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

fn random(len: usize) -> String {
    let mut b = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}

impl Attempt {
    pub fn new() -> Self {
        // 48 random bytes → a 64-character verifier.
        let verifier = random(48);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
            state: random(32),
        }
    }
}

impl Default for Attempt {
    fn default() -> Self {
        Self::new()
    }
}

/// Step 3: where the browser goes.
pub fn authorize_url(
    metadata: &AsMetadata,
    client_id: &str,
    redirect_uri: &str,
    attempt: &Attempt,
    resource: Option<&str>,
    scopes: &[&str],
) -> Result<Url, OAuthError> {
    let mut u = Url::parse(&metadata.authorization_endpoint)
        .map_err(|_| fail(Step::SignIn, "The sign-in address isn't valid"))?;
    {
        let mut q = u.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("code_challenge", &attempt.challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &attempt.state);
        if let Some(r) = resource {
            q.append_pair("resource", r);
        }
        if !scopes.is_empty() {
            q.append_pair("scope", &scopes.join(" "));
        }
    }
    Ok(u)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// A token endpoint's answer as a credential.
fn credential_from(
    v: &Value,
    client_secret: Option<&str>,
    old_refresh: Option<&str>,
) -> Option<Credential> {
    let access = v.get("access_token").and_then(Value::as_str)?.to_string();
    Some(Credential::OAuth {
        access_token: access,
        refresh_token: v
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| old_refresh.map(str::to_string)),
        expires_at: v
            .get("expires_in")
            .and_then(Value::as_i64)
            .map(|s| now_ms() + s * 1000),
        token_type: v
            .get("token_type")
            .and_then(Value::as_str)
            .unwrap_or("Bearer")
            .to_string(),
        client_secret: client_secret.map(str::to_string),
    })
}

async fn token_request(
    scope: &AuthScope,
    step: Step,
    token_endpoint: &str,
    form: Vec<(&str, String)>,
) -> Result<Value, OAuthError> {
    let url = scope.check(token_endpoint, step).await?;
    let mut req = reqwest::Request::new(reqwest::Method::POST, url);
    req.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    req.headers_mut()
        .insert(ACCEPT, HeaderValue::from_static("application/json"));
    let body: String = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form)
        .finish();
    *req.body_mut() = Some(body.into());
    let resp = fetch(scope, step, req).await?;
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !(200..300).contains(&status) || v.get("error").is_some() {
        return Err(fail(step, provider_text(&text)));
    }
    Ok(v)
}

/// Step 4: the code for tokens.
pub async fn exchange(
    scope: &AuthScope,
    metadata: &AsMetadata,
    client: &ClientReg,
    code: &str,
    attempt: &Attempt,
    redirect_uri: &str,
    resource: Option<&str>,
) -> Result<Credential, OAuthError> {
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("client_id", client.client_id.clone()),
        ("code_verifier", attempt.verifier.clone()),
    ];
    if let Some(s) = &client.client_secret {
        form.push(("client_secret", s.clone()));
    }
    if let Some(r) = resource {
        form.push(("resource", r.to_string()));
    }
    let v = token_request(scope, Step::TokenExchange, &metadata.token_endpoint, form).await?;
    credential_from(&v, client.client_secret.as_deref(), None)
        .ok_or_else(|| fail(Step::TokenExchange, "The server sent no access token"))
}

/// Non-secret facts a connection keeps to refresh and revoke.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OAuthMeta {
    pub client_id: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
    #[serde(default)]
    pub resource: Option<String>,
}

impl OAuthMeta {
    pub fn from_meta(meta: &Value) -> Option<Self> {
        serde_json::from_value(meta.get("oauth")?.clone()).ok()
    }
}

/// A transparent refresh (ARCHITECTURE §11.5). Errors carry the provider's
/// text.
pub async fn refresh(
    scope: &AuthScope,
    meta: &OAuthMeta,
    old: &Credential,
) -> Result<Credential, OAuthError> {
    let Some(rt) = old.refresh_token() else {
        return Err(fail(Step::TokenExchange, "The sign-in can't be renewed"));
    };
    let mut form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", rt.to_string()),
        ("client_id", meta.client_id.clone()),
    ];
    if let Some(s) = old.client_secret() {
        form.push(("client_secret", s.to_string()));
    }
    if let Some(r) = &meta.resource {
        form.push(("resource", r.clone()));
    }
    let v = token_request(scope, Step::TokenExchange, &meta.token_endpoint, form).await?;
    credential_from(&v, old.client_secret(), Some(rt))
        .ok_or_else(|| fail(Step::TokenExchange, "The server sent no access token"))
}

/// RFC 7009 revocation, where the server has an endpoint. Best effort.
pub async fn revoke(
    scope: &AuthScope,
    meta: &OAuthMeta,
    cred: &Credential,
) -> Result<(), OAuthError> {
    let Some(endpoint) = &meta.revocation_endpoint else {
        return Ok(());
    };
    let token = cred.refresh_token().unwrap_or(cred.bearer()).to_string();
    let mut form = vec![("token", token), ("client_id", meta.client_id.clone())];
    if let Some(s) = cred.client_secret() {
        form.push(("client_secret", s.to_string()));
    }
    token_request(scope, Step::TokenExchange, endpoint, form)
        .await
        .map(|_| ())
}

/// What the browser brought back to the callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackResult {
    Code(String),
    /// The provider said no (`error`, `error_description`).
    Denied(String),
}

type Waiters = Arc<Mutex<HashMap<String, oneshot::Sender<CallbackResult>>>>;

/// The one loopback listener for plugin sign-ins
/// (`http://127.0.0.1:<port>/callback`), shared by every attempt. A
/// callback is delivered only to the attempt whose `state` it carries and
/// that is still waiting; a late one (cancelled, timed out, finished) is
/// ignored (F-PL-03 AC4).
#[derive(Clone)]
pub struct Loopback {
    pub port: u16,
    waiters: Waiters,
    ignored: Arc<std::sync::atomic::AtomicU64>,
}

const PAGE_OK: &str =
    "<!doctype html><meta charset=utf-8><title>Vibezeo</title><p>You can return to Vibezeo.</p>";
const PAGE_LATE: &str = "<!doctype html><meta charset=utf-8><title>Vibezeo</title><p>This sign-in was cancelled or has expired. You can close this tab.</p>";

impl Loopback {
    pub async fn start() -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let me = Self {
            port,
            waiters: Arc::default(),
            ignored: Arc::default(),
        };
        let serving = me.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let me = serving.clone();
                tokio::spawn(async move { me.serve(stream).await });
            }
        });
        Ok(me)
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }

    /// Waits for the callback carrying `state`.
    pub fn expect(&self, state: &str) -> oneshot::Receiver<CallbackResult> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut w) = self.waiters.lock() {
            w.insert(state.to_string(), tx);
        }
        rx
    }

    /// The attempt stopped waiting; its callback is ignored from now on.
    pub fn forget(&self, state: &str) {
        if let Ok(mut w) = self.waiters.lock() {
            w.remove(state);
        }
    }

    /// Callbacks that matched no waiting attempt.
    pub fn ignored(&self) -> u64 {
        self.ignored.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Delivers a callback's query (also used for `tandem://plugin/callback`).
    /// Returns whether a waiting attempt took it.
    pub fn deliver(&self, query: &str) -> bool {
        let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();
        let tx = params
            .get("state")
            .and_then(|s| self.waiters.lock().ok().and_then(|mut w| w.remove(s)));
        let Some(tx) = tx else {
            self.ignored
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tracing::info!(
                code = "plugins.late_callback",
                "ignored a sign-in callback no attempt is waiting for"
            );
            return false;
        };
        let result = match (params.get("code"), params.get("error")) {
            (Some(code), None) => CallbackResult::Code(code.clone()),
            (_, Some(e)) => CallbackResult::Denied(match params.get("error_description") {
                Some(d) => format!("{e}: {d}"),
                None => e.clone(),
            }),
            (None, None) => CallbackResult::Denied("The sign-in came back without a code".into()),
        };
        tx.send(result).is_ok()
    }

    async fn serve(&self, mut stream: tokio::net::TcpStream) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buf = vec![0u8; 8192];
        let n = match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            _ => return,
        };
        let head = String::from_utf8_lossy(&buf[..n]);
        let target = head
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("");
        let (status, page) = match target.strip_prefix("/callback") {
            Some(rest) => {
                let query = rest.strip_prefix('?').unwrap_or("");
                if self.deliver(query) {
                    ("200 OK", PAGE_OK)
                } else {
                    ("200 OK", PAGE_LATE)
                }
            }
            None => ("404 Not Found", "Not found"),
        };
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{page}",
            page.len()
        );
        let _ = stream.write_all(resp.as_bytes()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_pl_03_resource_metadata_is_read_from_www_authenticate() {
        assert_eq!(
            resource_metadata_url(
                r#"Bearer error="invalid_token", resource_metadata="https://mcp.x.com/.well-known/oauth-protected-resource/mcp""#
            ),
            Some("https://mcp.x.com/.well-known/oauth-protected-resource/mcp".into())
        );
        assert_eq!(resource_metadata_url("Bearer"), None);
    }

    #[test]
    fn f_pl_03_pkce_uses_s256_and_a_64_character_verifier() {
        let a = Attempt::new();
        assert_eq!(a.verifier.len(), 64);
        assert_eq!(
            a.challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(a.verifier.as_bytes()))
        );
        assert_ne!(Attempt::new().state, a.state);
    }

    #[test]
    fn f_pl_03_provider_errors_keep_their_words() {
        assert_eq!(
            provider_text(
                r#"{"error":"invalid_grant","error_description":"The code has expired and this text is long on purpose"}"#
            ),
            "invalid_grant: The code has expired and this text is long on purpose"
        );
    }

    #[tokio::test]
    async fn f_pl_03_4_a_late_callback_is_ignored() {
        let lb = Loopback::start().await.expect("loopback");
        let rx = lb.expect("s1");
        lb.forget("s1");
        assert!(!lb.deliver("code=abc&state=s1"));
        assert!(rx.await.is_err());
        assert_eq!(lb.ignored(), 1);
        let rx = lb.expect("s2");
        let body = reqwest::get(format!("{}?code=xyz&state=s2", lb.redirect_uri()))
            .await
            .expect("get")
            .text()
            .await
            .expect("text");
        assert!(body.contains("You can return to Vibezeo."));
        assert_eq!(
            rx.await.expect("delivered"),
            CallbackResult::Code("xyz".into())
        );
    }
}
