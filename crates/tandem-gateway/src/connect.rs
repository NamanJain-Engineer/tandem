//! Connecting a plugin (F-PL-03): the OAuth flow step by step, API keys and
//! local ports. The order matters and is the same for each:
//! the new credential is verified first, then saved to the keychain, and
//! only then is success reported. A failure at any step leaves an existing
//! connection's credential exactly as it was (AC3).

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_core::ids::ConnectionId;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use url::Url;

use crate::credentials::{Credential, CredentialStore};
use crate::oauth::{
    self, AuthScope, CallbackResult, ClientReg, Loopback, OAuthError, OAuthMeta, Step,
};
use crate::pinned::PinnedClient;
use crate::vendors::{
    AccountInfo, AuthKind, CallCtx, ConnectionInfo, ModuleStatus, Upstream, VendorModule,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StepState {
    Active,
    Done,
    Failed,
}

/// Where a flow is, for the connect sheet (`plugins:connect`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub step: Step,
    pub state: StepState,
    /// The sign-in address while the browser is open ("Copy link").
    pub authorize_url: Option<String>,
    pub error: Option<String>,
}

/// A connection that worked.
#[derive(Debug, Clone, PartialEq)]
pub struct Connected {
    pub account: AccountInfo,
    pub auth_kind: AuthKind,
    /// Non-secret facts for `meta_json` (client id, token endpoint, mode).
    pub meta: Value,
}

pub type ProgressFn = Arc<dyn Fn(Progress) + Send + Sync>;
pub type OpenFn = Arc<dyn Fn(&Url) -> Result<(), String> + Send + Sync>;

pub struct OAuthRequest {
    pub module: Arc<dyn VendorModule>,
    /// The connection this credential is for (new, or the one reconnected).
    pub connection: ConnectionId,
    /// A client id from the config (`plugins.<id>.client_id`, D-960) for
    /// servers that don't register apps.
    pub static_client_id: Option<String>,
    pub open: OpenFn,
    pub progress: ProgressFn,
    pub cancel: CancellationToken,
    pub timeout: Duration,
}

fn emit(p: &ProgressFn, step: Step, state: StepState) {
    p(Progress {
        step,
        state,
        authorize_url: None,
        error: None,
    });
}

fn failed(p: &ProgressFn, e: OAuthError) -> OAuthError {
    p(Progress {
        step: e.step,
        state: StepState::Failed,
        authorize_url: None,
        error: Some(e.message.clone()),
    });
    e
}

/// Refuses modules that can't be connected (previews).
fn connectable(module: &dyn VendorModule) -> Result<(), OAuthError> {
    if module.manifest().status == ModuleStatus::Preview {
        return Err(OAuthError {
            step: Step::Discovery,
            message: format!("{} isn't ready yet", module.manifest().label),
        });
    }
    Ok(())
}

/// Verifies a credential with the module's cheap read.
async fn verify(
    module: &dyn VendorModule,
    connection: &ConnectionId,
    cred: &Credential,
    meta: &Value,
) -> Result<AccountInfo, OAuthError> {
    let mut c = PinnedClient::with_bearer(module.manifest().pinned.clone(), cred.bearer());
    for s in cred.secrets() {
        c = c.also_redact(s);
    }
    let mut conn = ConnectionInfo::new(connection.clone(), module.manifest().id);
    conn.meta = meta.clone();
    module
        .verify(&CallCtx {
            conn: &conn,
            http: &c,
        })
        .await
        .map_err(|e| OAuthError {
            step: Step::Verification,
            message: c.redact(&e.to_string()),
        })
}

fn save(
    store: &CredentialStore,
    module: &dyn VendorModule,
    connection: &ConnectionId,
    cred: &Credential,
) -> Result<(), OAuthError> {
    store
        .save(module.manifest().id, connection, cred)
        .map_err(|e| OAuthError {
            step: Step::Verification,
            message: format!("Couldn't save to your keychain: {e}"),
        })
}

/// The whole browser sign-in. Returns once the credential is verified and
/// in the keychain.
pub async fn oauth(
    store: &CredentialStore,
    loopback: &Loopback,
    req: OAuthRequest,
) -> Result<Connected, OAuthError> {
    let module = req.module.clone();
    connectable(module.as_ref())?;
    let m = module.manifest();
    let scope = AuthScope::of(m);
    let p = &req.progress;
    let run = async {
        emit(p, Step::Discovery, StepState::Active);
        let found = oauth::discover(m, &scope).await.map_err(|e| failed(p, e))?;
        emit(p, Step::Discovery, StepState::Done);

        let redirect_uri = loopback.redirect_uri();
        emit(p, Step::Registration, StepState::Active);
        // A client the config names wins (a hand-registered one, or the one
        // a reviewing vendor approved); otherwise the server registers us.
        let client = if let Some(id) = req
            .static_client_id
            .clone()
            .filter(|s| !s.trim().is_empty())
        {
            ClientReg {
                client_id: id,
                client_secret: None,
            }
        } else if found.metadata.registration_endpoint.is_some() {
            oauth::register(&scope, &found.metadata, &redirect_uri)
                .await
                .map_err(|e| failed(p, e))?
        } else {
            return Err(failed(
                p,
                OAuthError {
                    step: Step::Registration,
                    message: format!(
                        "{} needs an app registration that this build doesn't have yet",
                        m.label
                    ),
                },
            ));
        };
        emit(p, Step::Registration, StepState::Done);

        let attempt = oauth::Attempt::new();
        let url = oauth::authorize_url(
            &found.metadata,
            &client.client_id,
            &redirect_uri,
            &attempt,
            found.resource.as_deref(),
            &m.scopes,
        )
        .map_err(|e| failed(p, e))?;
        scope
            .check(url.as_str(), Step::SignIn)
            .await
            .map_err(|e| failed(p, e))?;
        let waiting = loopback.expect(&attempt.state);
        // However the flow ends (cancel, timeout, error), this attempt stops
        // waiting, so its callback is ignored from then on (AC4).
        let _forget = Forget(loopback, attempt.state.clone());
        p(Progress {
            step: Step::SignIn,
            state: StepState::Active,
            authorize_url: Some(url.to_string()),
            error: None,
        });
        if let Err(e) = (req.open)(&url) {
            tracing::warn!(code = "plugins.open_failed", error = %e, "couldn't open the browser");
        }
        let code = tokio::select! {
            r = waiting => match r {
                Ok(CallbackResult::Code(c)) => c,
                Ok(CallbackResult::Denied(text)) => return Err(failed(p, OAuthError { step: Step::SignIn, message: text })),
                Err(_) => return Err(failed(p, OAuthError { step: Step::SignIn, message: "The sign-in stopped".into() })),
            },
            _ = tokio::time::sleep(req.timeout) => {
                return Err(failed(p, OAuthError { step: Step::Timeout, message: "Nobody finished signing in in time".into() }));
            }
        };
        emit(p, Step::SignIn, StepState::Done);

        emit(p, Step::TokenExchange, StepState::Active);
        let cred = oauth::exchange(
            &scope,
            &found.metadata,
            &client,
            &code,
            &attempt,
            &redirect_uri,
            found.resource.as_deref(),
        )
        .await
        .map_err(|e| failed(p, e))?;
        emit(p, Step::TokenExchange, StepState::Done);

        let meta = json!({"oauth": OAuthMeta {
            client_id: client.client_id.clone(),
            token_endpoint: found.metadata.token_endpoint.clone(),
            revocation_endpoint: found.metadata.revocation_endpoint.clone(),
            resource: found.resource.clone(),
        }});
        emit(p, Step::Verification, StepState::Active);
        let account = verify(module.as_ref(), &req.connection, &cred, &meta)
            .await
            .map_err(|e| failed(p, e))?;
        // Saved before success is shown; a cancel from here on is too late
        // to matter and the connection stands.
        save(store, module.as_ref(), &req.connection, &cred).map_err(|e| failed(p, e))?;
        emit(p, Step::Verification, StepState::Done);
        let mut meta = meta;
        for (k, v) in &account.meta {
            meta[k] = v.clone();
        }
        Ok(Connected {
            account,
            auth_kind: m.auth,
            meta,
        })
    };
    let state_guard = req.cancel.clone();
    tokio::select! {
        r = run => r,
        _ = state_guard.cancelled() => Err(OAuthError { step: Step::SignIn, message: "Cancelled".into() }),
    }
}

struct Forget<'a>(&'a Loopback, String);

impl Drop for Forget<'_> {
    fn drop(&mut self) {
        self.0.forget(&self.1);
    }
}

/// Stripe's mode from a key's prefix (`sk_test_`, `rk_live_`, …).
pub fn key_mode(key: &str) -> Option<&'static str> {
    if key.contains("_test_") {
        Some("test")
    } else if key.contains("_live_") {
        Some("live")
    } else {
        None
    }
}

/// An API key: verified with a cheap read, then saved (F-PL-03).
pub async fn api_key(
    store: &CredentialStore,
    module: &dyn VendorModule,
    connection: &ConnectionId,
    key: &str,
) -> Result<Connected, OAuthError> {
    connectable(module)?;
    let m = module.manifest();
    if m.auth != AuthKind::ApiKey && m.alt_auth != Some(AuthKind::ApiKey) {
        return Err(OAuthError {
            step: Step::Verification,
            message: format!("{} doesn't take an API key", m.label),
        });
    }
    let key = key.trim();
    let cred = Credential::ApiKey {
        api_key: key.to_string(),
    };
    let mut meta = json!({});
    if let Some(mode) = key_mode(key) {
        meta["mode"] = json!(mode);
    }
    let account = verify(module, connection, &cred, &meta).await?;
    save(store, module, connection, &cred)?;
    for (k, v) in &account.meta {
        meta[k] = v.clone();
    }
    Ok(Connected {
        account,
        auth_kind: AuthKind::ApiKey,
        meta,
    })
}

/// A desktop app's local MCP add-on: is the right server on `port`?
pub async fn probe_local(module: &dyn VendorModule, port: u16) -> Result<Connected, OAuthError> {
    connectable(module)?;
    let Upstream::LocalPort { probe_path, .. } = &module.manifest().upstream else {
        return Err(OAuthError {
            step: Step::Verification,
            message: format!("{} isn't a local service", module.manifest().label),
        });
    };
    let url = format!("http://127.0.0.1:{port}{probe_path}");
    let resp = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|_| OAuthError {
            step: Step::Verification,
            message: format!("Nothing is listening on port {port}"),
        })?;
    let server = resp
        .headers()
        .get("server")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    let id = module.manifest().id;
    if body.get("plugin").and_then(Value::as_str) != Some(id) {
        let who = if server.is_empty() {
            String::new()
        } else {
            format!(" ({server})")
        };
        return Err(OAuthError {
            step: Step::Verification,
            message: format!("Another app is listening on port {port}{who}"),
        });
    }
    Ok(Connected {
        account: AccountInfo::default(),
        auth_kind: AuthKind::LocalPort,
        meta: json!({"port": port}),
    })
}

/// Disconnect (F-PL-03): revoke upstream where the vendor can, then delete
/// the credential. Revocation is best effort; the keychain entry always goes.
pub async fn disconnect(
    store: &CredentialStore,
    module: &dyn VendorModule,
    conn: &ConnectionInfo,
) -> Result<(), String> {
    let id = module.manifest().id;
    if let Ok(Some(cred)) = store.load(id, &conn.id) {
        if let Some(meta) = OAuthMeta::from_meta(&conn.meta) {
            if let Err(e) = oauth::revoke(&AuthScope::of(module.manifest()), &meta, &cred).await {
                tracing::info!(
                    code = "plugins.revoke_failed",
                    step = e.step.as_str(),
                    "the vendor didn't confirm the revocation"
                );
            }
        } else {
            let c = PinnedClient::with_bearer(module.manifest().pinned.clone(), cred.bearer());
            let _ = module.revoke(&CallCtx { conn, http: &c }).await;
        }
    }
    store.delete(id, &conn.id).map_err(|e| e.to_string())
}
