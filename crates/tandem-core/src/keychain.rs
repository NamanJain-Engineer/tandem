//! The only door to secrets (DECISIONS rule 2). Production uses the OS keychain
//! through `keyring`; tests and E2E use [`MemoryKeychain`]
//! (`TANDEM_FAKE_KEYCHAIN=1`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

/// Largest secret Tandem stores (ARCHITECTURE §4.4).
pub const MAX_SECRET_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeychainError {
    #[error("Your system keyring is locked or missing")]
    Unavailable(String),
    #[error("secret is larger than 16 KB")]
    TooLarge,
    #[error("keychain error: {0}")]
    Backend(String),
}

pub trait Keychain: Send + Sync {
    fn get(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, KeychainError>;
    fn set(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), KeychainError>;
    fn delete(&self, service: &str, account: &str) -> Result<(), KeychainError>;
}

/// Keychain service namespace: `app.tandem` in release builds, `app.tandem.dev`
/// in debug builds, so a dev build never touches release secrets.
pub fn namespace() -> &'static str {
    if cfg!(debug_assertions) {
        "app.tandem.dev"
    } else {
        "app.tandem"
    }
}

pub fn service(suffix: &str) -> String {
    format!("{}.{suffix}", namespace())
}

type Entries = HashMap<(String, String), Zeroizing<Vec<u8>>>;

#[derive(Default, Clone)]
pub struct MemoryKeychain {
    entries: Arc<Mutex<Entries>>,
}

impl MemoryKeychain {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Keychain for MemoryKeychain {
    fn get(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, KeychainError> {
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        Ok(entries
            .get(&(service.to_string(), account.to_string()))
            .cloned())
    }

    fn set(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), KeychainError> {
        if secret.len() > MAX_SECRET_BYTES {
            return Err(KeychainError::TooLarge);
        }
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.insert(
            (service.to_string(), account.to_string()),
            Zeroizing::new(secret.to_vec()),
        );
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), KeychainError> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.remove(&(service.to_string(), account.to_string()));
        Ok(())
    }
}

/// The OS keychain: Keychain Services on macOS, Credential Manager on Windows,
/// Secret Service on Linux.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsKeychain;

fn map_err(err: keyring::Error) -> KeychainError {
    match err {
        keyring::Error::PlatformFailure(e) | keyring::Error::NoStorageAccess(e) => {
            KeychainError::Unavailable(e.to_string())
        }
        other => KeychainError::Backend(other.to_string()),
    }
}

impl Keychain for OsKeychain {
    fn get(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, KeychainError> {
        let entry = keyring::Entry::new(service, account).map_err(map_err)?;
        match entry.get_secret() {
            Ok(bytes) => Ok(Some(Zeroizing::new(bytes))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(map_err(err)),
        }
    }

    fn set(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), KeychainError> {
        if secret.len() > MAX_SECRET_BYTES {
            return Err(KeychainError::TooLarge);
        }
        keyring::Entry::new(service, account)
            .and_then(|entry| entry.set_secret(secret))
            .map_err(map_err)
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), KeychainError> {
        let entry = keyring::Entry::new(service, account).map_err(map_err)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(map_err(err)),
        }
    }
}

/// `TANDEM_FAKE_KEYCHAIN=1` selects the in-memory keychain; otherwise the OS one.
pub fn open_keychain() -> Arc<dyn Keychain> {
    if std::env::var("TANDEM_FAKE_KEYCHAIN").as_deref() == Ok("1") {
        Arc::new(MemoryKeychain::new())
    } else {
        Arc::new(OsKeychain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_keychain_round_trip_and_size_cap() {
        let keys = MemoryKeychain::new();
        let svc = service("plugins");
        keys.set(&svc, "github:pc_1", b"{\"api_key\":\"x\"}")
            .expect("set");
        let got = keys
            .get(&svc, "github:pc_1")
            .expect("get")
            .expect("present");
        assert_eq!(got.as_slice(), b"{\"api_key\":\"x\"}");
        keys.delete(&svc, "github:pc_1").expect("delete");
        assert!(keys.get(&svc, "github:pc_1").expect("get").is_none());
        let big = vec![0u8; MAX_SECRET_BYTES + 1];
        assert_eq!(keys.set(&svc, "big", &big), Err(KeychainError::TooLarge));
    }

    #[test]
    fn debug_builds_use_the_dev_namespace() {
        assert_eq!(service("auth"), "app.tandem.dev.auth");
    }
}
