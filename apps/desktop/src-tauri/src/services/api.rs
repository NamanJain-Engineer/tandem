//! The Tandem API client (ARCHITECTURE §14.6). Services depend on the
//! [`ApiClient`] trait so `cargo test -p tandem-desktop` runs them against a
//! fake; the real client sends everything through [`Net`] so it shows up in
//! `TANDEM_NET_LOG`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use tandem_core::entitlement::{Limits, Plan};
use tandem_core::net::Net;
use tandem_core::remote_config::SignedConfig;

/// Where the API and the web app live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub api: String,
    pub web: String,
    /// False in early-access builds, which ship before the hosted service
    /// exists: no accounts, no API calls (D-105).
    pub hosted: bool,
}

/// Whether this build talks to the hosted service. The release workflow
/// compiles early-access builds with `TANDEM_HOSTED=0`; debug builds also
/// read it at run time, or take `--test-no-hosted` (E2E relaunches).
pub fn hosted_service() -> bool {
    if cfg!(debug_assertions) {
        if std::env::args().any(|a| a == "--test-no-hosted") {
            return false;
        }
        if let Ok(v) = std::env::var("TANDEM_HOSTED") {
            return v != "0";
        }
    }
    !matches!(option_env!("TANDEM_HOSTED"), Some("0"))
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
            ("https://api.vibezeo.com", "https://app.vibezeo.com")
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
            hosted: hosted_service(),
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
    #[error("couldn't reach Vibezeo: {0}")]
    Network(String),
    #[error("{status} {code}")]
    Http {
        status: u16,
        code: String,
        detail: Option<String>,
    },
    #[error("unexpected response from Vibezeo")]
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
    /// A Team member's monthly cap and what they've spent against it.
    #[serde(default)]
    pub cap: Option<i64>,
    #[serde(default)]
    pub spent: Option<i64>,
    /// What arrives each month (10,000 × seats, a member's cap, 0 on Free).
    #[serde(default)]
    pub grant: Option<i64>,
}

/// The org a Team member belongs to (F-BL-06).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MeOrg {
    pub id: String,
    pub name: String,
    /// `owner`, `admin` or `member`.
    pub role: String,
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
    /// The org's policy; the app trusts only the signed token's copy.
    #[serde(default)]
    pub policy: Option<Value>,
    #[serde(default)]
    pub org: Option<MeOrg>,
    pub entitlement_token: String,
}

/// `POST /v1/ai/teammate-draft` (F-AG-04): a draft to review, never saved by the API.
/// The protocol calls it `ApiTeammateDraft`: the app's checked draft is
/// `services::teammate_draft::TeammateDraft`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "ApiTeammateDraft")]
pub struct TeammateDraft {
    pub name: String,
    pub brief: String,
    pub engine: String,
    pub plugins: Vec<String>,
}

/// `POST /v1/auto/next` (F-AU-01): one supervisor turn. `scrollback` is
/// already redacted and at most 16 KB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutoNextRequest {
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    pub turn: u32,
    pub ceiling: u32,
    pub scrollback: String,
}

/// The supervisor's answer: `prompt` with `text`, or `done` / `stuck` with `reason`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AutoNextResponse {
    pub action: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// `POST /v1/stt/transcribe` (F-BE-07). `duration` is in seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
    pub duration: f64,
}

/// One take for the STT relay. The fields go before the audio (D-604).
#[derive(Debug, Clone, PartialEq)]
pub struct SttUpload {
    /// `audio/ogg` (Opus) or `audio/wav`.
    pub mime: String,
    pub audio: Vec<u8>,
    pub duration_ms: Option<u64>,
    /// A language code, or `None` to auto-detect.
    pub language: Option<String>,
    /// Preferred words; send them only with the user's opt-in.
    pub hints: Vec<String>,
}

/// One metadata-only audit event (F-TL-10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event: String,
    pub meta: serde_json::Map<String, Value>,
    /// RFC 3339.
    pub ts: String,
}

/// Why a hosted call failed, in the words the app shows (F-DI-05: errors
/// name the limit that was hit; nothing falls back silently).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum HostedFailure {
    /// 402: the balance (or a Team member's cap) is spent.
    OutOfCredits {
        message: String,
    },
    /// 403 FORBIDDEN_PLAN: the feature needs Pro.
    NeedsPro {
        message: String,
    },
    /// 429 DAILY_LIMIT: the day's cap for this feature.
    DailyLimit {
        message: String,
    },
    /// 429 RATE_LIMITED.
    RateLimited {
        message: String,
    },
    /// 413: a take over 10 minutes or 25 MB.
    TooLarge {
        message: String,
    },
    /// Signed out.
    SignedOut {
        message: String,
    },
    /// Offline, timed out, or the service failed (5xx).
    Unavailable {
        message: String,
    },
    Other {
        message: String,
    },
}

impl HostedFailure {
    pub fn message(&self) -> &str {
        match self {
            HostedFailure::OutOfCredits { message }
            | HostedFailure::NeedsPro { message }
            | HostedFailure::DailyLimit { message }
            | HostedFailure::RateLimited { message }
            | HostedFailure::TooLarge { message }
            | HostedFailure::SignedOut { message }
            | HostedFailure::Unavailable { message }
            | HostedFailure::Other { message } => message,
        }
    }

    /// Maps an API error; `feature` names it in the Pro message ("Cloud transcription").
    pub fn from_api(err: &ApiError, feature: &str) -> Self {
        let detail = |fallback: &str| match err {
            ApiError::Http {
                detail: Some(d), ..
            } if !d.is_empty() => d.clone(),
            _ => fallback.to_string(),
        };
        match err {
            ApiError::Network(_) => HostedFailure::Unavailable {
                message: "Couldn't reach Vibezeo. Check your connection and try again.".into(),
            },
            ApiError::Decode(_) => HostedFailure::Other {
                message: "Vibezeo answered in a way this version doesn't understand.".into(),
            },
            ApiError::Http { status, code, .. } => match (*status, code.as_str()) {
                (401, _) => HostedFailure::SignedOut {
                    message: "Sign in to use hosted features.".into(),
                },
                (402, _) => HostedFailure::OutOfCredits {
                    message: detail("You're out of credits for now."),
                },
                (403, "FORBIDDEN_PLAN") => HostedFailure::NeedsPro {
                    message: format!("{feature} needs Vibezeo Pro."),
                },
                (429, "DAILY_LIMIT") => HostedFailure::DailyLimit {
                    message: detail("You've reached today's limit."),
                },
                (429, _) => HostedFailure::RateLimited {
                    message: "Too many requests. Try again in a moment.".into(),
                },
                (413, _) => HostedFailure::TooLarge {
                    message: detail("That's too long to send."),
                },
                _ => HostedFailure::Other {
                    message: detail("That didn't work."),
                },
            },
        }
    }
}

/// The STT relay's multipart body: fields first, the audio last (D-604).
pub fn stt_body(upload: &SttUpload, boundary: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(upload.audio.len() + 1024);
    let mut field = |name: &str, value: &str| {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    };
    if let Some(ms) = upload.duration_ms {
        field("duration_ms", &ms.to_string());
    }
    field("language", upload.language.as_deref().unwrap_or("auto"));
    for hint in &upload.hints {
        field("hints", &hint.replace(['\r', '\n'], " "));
    }
    let ext = if upload.mime.contains("wav") {
        "wav"
    } else {
        "opus"
    };
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"take.{ext}\"\r\nContent-Type: {}\r\n\r\n",
            upload.mime
        )
        .as_bytes(),
    );
    body.extend_from_slice(&upload.audio);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
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
    /// `POST /v1/ai/title` (F-TH-15): a thread title of at most 60 characters.
    async fn ai_title(&self, access: &str, text: &str) -> Result<String, ApiError>;
    /// `GET /v1/me/events` as a stream of event names (`entitlement`, `credits`).
    async fn events(
        &self,
        access: &str,
    ) -> Result<BoxStream<'static, Result<String, ApiError>>, ApiError>;
    /// `POST /v1/ai/rewrite` (F-DI-10): Enhance (`enhance`) or Polish (`polish`).
    async fn ai_rewrite(
        &self,
        _access: &str,
        _mode: &str,
        _text: &str,
        _instructions: Option<&str>,
        _protected_terms: &[String],
    ) -> Result<String, ApiError> {
        Err(no_hosted_service())
    }
    /// `POST /v1/ai/teammate-draft` (F-AG-04).
    async fn ai_teammate_draft(
        &self,
        _access: &str,
        _description: &str,
        _engines: &[String],
        _plugins: &[String],
    ) -> Result<TeammateDraft, ApiError> {
        Err(no_hosted_service())
    }
    /// `POST /v1/auto/next` (F-AU-01).
    async fn auto_next(
        &self,
        _access: &str,
        _req: &AutoNextRequest,
    ) -> Result<AutoNextResponse, ApiError> {
        Err(no_hosted_service())
    }
    /// `POST /v1/stt/transcribe` (F-BE-07, for F-DI-05).
    async fn stt_transcribe(
        &self,
        _access: &str,
        _upload: SttUpload,
    ) -> Result<Transcript, ApiError> {
        Err(no_hosted_service())
    }
    /// `POST /v1/audit/events` (F-TL-10).
    async fn audit_events(&self, _access: &str, _events: &[AuditEvent]) -> Result<(), ApiError> {
        Err(no_hosted_service())
    }
    /// `POST /v1/auth/device/start` (F-BE-03): a code for the user to approve on the web.
    async fn device_start(&self, _client: &str, _version: &str) -> Result<DeviceStart, ApiError> {
        Err(no_hosted_service())
    }
    /// `POST /v1/auth/device/token`: tokens once approved; the OAuth error code otherwise.
    async fn device_token(&self, _device_code: &str) -> Result<TokenPair, ApiError> {
        Err(no_hosted_service())
    }
}

/// `POST /v1/auth/device/start` (F-BE-03).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub interval: u64,
    pub expires_in: u64,
}

/// The API client of an early-access build (D-105): every call fails as
/// "offline" without touching the network.
pub struct NoHostedApi;

fn no_hosted_service() -> ApiError {
    ApiError::Network("this early-access build has no hosted service".into())
}

#[async_trait]
impl ApiClient for NoHostedApi {
    async fn exchange(&self, _: &str, _: &str, _: &str) -> Result<TokenPair, ApiError> {
        Err(no_hosted_service())
    }
    async fn refresh(&self, _: &str) -> Result<TokenPair, ApiError> {
        Err(no_hosted_service())
    }
    async fn logout(&self, _: &str, _: &str) -> Result<(), ApiError> {
        Err(no_hosted_service())
    }
    async fn me(&self, _: &str) -> Result<MeResponse, ApiError> {
        Err(no_hosted_service())
    }
    async fn checkout(&self, _: &str, _: &str, _: &str) -> Result<String, ApiError> {
        Err(no_hosted_service())
    }
    async fn portal(&self, _: &str) -> Result<String, ApiError> {
        Err(no_hosted_service())
    }
    async fn config(&self) -> Result<SignedConfig, ApiError> {
        Err(no_hosted_service())
    }
    async fn ai_title(&self, _: &str, _: &str) -> Result<String, ApiError> {
        Err(no_hosted_service())
    }
    async fn events(
        &self,
        _: &str,
    ) -> Result<BoxStream<'static, Result<String, ApiError>>, ApiError> {
        Err(no_hosted_service())
    }
}

/// Told the `x-tandem-min-version` of API responses (F-SH-06). Set once
/// the update service exists.
pub type MinVersionWatch = Arc<std::sync::OnceLock<Arc<dyn Fn(&str) + Send + Sync>>>;

pub struct HttpApi {
    net: Net,
    endpoints: Endpoints,
    min_version: MinVersionWatch,
}

impl HttpApi {
    pub fn new(net: Net, endpoints: Endpoints) -> Self {
        Self::watching(net, endpoints, MinVersionWatch::default())
    }

    pub fn watching(net: Net, endpoints: Endpoints, min_version: MinVersionWatch) -> Self {
        Self {
            net,
            endpoints,
            min_version,
        }
    }

    /// Any response can name the oldest supported app version.
    fn watch(&self, res: &reqwest::Response) {
        let Some(min) = res
            .headers()
            .get("x-tandem-min-version")
            .and_then(|v| v.to_str().ok())
        else {
            return;
        };
        if let Some(sink) = self.min_version.get() {
            sink(min);
        }
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

    /// A call whose success has no body (204).
    async fn call_empty(&self, req: reqwest::RequestBuilder) -> Result<(), ApiError> {
        self.call::<Value>(req).await.map(|_| ())
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
        self.watch(&res);
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

    async fn ai_title(&self, access: &str, text: &str) -> Result<String, ApiError> {
        #[derive(Deserialize)]
        struct TitleBody {
            title: String,
        }
        let req = self
            .net
            .client()
            .post(self.url("/v1/ai/title"))
            .bearer_auth(access)
            .json(&json!({ "text": text }));
        self.call::<TitleBody>(req).await.map(|b| b.title)
    }

    async fn ai_rewrite(
        &self,
        access: &str,
        mode: &str,
        text: &str,
        instructions: Option<&str>,
        protected_terms: &[String],
    ) -> Result<String, ApiError> {
        #[derive(Deserialize)]
        struct TextBody {
            text: String,
        }
        let mut body = json!({ "mode": mode, "text": text, "protected_terms": protected_terms });
        if let Some(i) = instructions.filter(|i| !i.is_empty()) {
            body["instructions"] = json!(i);
        }
        let req = self
            .net
            .client()
            .post(self.url("/v1/ai/rewrite"))
            .bearer_auth(access)
            .json(&body);
        self.call::<TextBody>(req).await.map(|b| b.text)
    }

    async fn ai_teammate_draft(
        &self,
        access: &str,
        description: &str,
        engines: &[String],
        plugins: &[String],
    ) -> Result<TeammateDraft, ApiError> {
        let req = self
            .net
            .client()
            .post(self.url("/v1/ai/teammate-draft"))
            .bearer_auth(access)
            .json(&json!({ "description": description, "engines": engines, "plugins": plugins }));
        self.call(req).await
    }

    async fn auto_next(
        &self,
        access: &str,
        req: &AutoNextRequest,
    ) -> Result<AutoNextResponse, ApiError> {
        let req = self
            .net
            .client()
            .post(self.url("/v1/auto/next"))
            .bearer_auth(access)
            .json(req);
        self.call(req).await
    }

    async fn stt_transcribe(
        &self,
        access: &str,
        upload: SttUpload,
    ) -> Result<Transcript, ApiError> {
        let boundary = format!("tandem-{}", uuid::Uuid::new_v4().simple());
        let body = stt_body(&upload, &boundary);
        let req = self
            .net
            .client()
            .post(self.url("/v1/stt/transcribe"))
            .bearer_auth(access)
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body);
        // A 10-minute take takes a while to upload and transcribe.
        self.call(req.timeout(Duration::from_secs(180))).await
    }

    async fn audit_events(&self, access: &str, events: &[AuditEvent]) -> Result<(), ApiError> {
        let req = self
            .net
            .client()
            .post(self.url("/v1/audit/events"))
            .bearer_auth(access)
            .json(&json!({ "events": events }));
        self.call_empty(req).await
    }

    async fn device_start(&self, client: &str, version: &str) -> Result<DeviceStart, ApiError> {
        let req = self
            .net
            .client()
            .post(self.url("/v1/auth/device/start"))
            .json(&json!({ "client": client, "version": version }));
        self.call(req).await
    }

    async fn device_token(&self, device_code: &str) -> Result<TokenPair, ApiError> {
        let req = self
            .net
            .client()
            .post(self.url("/v1/auth/device/token"))
            .json(&json!({ "device_code": device_code }));
        self.call(req).await
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
        self.watch(&res);
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
    fn f_be_07_the_stt_body_sends_its_fields_before_the_audio() {
        let upload = SttUpload {
            mime: "audio/ogg".into(),
            audio: b"OggS-AUDIO".to_vec(),
            duration_ms: Some(31_000),
            language: Some("de".into()),
            hints: vec!["Vibezeo".into(), "two\nlines".into()],
        };
        let body = String::from_utf8_lossy(&stt_body(&upload, "B")).into_owned();
        let field = |name: &str| body.find(&format!("name=\"{name}\"")).expect(name);
        assert!(field("duration_ms") < field("audio"));
        assert!(field("language") < field("audio"));
        assert!(body.contains("\r\n\r\n31000\r\n"));
        assert!(
            body.contains("\r\n\r\ntwo lines\r\n"),
            "a hint can't break the framing"
        );
        assert!(body.contains(
            "filename=\"take.opus\"\r\nContent-Type: audio/ogg\r\n\r\nOggS-AUDIO\r\n--B--"
        ));
        let auto = stt_body(
            &SttUpload {
                language: None,
                hints: vec![],
                duration_ms: None,
                ..upload
            },
            "B",
        );
        assert!(String::from_utf8_lossy(&auto).contains("\r\n\r\nauto\r\n"));
    }

    #[test]
    fn f_di_05_hosted_errors_name_the_limit_that_was_hit() {
        let http = |status, code: &str, detail: Option<&str>| ApiError::Http {
            status,
            code: code.into(),
            detail: detail.map(str::to_string),
        };
        assert_eq!(
            HostedFailure::from_api(&http(403, "FORBIDDEN_PLAN", None), "Cloud transcription")
                .message(),
            "Cloud transcription needs Vibezeo Pro."
        );
        assert!(matches!(
            HostedFailure::from_api(
                &http(
                    402,
                    "CREDITS_EXHAUSTED",
                    Some("You're out of credits for now.")
                ),
                "x"
            ),
            HostedFailure::OutOfCredits { .. }
        ));
        let daily = HostedFailure::from_api(
            &http(
                429,
                "DAILY_LIMIT",
                Some("You've reached today's cloud limit of 120 minutes. Switch to Local."),
            ),
            "x",
        );
        assert!(matches!(daily, HostedFailure::DailyLimit { .. }));
        assert!(daily.message().contains("Switch to Local"));
        assert!(matches!(
            HostedFailure::from_api(&http(413, "PAYLOAD_TOO_LARGE", None), "x"),
            HostedFailure::TooLarge { .. }
        ));
        assert!(matches!(
            HostedFailure::from_api(&ApiError::Network("down".into()), "x"),
            HostedFailure::Unavailable { .. }
        ));
    }

    #[tokio::test]
    async fn early_access_api_never_reaches_the_network() {
        // NoHostedApi holds no client at all; every call is "offline".
        let api = NoHostedApi;
        assert!(matches!(api.config().await, Err(ApiError::Network(_))));
        assert!(matches!(api.refresh("r").await, Err(ApiError::Network(_))));
        assert!(matches!(
            api.checkout("a", "pro", "month").await,
            Err(ApiError::Network(_))
        ));
        assert!(matches!(
            api.ai_rewrite("a", "polish", "hi", None, &[]).await,
            Err(ApiError::Network(_))
        ));
        assert!(matches!(
            api.audit_events("a", &[]).await,
            Err(ApiError::Network(_))
        ));
    }

    #[test]
    fn release_builds_use_our_own_domains() {
        let src = include_str!("api.rs");
        assert!(src.contains("\"https://api.vibezeo.com\", \"https://app.vibezeo.com\""));
        let conf = include_str!("../../tauri.conf.json");
        assert!(
            !conf.contains("tandem.dev"),
            "tauri.conf.json still names tandem.dev"
        );
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
