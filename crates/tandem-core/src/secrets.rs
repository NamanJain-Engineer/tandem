use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::Error;

pub trait Keychain: Send + Sync {
    fn set(&self, service: &str, key: &str, value: &str) -> Result<(), Error>;
    fn get(&self, service: &str, key: &str) -> Result<Option<String>, Error>;
    fn delete(&self, service: &str, key: &str) -> Result<(), Error>;
}

#[derive(Default)]
pub struct MemoryKeychain {
    entries: Mutex<HashMap<(String, String), String>>,
}

impl MemoryKeychain {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Keychain for MemoryKeychain {
    fn set(&self, service: &str, key: &str, value: &str) -> Result<(), Error> {
        let mut entries = self.entries.lock().unwrap_or_else(|err| err.into_inner());
        entries.insert((service.to_string(), key.to_string()), value.to_string());
        Ok(())
    }

    fn get(&self, service: &str, key: &str) -> Result<Option<String>, Error> {
        let entries = self.entries.lock().unwrap_or_else(|err| err.into_inner());
        Ok(entries
            .get(&(service.to_string(), key.to_string()))
            .cloned())
    }

    fn delete(&self, service: &str, key: &str) -> Result<(), Error> {
        let mut entries = self.entries.lock().unwrap_or_else(|err| err.into_inner());
        entries.remove(&(service.to_string(), key.to_string()));
        Ok(())
    }
}

/// Tests and CI set `TANDEM_FAKE_KEYCHAIN=1`. Production will use the OS keychain.
pub fn open_keychain() -> Result<Box<dyn Keychain>, Error> {
    if std::env::var("TANDEM_FAKE_KEYCHAIN").ok().as_deref() == Some("1") {
        return Ok(Box::new(MemoryKeychain::new()));
    }
    Err(Error::new(
        "secrets",
        "OS keychain is not linked yet",
        "set TANDEM_FAKE_KEYCHAIN=1 for tests, or wait for the system keychain slice",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_round_trip() {
        let keys = MemoryKeychain::new();
        keys.set("tandem", "plugin", "secret-value").expect("set");
        assert_eq!(
            keys.get("tandem", "plugin").expect("get").as_deref(),
            Some("secret-value")
        );
        keys.delete("tandem", "plugin").expect("delete");
        assert_eq!(keys.get("tandem", "plugin").expect("get"), None);
    }
}
