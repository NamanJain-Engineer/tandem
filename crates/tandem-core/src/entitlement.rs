//! Client-side entitlement verification (F-BE-04, ARCHITECTURE §14.4).
//!
//! The API signs `{sub, plan, status, limits, policy, iat, exp}` as an EdDSA
//! JWS. The desktop verifies it with the bundled public keys and applies the
//! offline rule: a valid token counts until `exp`, then for a 24 h grace, then
//! the client falls back to Free limits. Only an explicit inactive status from
//! a fresh `/v1/me` downgrades; network errors never do.

use std::collections::HashMap;

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Offline grace past `exp`.
pub const GRACE_SECONDS: i64 = 24 * 3600;
pub const AUDIENCE: &str = "tandem-desktop";
pub const ISSUER: &str = "tandem-api";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Plan {
    Free,
    Pro,
    Team,
}

/// Mirrors `Limits` in `packages/protocol/src/plans.ts`. `None` = unlimited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Limits {
    pub panes: Option<u32>,
    pub workspaces: Option<u32>,
    pub thread_grid: u32,
    pub teammates: Option<u32>,
    pub routines: bool,
    pub background_routines: bool,
    pub task_execution: bool,
    pub plugin_connections: Option<u32>,
    pub accounts_per_engine: Option<u32>,
    pub cloud_stt: bool,
    pub enhance: bool,
    pub conductor: bool,
    pub auto: bool,
    pub phone: bool,
    pub admin: bool,
    pub chat_engines: Option<Vec<String>>,
    pub credit_packs: bool,
}

impl Limits {
    /// The Free matrix (PRODUCT_SPEC §3.2). Pinned to the same fixture as the
    /// TypeScript plans module.
    pub fn free() -> Self {
        Self {
            panes: Some(4),
            workspaces: Some(3),
            thread_grid: 1,
            teammates: Some(1),
            routines: false,
            background_routines: false,
            task_execution: false,
            plugin_connections: Some(1),
            accounts_per_engine: Some(1),
            cloud_stt: false,
            enhance: false,
            conductor: false,
            auto: false,
            phone: false,
            admin: false,
            chat_engines: Some(vec!["claude".to_string(), "codex".to_string()]),
            credit_packs: false,
        }
    }

    /// Whether Thread mode may run `engine_id` on this plan (`chat_engines`:
    /// `None` = every chat engine; Free lists Claude Code and Codex).
    pub fn allows_chat_engine(&self, engine_id: &str) -> bool {
        self.chat_engines
            .as_ref()
            .is_none_or(|allowed| allowed.iter().any(|e| e == engine_id))
    }
}

/// Why a Free thread can't use an ACP engine (PRODUCT_SPEC §3.2).
pub const CHAT_ENGINE_ON_PRO: &str =
    "Free threads run Claude Code and Codex. Pro runs every chat engine.";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EntitlementClaims {
    pub sub: String,
    pub plan: Plan,
    /// Subscription status as the server saw it (`active`, `past_due`, `none`…).
    pub status: String,
    pub limits: Limits,
    #[ts(type = "Record<string, unknown> | null")]
    pub policy: Option<serde_json::Value>,
    pub exp: i64,
    pub iat: i64,
    pub kid: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LimitSource {
    /// A valid, unexpired token.
    Token,
    /// Valid signature, past `exp` but within the 24 h grace.
    Grace,
    /// No usable token: Free limits.
    Free,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EntitlementError {
    #[error("malformed token")]
    Malformed,
    #[error("unknown signing key")]
    UnknownKey,
    #[error("signature doesn't verify")]
    BadSignature,
    #[error("wrong issuer or audience")]
    WrongAudience,
}

/// Public keys by `kid` (`ek-*` for entitlements, `ck-*` for config).
#[derive(Debug, Clone, Default)]
pub struct PublicKeySet {
    keys: HashMap<String, VerifyingKey>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BundledKeys {
    pub entitlement: HashMap<String, String>,
    pub config: HashMap<String, String>,
}

impl PublicKeySet {
    /// From `{kid: base64(raw 32-byte public key)}`.
    pub fn from_base64(map: &HashMap<String, String>) -> Self {
        let keys = map
            .iter()
            .filter_map(|(kid, b64)| {
                let raw = STANDARD.decode(b64).ok()?;
                let bytes: [u8; 32] = raw.try_into().ok()?;
                VerifyingKey::from_bytes(&bytes)
                    .ok()
                    .map(|k| (kid.clone(), k))
            })
            .collect();
        Self { keys }
    }

    pub fn insert(&mut self, kid: &str, key: VerifyingKey) {
        self.keys.insert(kid.to_string(), key);
    }

    pub fn get(&self, kid: &str) -> Option<&VerifyingKey> {
        self.keys.get(kid)
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// Parses `resources/keys.json`: `(entitlement keys, config keys)`.
pub fn bundled_keys(json: &str) -> Result<(PublicKeySet, PublicKeySet), serde_json::Error> {
    let bundled: BundledKeys = serde_json::from_str(json)?;
    Ok((
        PublicKeySet::from_base64(&bundled.entitlement),
        PublicKeySet::from_base64(&bundled.config),
    ))
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: String,
}

#[derive(Deserialize)]
struct Payload {
    sub: String,
    plan: Plan,
    status: String,
    limits: Limits,
    #[serde(default)]
    policy: Option<serde_json::Value>,
    exp: i64,
    iat: i64,
    iss: String,
    aud: serde_json::Value,
}

/// Verifies the signature, key id, issuer and audience. Time is not checked
/// here; [`effective_limits`] applies the expiry and grace rule.
pub fn verify_entitlement(
    jws: &str,
    keys: &PublicKeySet,
) -> Result<EntitlementClaims, EntitlementError> {
    let mut parts = jws.split('.');
    let (Some(h), Some(p), Some(s), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(EntitlementError::Malformed);
    };
    let header: Header = URL_SAFE_NO_PAD
        .decode(h)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or(EntitlementError::Malformed)?;
    if header.alg != "EdDSA" {
        return Err(EntitlementError::Malformed);
    }
    let key = keys.get(&header.kid).ok_or(EntitlementError::UnknownKey)?;
    let sig_bytes: [u8; 64] = URL_SAFE_NO_PAD
        .decode(s)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or(EntitlementError::Malformed)?;
    let signing_input = format!("{h}.{p}");
    key.verify_strict(signing_input.as_bytes(), &Signature::from_bytes(&sig_bytes))
        .map_err(|_| EntitlementError::BadSignature)?;
    let payload: Payload = URL_SAFE_NO_PAD
        .decode(p)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or(EntitlementError::Malformed)?;
    let aud_ok = match &payload.aud {
        serde_json::Value::String(a) => a == AUDIENCE,
        serde_json::Value::Array(list) => list.iter().any(|a| a.as_str() == Some(AUDIENCE)),
        _ => false,
    };
    if payload.iss != ISSUER || !aud_ok {
        return Err(EntitlementError::WrongAudience);
    }
    Ok(EntitlementClaims {
        sub: payload.sub,
        plan: payload.plan,
        status: payload.status,
        limits: payload.limits,
        policy: payload.policy,
        exp: payload.exp,
        iat: payload.iat,
        kid: header.kid,
    })
}

/// The grace rule (ARCHITECTURE §14.4). `now` is Unix seconds.
pub fn effective_limits(
    cached: Option<&EntitlementClaims>,
    now: i64,
) -> (Plan, Limits, LimitSource) {
    match cached {
        Some(c) if now < c.exp => (c.plan, c.limits.clone(), LimitSource::Token),
        Some(c) if now < c.exp + GRACE_SECONDS => (c.plan, c.limits.clone(), LimitSource::Grace),
        _ => (Plan::Free, Limits::free(), LimitSource::Free),
    }
}

/// What a background refresh produced.
#[derive(Debug, Clone)]
pub enum RefreshOutcome {
    /// A fresh `/v1/me` with its entitlement token.
    Fresh(String),
    /// Network down, timeout, 5xx: keep what we have.
    NetworkError,
    /// The server said the session is gone (401 after refresh failed).
    SignedOut,
}

/// Holds the current verified token; only explicit server answers change it.
#[derive(Debug, Clone, Default)]
pub struct EntitlementState {
    pub claims: Option<EntitlementClaims>,
}

impl EntitlementState {
    /// Applies a refresh result (F-BE-04 AC3). A fresh token that fails
    /// verification is treated as tampered and drops to Free (AC1).
    pub fn apply(&mut self, outcome: RefreshOutcome, keys: &PublicKeySet) {
        match outcome {
            RefreshOutcome::Fresh(token) => self.claims = verify_entitlement(&token, keys).ok(),
            RefreshOutcome::NetworkError => {}
            RefreshOutcome::SignedOut => self.claims = None,
        }
    }

    pub fn limits(&self, now: i64) -> (Plan, Limits, LimitSource) {
        effective_limits(self.claims.as_ref(), now)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    pub fn keypair() -> (SigningKey, PublicKeySet) {
        let signing = SigningKey::generate(&mut rand::rngs::OsRng);
        let mut set = PublicKeySet::default();
        set.insert("ek-1", signing.verifying_key());
        (signing, set)
    }

    pub fn token(signing: &SigningKey, plan: &str, exp: i64) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","kid":"ek-1","typ":"JWT"}"#);
        let limits = if plan == "free" {
            serde_json::to_value(Limits::free()).expect("limits")
        } else {
            let mut l = Limits::free();
            l.panes = Some(64);
            l.workspaces = None;
            l.routines = true;
            l.chat_engines = None;
            serde_json::to_value(l).expect("limits")
        };
        let payload = serde_json::json!({
            "sub": "u_1", "plan": plan, "status": "active", "limits": limits, "policy": null,
            "iat": exp - 86_400, "exp": exp, "iss": ISSUER, "aud": AUDIENCE,
        });
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).expect("json"));
        let sig = signing.sign(format!("{header}.{payload}").as_bytes());
        format!(
            "{header}.{payload}.{}",
            URL_SAFE_NO_PAD.encode(sig.to_bytes())
        )
    }

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn f_th_06_free_threads_run_only_claude_code_and_codex() {
        let free = Limits::free();
        assert!(free.allows_chat_engine("claude"));
        assert!(free.allows_chat_engine("codex"));
        for acp in ["cursor", "grok", "opencode"] {
            assert!(!free.allows_chat_engine(acp), "{acp} runs on Free");
        }
        let mut pro = Limits::free();
        pro.chat_engines = None;
        for any in ["claude", "codex", "cursor", "grok", "opencode"] {
            assert!(pro.allows_chat_engine(any), "{any} blocked on Pro");
        }
    }

    #[test]
    fn f_be_04_1_tampered_token_fails_and_client_uses_free_limits() {
        let (signing, keys) = keypair();
        let good = token(&signing, "pro", NOW + 3600);
        assert_eq!(
            verify_entitlement(&good, &keys).expect("valid").plan,
            Plan::Pro
        );

        // Flip the plan inside the payload without re-signing.
        let mut parts: Vec<String> = good.split('.').map(str::to_string).collect();
        let payload =
            String::from_utf8(URL_SAFE_NO_PAD.decode(&parts[1]).expect("b64")).expect("utf8");
        parts[1] = URL_SAFE_NO_PAD.encode(payload.replace("\"pro\"", "\"team\""));
        let tampered = parts.join(".");
        assert_eq!(
            verify_entitlement(&tampered, &keys),
            Err(EntitlementError::BadSignature)
        );

        let mut state = EntitlementState::default();
        state.apply(RefreshOutcome::Fresh(tampered), &keys);
        let (plan, limits, source) = state.limits(NOW);
        assert_eq!((plan, source), (Plan::Free, LimitSource::Free));
        assert_eq!(limits, Limits::free());

        let (other, _) = keypair();
        let foreign = token(&other, "pro", NOW + 3600);
        assert_eq!(
            verify_entitlement(&foreign, &keys),
            Err(EntitlementError::BadSignature)
        );
    }

    #[test]
    fn f_be_04_2_offline_grace_is_24_hours_past_exp() {
        let (signing, keys) = keypair();
        let exp = NOW;
        let claims = verify_entitlement(&token(&signing, "pro", exp), &keys).expect("valid");
        let at_23h = effective_limits(Some(&claims), exp + 23 * 3600);
        assert_eq!((at_23h.0, at_23h.2), (Plan::Pro, LimitSource::Grace));
        assert_eq!(at_23h.1.panes, Some(64));
        let at_25h = effective_limits(Some(&claims), exp + 25 * 3600);
        assert_eq!((at_25h.0, at_25h.2), (Plan::Free, LimitSource::Free));
        assert_eq!(
            effective_limits(Some(&claims), exp - 1).2,
            LimitSource::Token
        );
    }

    #[test]
    fn f_be_04_3_network_error_during_refresh_does_not_downgrade() {
        let (signing, keys) = keypair();
        let mut state = EntitlementState::default();
        state.apply(
            RefreshOutcome::Fresh(token(&signing, "pro", NOW + 3600)),
            &keys,
        );
        state.apply(RefreshOutcome::NetworkError, &keys);
        assert_eq!(state.limits(NOW).0, Plan::Pro);
        // An explicit answer from the server does change it.
        state.apply(
            RefreshOutcome::Fresh(token(&signing, "free", NOW + 3600)),
            &keys,
        );
        assert_eq!(state.limits(NOW).0, Plan::Free);
    }

    #[test]
    fn free_limits_match_the_shared_fixture() {
        let fixture = include_str!("../../../packages/protocol/fixtures/free-limits.json");
        let parsed: Limits = serde_json::from_str(fixture).expect("fixture");
        assert_eq!(parsed, Limits::free());
    }

    #[test]
    fn bundled_keys_json_parses() {
        let json = include_str!("../../../apps/desktop/src-tauri/resources/keys.json");
        let (ent, cfg) = bundled_keys(json).expect("keys.json");
        assert!(!ent.is_empty() && !cfg.is_empty());
    }
}

#[cfg(test)]
mod interop {
    use super::*;
    use crate::remote_config::{verify_config, SignedConfig};

    /// Signed by apps/api's own code; see the fixture's `note`.
    const FIXTURE: &str = include_str!("../tests/fixtures/api-signed.json");

    #[test]
    fn tokens_and_config_signed_by_the_api_verify_in_rust() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).expect("fixture");
        let (ent, cfg) = bundled_keys(&fixture["keys"].to_string()).expect("keys");
        let token = fixture["entitlement_token"].as_str().expect("token");
        let claims = verify_entitlement(token, &ent).expect("API token verifies");
        assert_eq!(
            (claims.plan, claims.sub.as_str(), claims.kid.as_str()),
            (Plan::Pro, "user_fixture", "ek-7")
        );
        assert_eq!(claims.limits.panes, Some(64));
        assert_eq!(claims.exp - claims.iat, GRACE_SECONDS);
        let envelope: SignedConfig =
            serde_json::from_value(fixture["config"].clone()).expect("envelope");
        let config = verify_config(&envelope, &cfg).expect("API config verifies");
        assert_eq!(config["prices"]["voice_10s"], 20);
    }
}
