//! `PinnedClient` (ARCHITECTURE §11.5, F-PL-04): every upstream request of
//! a plugin goes through it. The credential is attached only when the URL
//! matches one of the module's pinned hosts exactly; redirects are followed
//! by hand (at most 5), the credential is dropped on any hop that isn't
//! pinned, and a hop to a private, loopback or link-local address that isn't
//! pinned is refused. Response bodies are scanned for the credential and
//! anything echoed back is redacted before it can reach an engine.

use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, LOCATION};
use reqwest::{Method, StatusCode};
use url::Url;
use zeroize::Zeroizing;

use crate::netguard::{any_matches, is_private_url, PinnedHost};
use crate::vendors::VendorError;

/// At most this many redirects per request.
pub const MAX_REDIRECTS: usize = 5;
/// Largest response body a plugin may hand back (web fetch caps lower).
pub const MAX_BODY: usize = 4 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(60);
pub const REDACTED: &str = "[REDACTED]";

/// One request, before the credential is (maybe) attached.
#[derive(Debug, Clone)]
pub struct Outgoing {
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
}

impl Outgoing {
    pub fn get(url: Url) -> Self {
        Self {
            method: Method::GET,
            url,
            headers: HeaderMap::new(),
            body: None,
        }
    }

    pub fn json(method: Method, url: Url, body: &serde_json::Value) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        Self {
            method,
            url,
            headers,
            body: Some(body.to_string().into_bytes()),
        }
    }

    pub fn header(mut self, name: &'static str, value: &str) -> Self {
        if let Ok(v) = HeaderValue::from_str(value) {
            self.headers.insert(HeaderName::from_static(name), v);
        }
        self
    }
}

/// What came back, with the credential redacted out of the body.
#[derive(Debug, Clone)]
pub struct Incoming {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
    /// The URL that answered (after redirects).
    pub url: Url,
}

impl Incoming {
    pub fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_str(&self.body).ok()
    }
}

/// The plugin's HTTP client for one connection.
pub struct PinnedClient {
    client: reqwest::Client,
    pins: Vec<PinnedHost>,
    /// Places a redirect may reach although they're private, never with
    /// the credential (tests; a vendor's own local helper).
    reachable: Vec<PinnedHost>,
    /// The `Authorization` value (`Bearer …`), if the connection has one.
    authorization: Option<Zeroizing<String>>,
    /// Every secret string that must never come back to an engine.
    secrets: Vec<Zeroizing<String>>,
    net: Option<tandem_core::net::Net>,
}

impl std::fmt::Debug for PinnedClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PinnedClient")
            .field("pins", &self.pins)
            .field(
                "authorization",
                &self.authorization.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(TIMEOUT)
        .user_agent(concat!("Vibezeo/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

impl PinnedClient {
    /// A client with no credential (discovery, web fetch, verification of
    /// a key not yet saved goes through [`PinnedClient::with_bearer`]).
    pub fn anonymous(pins: Vec<PinnedHost>) -> Self {
        Self {
            client: http(),
            pins,
            reachable: Vec::new(),
            authorization: None,
            secrets: Vec::new(),
            net: None,
        }
    }

    /// A client that sends `Authorization: Bearer <token>` to pinned hosts.
    pub fn with_bearer(pins: Vec<PinnedHost>, token: &str) -> Self {
        let mut me = Self::anonymous(pins);
        me.authorization = Some(Zeroizing::new(format!("Bearer {token}")));
        me.secrets.push(Zeroizing::new(token.to_string()));
        me
    }

    /// Other secret strings of the connection (a refresh token, a client
    /// secret) that must be redacted if an upstream echoes them.
    pub fn also_redact(mut self, secret: &str) -> Self {
        if secret.len() >= 4 {
            self.secrets.push(Zeroizing::new(secret.to_string()));
        }
        self
    }

    /// Lets redirects reach `pin` (a private address) without the credential.
    pub fn reachable(mut self, pin: PinnedHost) -> Self {
        self.reachable.push(pin);
        self
    }

    pub fn with_net(mut self, net: tandem_core::net::Net) -> Self {
        self.net = Some(net);
        self
    }

    pub fn pins(&self) -> &[PinnedHost] {
        &self.pins
    }

    pub fn has_credential(&self) -> bool {
        self.authorization.is_some()
    }

    /// Replaces every secret of the connection in `text`.
    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        for s in &self.secrets {
            if !s.is_empty() && out.contains(s.as_str()) {
                out = out.replace(s.as_str(), REDACTED);
            }
        }
        out
    }

    fn log(&self, method: &Method, url: &Url, status: &str, started: Instant) {
        if let Some(net) = &self.net {
            net.record(&format!(
                "{} {status} {}ms",
                tandem_core::net::describe(method.as_str(), url),
                started.elapsed().as_millis()
            ));
        }
    }

    /// Sends a request; see the module docs for what is attached where.
    pub async fn send(&self, req: Outgoing) -> Result<Incoming, VendorError> {
        let mut method = req.method;
        let mut url = req.url;
        let mut body = req.body;
        let mut headers = req.headers;
        // The caller never sets the credential itself.
        headers.remove(AUTHORIZATION);
        for hop in 0..=MAX_REDIRECTS {
            let pinned = any_matches(&self.pins, &url);
            if hop > 0
                && !pinned
                && !any_matches(&self.reachable, &url)
                && is_private_url(&url).await
            {
                return Err(VendorError::Unavailable(format!(
                    "Refused a redirect to a private address ({})",
                    url.host_str().unwrap_or_default()
                )));
            }
            let mut builder = self
                .client
                .request(method.clone(), url.clone())
                .headers(headers.clone());
            if pinned {
                if let Some(auth) = &self.authorization {
                    builder = builder.header(AUTHORIZATION, auth.as_str());
                }
            }
            if let Some(b) = &body {
                builder = builder.body(b.clone());
            }
            let started = Instant::now();
            let resp = match builder.send().await {
                Ok(r) => r,
                Err(e) => {
                    self.log(&method, &url, "error", started);
                    let msg = if e.is_timeout() {
                        "The request timed out".to_string()
                    } else {
                        format!("Couldn't reach {}", url.host_str().unwrap_or_default())
                    };
                    return Err(VendorError::Unavailable(msg));
                }
            };
            let status = resp.status();
            self.log(&method, &url, status.as_str(), started);
            if status.is_redirection() {
                let Some(next) = resp
                    .headers()
                    .get(LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|loc| url.join(loc).ok())
                else {
                    return Err(VendorError::Unavailable(
                        "A redirect without a location".into(),
                    ));
                };
                if next.scheme() != "https" && next.scheme() != "http" {
                    return Err(VendorError::Unavailable(
                        "Refused a redirect to another scheme".into(),
                    ));
                }
                if status == StatusCode::SEE_OTHER
                    || ((status == StatusCode::MOVED_PERMANENTLY || status == StatusCode::FOUND)
                        && method == Method::POST)
                {
                    method = Method::GET;
                    body = None;
                    headers.remove(reqwest::header::CONTENT_TYPE);
                }
                url = next;
                continue;
            }
            let resp_headers = resp.headers().clone();
            let bytes = read_capped(resp, MAX_BODY).await?;
            let text = String::from_utf8_lossy(&bytes);
            return Ok(Incoming {
                status,
                headers: resp_headers,
                body: self.redact(&text),
                url,
            });
        }
        Err(VendorError::Unavailable("Too many redirects".into()))
    }
}

/// Reads at most `cap` bytes of a body (the rest is dropped).
pub async fn read_capped(mut resp: reqwest::Response, cap: usize) -> Result<Vec<u8>, VendorError> {
    let mut out = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                let room = cap.saturating_sub(out.len());
                out.extend_from_slice(&chunk[..chunk.len().min(room)]);
                if out.len() >= cap {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => return Err(VendorError::Unavailable("The response was cut off".into())),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn pin_of(server: &MockServer, prefix: &str) -> PinnedHost {
        let u = Url::parse(&server.uri()).expect("uri");
        PinnedHost::new(
            "http",
            u.host_str().expect("host"),
            u.port().expect("port"),
            prefix,
        )
    }

    fn auth_of(r: &Request) -> Option<String> {
        r.headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    }

    #[tokio::test]
    async fn f_pl_04_1_an_echoed_token_is_redacted_before_it_reaches_the_engine() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/me"))
            .respond_with(|r: &Request| {
                let auth = auth_of(r).unwrap_or_default();
                ResponseTemplate::new(200).set_body_string(format!(
                    "{{\"you_sent\":\"{auth}\",\"token\":\"{}\"}}",
                    auth.trim_start_matches("Bearer ")
                ))
            })
            .mount(&server)
            .await;
        let client =
            PinnedClient::with_bearer(vec![pin_of(&server, "/api/")], "tok_live_ABC123xyz");
        let url = Url::parse(&format!("{}/api/me", server.uri())).expect("url");
        let resp = client.send(Outgoing::get(url)).await.expect("send");
        assert!(!resp.body.contains("tok_live_ABC123xyz"), "{}", resp.body);
        assert!(resp.body.contains(REDACTED));
    }

    #[tokio::test]
    async fn f_pl_04_2_a_redirect_to_another_host_is_followed_without_the_authorization_header() {
        let pinned = MockServer::start().await;
        let other = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/start"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/landing", other.uri()).as_str()),
            )
            .mount(&pinned)
            .await;
        Mock::given(method("GET"))
            .and(path("/landing"))
            .respond_with(|r: &Request| {
                ResponseTemplate::new(200).set_body_string(if auth_of(r).is_some() {
                    "LEAKED"
                } else {
                    "clean"
                })
            })
            .mount(&other)
            .await;
        let url = Url::parse(&format!("{}/api/start", pinned.uri())).expect("url");
        // A redirect to a private address that isn't pinned is refused.
        let client = PinnedClient::with_bearer(vec![pin_of(&pinned, "/api/")], "secret-token-1");
        let err = client.send(Outgoing::get(url.clone())).await;
        assert!(matches!(err, Err(VendorError::Unavailable(m)) if m.contains("private")));
        // One it may reach is followed, without the credential.
        let client = PinnedClient::with_bearer(vec![pin_of(&pinned, "/api/")], "secret-token-1")
            .reachable(pin_of(&other, "/"));
        let resp = client.send(Outgoing::get(url)).await.expect("send");
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.body, "clean");
        let first = pinned.received_requests().await.expect("recorded");
        assert!(first
            .iter()
            .all(|r| auth_of(r).as_deref() == Some("Bearer secret-token-1")));
    }

    /// F-TL-06 rule 3: plugin credentials go only to pinned hosts. A pin is
    /// scheme + host + port + path prefix, all exact; every near miss and
    /// every redirect off the pin gets the request without the credential.
    #[tokio::test]
    async fn invariant_rule_3_pinned_hosts_only() {
        let a = MockServer::start().await;
        let b = MockServer::start().await;
        let echo = |r: &Request| {
            ResponseTemplate::new(200).set_body_string(if auth_of(r).is_some() {
                "CRED"
            } else {
                "none"
            })
        };
        Mock::given(method("GET"))
            .and(path("/api/hop"))
            .respond_with(
                ResponseTemplate::new(307)
                    .insert_header("location", format!("{}/api/x", b.uri()).as_str()),
            )
            .mount(&a)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/hop-inside"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/elsewhere"))
            .mount(&a)
            .await;
        Mock::given(method("GET"))
            .respond_with(echo)
            .mount(&a)
            .await;
        Mock::given(method("GET"))
            .respond_with(echo)
            .mount(&b)
            .await;
        let pin = pin_of(&a, "/api/");
        let mut https_pin = pin.clone();
        https_pin.scheme = "https".into();
        let get = |client: PinnedClient, url: String| async move {
            client
                .send(Outgoing::get(Url::parse(&url).expect("url")))
                .await
                .expect("send")
                .body
        };
        let token = "invariant-3-token-xyz";
        let client = || {
            PinnedClient::with_bearer(vec![pin.clone()], token)
                .reachable(pin_of(&a, "/"))
                .reachable(pin_of(&b, "/"))
        };
        // The exact pin gets it.
        assert_eq!(
            get(client(), format!("{}/api/issues", a.uri())).await,
            "CRED"
        );
        // Another path prefix, another port (host:port), a redirect off the
        // pin and a redirect within the host but off the prefix: never.
        assert_eq!(
            get(client(), format!("{}/apix/issues", a.uri())).await,
            "none"
        );
        assert_eq!(get(client(), format!("{}/issues", a.uri())).await, "none");
        assert_eq!(
            get(client(), format!("{}/api/issues", b.uri())).await,
            "none"
        );
        assert_eq!(get(client(), format!("{}/api/hop", a.uri())).await, "none");
        assert_eq!(
            get(client(), format!("{}/api/hop-inside", a.uri())).await,
            "none"
        );
        // A pin with another scheme on the same host and port: never.
        let c = PinnedClient::with_bearer(vec![https_pin], token).reachable(pin_of(&a, "/"));
        assert_eq!(get(c, format!("{}/api/issues", a.uri())).await, "none");
        // A caller can't smuggle its own Authorization header anywhere.
        let c = PinnedClient::anonymous(vec![pin.clone()]).reachable(pin_of(&b, "/"));
        let resp = c
            .send(
                Outgoing::get(Url::parse(&format!("{}/x", b.uri())).expect("url"))
                    .header("authorization", "Bearer smuggled"),
            )
            .await
            .expect("send");
        assert_eq!(resp.body, "none");
        // Over everything the two servers saw: the credential reached only
        // the pinned server, under the pinned prefix.
        for r in a.received_requests().await.expect("a") {
            if auth_of(&r).is_some() {
                assert!(
                    r.url.path().starts_with("/api/"),
                    "credential sent to {}",
                    r.url.path()
                );
            }
        }
        for r in b.received_requests().await.expect("b") {
            assert!(
                auth_of(&r).is_none(),
                "credential sent to the other server: {}",
                r.url.path()
            );
        }
    }
}
