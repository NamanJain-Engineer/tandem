//! Signed remote config (F-BE-09, ARCHITECTURE §14.10). `GET /v1/config`
//! returns `{payload, sig, kid}` where `payload` is canonical JSON text and
//! `sig` is an Ed25519 signature over its UTF-8 bytes. Anything that doesn't
//! verify is ignored and the bundled copy is used.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::entitlement::PublicKeySet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedConfig {
    pub payload: String,
    pub sig: String,
    pub kid: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ConfigSource {
    Remote,
    Bundled,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("unknown config key")]
    UnknownKey,
    #[error("config signature doesn't verify")]
    BadSignature,
    #[error("config isn't valid JSON")]
    Malformed,
}

pub fn verify_config(envelope: &SignedConfig, keys: &PublicKeySet) -> Result<Value, ConfigError> {
    let key = keys.get(&envelope.kid).ok_or(ConfigError::UnknownKey)?;
    let sig: [u8; 64] = STANDARD
        .decode(&envelope.sig)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or(ConfigError::BadSignature)?;
    key.verify_strict(envelope.payload.as_bytes(), &Signature::from_bytes(&sig))
        .map_err(|_| ConfigError::BadSignature)?;
    serde_json::from_str(&envelope.payload).map_err(|_| ConfigError::Malformed)
}

/// The config to use: the fetched one if it verifies, otherwise the bundled copy.
pub fn resolve(
    fetched: Option<&SignedConfig>,
    keys: &PublicKeySet,
    bundled: &str,
) -> (Value, ConfigSource) {
    if let Some(envelope) = fetched {
        match verify_config(envelope, keys) {
            Ok(value) => return (value, ConfigSource::Remote),
            Err(err) => tracing::warn!(error = %err, "remote config rejected; using bundled copy"),
        }
    }
    (
        serde_json::from_str(bundled).unwrap_or(Value::Object(Default::default())),
        ConfigSource::Bundled,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    const BUNDLED: &str = include_str!("../../../apps/desktop/src-tauri/resources/config.json");

    fn signed(signing: &SigningKey, payload: &str) -> SignedConfig {
        SignedConfig {
            payload: payload.to_string(),
            sig: STANDARD.encode(signing.sign(payload.as_bytes()).to_bytes()),
            kid: "ck-1".into(),
        }
    }

    #[test]
    fn f_be_09_1_tampered_config_is_rejected_and_bundled_copy_used() {
        let signing = SigningKey::generate(&mut rand::rngs::OsRng);
        let mut keys = PublicKeySet::default();
        keys.insert("ck-1", signing.verifying_key());

        let remote = r#"{"min_app_version":"0.2.0","prices":{"rewrite":4}}"#;
        let good = signed(&signing, remote);
        let (value, source) = resolve(Some(&good), &keys, BUNDLED);
        assert_eq!(source, ConfigSource::Remote);
        assert_eq!(value["prices"]["rewrite"], 4);

        let mut tampered = good.clone();
        tampered.payload = tampered.payload.replace("4", "0");
        let (value, source) = resolve(Some(&tampered), &keys, BUNDLED);
        assert_eq!(source, ConfigSource::Bundled);
        assert_eq!(value["prices"]["rewrite"], 3);

        let mut wrong_kid = good;
        wrong_kid.kid = "ck-9".into();
        assert_eq!(
            verify_config(&wrong_kid, &keys),
            Err(ConfigError::UnknownKey)
        );
        assert_eq!(resolve(None, &keys, BUNDLED).1, ConfigSource::Bundled);
    }

    #[test]
    fn bundled_config_matches_the_api_source() {
        let api = include_str!("../../../apps/api/config/remote-config.json");
        let a: Value = serde_json::from_str(api).expect("api config");
        let b: Value = serde_json::from_str(BUNDLED).expect("bundled config");
        assert_eq!(
            a, b,
            "copy apps/api/config/remote-config.json to resources/config.json"
        );
    }
}
