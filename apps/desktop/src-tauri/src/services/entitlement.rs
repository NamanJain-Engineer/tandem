//! The plan the app enforces (F-BE-04, ARCHITECTURE §14.4). `/v1/me` returns
//! an Ed25519-signed entitlement token; it's verified with the bundled keys
//! and cached in `$DATA/cache/entitlement.jws`. Network errors keep the cached
//! token (24 h grace past expiry); a tampered token or a signed-out user gets
//! Free. `/v1/me/events` (SSE) triggers a refetch on plan or credit changes.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::clock::Clock;
use tandem_core::entitlement::{
    effective_limits, verify_entitlement, EntitlementClaims, LimitSource, Limits, Plan,
    PublicKeySet, GRACE_SECONDS,
};
use tandem_core::policy::OrgPolicy;
use tandem_core::DataDir;

use super::api::{ApiClient, ApiError, MeOrg, MeResponse};
use super::auth::AuthService;

pub const MONTHLY_GRANT: i64 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CreditsView {
    pub monthly: i64,
    pub purchased: i64,
    pub resets_at: Option<String>,
    /// A Team member's monthly cap and spend against it (F-BL-06).
    #[serde(default)]
    pub cap: Option<i64>,
    #[serde(default)]
    pub spent: Option<i64>,
    /// What arrives each month; the ◈ display warns below 10% of it.
    #[serde(default)]
    pub grant: Option<i64>,
}

impl CreditsView {
    /// What can be spent now: the balance, held to a member's remaining cap.
    pub fn available(&self) -> i64 {
        let total = self.monthly + self.purchased;
        match (self.cap, self.spent) {
            (Some(cap), spent) => total.min((cap - spent.unwrap_or(0)).max(0)),
            _ => total,
        }
    }
}

/// `entitlement_get` and the `entitlement:changed` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EntitlementView {
    pub plan: Plan,
    /// Subscription status from the server (`active`, `trialing`, `past_due`,
    /// `canceled`, `none`); `none` when signed out.
    pub status: String,
    pub limits: Limits,
    pub source: LimitSource,
    pub credits: Option<CreditsView>,
    /// The Team org's admin policy, from the last verified token (D-607).
    pub policy: Option<OrgPolicy>,
    pub org: Option<MeOrg>,
    pub paid_until: Option<String>,
    pub cancel_at_period_end: bool,
    /// False after the last refresh failed on the network.
    pub online: bool,
    /// Unix ms until which the cached plan stays confirmed offline (token
    /// expiry + 24 h), for the UI-133 banner.
    pub confirmed_until: Option<i64>,
    pub monthly_grant: i64,
}

/// The non-secret parts of the last `/me`, cached for offline launches.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MeCache {
    email: String,
    status: String,
    credits: CreditsView,
    #[serde(default)]
    org: Option<MeOrg>,
    paid_until: Option<String>,
    cancel_at_period_end: bool,
}

#[derive(Default)]
struct Inner {
    claims: Option<EntitlementClaims>,
    /// The policy of the last token that verified, kept while signed in even
    /// when the cached token later fails to verify (D-607).
    policy: Option<OrgPolicy>,
    me: Option<MeCache>,
    online: bool,
    /// Limits a unit test enforces instead (no local limit exists with no
    /// account since D-2900, so the generic checks need a seam to be tested).
    #[cfg(test)]
    test_limits: Option<Limits>,
}

pub struct EntitlementService {
    auth: Arc<AuthService>,
    api: Arc<dyn ApiClient>,
    keys: PublicKeySet,
    data: DataDir,
    clock: Arc<dyn Clock>,
    bus: EventBus,
    inner: RwLock<Inner>,
    kick: tokio::sync::Notify,
}

impl EntitlementService {
    pub fn new(
        auth: Arc<AuthService>,
        api: Arc<dyn ApiClient>,
        keys: PublicKeySet,
        data: DataDir,
        clock: Arc<dyn Clock>,
        bus: EventBus,
    ) -> Arc<Self> {
        let mut inner = Inner {
            online: true,
            ..Default::default()
        };
        if auth.is_signed_in() {
            inner.claims = std::fs::read_to_string(data.entitlement_cache())
                .ok()
                .and_then(|t| verify_entitlement(t.trim(), &keys).ok());
            inner.policy = std::fs::read_to_string(policy_cache_path(&data))
                .ok()
                .and_then(|t| verify_entitlement(t.trim(), &keys).ok())
                .and_then(|c| c.policy);
            inner.me = std::fs::read(me_cache_path(&data))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
        }
        Arc::new(Self {
            auth,
            api,
            keys,
            data,
            clock,
            bus,
            inner: RwLock::new(inner),
            kick: tokio::sync::Notify::new(),
        })
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        self.inner.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        self.inner.write().unwrap_or_else(|e| e.into_inner())
    }

    pub fn view(&self) -> EntitlementView {
        let inner = self.read();
        let now = self.clock.now_ms() / 1000;
        #[allow(unused_mut)]
        let (plan, mut limits, source) = effective_limits(inner.claims.as_ref(), now);
        #[cfg(test)]
        if let Some(l) = &inner.test_limits {
            limits = l.clone();
        }
        let me = inner.me.as_ref().filter(|_| inner.claims.is_some());
        EntitlementView {
            plan,
            status: inner
                .claims
                .as_ref()
                .map(|c| c.status.clone())
                .unwrap_or_else(|| "none".into()),
            limits,
            source,
            credits: me.map(|m| m.credits.clone()),
            policy: inner.policy.clone(),
            org: me.and_then(|m| m.org.clone()),
            paid_until: me.and_then(|m| m.paid_until.clone()),
            cancel_at_period_end: me.map(|m| m.cancel_at_period_end).unwrap_or(false),
            online: inner.online,
            confirmed_until: inner
                .claims
                .as_ref()
                .map(|c| (c.exp + GRACE_SECONDS) * 1000),
            monthly_grant: me.and_then(|m| m.credits.grant).unwrap_or(MONTHLY_GRANT),
        }
    }

    /// The org policy to enforce now, if any (F-BL-06).
    pub fn policy(&self) -> Option<OrgPolicy> {
        self.read().policy.clone()
    }

    /// The limits to enforce right now.
    pub fn limits(&self) -> Limits {
        self.view().limits
    }

    /// Enforces `limits` from now on, in unit tests only.
    #[cfg(test)]
    pub(crate) fn set_test_limits(&self, limits: Limits) {
        self.write().test_limits = Some(limits);
    }

    fn emit(&self) {
        self.bus.publish(BusEvent::new(
            Topic::Entitlement,
            "entitlement:changed",
            self.view(),
        ));
    }

    /// Refetches `/me`. Only an explicit server answer changes the plan.
    pub async fn refresh(&self) -> EntitlementView {
        if !self.auth.is_signed_in() {
            self.clear();
            self.emit();
            return self.view();
        }
        let api = self.api.clone();
        let result = self.auth.with_token(move |t| {
            let api = api.clone();
            async move { api.me(&t).await }
        });
        match result.await {
            Ok(me) => self.apply_me(me),
            Err(e) if e.is_network() => {
                tracing::warn!(
                    code = "entitlement.offline",
                    "couldn't refresh the plan; keeping the cached one"
                );
                self.write().online = false;
            }
            Err(e) if e.is_unauthorized() => self.clear(),
            Err(e) => {
                tracing::warn!(code = "entitlement.refresh_failed", error = %e, "plan refresh failed")
            }
        }
        self.emit();
        self.view()
    }

    fn apply_me(&self, me: MeResponse) {
        let claims = match verify_entitlement(&me.entitlement_token, &self.keys) {
            Ok(c) => Some(c),
            Err(err) => {
                tracing::warn!(code = "entitlement.rejected", error = %err, "entitlement token rejected; Free applies");
                None
            }
        };
        let cache = MeCache {
            email: me.user.email.clone(),
            status: me.status.clone(),
            credits: CreditsView {
                monthly: me.credits.monthly,
                purchased: me.credits.purchased,
                resets_at: me.credits.resets_at.clone(),
                cap: me.credits.cap,
                spent: me.credits.spent,
                grant: me.credits.grant,
            },
            org: me.org.clone(),
            paid_until: me.paid_until.clone(),
            cancel_at_period_end: me.cancel_at_period_end,
        };
        if claims.is_some() {
            let _ = std::fs::create_dir_all(self.data.cache());
            let _ = tandem_core::atomic::atomic_write(
                &self.data.entitlement_cache(),
                me.entitlement_token.as_bytes(),
                Some(0o600),
            );
            // The policy's own copy: only a newer verified token replaces it.
            let policy_path = policy_cache_path(&self.data);
            if claims.as_ref().is_some_and(|c| c.policy.is_some()) {
                let _ = tandem_core::atomic::atomic_write(
                    &policy_path,
                    me.entitlement_token.as_bytes(),
                    Some(0o600),
                );
            } else {
                let _ = std::fs::remove_file(&policy_path);
            }
            if let Ok(bytes) = serde_json::to_vec(&cache) {
                let _ = tandem_core::atomic::atomic_write(
                    &me_cache_path(&self.data),
                    &bytes,
                    Some(0o600),
                );
            }
        }
        {
            let mut inner = self.write();
            if let Some(c) = &claims {
                inner.policy = c.policy.clone();
            }
            inner.claims = claims;
            inner.me = Some(cache);
            inner.online = true;
        }
        self.auth.set_email(&me.user.email);
    }

    fn clear(&self) {
        let mut inner = self.write();
        inner.claims = None;
        inner.me = None;
        inner.policy = None;
        let _ = std::fs::remove_file(self.data.entitlement_cache());
        let _ = std::fs::remove_file(policy_cache_path(&self.data));
        let _ = std::fs::remove_file(me_cache_path(&self.data));
    }

    /// Refetches on every `auth:changed` and restarts the event stream.
    pub async fn run_auth_listener(self: Arc<Self>) {
        let mut rx = self.bus.subscribe(Topic::App);
        loop {
            match rx.recv().await {
                Ok(ev) if ev.name == "auth:changed" => {
                    let signed_in = ev
                        .payload
                        .get("signedIn")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let had = self.read().claims.is_some() || self.read().me.is_some();
                    if signed_in != had || signed_in {
                        self.refresh().await;
                        self.kick.notify_waiters();
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => return,
            }
        }
    }

    /// Keeps `/v1/me/events` open while signed in; each event refetches `/me`
    /// (F-BL-01 AC2: a plan change shows up within seconds).
    pub async fn run_events(self: Arc<Self>) {
        let mut backoff = Duration::from_secs(1);
        loop {
            if !self.auth.is_signed_in() {
                self.kick.notified().await;
                continue;
            }
            let api = self.api.clone();
            let opened = self
                .auth
                .with_token(move |t| {
                    let api = api.clone();
                    async move { api.events(&t).await }
                })
                .await;
            match opened {
                Ok(mut stream) => {
                    backoff = Duration::from_secs(1);
                    // Catch up on anything missed while disconnected.
                    self.refresh().await;
                    loop {
                        tokio::select! {
                            next = stream.next() => match next {
                                Some(Ok(name)) if name == "entitlement" || name == "credits" => { self.refresh().await; }
                                Some(Ok(_)) => {}
                                Some(Err(_)) | None => break,
                            },
                            _ = self.kick.notified() => break,
                        }
                    }
                }
                Err(ApiError::Http { status: 401, .. }) => {
                    self.kick.notified().await;
                    continue;
                }
                Err(_) => {
                    self.write().online = false;
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                _ = self.kick.notified() => {}
            }
            backoff = (backoff * 2).min(Duration::from_secs(30));
        }
    }
}

fn me_cache_path(data: &DataDir) -> std::path::PathBuf {
    data.cache().join("me.json")
}

/// The last verified token that carried an org policy.
fn policy_cache_path(data: &DataDir) -> std::path::PathBuf {
    data.cache().join("policy.jws")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::api::{MeCredits, MeUser};
    use crate::services::auth::tests::{rig, Rig};
    use crate::services::auth::Callback;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};

    fn token(key: &SigningKey, plan: &str, exp: i64) -> String {
        token_with(key, plan, exp, Value::Null)
    }

    fn token_with(key: &SigningKey, plan: &str, exp: i64, policy: Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","kid":"ek-1"}"#);
        let limits = if plan == "free" {
            serde_json::to_value(Limits::free())
        } else {
            serde_json::to_value(Limits {
                conductor: true,
                ..Limits::free()
            })
        }
        .expect("limits");
        let payload = serde_json::json!({
            "sub": "u1", "plan": plan, "status": "active", "limits": limits, "policy": policy,
            "iat": exp - 86_400, "exp": exp, "iss": "tandem-api", "aud": "tandem-desktop"
        });
        let payload = URL_SAFE_NO_PAD.encode(payload.to_string());
        let sig = key.sign(format!("{header}.{payload}").as_bytes());
        format!(
            "{header}.{payload}.{}",
            URL_SAFE_NO_PAD.encode(sig.to_bytes())
        )
    }

    fn me(tok: String, plan: Plan) -> MeResponse {
        MeResponse {
            user: MeUser {
                id: "u1".into(),
                email: "ana@tandem.test".into(),
                name: "Ana".into(),
            },
            plan,
            status: "active".into(),
            limits: Limits::free(),
            paid_until: None,
            cancel_at_period_end: false,
            credits: MeCredits {
                monthly: 10_000,
                purchased: 0,
                resets_at: None,
                cap: None,
                spent: None,
                grant: Some(10_000),
            },
            flags: Default::default(),
            policy: None,
            org: None,
            entitlement_token: tok,
        }
    }

    async fn signed_in(r: &Rig) {
        r.auth.sign_in().await.expect("start");
        let url = r
            .opener
            .opened
            .lock()
            .expect("lock")
            .last()
            .cloned()
            .unwrap_or_default();
        let state = url::Url::parse(&url)
            .expect("url")
            .query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        r.auth.handle_callback(Callback {
            code: "good-code".into(),
            state,
        });
        for _ in 0..100 {
            if r.auth.is_signed_in() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn service(r: &Rig, key: &SigningKey) -> Arc<EntitlementService> {
        let mut keys = PublicKeySet::default();
        keys.insert("ek-1", key.verifying_key());
        EntitlementService::new(
            r.auth.clone(),
            r.api.clone(),
            keys,
            r.data.clone(),
            Arc::new(r.clock.clone()),
            r.bus.clone(),
        )
    }

    #[tokio::test]
    async fn f_be_04_signed_out_is_free_and_a_token_unlocks_hosted_features() {
        let r = rig();
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let svc = service(&r, &key);
        assert_eq!(svc.view().plan, Plan::Free);
        // Signed out, everything local is unlimited; hosted features are off (D-2900).
        assert_eq!(svc.limits(), Limits::free());
        assert_eq!(svc.limits().workspaces, None);
        assert!(!svc.limits().conductor);

        signed_in(&r).await;
        let now = r.clock.now_ms() / 1000;
        *r.api.me.lock().expect("lock") = Some(me(token(&key, "pro", now + 86_400), Plan::Pro));
        let v = svc.refresh().await;
        assert_eq!((v.plan, v.source), (Plan::Pro, LimitSource::Token));
        assert!(v.limits.conductor);
        assert_eq!(v.credits.as_ref().map(|c| c.monthly), Some(10_000));
        assert_eq!(
            r.auth.state().email.as_deref(),
            Some("ana@tandem.test"),
            "/me names the account"
        );
    }

    #[tokio::test]
    async fn f_be_04_tampered_token_is_free_and_network_errors_keep_the_plan() {
        let r = rig();
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let svc = service(&r, &key);
        signed_in(&r).await;
        let now = r.clock.now_ms() / 1000;
        *r.api.me.lock().expect("lock") = Some(me(token(&key, "pro", now + 86_400), Plan::Pro));
        svc.refresh().await;

        // Offline: the cached Pro stays, and survives a relaunch.
        *r.api.offline.lock().expect("lock") = true;
        let v = svc.refresh().await;
        assert_eq!((v.plan, v.online), (Plan::Pro, false));
        let relaunched = service(&r, &key);
        assert_eq!(relaunched.view().plan, Plan::Pro);
        // Past expiry + 24 h grace: Free until it reconnects.
        r.clock.advance(Duration::from_secs(2 * 86_400 + 60));
        assert_eq!(svc.view().source, LimitSource::Free);

        // A forged token (other key) is Free.
        *r.api.offline.lock().expect("lock") = false;
        let forger = SigningKey::generate(&mut rand::rngs::OsRng);
        let now = r.clock.now_ms() / 1000;
        *r.api.me.lock().expect("lock") = Some(me(token(&forger, "pro", now + 86_400), Plan::Pro));
        let v = svc.refresh().await;
        assert_eq!((v.plan, v.source), (Plan::Free, LimitSource::Free));
    }

    #[tokio::test]
    async fn f_bl_06_1_the_policy_comes_from_the_token_and_survives_a_forged_cache() {
        let r = rig();
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let svc = service(&r, &key);
        signed_in(&r).await;
        let now = r.clock.now_ms() / 1000;
        let policy = serde_json::json!({ "maxAccess": "edits", "auditEnabled": true });
        let mut team = me(token_with(&key, "team", now + 86_400, policy), Plan::Team);
        // What /me says next to the token is never trusted for the policy.
        team.policy = Some(serde_json::json!({ "maxAccess": "full" }));
        *r.api.me.lock().expect("lock") = Some(team);
        let v = svc.refresh().await;
        let p = v.policy.expect("policy");
        assert_eq!(p.max_access, tandem_core::access::Access::Edits);
        assert!(p.audit_enabled);

        // Someone edits the cached token to drop the policy: it no longer
        // verifies, so the plan falls to Free, but the org's rules stay.
        let cached = std::fs::read_to_string(r.data.entitlement_cache()).expect("cache");
        let mut parts: Vec<&str> = cached.split('.').collect();
        let forged = URL_SAFE_NO_PAD.encode(br#"{"sub":"u1","plan":"pro","policy":null}"#);
        parts[1] = &forged;
        std::fs::write(r.data.entitlement_cache(), parts.join(".")).expect("write");
        let relaunched = service(&r, &key);
        let v = relaunched.view();
        assert_eq!(v.plan, Plan::Free);
        assert_eq!(
            v.policy.map(|p| p.max_access),
            Some(tandem_core::access::Access::Edits)
        );

        // A newer verified token without a policy (left the org) clears it.
        *r.api.me.lock().expect("lock") = Some(me(token(&key, "pro", now + 86_400), Plan::Pro));
        assert_eq!(svc.refresh().await.policy, None);
        assert!(!r.data.cache().join("policy.jws").exists());
    }

    #[test]
    fn f_bl_06_2_a_member_can_spend_their_cap_at_most() {
        let c = CreditsView {
            monthly: 20_000,
            purchased: 0,
            resets_at: None,
            cap: Some(1_000),
            spent: Some(700),
            grant: Some(1_000),
        };
        assert_eq!(c.available(), 300);
        let solo = CreditsView {
            cap: None,
            spent: None,
            ..c
        };
        assert_eq!(solo.available(), 20_000);
    }

    #[tokio::test]
    async fn f_tl_10_1_audit_events_leave_only_when_the_org_turned_audit_on() {
        let r = rig();
        let key = SigningKey::generate(&mut rand::rngs::OsRng);
        let svc = service(&r, &key);
        let audit = crate::services::audit::AuditService::new(
            r.auth.clone(),
            r.api.clone(),
            svc.clone(),
            Arc::new(r.clock.clone()),
        );
        signed_in(&r).await;
        let now = r.clock.now_ms() / 1000;
        // A Pro user (no org): nothing is even queued.
        *r.api.me.lock().expect("lock") = Some(me(token(&key, "pro", now + 86_400), Plan::Pro));
        svc.refresh().await;
        assert!(!audit.record("full_access_used", &[("engine", "claude")]));
        assert_eq!(audit.flush().await, 0);
        // A Team member whose org turned audit on.
        let on = serde_json::json!({ "auditEnabled": true });
        *r.api.me.lock().expect("lock") =
            Some(me(token_with(&key, "team", now + 86_400, on), Plan::Team));
        svc.refresh().await;
        assert!(audit.record(
            "full_access_used",
            &[("engine", "claude"), ("surface", "thread")]
        ));
        // Offline: kept for later.
        *r.api.offline.lock().expect("lock") = true;
        assert_eq!(audit.flush().await, 0);
        assert_eq!(audit.pending(), 1);
        *r.api.offline.lock().expect("lock") = false;
        assert_eq!(audit.flush().await, 1);
        let sent = r.api.audits.lock().expect("lock").clone();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].event, "full_access_used");
        assert_eq!(
            sent[0].meta.get("engine").and_then(Value::as_str),
            Some("claude")
        );
        // The API's schema: an RFC 3339 time in UTC (it 422s anything else).
        assert!(
            chrono::DateTime::parse_from_rfc3339(&sent[0].ts).is_ok() && sent[0].ts.ends_with('Z'),
            "{}",
            sent[0].ts
        );
    }
}
