//! Plugin credentials (ARCHITECTURE §4.4, F-PL-04): only ever in the OS
//! keychain, service `<NS>.plugins` (`app.tandem.plugins` in release),
//! account `<plugin>:<connection-id>`, as JSON of at most 16 KB. Every
//! secret read or written is registered with the shared redactor, so it is
//! redacted from logs and notifications from then on (F-TL-02's filter of
//! the user's credentials).

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tandem_core::ids::ConnectionId;
use tandem_core::keychain::{self, Keychain, KeychainError, MAX_SECRET_BYTES};
use zeroize::{Zeroize, Zeroizing};

/// What a connection signs in with. Never logged (the Debug impl hides it).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Credential {
    OAuth {
        access_token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh_token: Option<String>,
        /// Unix ms.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_at: Option<i64>,
        #[serde(default = "bearer")]
        token_type: String,
        /// A confidential client's secret (RFC 7591 fallback).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_secret: Option<String>,
    },
    ApiKey {
        api_key: String,
    },
}

fn bearer() -> String {
    "Bearer".into()
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Credential::OAuth { expires_at, .. } => f
                .debug_struct("OAuth")
                .field("expires_at", expires_at)
                .finish_non_exhaustive(),
            Credential::ApiKey { .. } => f.write_str("ApiKey(<redacted>)"),
        }
    }
}

impl Drop for Credential {
    fn drop(&mut self) {
        match self {
            Credential::OAuth {
                access_token,
                refresh_token,
                client_secret,
                ..
            } => {
                access_token.zeroize();
                if let Some(r) = refresh_token {
                    r.zeroize();
                }
                if let Some(s) = client_secret {
                    s.zeroize();
                }
            }
            Credential::ApiKey { api_key } => api_key.zeroize(),
        }
    }
}

impl Credential {
    /// The value sent as `Authorization: Bearer …`.
    pub fn bearer(&self) -> &str {
        match self {
            Credential::OAuth { access_token, .. } => access_token,
            Credential::ApiKey { api_key } => api_key,
        }
    }

    /// Every secret string in it (for redaction).
    pub fn secrets(&self) -> Vec<&str> {
        match self {
            Credential::OAuth {
                access_token,
                refresh_token,
                client_secret,
                ..
            } => {
                let mut v = vec![access_token.as_str()];
                v.extend(refresh_token.as_deref());
                v.extend(client_secret.as_deref());
                v
            }
            Credential::ApiKey { api_key } => vec![api_key.as_str()],
        }
    }

    /// Whether an OAuth token is within `skew_ms` of expiring.
    pub fn expiring(&self, now_ms: i64, skew_ms: i64) -> bool {
        matches!(self, Credential::OAuth { expires_at: Some(at), .. } if *at - skew_ms <= now_ms)
    }

    pub fn refresh_token(&self) -> Option<&str> {
        match self {
            Credential::OAuth { refresh_token, .. } => refresh_token.as_deref(),
            Credential::ApiKey { .. } => None,
        }
    }

    pub fn client_secret(&self) -> Option<&str> {
        match self {
            Credential::OAuth { client_secret, .. } => client_secret.as_deref(),
            Credential::ApiKey { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    #[error("{0}")]
    Keychain(#[from] KeychainError),
    #[error("The credential is larger than 16 KB")]
    TooLarge,
    #[error("The saved credential can't be read")]
    Corrupt,
}

/// The keychain account of a connection.
pub fn account(plugin_id: &str, connection: &ConnectionId) -> String {
    format!("{plugin_id}:{}", connection.as_str())
}

/// The keychain service for plugin credentials.
pub fn service() -> String {
    keychain::service("plugins")
}

#[derive(Clone)]
pub struct CredentialStore {
    keychain: Arc<dyn Keychain>,
    /// Single-flight refresh per connection (ARCHITECTURE §11.5).
    refreshing: Arc<std::sync::Mutex<HashMap<ConnectionId, Arc<tokio::sync::Mutex<()>>>>>,
}

impl CredentialStore {
    pub fn new(keychain: Arc<dyn Keychain>) -> Self {
        Self {
            keychain,
            refreshing: Arc::default(),
        }
    }

    pub fn keychain(&self) -> &Arc<dyn Keychain> {
        &self.keychain
    }

    fn register(cred: &Credential) {
        for s in cred.secrets() {
            if s.len() >= 8 {
                tandem_core::redact::Redactor::global().register_secret(s);
            }
        }
    }

    /// Saves a credential. Over 16 KB is refused before the keychain sees it.
    pub fn save(
        &self,
        plugin_id: &str,
        connection: &ConnectionId,
        cred: &Credential,
    ) -> Result<(), CredentialError> {
        let json = Zeroizing::new(serde_json::to_vec(cred).map_err(|_| CredentialError::Corrupt)?);
        if json.len() > MAX_SECRET_BYTES {
            return Err(CredentialError::TooLarge);
        }
        Self::register(cred);
        self.keychain
            .set(&service(), &account(plugin_id, connection), &json)
            .map_err(|e| match e {
                KeychainError::TooLarge => CredentialError::TooLarge,
                other => CredentialError::Keychain(other),
            })
    }

    pub fn load(
        &self,
        plugin_id: &str,
        connection: &ConnectionId,
    ) -> Result<Option<Credential>, CredentialError> {
        let Some(bytes) = self
            .keychain
            .get(&service(), &account(plugin_id, connection))?
        else {
            return Ok(None);
        };
        let cred: Credential =
            serde_json::from_slice(&bytes).map_err(|_| CredentialError::Corrupt)?;
        Self::register(&cred);
        Ok(Some(cred))
    }

    pub fn delete(
        &self,
        plugin_id: &str,
        connection: &ConnectionId,
    ) -> Result<(), CredentialError> {
        self.keychain
            .delete(&service(), &account(plugin_id, connection))?;
        Ok(())
    }

    /// The lock a refresh of `connection` holds, so only one runs at a time.
    pub fn refresh_lock(&self, connection: &ConnectionId) -> Arc<tokio::sync::Mutex<()>> {
        let mut map = self.refreshing.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(connection.clone()).or_default().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::keychain::MemoryKeychain;

    #[test]
    fn f_pl_04_credentials_live_under_the_plugins_service_and_over_16_kb_is_refused() {
        let mem = MemoryKeychain::new();
        let store = CredentialStore::new(Arc::new(mem.clone()));
        let conn = ConnectionId::new();
        let cred = Credential::ApiKey {
            api_key: "lin_api_0123456789abcdef".into(),
        };
        store.save("linear", &conn, &cred).expect("save");
        let raw = mem
            .get(
                "app.tandem.dev.plugins",
                &format!("linear:{}", conn.as_str()),
            )
            .expect("get")
            .expect("present");
        assert!(String::from_utf8_lossy(&raw).contains("api_key"));
        assert_eq!(store.load("linear", &conn).expect("load"), Some(cred));
        let big = Credential::ApiKey {
            api_key: "x".repeat(MAX_SECRET_BYTES),
        };
        assert_eq!(
            store.save("linear", &conn, &big),
            Err(CredentialError::TooLarge)
        );
        store.delete("linear", &conn).expect("delete");
        assert_eq!(store.load("linear", &conn).expect("load"), None);
        assert!(!format!(
            "{:?}",
            Credential::ApiKey {
                api_key: "k-123456789".into()
            }
        )
        .contains("k-123456789"));
    }

    #[test]
    fn f_tl_02_saved_credentials_are_redacted_everywhere() {
        let store = CredentialStore::new(Arc::new(MemoryKeychain::new()));
        let token = "plainplugintoken-77441";
        store
            .save(
                "github",
                &ConnectionId::new(),
                &Credential::OAuth {
                    access_token: token.into(),
                    refresh_token: None,
                    expires_at: None,
                    token_type: "Bearer".into(),
                    client_secret: None,
                },
            )
            .expect("save");
        let out = tandem_core::redact::redact(&format!("calling with {token} now"));
        assert!(!out.contains(token), "{out}");
    }
}
