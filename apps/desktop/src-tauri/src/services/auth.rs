//! Desktop sign-in (F-BE-02, ARCHITECTURE §14.3). PKCE through the system
//! browser, with the callback arriving on an ephemeral loopback port or the
//! `tandem://auth/callback` deep link. Tokens are stored AES-256-GCM encrypted
//! in `$DATA/auth/credentials.enc` with the key in the keychain. Refresh runs
//! at 80% of the access token's lifetime, after the machine wakes, and on a
//! 401; network errors never sign the user out.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Notify};
use ts_rs::TS;
use zeroize::{Zeroize, Zeroizing};

use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::clock::Clock;
use tandem_core::ids::OpId;
use tandem_core::keychain::{self, Keychain};
use tandem_core::{DataDir, ErrorCode, IpcError};

use super::api::{platform, ApiClient, ApiError, Endpoints, TokenPair};
use super::opener::UrlOpener;

pub const KEY_ACCOUNT: &str = "credentials-key";
pub const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Offline mode works this long before sign-in is needed (UI-133).
pub const OFFLINE_DAYS: i64 = 14;
const AAD: &[u8] = b"tandem-credentials-v1";
const FILE_VERSION: u8 = 1;

#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub access: String,
    pub refresh: String,
    /// Unix ms.
    pub issued_at: i64,
    pub expires_at: i64,
    pub email: Option<String>,
}

impl Drop for Credentials {
    fn drop(&mut self) {
        self.access.zeroize();
        self.refresh.zeroize();
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// `auth/credentials.enc`: `version ‖ nonce(12) ‖ AES-256-GCM(json)`.
pub struct CredentialStore {
    path: PathBuf,
    keychain: Arc<dyn Keychain>,
    /// Used when the OS keyring is missing: sign-in lasts for this session only.
    session_key: Mutex<Option<Zeroizing<[u8; 32]>>>,
}

impl CredentialStore {
    pub fn new(data: &DataDir, keychain: Arc<dyn Keychain>) -> Self {
        Self {
            path: data.auth_credentials(),
            keychain,
            session_key: Mutex::new(None),
        }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn key(&self, create: bool) -> Option<Zeroizing<[u8; 32]>> {
        let service = keychain::service("auth");
        match self.keychain.get(&service, KEY_ACCOUNT) {
            Ok(Some(raw)) if raw.len() == 32 => {
                let mut key = Zeroizing::new([0u8; 32]);
                key.copy_from_slice(&raw);
                return Some(key);
            }
            Ok(_) if create => {
                let mut key = Zeroizing::new([0u8; 32]);
                rand::rngs::OsRng.fill_bytes(key.as_mut());
                if self
                    .keychain
                    .set(&service, KEY_ACCOUNT, key.as_ref())
                    .is_ok()
                {
                    return Some(key);
                }
            }
            Ok(_) => return None,
            Err(err) => {
                tracing::warn!(error = %err, "keychain unavailable; sign-in lasts for this session")
            }
        }
        let mut session = self.session_key.lock().ok()?;
        if session.is_none() && create {
            let mut key = Zeroizing::new([0u8; 32]);
            rand::rngs::OsRng.fill_bytes(key.as_mut());
            *session = Some(key);
        }
        session.clone()
    }

    pub fn save(&self, creds: &Credentials) -> Result<(), String> {
        let key = self.key(true).ok_or("no key")?;
        let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|e| e.to_string())?;
        let mut nonce = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let plain = Zeroizing::new(serde_json::to_vec(creds).map_err(|e| e.to_string())?);
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: AAD,
                },
            )
            .map_err(|_| "encrypt failed".to_string())?;
        let mut file = Vec::with_capacity(1 + 12 + sealed.len());
        file.push(FILE_VERSION);
        file.extend_from_slice(&nonce);
        file.extend_from_slice(&sealed);
        if let Some(dir) = self.path.parent() {
            tandem_core::paths::create_private_dir(dir).map_err(|e| e.to_string())?;
        }
        tandem_core::atomic::atomic_write(&self.path, &file, Some(0o600)).map_err(|e| e.to_string())
    }

    pub fn load(&self) -> Option<Credentials> {
        let file = std::fs::read(&self.path).ok()?;
        let result = (|| {
            if file.len() < 1 + 12 + 16 || file[0] != FILE_VERSION {
                return None;
            }
            let key = self.key(false)?;
            let cipher = Aes256Gcm::new_from_slice(key.as_ref()).ok()?;
            let plain = Zeroizing::new(
                cipher
                    .decrypt(
                        Nonce::from_slice(&file[1..13]),
                        Payload {
                            msg: &file[13..],
                            aad: AAD,
                        },
                    )
                    .ok()?,
            );
            serde_json::from_slice::<Credentials>(&plain).ok()
        })();
        if result.is_none() {
            tracing::warn!(
                code = "auth.credentials_unreadable",
                "stored sign-in can't be read; signing out"
            );
            let _ = std::fs::remove_file(&self.path);
        }
        result
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// PKCE material for one attempt.
pub struct Pkce {
    pub state: String,
    pub verifier: Zeroizing<String>,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Self {
        let mut state = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut state);
        // 48 random bytes → 64 base64url chars (RFC 7636 unreserved set).
        let mut v = [0u8; 48];
        rand::rngs::OsRng.fill_bytes(&mut v);
        let verifier = Zeroizing::new(URL_SAFE_NO_PAD.encode(v));
        v.zeroize();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            state: URL_SAFE_NO_PAD.encode(state),
            verifier,
            challenge,
        }
    }
}

/// sha256(machine id ‖ install id), hex (64 chars).
pub fn device_fingerprint(data: &DataDir) -> String {
    let install_path = data.root().join("install-id");
    let install = std::fs::read_to_string(&install_path)
        .ok()
        .filter(|s| s.trim().len() >= 16)
        .unwrap_or_else(|| {
            let id = uuid::Uuid::new_v4().to_string();
            let _ = tandem_core::atomic::atomic_write(&install_path, id.as_bytes(), Some(0o600));
            id
        });
    let mut h = Sha256::new();
    h.update(machine_id().as_bytes());
    h.update(b"\x00");
    h.update(install.trim().as_bytes());
    hex::encode(h.finalize())
}

fn machine_id() -> String {
    for p in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(s) = std::fs::read_to_string(p) {
            if !s.trim().is_empty() {
                return s.trim().to_string();
            }
        }
    }
    device_name()
}

pub fn device_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "This computer".into())
        .chars()
        .take(100)
        .collect()
}

pub fn deep_link_scheme() -> &'static str {
    if cfg!(debug_assertions) {
        "tandem-dev"
    } else {
        "tandem"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AuthFailureKind {
    /// The browser didn't open.
    Browser,
    /// The callback never arrived (blocked, or the attempt timed out).
    Callback,
    /// The code expired or was rejected.
    Expired,
    Network,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AuthFailure {
    pub kind: AuthFailureKind,
    pub message: String,
    /// For "Copy details": category, stage and error code; never tokens.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PendingSignIn {
    pub op_id: String,
    /// The browser URL, for "Didn't open? Copy link".
    pub url: String,
}

/// `auth_state` and the `auth:changed` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AuthState {
    pub signed_in: bool,
    pub email: Option<String>,
    /// Using Tandem without an account (UI-03 offline path).
    pub offline: bool,
    /// `None` when there's no sign-in deadline (early-access builds).
    pub offline_days_left: Option<i64>,
    /// False in early-access builds: no accounts yet, the Free plan runs
    /// locally with no deadline (D-105).
    pub hosted: bool,
    pub pending: Option<PendingSignIn>,
    pub error: Option<AuthFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callback {
    pub code: String,
    pub state: String,
}

struct Attempt {
    op_id: String,
    url: String,
    tx: mpsc::UnboundedSender<Callback>,
    cancel: Arc<Notify>,
}

#[derive(Default)]
struct Inner {
    creds: Option<Credentials>,
    attempt: Option<Attempt>,
    error: Option<AuthFailure>,
    offline_started_at: Option<i64>,
}

pub struct AuthService {
    data: DataDir,
    creds: CredentialStore,
    api: Arc<dyn ApiClient>,
    opener: Arc<dyn UrlOpener>,
    endpoints: Endpoints,
    clock: Arc<dyn Clock>,
    bus: EventBus,
    inner: Mutex<Inner>,
    refresh_gate: tokio::sync::Mutex<()>,
    wake: Notify,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("Sign in to use this")]
    Unauthenticated,
    #[error("Couldn't reach Vibezeo. Check your connection.")]
    Network,
}

impl From<AuthError> for IpcError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::Unauthenticated => {
                IpcError::new(ErrorCode::Unauthenticated, e.to_string()).with_stage("auth")
            }
            AuthError::Network => {
                IpcError::new(ErrorCode::Unavailable, e.to_string()).with_stage("network")
            }
        }
    }
}

fn failure(kind: AuthFailureKind, err: Option<&ApiError>) -> AuthFailure {
    let message = match kind {
        AuthFailureKind::Browser => {
            "Couldn't open your browser. Copy the link and open it yourself."
        }
        AuthFailureKind::Callback => "Sign-in timed out. Try again.",
        AuthFailureKind::Expired => "That link has expired.",
        AuthFailureKind::Network => {
            "Couldn't reach Vibezeo. Check your connection or continue offline."
        }
    };
    let code = match err {
        Some(ApiError::Http { status, code, .. }) => format!("{status} {code}"),
        Some(ApiError::Network(_)) => "network".into(),
        Some(ApiError::Decode(_)) => "decode".into(),
        None => "-".into(),
    };
    AuthFailure {
        kind,
        message: message.into(),
        detail: format!(
            "Sign-in failed\ncategory: {kind:?}\nerror: {code}\napp: {} ({})\nplatform: {}",
            env!("CARGO_PKG_VERSION"),
            super::boot::build_id(),
            platform()
        ),
    }
}

impl AuthService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        data: DataDir,
        keychain: Arc<dyn Keychain>,
        api: Arc<dyn ApiClient>,
        opener: Arc<dyn UrlOpener>,
        endpoints: Endpoints,
        clock: Arc<dyn Clock>,
        bus: EventBus,
        offline_started_at: Option<i64>,
    ) -> Arc<Self> {
        let creds = CredentialStore::new(&data, keychain);
        let stored = creds.load();
        Arc::new(Self {
            data,
            creds,
            api,
            opener,
            endpoints,
            clock,
            bus,
            inner: Mutex::new(Inner {
                creds: stored,
                offline_started_at,
                ..Default::default()
            }),
            refresh_gate: tokio::sync::Mutex::new(()),
            wake: Notify::new(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn state(&self) -> AuthState {
        let inner = self.lock();
        let now = self.clock.now_ms();
        let offline = inner.creds.is_none() && inner.offline_started_at.is_some();
        AuthState {
            signed_in: inner.creds.is_some(),
            email: inner.creds.as_ref().and_then(|c| c.email.clone()),
            offline,
            offline_days_left: inner
                .offline_started_at
                .filter(|_| offline && self.endpoints.hosted)
                .map(|start| (OFFLINE_DAYS - (now - start) / 86_400_000).max(0)),
            hosted: self.endpoints.hosted,
            pending: inner.attempt.as_ref().map(|a| PendingSignIn {
                op_id: a.op_id.clone(),
                url: a.url.clone(),
            }),
            error: inner.error.clone(),
        }
    }

    pub fn is_signed_in(&self) -> bool {
        self.lock().creds.is_some()
    }

    fn emit(&self) {
        self.bus
            .publish(BusEvent::new(Topic::App, "auth:changed", self.state()));
    }

    /// Starts an attempt (replacing any current one) and returns its op id.
    /// The result arrives as `auth:changed`.
    pub async fn sign_in(self: &Arc<Self>) -> Result<String, IpcError> {
        if !self.endpoints.hosted {
            return Err(IpcError::new(
                ErrorCode::Unavailable,
                "Vibezeo accounts aren't available in this build. It runs the Free plan without one.",
            ));
        }
        self.cancel_attempt();
        let pkce = Pkce::generate();
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| {
            IpcError::new(
                ErrorCode::Unavailable,
                format!("couldn't listen for the sign-in callback: {e}"),
            )
        })?;
        let port = listener
            .local_addr()
            .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()))?
            .port();
        let redirect = format!("http://127.0.0.1:{port}/callback");
        let fingerprint = device_fingerprint(&self.data);
        let mut url = url::Url::parse(&format!("{}/desktop-login", self.endpoints.web))
            .map_err(|e| IpcError::new(ErrorCode::Invalid, e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("challenge", &pkce.challenge)
            .append_pair("state", &pkce.state)
            .append_pair("redirect_uri", &redirect)
            .append_pair("df", &fingerprint)
            .append_pair("device_name", &device_name())
            .append_pair("platform", platform());
        let url = url.to_string();
        let op_id = OpId::new().to_string();
        let (tx, rx) = mpsc::unbounded_channel();
        let cancel = Arc::new(Notify::new());
        tokio::spawn(serve_loopback(listener, tx.clone(), cancel.clone()));
        {
            let mut inner = self.lock();
            inner.error = None;
            inner.attempt = Some(Attempt {
                op_id: op_id.clone(),
                url: url.clone(),
                tx,
                cancel: cancel.clone(),
            });
        }
        tracing::info!(code = "auth.sign_in_started", port, "sign-in started");
        if let Err(err) = self.opener.open(&url) {
            tracing::warn!(code = "auth.browser_failed", error = %err, "couldn't open the browser");
            // Keep listening: the user can copy the link.
            self.lock().error = Some(failure(AuthFailureKind::Browser, None));
        }
        self.emit();
        let me = self.clone();
        let verifier = pkce.verifier;
        let state = pkce.state;
        let op = op_id.clone();
        tokio::spawn(async move { me.drive(op, rx, cancel, state, verifier, fingerprint).await });
        Ok(op_id)
    }

    async fn drive(
        self: Arc<Self>,
        op_id: String,
        mut rx: mpsc::UnboundedReceiver<Callback>,
        cancel: Arc<Notify>,
        state: String,
        verifier: Zeroizing<String>,
        fingerprint: String,
    ) {
        let timeout = self.clock.sleep(ATTEMPT_TIMEOUT);
        tokio::pin!(timeout);
        let outcome = loop {
            tokio::select! {
                cb = rx.recv() => {
                    let Some(cb) = cb else { break None };
                    if !constant_time_eq(cb.state.as_bytes(), state.as_bytes()) {
                        // F-BE-02 AC4: ignore it and keep listening.
                        tracing::warn!(code = "auth.state_mismatch", "ignored a sign-in callback for another attempt");
                        continue;
                    }
                    break Some(self.api.exchange(&cb.code, &verifier, &fingerprint).await);
                }
                _ = cancel.notified() => return,
                _ = &mut timeout => {
                    break Some(Err(ApiError::Http { status: 0, code: "timeout".into(), detail: None }));
                }
            }
        };
        cancel.notify_waiters();
        let still_current = self
            .lock()
            .attempt
            .as_ref()
            .map(|a| a.op_id == op_id)
            .unwrap_or(false);
        if !still_current {
            return;
        }
        let error = match outcome {
            Some(Ok(tokens)) => {
                self.store_tokens(tokens, None);
                tracing::info!(code = "auth.signed_in", "signed in");
                None
            }
            Some(Err(ApiError::Http { status: 0, .. })) | None => {
                Some(failure(AuthFailureKind::Callback, None))
            }
            Some(Err(e)) if e.is_network() => Some(failure(AuthFailureKind::Network, Some(&e))),
            Some(Err(e)) => Some(failure(AuthFailureKind::Expired, Some(&e))),
        };
        {
            let mut inner = self.lock();
            inner.attempt = None;
            if error.is_none() {
                inner.offline_started_at = None;
            }
            inner.error = error;
        }
        self.emit();
    }

    /// A callback from the deep link (`tandem://auth/callback?code&state`).
    pub fn handle_callback(&self, cb: Callback) {
        match self.lock().attempt.as_ref() {
            Some(a) => {
                let _ = a.tx.send(cb);
            }
            None => tracing::warn!(
                code = "auth.callback_without_attempt",
                "ignored a sign-in callback"
            ),
        }
    }

    pub fn cancel_attempt(&self) {
        let attempt = self.lock().attempt.take();
        if let Some(a) = attempt {
            a.cancel.notify_waiters();
            self.emit();
        }
    }

    fn store_tokens(&self, tokens: TokenPair, email: Option<String>) {
        let now = self.clock.now_ms();
        let mut inner = self.lock();
        let email = email.or_else(|| inner.creds.as_ref().and_then(|c| c.email.clone()));
        let creds = Credentials {
            access: tokens.access_token.clone(),
            refresh: tokens.refresh_token.clone(),
            issued_at: now,
            expires_at: now + tokens.expires_in.max(60) * 1000,
            email,
        };
        if let Err(err) = self.creds.save(&creds) {
            tracing::warn!(error = %err, "couldn't store the sign-in; it lasts for this session");
        }
        inner.creds = Some(creds);
        drop(inner);
        self.wake.notify_waiters();
    }

    /// Called by the entitlement service once `/me` names the account.
    pub fn set_email(&self, email: &str) {
        let changed = {
            let mut inner = self.lock();
            match inner.creds.as_mut() {
                Some(c) if c.email.as_deref() != Some(email) => {
                    c.email = Some(email.to_string());
                    let _ = self.creds.save(c);
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.emit();
        }
    }

    /// A valid access token, refreshing first if it's about to expire.
    pub async fn access_token(&self) -> Result<String, AuthError> {
        let _gate = self.refresh_gate.lock().await;
        let creds = self
            .lock()
            .creds
            .clone()
            .ok_or(AuthError::Unauthenticated)?;
        if self.clock.now_ms() < creds.expires_at - 60_000 {
            return Ok(creds.access.clone());
        }
        self.refresh_locked(&creds).await
    }

    /// Refreshes after `rejected` got a 401. Single-flight: if another caller
    /// already refreshed, the newer token is returned without another call.
    pub async fn refresh_after_401(&self, rejected: &str) -> Result<String, AuthError> {
        let _gate = self.refresh_gate.lock().await;
        let creds = self
            .lock()
            .creds
            .clone()
            .ok_or(AuthError::Unauthenticated)?;
        if creds.access != rejected {
            return Ok(creds.access.clone());
        }
        self.refresh_locked(&creds).await
    }

    /// Forces the refresh path (`test_auth_refresh`, wake from sleep).
    pub async fn force_refresh(&self) -> Result<String, AuthError> {
        let _gate = self.refresh_gate.lock().await;
        let creds = self
            .lock()
            .creds
            .clone()
            .ok_or(AuthError::Unauthenticated)?;
        self.refresh_locked(&creds).await
    }

    async fn refresh_locked(&self, creds: &Credentials) -> Result<String, AuthError> {
        match self.api.refresh(&creds.refresh).await {
            Ok(tokens) => {
                let access = tokens.access_token.clone();
                self.store_tokens(tokens, creds.email.clone());
                tracing::info!(code = "auth.refreshed", "access token refreshed");
                Ok(access)
            }
            Err(e) if e.is_network() => {
                tracing::warn!(code = "auth.refresh_network", "refresh failed: offline");
                Err(AuthError::Network)
            }
            Err(e) => {
                // Revoked device or reused token (F-WB-05 AC1, F-BE-02 AC3).
                tracing::warn!(
                    code = "auth.session_ended",
                    status = e.status().unwrap_or(0),
                    "session ended"
                );
                self.drop_session();
                Err(AuthError::Unauthenticated)
            }
        }
    }

    fn drop_session(&self) {
        self.creds.clear();
        self.lock().creds = None;
        self.wake.notify_waiters();
        self.emit();
    }

    /// Calls the API with a token, refreshing once on a 401.
    pub async fn with_token<T, F, Fut>(&self, f: F) -> Result<T, ApiError>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = Result<T, ApiError>>,
    {
        let token = self.access_token().await.map_err(auth_to_api)?;
        match f(token.clone()).await {
            Err(e) if e.is_unauthorized() => {
                let fresh = self.refresh_after_401(&token).await.map_err(auth_to_api)?;
                f(fresh).await
            }
            other => other,
        }
    }

    pub async fn sign_out(&self) {
        self.cancel_attempt();
        let creds = self.lock().creds.clone();
        if let Some(c) = creds {
            if let Err(err) = self.api.logout(&c.access, &c.refresh).await {
                tracing::warn!(error = %err, "server sign-out failed; signing out locally");
            }
        }
        {
            let mut inner = self.lock();
            inner.error = None;
            inner.offline_started_at = None;
        }
        self.drop_session();
        tracing::info!(code = "auth.signed_out", "signed out");
    }

    /// "Use Tandem offline": Free limits, sign-in needed within 14 days.
    pub fn continue_offline(&self) -> i64 {
        let now = self.clock.now_ms();
        let started = *self.lock().offline_started_at.get_or_insert(now);
        self.emit();
        started
    }

    /// Background refresh at 80% of the token lifetime (F-BE-02).
    pub async fn run_refresher(self: Arc<Self>) {
        loop {
            let due = self
                .lock()
                .creds
                .as_ref()
                .map(|c| c.issued_at + (c.expires_at - c.issued_at) * 8 / 10);
            let Some(due) = due else {
                self.wake.notified().await;
                continue;
            };
            let now = self.clock.now_ms();
            if now >= due {
                if self.force_refresh().await == Err(AuthError::Network) {
                    tokio::select! {
                        _ = self.clock.sleep(Duration::from_secs(60)) => {}
                        _ = self.wake.notified() => {}
                    }
                }
                continue;
            }
            tokio::select! {
                _ = self.clock.sleep(Duration::from_millis((due - now) as u64)) => {}
                _ = self.wake.notified() => {}
            }
        }
    }

    /// Refreshes after the machine wakes: the wall clock jumps ahead of the
    /// monotonic one while suspended.
    pub async fn run_resume_watcher(self: Arc<Self>) {
        let tick = Duration::from_secs(30);
        let mut last_mono = Instant::now();
        let mut last_wall = SystemTime::now();
        loop {
            tokio::time::sleep(tick).await;
            let mono = last_mono.elapsed();
            let wall = SystemTime::now()
                .duration_since(last_wall)
                .unwrap_or_default();
            last_mono = Instant::now();
            last_wall = SystemTime::now();
            if wall > mono + Duration::from_secs(60) && self.is_signed_in() {
                tracing::info!(code = "auth.resume", "woke from sleep; refreshing");
                let _ = self.force_refresh().await;
            }
        }
    }
}

fn auth_to_api(e: AuthError) -> ApiError {
    match e {
        AuthError::Unauthenticated => ApiError::Http {
            status: 401,
            code: "UNAUTHENTICATED".into(),
            detail: None,
        },
        AuthError::Network => ApiError::Network("offline".into()),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn token_chars(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Parses `/callback?code=…&state=…` from an HTTP request line.
pub fn parse_callback_request(request: &str) -> Option<Callback> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?;
    let url = url::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    let mut code = None;
    let mut state = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            _ => {}
        }
    }
    let (code, state) = (code?, state?);
    (token_chars(&code) && token_chars(&state)).then_some(Callback { code, state })
}

fn callback_page(cb: &Callback) -> String {
    // Both values are checked to be base64url, so they're safe in HTML.
    let link = format!(
        "{}://auth/callback?code={}&state={}",
        deep_link_scheme(),
        cb.code,
        cb.state
    );
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Vibezeo</title>\
         <style>body{{font:15px system-ui,sans-serif;background:#0b111c;color:#dbe4ef;display:grid;place-items:center;height:100vh;margin:0}}\
         a{{color:#2dd4bf}}</style></head><body><main><h1>You can return to Vibezeo.</h1>\
         <p>If Vibezeo didn't come to the front, <a href=\"{link}\">open Vibezeo</a>.</p></main></body></html>"
    )
}

async fn serve_loopback(
    listener: TcpListener,
    tx: mpsc::UnboundedSender<Callback>,
    cancel: Arc<Notify>,
) {
    loop {
        let accepted = tokio::select! {
            a = listener.accept() => a,
            _ = cancel.notified() => return,
        };
        let Ok((mut sock, _)) = accepted else {
            continue;
        };
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 8192];
            let mut len = 0;
            let read = tokio::time::timeout(Duration::from_secs(5), async {
                while len < buf.len() {
                    match sock.read(&mut buf[len..]).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            len += n;
                            if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
            })
            .await;
            if read.is_err() {
                return;
            }
            let request = String::from_utf8_lossy(&buf[..len]).to_string();
            let (status, body) = match parse_callback_request(&request) {
                Some(cb) => {
                    let page = callback_page(&cb);
                    let _ = tx.send(cb);
                    ("200 OK", page)
                }
                None => ("404 Not Found", "Not found".to_string()),
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(response.as_bytes()).await;
            let _ = sock.shutdown().await;
        });
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::services::api::{MeResponse, TokenPair};
    use crate::services::opener::testing::RecordingOpener;
    use async_trait::async_trait;
    use futures::stream::BoxStream;
    use tandem_core::clock::MockClock;
    use tandem_core::keychain::MemoryKeychain;
    use tandem_core::remote_config::SignedConfig;

    /// A scripted API: exchange accepts only the code `good-code` with the
    /// verifier from the matching attempt; refresh rotates tokens.
    #[derive(Default)]
    pub struct FakeApi {
        pub exchanges: Mutex<Vec<(String, String)>>,
        pub refreshes: Mutex<u32>,
        pub revoked: Mutex<bool>,
        pub offline: Mutex<bool>,
        pub me: Mutex<Option<MeResponse>>,
    }

    fn pair(n: u32) -> TokenPair {
        TokenPair {
            access_token: format!("access-{n}"),
            refresh_token: format!("refresh-{n}"),
            expires_in: 3600,
        }
    }

    #[async_trait]
    impl ApiClient for FakeApi {
        async fn exchange(
            &self,
            code: &str,
            verifier: &str,
            _df: &str,
        ) -> Result<TokenPair, ApiError> {
            if *self.offline.lock().expect("lock") {
                return Err(ApiError::Network("down".into()));
            }
            self.exchanges
                .lock()
                .expect("lock")
                .push((code.into(), verifier.into()));
            if code == "good-code" && verifier.len() == 64 {
                Ok(pair(0))
            } else {
                Err(ApiError::Http {
                    status: 400,
                    code: "invalid_grant".into(),
                    detail: None,
                })
            }
        }
        async fn refresh(&self, _r: &str) -> Result<TokenPair, ApiError> {
            if *self.offline.lock().expect("lock") {
                return Err(ApiError::Network("down".into()));
            }
            if *self.revoked.lock().expect("lock") {
                return Err(ApiError::Http {
                    status: 401,
                    code: "UNAUTHENTICATED".into(),
                    detail: None,
                });
            }
            let mut n = self.refreshes.lock().expect("lock");
            *n += 1;
            Ok(pair(*n))
        }
        async fn logout(&self, _a: &str, _r: &str) -> Result<(), ApiError> {
            Ok(())
        }
        async fn me(&self, access: &str) -> Result<MeResponse, ApiError> {
            if *self.offline.lock().expect("lock") {
                return Err(ApiError::Network("down".into()));
            }
            if access.is_empty() {
                return Err(ApiError::Http {
                    status: 401,
                    code: "UNAUTHENTICATED".into(),
                    detail: None,
                });
            }
            self.me
                .lock()
                .expect("lock")
                .clone()
                .ok_or(ApiError::Decode("no me".into()))
        }
        async fn checkout(&self, _a: &str, plan: &str, interval: &str) -> Result<String, ApiError> {
            Ok(format!(
                "http://web.test/billing/mock?plan={plan}&interval={interval}"
            ))
        }
        async fn portal(&self, _a: &str) -> Result<String, ApiError> {
            Ok("http://web.test/billing/mock-portal".into())
        }
        async fn config(&self) -> Result<SignedConfig, ApiError> {
            Err(ApiError::Network("not in tests".into()))
        }
        async fn events(
            &self,
            _a: &str,
        ) -> Result<BoxStream<'static, Result<String, ApiError>>, ApiError> {
            Err(ApiError::Network("not in tests".into()))
        }
    }

    pub struct Rig {
        pub dir: tempfile::TempDir,
        pub data: DataDir,
        pub api: Arc<FakeApi>,
        pub opener: Arc<RecordingOpener>,
        pub keychain: Arc<MemoryKeychain>,
        pub clock: MockClock,
        pub bus: EventBus,
        pub auth: Arc<AuthService>,
    }

    pub fn rig() -> Rig {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let api = Arc::new(FakeApi::default());
        let opener = Arc::new(RecordingOpener::default());
        let keychain = Arc::new(MemoryKeychain::new());
        let clock = MockClock::new(chrono::Utc::now());
        let bus = EventBus::new();
        let auth = AuthService::new(
            data.clone(),
            keychain.clone(),
            api.clone(),
            opener.clone(),
            Endpoints {
                api: "http://api.test".into(),
                web: "http://web.test".into(),
                hosted: true,
            },
            Arc::new(clock.clone()),
            bus.clone(),
            None,
        );
        Rig {
            dir,
            data,
            api,
            opener,
            keychain,
            clock,
            bus,
            auth,
        }
    }

    fn query(url: &str, key: &str) -> String {
        url::Url::parse(url)
            .expect("url")
            .query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default()
    }

    async fn wait_for(auth: &AuthService, cond: impl Fn(&AuthState) -> bool) -> AuthState {
        for _ in 0..200 {
            let s = auth.state();
            if cond(&s) {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        auth.state()
    }

    async fn http_get(url: &str) -> String {
        let u = url::Url::parse(url).expect("url");
        let mut sock = tokio::net::TcpStream::connect((
            u.host_str().unwrap_or("127.0.0.1"),
            u.port().unwrap_or(80),
        ))
        .await
        .expect("connect");
        let target = format!("{}?{}", u.path(), u.query().unwrap_or(""));
        sock.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
            .await
            .expect("write");
        let mut out = String::new();
        sock.read_to_string(&mut out).await.expect("read");
        out
    }

    #[tokio::test]
    async fn f_be_02_1_loopback_flow_signs_in_and_encrypts_tokens() {
        let r = rig();
        r.auth.sign_in().await.expect("start");
        let opened = r.opener.opened.lock().expect("lock")[0].clone();
        assert!(opened.starts_with("http://web.test/desktop-login?"));
        let redirect = query(&opened, "redirect_uri");
        assert!(redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/callback"));
        assert_ne!(
            redirect, "http://127.0.0.1:0/callback",
            "never a fixed port"
        );
        assert_eq!(query(&opened, "challenge").len(), 43);
        assert_eq!(query(&opened, "df").len(), 64);
        let state = query(&opened, "state");

        let page = http_get(&format!("{redirect}?code=good-code&state={state}")).await;
        assert!(page.contains("You can return to Vibezeo."));
        let s = wait_for(&r.auth, |s| s.signed_in).await;
        assert!(
            s.signed_in && s.pending.is_none() && s.error.is_none(),
            "{s:?}"
        );

        // Encrypted at rest, key in the keychain under NS.auth/credentials-key.
        let file = std::fs::read(r.data.auth_credentials()).expect("credentials.enc");
        assert!(!String::from_utf8_lossy(&file).contains("access-0"));
        assert!(r
            .keychain
            .get(&keychain::service("auth"), KEY_ACCOUNT)
            .expect("kc")
            .is_some());
        // A relaunch reads them back.
        let again = CredentialStore::new(&r.data, r.keychain.clone())
            .load()
            .expect("load");
        assert_eq!(again.access, "access-0");
        // Without the key the file is useless and gets removed.
        let other = CredentialStore::new(&r.data, Arc::new(MemoryKeychain::new()));
        assert!(other.load().is_none());
    }

    #[tokio::test]
    async fn f_be_02_4_mismatched_state_is_ignored_then_valid_callback_succeeds() {
        let r = rig();
        r.auth.sign_in().await.expect("start");
        let opened = r.opener.opened.lock().expect("lock")[0].clone();
        let state = query(&opened, "state");
        r.auth.handle_callback(Callback {
            code: "good-code".into(),
            state: "someone-elses-state".into(),
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let s = r.auth.state();
        assert!(
            !s.signed_in && s.pending.is_some() && s.error.is_none(),
            "still waiting: {s:?}"
        );
        assert!(
            r.api.exchanges.lock().expect("lock").is_empty(),
            "no exchange for a foreign state"
        );
        // The deep-link path with the right state completes it.
        r.auth.handle_callback(Callback {
            code: "good-code".into(),
            state,
        });
        assert!(wait_for(&r.auth, |s| s.signed_in).await.signed_in);
    }

    #[tokio::test]
    async fn f_be_02_2_rejected_exchange_reports_expired() {
        let r = rig();
        r.auth.sign_in().await.expect("start");
        let state = query(&r.opener.opened.lock().expect("lock")[0], "state");
        r.auth.handle_callback(Callback {
            code: "stale-code".into(),
            state,
        });
        let s = wait_for(&r.auth, |s| s.error.is_some()).await;
        let err = s.error.expect("error");
        assert_eq!(err.kind, AuthFailureKind::Expired);
        assert_eq!(err.message, "That link has expired.");
        assert!(err.detail.contains("400 invalid_grant") && !err.detail.contains("stale-code"));
        assert!(!s.signed_in && s.pending.is_none());
    }

    #[tokio::test]
    async fn f_be_02_failures_are_categorized() {
        // Browser didn't open: keep listening, offer the link.
        let r = rig();
        let failing = AuthService::new(
            r.data.clone(),
            r.keychain.clone(),
            r.api.clone(),
            Arc::new(RecordingOpener {
                fail: true,
                ..Default::default()
            }),
            Endpoints {
                api: "http://api.test".into(),
                web: "http://web.test".into(),
                hosted: true,
            },
            Arc::new(r.clock.clone()),
            r.bus.clone(),
            None,
        );
        failing.sign_in().await.expect("start");
        let s = failing.state();
        assert_eq!(
            s.error.as_ref().map(|e| e.kind),
            Some(AuthFailureKind::Browser)
        );
        assert!(s.pending.is_some(), "still waiting with a copyable link");

        // Network down during exchange.
        *r.api.offline.lock().expect("lock") = true;
        r.auth.sign_in().await.expect("start");
        let state = query(&r.opener.opened.lock().expect("lock")[0], "state");
        r.auth.handle_callback(Callback {
            code: "good-code".into(),
            state,
        });
        let s = wait_for(&r.auth, |s| s.error.is_some()).await;
        assert_eq!(s.error.map(|e| e.kind), Some(AuthFailureKind::Network));

        // No callback within 5 minutes.
        *r.api.offline.lock().expect("lock") = false;
        r.auth.sign_in().await.expect("start");
        tokio::time::sleep(Duration::from_millis(20)).await;
        r.clock.advance(Duration::from_secs(6 * 60));
        let s = wait_for(&r.auth, |s| s.error.is_some()).await;
        let err = s.error.expect("timeout");
        assert_eq!(err.kind, AuthFailureKind::Callback);
        assert_eq!(err.message, "Sign-in timed out. Try again.");
    }

    #[tokio::test]
    async fn replacing_an_attempt_is_safe() {
        let r = rig();
        r.auth.sign_in().await.expect("first");
        let first_state = query(&r.opener.opened.lock().expect("lock")[0], "state");
        r.auth.sign_in().await.expect("second");
        let second_state = query(&r.opener.opened.lock().expect("lock")[1], "state");
        // The first attempt's callback no longer counts.
        r.auth.handle_callback(Callback {
            code: "good-code".into(),
            state: first_state,
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!r.auth.state().signed_in);
        r.auth.handle_callback(Callback {
            code: "good-code".into(),
            state: second_state,
        });
        assert!(wait_for(&r.auth, |s| s.signed_in).await.signed_in);
        r.auth.cancel_attempt();
    }

    async fn signed_in(r: &Rig) {
        r.auth.sign_in().await.expect("start");
        let state = query(
            &r.opener
                .opened
                .lock()
                .expect("lock")
                .last()
                .cloned()
                .unwrap_or_default(),
            "state",
        );
        r.auth.handle_callback(Callback {
            code: "good-code".into(),
            state,
        });
        assert!(wait_for(&r.auth, |s| s.signed_in).await.signed_in);
    }

    #[tokio::test]
    async fn f_be_02_refresh_rotates_and_network_errors_keep_the_session() {
        let r = rig();
        signed_in(&r).await;
        assert_eq!(r.auth.access_token().await.expect("token"), "access-0");
        // Near expiry the next call refreshes first.
        r.clock.advance(Duration::from_secs(59 * 60 + 30));
        assert_eq!(r.auth.access_token().await.expect("token"), "access-1");
        // Offline: refresh fails but the user stays signed in.
        *r.api.offline.lock().expect("lock") = true;
        assert_eq!(r.auth.force_refresh().await, Err(AuthError::Network));
        assert!(r.auth.state().signed_in);
        *r.api.offline.lock().expect("lock") = false;
        // A 401 triggers exactly one refresh even with two callers.
        let (a, b) = tokio::join!(
            r.auth.refresh_after_401("access-1"),
            r.auth.refresh_after_401("access-1")
        );
        assert_eq!(
            (a.expect("a"), b.expect("b")),
            ("access-2".to_string(), "access-2".to_string())
        );
    }

    #[tokio::test]
    async fn f_be_02_3_f_wb_05_1_revoked_family_signs_out_on_refresh() {
        let r = rig();
        signed_in(&r).await;
        let mut events = r.bus.subscribe(Topic::App);
        *r.api.revoked.lock().expect("lock") = true;
        assert_eq!(
            r.auth.force_refresh().await,
            Err(AuthError::Unauthenticated)
        );
        let s = r.auth.state();
        assert!(!s.signed_in, "the desktop shows the sign-in prompt");
        assert!(!r.data.auth_credentials().exists());
        let ev = events.recv().await.expect("event");
        assert_eq!(ev.name, "auth:changed");
        assert_eq!(ev.payload["signedIn"], false);
    }

    #[tokio::test]
    async fn refresher_runs_at_eighty_percent_of_lifetime() {
        let r = rig();
        signed_in(&r).await;
        tokio::spawn(r.auth.clone().run_refresher());
        tokio::time::sleep(Duration::from_millis(20)).await;
        r.clock.advance(Duration::from_secs(47 * 60));
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(*r.api.refreshes.lock().expect("lock"), 0, "not before 80%");
        r.clock.advance(Duration::from_secs(2 * 60));
        for _ in 0..100 {
            if *r.api.refreshes.lock().expect("lock") == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(*r.api.refreshes.lock().expect("lock"), 1);
    }

    #[tokio::test]
    async fn early_access_runs_free_without_a_deadline_or_sign_in() {
        let r = rig();
        let auth = AuthService::new(
            r.data.clone(),
            r.keychain.clone(),
            r.api.clone(),
            r.opener.clone(),
            Endpoints {
                api: "http://api.test".into(),
                web: "http://web.test".into(),
                hosted: false,
            },
            Arc::new(r.clock.clone()),
            r.bus.clone(),
            None,
        );
        auth.continue_offline();
        r.clock.advance(Duration::from_secs(30 * 86_400));
        let s = auth.state();
        assert!(s.offline && !s.hosted);
        assert_eq!(s.offline_days_left, None, "no sign-in deadline");
        let err = auth.sign_in().await.expect_err("no accounts yet");
        assert_eq!(err.code, ErrorCode::Unavailable);
        assert!(
            r.opener.opened.lock().expect("opened").is_empty(),
            "no browser opened"
        );
    }

    #[tokio::test]
    async fn sign_out_and_offline_mode() {
        let r = rig();
        r.auth.continue_offline();
        let s = r.auth.state();
        assert!(s.offline && s.offline_days_left == Some(OFFLINE_DAYS));
        r.clock.advance(Duration::from_secs(12 * 86_400));
        assert_eq!(r.auth.state().offline_days_left, Some(2));
        signed_in(&r).await;
        assert!(!r.auth.state().offline);
        r.auth.sign_out().await;
        assert!(!r.auth.state().signed_in);
        assert!(!r.data.auth_credentials().exists());
    }

    #[test]
    fn callback_requests_are_parsed_strictly() {
        let cb = parse_callback_request("GET /callback?code=abc_DEF-1&state=xyz HTTP/1.1\r\n")
            .expect("cb");
        assert_eq!(
            cb,
            Callback {
                code: "abc_DEF-1".into(),
                state: "xyz".into()
            }
        );
        assert!(parse_callback_request("GET /other?code=a&state=b HTTP/1.1").is_none());
        assert!(parse_callback_request("POST /callback?code=a&state=b HTTP/1.1").is_none());
        assert!(parse_callback_request("GET /callback?code=<script>&state=b HTTP/1.1").is_none());
        assert!(parse_callback_request("GET /callback?code=a HTTP/1.1").is_none());
    }

    #[test]
    fn pkce_challenge_is_s256_of_the_verifier() {
        let p = Pkce::generate();
        assert_eq!(p.verifier.len(), 64);
        assert_eq!(
            p.challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(p.verifier.as_bytes()))
        );
        assert_eq!(URL_SAFE_NO_PAD.decode(&p.state).expect("b64").len(), 32);
    }
}
