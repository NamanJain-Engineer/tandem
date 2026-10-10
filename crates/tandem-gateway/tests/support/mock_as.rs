//! A mock OAuth authorization server with a GitHub-shaped MCP upstream
//! behind it (MILESTONES M12 "Mocks and fakes"): RFC 9728 resource
//! metadata, RFC 8414 metadata, RFC 7591 registration, PKCE (S256) at the
//! authorize and token endpoints, refresh, revocation, and switches for the
//! failures the tests need. Used by the gateway's tests and, as the
//! test-only `mock-plugins` binary, by the desktop E2E suite.
//!
//! Plugin upstreams are served under `/<plugin>/…` (the catalog's
//! `rebase`); `/__switch?name=value`, `/__state` and `/__reset` steer it.
#![allow(dead_code)]

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, Response, StatusCode};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default)]
pub struct Switches {
    /// Registration turns public clients away (`invalid_client_metadata`).
    pub reject_public: bool,
    /// The metadata's endpoints point at the cloud metadata address.
    pub private_redirect: bool,
    /// `/authorize` redirects only after this many ms (late callbacks).
    pub authorize_delay_ms: u64,
    /// The token endpoint fails the code exchange with this text.
    pub token_error: Option<String>,
    /// Refresh grants fail.
    pub refresh_fails: bool,
    /// The upstream refuses every token (a verification that fails).
    pub upstream_rejects: bool,
    /// Tokens expire after this many seconds.
    pub expires_in: i64,
    /// The account `get_me` names.
    pub login: String,
}

#[derive(Debug, Default)]
pub struct AsState {
    pub switches: Switches,
    /// client_id → (secret, redirect URIs).
    pub clients: HashMap<String, (Option<String>, Vec<String>)>,
    /// code → (client_id, challenge, redirect_uri).
    codes: HashMap<String, (String, String, String)>,
    /// Valid access tokens and refresh tokens.
    pub access: Vec<String>,
    pub refresh: Vec<String>,
    pub revoked: Vec<String>,
    /// Upstream `tools/call`s that reached the vendor, by tool name.
    pub calls: HashMap<String, u64>,
    /// Every request path, in order.
    pub paths: Vec<String>,
    n: u64,
}

impl AsState {
    fn next(&mut self, prefix: &str) -> String {
        self.n += 1;
        format!(
            "{prefix}{:04}{}",
            self.n,
            URL_SAFE_NO_PAD.encode(rand_bytes())
        )
    }
}

fn rand_bytes() -> [u8; 12] {
    use rand::RngCore;
    let mut b = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut b);
    b
}

#[derive(Clone)]
pub struct MockAs {
    pub addr: SocketAddr,
    pub state: Arc<Mutex<AsState>>,
}

fn lock(s: &Arc<Mutex<AsState>>) -> std::sync::MutexGuard<'_, AsState> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

fn default_switches() -> Switches {
    Switches {
        expires_in: 3600,
        login: "octo-shop".into(),
        ..Switches::default()
    }
}

impl MockAs {
    /// Starts on `port` (0: any) on 127.0.0.1.
    pub async fn start(port: u16) -> std::io::Result<Self> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
        let addr = listener.local_addr()?;
        let state = Arc::new(Mutex::new(AsState {
            switches: default_switches(),
            ..AsState::default()
        }));
        let app = axum::Router::new()
            .fallback(handle)
            .with_state((state.clone(), addr));
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self { addr, state })
    }

    pub fn uri(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn switches(&self, f: impl FnOnce(&mut Switches)) {
        f(&mut lock(&self.state).switches);
    }

    pub fn calls(&self, tool: &str) -> u64 {
        lock(&self.state).calls.get(tool).copied().unwrap_or(0)
    }

    pub fn access_tokens(&self) -> Vec<String> {
        lock(&self.state).access.clone()
    }
}

fn query_map(q: Option<&str>) -> HashMap<String, String> {
    url::form_urlencoded::parse(q.unwrap_or_default().as_bytes())
        .into_owned()
        .collect()
}

fn json_resp(status: StatusCode, v: Value) -> Response<Body> {
    let mut r = Response::new(Body::from(v.to_string()));
    *r.status_mut() = status;
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    r
}

fn redirect(to: &str) -> Response<Body> {
    let mut r = Response::new(Body::empty());
    *r.status_mut() = StatusCode::FOUND;
    if let Ok(v) = HeaderValue::from_str(to) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    r
}

/// The GitHub-shaped tools the mock upstream offers.
fn github_tools() -> Value {
    let obj =
        |props: Value, req: Value| json!({"type": "object", "properties": props, "required": req});
    json!([
        {"name": "get_me", "description": "Who you are.", "inputSchema": obj(json!({}), json!([]))},
        {"name": "list_issues", "description": "Lists a repository's issues.", "inputSchema": obj(json!({"owner": {"type": "string"}, "repo": {"type": "string"}}), json!(["owner", "repo"]))},
        {"name": "create_issue", "description": "Opens an issue.", "inputSchema": obj(json!({"owner": {"type": "string"}, "repo": {"type": "string"}, "title": {"type": "string"}, "body": {"type": "string"}}), json!(["owner", "repo", "title"]))},
        {"name": "delete_file", "description": "Deletes a file in a repository.", "inputSchema": obj(json!({"owner": {"type": "string"}, "repo": {"type": "string"}, "path": {"type": "string"}}), json!(["owner", "repo", "path"]))}
    ])
}

async fn handle(
    State((state, addr)): State<(Arc<Mutex<AsState>>, SocketAddr)>,
    req: Request,
) -> Response<Body> {
    let base = format!("http://{addr}");
    let path = req.uri().path().to_string();
    let query = req.uri().query().map(str::to_string);
    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    let body = to_bytes(req.into_body(), 1 << 20).await.unwrap_or_default();
    lock(&state).paths.push(path.clone());
    let sw = lock(&state).switches.clone();
    let metadata_host = if sw.private_redirect {
        "http://169.254.169.254".to_string()
    } else {
        base.clone()
    };

    match path.as_str() {
        "/__reset" => {
            *lock(&state) = AsState {
                switches: default_switches(),
                ..AsState::default()
            };
            json_resp(StatusCode::OK, json!({"ok": true}))
        }
        "/__switch" => {
            let q = query_map(query.as_deref());
            let mut s = lock(&state);
            for (k, v) in q {
                match k.as_str() {
                    "reject_public" => s.switches.reject_public = v == "1",
                    "private_redirect" => s.switches.private_redirect = v == "1",
                    "authorize_delay_ms" => s.switches.authorize_delay_ms = v.parse().unwrap_or(0),
                    "token_error" => s.switches.token_error = (!v.is_empty()).then_some(v),
                    "refresh_fails" => s.switches.refresh_fails = v == "1",
                    "upstream_rejects" => s.switches.upstream_rejects = v == "1",
                    "expires_in" => s.switches.expires_in = v.parse().unwrap_or(3600),
                    "login" => s.switches.login = v,
                    _ => {}
                }
            }
            json_resp(StatusCode::OK, json!({"ok": true}))
        }
        "/__state" => {
            let s = lock(&state);
            json_resp(
                StatusCode::OK,
                json!({
                    "calls": s.calls, "access": s.access, "refresh": s.refresh,
                    "revoked": s.revoked, "clients": s.clients.len(), "paths": s.paths,
                }),
            )
        }
        p if p.starts_with("/.well-known/oauth-protected-resource") => json_resp(
            StatusCode::OK,
            json!({
                "resource": format!("{base}/github/api.githubcopilot.com/mcp/"),
                "authorization_servers": [base],
            }),
        ),
        p if p.starts_with("/.well-known/oauth-authorization-server")
            || p.starts_with("/.well-known/openid-configuration") =>
        {
            json_resp(
                StatusCode::OK,
                json!({
                    "issuer": base,
                    "authorization_endpoint": format!("{metadata_host}/authorize"),
                    "token_endpoint": format!("{base}/token"),
                    "registration_endpoint": format!("{metadata_host}/register"),
                    "revocation_endpoint": format!("{base}/revoke"),
                    "code_challenge_methods_supported": ["S256"],
                    "token_endpoint_auth_methods_supported": if sw.reject_public { json!(["client_secret_post"]) } else { json!(["none", "client_secret_post"]) },
                }),
            )
        }
        "/register" => {
            let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let method = v
                .get("token_endpoint_auth_method")
                .and_then(Value::as_str)
                .unwrap_or("none");
            if sw.reject_public && method == "none" {
                return json_resp(
                    StatusCode::BAD_REQUEST,
                    json!({"error": "invalid_client_metadata", "error_description": "token_endpoint_auth_method none is not allowed"}),
                );
            }
            let redirects: Vec<String> = v
                .get("redirect_uris")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let mut s = lock(&state);
            let id = s.next("client_");
            let secret = (method != "none").then(|| s.next("secret_"));
            s.clients.insert(id.clone(), (secret.clone(), redirects));
            let mut out = json!({"client_id": id, "token_endpoint_auth_method": method});
            if let Some(sec) = secret {
                out["client_secret"] = json!(sec);
            }
            json_resp(StatusCode::CREATED, out)
        }
        "/authorize" => {
            let q = query_map(query.as_deref());
            let (Some(client), Some(redirect_uri), Some(challenge), Some(st)) = (
                q.get("client_id"),
                q.get("redirect_uri"),
                q.get("code_challenge"),
                q.get("state"),
            ) else {
                return json_resp(StatusCode::BAD_REQUEST, json!({"error": "invalid_request"}));
            };
            if q.get("code_challenge_method").map(String::as_str) != Some("S256") {
                return json_resp(
                    StatusCode::BAD_REQUEST,
                    json!({"error": "invalid_request", "error_description": "PKCE S256 required"}),
                );
            }
            {
                let s = lock(&state);
                match s.clients.get(client) {
                    Some((_, uris)) if uris.is_empty() || uris.contains(redirect_uri) => {}
                    _ => {
                        return json_resp(
                            StatusCode::BAD_REQUEST,
                            json!({"error": "invalid_client"}),
                        )
                    }
                }
            }
            if sw.authorize_delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(sw.authorize_delay_ms)).await;
            }
            let code = {
                let mut s = lock(&state);
                let code = s.next("code_");
                s.codes.insert(
                    code.clone(),
                    (client.clone(), challenge.clone(), redirect_uri.clone()),
                );
                code
            };
            let sep = if redirect_uri.contains('?') { '&' } else { '?' };
            let st = url::form_urlencoded::byte_serialize(st.as_bytes()).collect::<String>();
            redirect(&format!("{redirect_uri}{sep}code={code}&state={st}"))
        }
        "/token" => {
            let f: HashMap<String, String> =
                url::form_urlencoded::parse(&body).into_owned().collect();
            let mut s = lock(&state);
            let client_ok = |s: &AsState, id: Option<&String>| -> bool {
                match id.and_then(|i| s.clients.get(i)) {
                    Some((Some(secret), _)) => f.get("client_secret") == Some(secret),
                    Some((None, _)) => true,
                    None => false,
                }
            };
            match f.get("grant_type").map(String::as_str) {
                Some("authorization_code") => {
                    if let Some(e) = &sw.token_error {
                        return json_resp(
                            StatusCode::BAD_REQUEST,
                            json!({"error": "invalid_grant", "error_description": e}),
                        );
                    }
                    let Some((client, challenge, redirect_uri)) =
                        f.get("code").and_then(|c| s.codes.remove(c))
                    else {
                        return json_resp(
                            StatusCode::BAD_REQUEST,
                            json!({"error": "invalid_grant", "error_description": "unknown or used code"}),
                        );
                    };
                    let verifier = f.get("code_verifier").cloned().unwrap_or_default();
                    let computed = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
                    if computed != challenge
                        || f.get("client_id") != Some(&client)
                        || f.get("redirect_uri") != Some(&redirect_uri)
                        || !client_ok(&s, Some(&client))
                    {
                        return json_resp(
                            StatusCode::BAD_REQUEST,
                            json!({"error": "invalid_grant", "error_description": "PKCE or client check failed"}),
                        );
                    }
                    let (a, r) = (s.next("gho_access_"), s.next("ghr_refresh_"));
                    s.access.push(a.clone());
                    s.refresh.push(r.clone());
                    json_resp(
                        StatusCode::OK,
                        json!({"access_token": a, "refresh_token": r, "token_type": "Bearer", "expires_in": sw.expires_in}),
                    )
                }
                Some("refresh_token") => {
                    let rt = f.get("refresh_token").cloned().unwrap_or_default();
                    if sw.refresh_fails
                        || !s.refresh.contains(&rt)
                        || !client_ok(&s, f.get("client_id"))
                    {
                        return json_resp(
                            StatusCode::BAD_REQUEST,
                            json!({"error": "invalid_grant", "error_description": "refresh token revoked"}),
                        );
                    }
                    let a = s.next("gho_access_");
                    s.access.push(a.clone());
                    json_resp(
                        StatusCode::OK,
                        json!({"access_token": a, "token_type": "Bearer", "expires_in": sw.expires_in}),
                    )
                }
                _ => json_resp(
                    StatusCode::BAD_REQUEST,
                    json!({"error": "unsupported_grant_type"}),
                ),
            }
        }
        "/revoke" => {
            let f: HashMap<String, String> =
                url::form_urlencoded::parse(&body).into_owned().collect();
            let mut s = lock(&state);
            if let Some(t) = f.get("token") {
                s.revoked.push(t.clone());
                s.refresh.retain(|x| x != t);
                s.access.retain(|x| x != t);
            }
            json_resp(StatusCode::OK, json!({}))
        }
        p if p.starts_with("/github/") => {
            let ok = !sw.upstream_rejects
                && auth
                    .as_ref()
                    .is_some_and(|t| lock(&state).access.contains(t));
            if !ok {
                let mut r = json_resp(StatusCode::UNAUTHORIZED, json!({"error": "unauthorized"}));
                if let Ok(v) = HeaderValue::from_str(&format!(
                    "Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource\""
                )) {
                    r.headers_mut().insert(header::WWW_AUTHENTICATE, v);
                }
                return r;
            }
            let msg: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            match msg.get("method").and_then(Value::as_str) {
                Some("initialize") => {
                    let mut r = json_resp(
                        StatusCode::OK,
                        json!({"jsonrpc": "2.0", "id": id, "result": {
                        "protocolVersion": "2025-03-26", "capabilities": {"tools": {}},
                        "serverInfo": {"name": "mock-github", "version": "1"}}}),
                    );
                    r.headers_mut()
                        .insert("mcp-session-id", HeaderValue::from_static("mock-session"));
                    r
                }
                Some("tools/list") => json_resp(
                    StatusCode::OK,
                    json!({"jsonrpc": "2.0", "id": id, "result": {"tools": github_tools()}}),
                ),
                Some("tools/call") => {
                    let name = msg
                        .pointer("/params/name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let args = msg
                        .pointer("/params/arguments")
                        .cloned()
                        .unwrap_or(Value::Null);
                    *lock(&state).calls.entry(name.clone()).or_default() += 1;
                    let text = match name.as_str() {
                        "get_me" => json!({"login": sw.login}).to_string(),
                        "list_issues" => json!([{"number": 1, "title": "First"}]).to_string(),
                        "create_issue" => json!({"number": 42, "title": args.get("title"), "html_url": "https://github.example/issues/42"}).to_string(),
                        "delete_file" => json!({"deleted": args.get("path")}).to_string(),
                        _ => "{}".to_string(),
                    };
                    json_resp(
                        StatusCode::OK,
                        json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": text}], "isError": false}}),
                    )
                }
                None => {
                    let mut r = Response::new(Body::empty());
                    *r.status_mut() = StatusCode::ACCEPTED;
                    r
                }
                _ => json_resp(
                    StatusCode::OK,
                    json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "unknown method"}}),
                ),
            }
        }
        _ => json_resp(StatusCode::NOT_FOUND, json!({"error": "not_found"})),
    }
}
