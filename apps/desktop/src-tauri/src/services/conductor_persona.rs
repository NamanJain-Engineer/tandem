//! Conductor's persona and opt-in memory on the desktop (F-CN-06). The
//! Conductor call reads [`ConductorPersona::call_context`] once when it
//! starts; its voice tools ("remember that…", "what do you know about me",
//! "forget…") and post-call reflection come back through the other methods.
//! Settings › Conductor views, edits and clears memory through the
//! `conductor_memory_*` commands.

use std::collections::BTreeMap;

use serde_json::Value;

use tandem_core::conductor_persona::{
    CallContext, ConductorMemory, MemoryError, MemoryStore, Reflection,
};
use tandem_core::settings::SettingKey;
use tandem_core::{DataDir, ErrorCode, IpcError};
use tandem_store::repo::settings;
use tandem_store::Store;

pub struct ConductorPersona {
    store: Store,
    memory: MemoryStore,
}

fn ipc(e: MemoryError) -> IpcError {
    match e {
        MemoryError::Off => IpcError::new(
            ErrorCode::Unavailable,
            "Turn on \"Remember things between calls\" first.",
        ),
        MemoryError::Credential | MemoryError::Empty => {
            IpcError::new(ErrorCode::Invalid, e.to_string())
        }
        MemoryError::Io(m) => {
            tracing::warn!(error = %m, "conductor memory");
            IpcError::new(ErrorCode::Internal, "Couldn't update Conductor's memory.")
        }
    }
}

impl ConductorPersona {
    pub fn new(store: Store, data: &DataDir) -> Self {
        Self {
            store,
            memory: MemoryStore::new(data.conductor()),
        }
    }

    async fn values(&self) -> BTreeMap<SettingKey, Value> {
        settings::get_all(&self.store).await.unwrap_or_default()
    }

    async fn enabled(&self) -> bool {
        settings::get(&self.store, SettingKey::ConductorMemory)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// The one read a call makes when it starts: the persona and, only while
    /// memory is on, what's remembered (F-CN-06 seam for the Conductor call).
    pub async fn call_context(&self) -> CallContext {
        let values = self.values().await;
        CallContext::from_settings(|k| values.get(&k).cloned(), &self.memory)
    }

    pub async fn view(&self) -> Result<ConductorMemory, IpcError> {
        self.memory.view(self.enabled().await).map_err(ipc)
    }

    /// "Remember that …".
    pub async fn remember(&self, fact: &str) -> Result<(), IpcError> {
        self.memory
            .remember(self.enabled().await, fact)
            .map_err(ipc)
    }

    /// "What do you know about me": the remembered facts (none while off).
    pub async fn recall(&self) -> Vec<String> {
        if !self.enabled().await {
            return Vec::new();
        }
        self.memory.facts().unwrap_or_default()
    }

    /// "Forget …": how many facts went.
    pub async fn forget(&self, query: &str) -> Result<usize, IpcError> {
        self.memory.forget(query).map_err(ipc)
    }

    /// Settings' editor.
    pub async fn save(&self, text: &str) -> Result<ConductorMemory, IpcError> {
        let on = self.enabled().await;
        self.memory.save_text(on, text).map_err(ipc)?;
        self.memory.view(on).map_err(ipc)
    }

    /// "Forget everything…".
    pub async fn clear(&self) -> Result<ConductorMemory, IpcError> {
        self.memory.clear().map_err(ipc)?;
        self.view().await
    }

    /// Keep / Discard on proposal cards.
    pub async fn resolve(&self, ids: &[String], keep: bool) -> Result<ConductorMemory, IpcError> {
        let on = self.enabled().await;
        self.memory.resolve(on, ids, keep).map_err(ipc)?;
        self.memory.view(on).map_err(ipc)
    }

    /// Post-call reflection: the doer's proposed facts, handled per "After
    /// each call". Returns what was queued or saved.
    pub async fn reflect(&self, proposed: &[String]) -> Vec<String> {
        let values = self.values().await;
        let mode = values
            .get(&SettingKey::ConductorReflection)
            .cloned()
            .and_then(|v| serde_json::from_value::<Reflection>(v).ok())
            .unwrap_or_default();
        let on = values
            .get(&SettingKey::ConductorMemory)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        self.memory.reflect(on, mode, proposed).unwrap_or_default()
    }

    /// One line in today's call journal (only while memory is on).
    pub async fn journal(&self, date: &str, line: &str) {
        let on = self.enabled().await;
        if let Err(e) = self.memory.journal(on, date, line) {
            if !matches!(e, MemoryError::Off) {
                tracing::debug!(error = %e, "conductor journal line skipped");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn rig() -> (tempfile::TempDir, ConductorPersona, Store) {
        let dir = tempfile::tempdir().expect("test");
        let data = DataDir::at(dir.path().join("data")).expect("test");
        let store = Store::open(&data).expect("test");
        (dir, ConductorPersona::new(store.clone(), &data), store)
    }

    #[tokio::test]
    async fn f_cn_06_1_memory_off_nothing_written_after_a_call() {
        let (dir, p, _store) = rig().await;
        assert_eq!(
            p.remember("I like tea").await.expect_err("test").code,
            ErrorCode::Unavailable
        );
        assert!(p.reflect(&["Works on billing".into()]).await.is_empty());
        p.journal("2026-10-10", "Call: launched 2 agents").await;
        assert!(!dir.path().join("data/conductor/memory.md").exists());
        let ctx = p.call_context().await;
        assert!(!ctx.memory_enabled && ctx.memory.is_none());
    }

    #[tokio::test]
    async fn f_cn_06_2_token_refused_and_memory_flows_into_the_call() {
        let (_dir, p, store) = rig().await;
        settings::set(&store, SettingKey::ConductorMemory, json!(true))
            .await
            .expect("test");
        settings::set(&store, SettingKey::ConductorName, json!("Ada"))
            .await
            .expect("test");
        let said = format!(
            "Remember my token is sk-{}",
            "proj-Qdxidq7OyFjVisrM1h8vbvTXfQNNTGnNZlhyg74g"
        );
        let err = p.remember(&said).await.expect_err("test");
        assert_eq!(err.code, ErrorCode::Invalid);
        assert_eq!(
            err.message,
            "That looks like a secret, so Conductor won't remember it."
        );
        p.remember("Works on the billing service")
            .await
            .expect("test");
        assert_eq!(p.recall().await, vec!["Works on the billing service"]);
        let ctx = p.call_context().await;
        assert_eq!(ctx.persona.name, "Ada");
        assert!(ctx
            .instructions()
            .contains("- Works on the billing service"));
        let view = p.clear().await.expect("test");
        assert!(view.facts.is_empty() && view.text.is_empty());
    }
}
