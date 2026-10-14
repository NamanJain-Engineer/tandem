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
use tandem_voice::conductor::persona::{PersonaBrief, SPEEDS};

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

/// A call's [`PersonaBrief`] from the persona and memory (the F-CN-06 seam
/// the call reads once at start): the voice ("default" lets the provider
/// choose), the speed step (1–5 onto [`SPEEDS`]) and the instructions.
pub fn brief(ctx: &CallContext) -> PersonaBrief {
    let p = &ctx.persona;
    let step = usize::from(p.speed.clamp(1, SPEEDS.len() as u8)) - 1;
    PersonaBrief {
        voice: (p.voice != "default").then(|| p.voice.clone()),
        instructions: ctx.instructions(),
        speed: SPEEDS[step],
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

    pub async fn enabled(&self) -> bool {
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

    /// What a starting call gets: [`brief`] of [`Self::call_context`].
    pub async fn call_brief(&self) -> PersonaBrief {
        brief(&self.call_context().await)
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

    /// "Remember that …" said during a call: why it wasn't kept, if it
    /// wasn't. Neither the fact nor the error text is logged.
    pub async fn remember_in_call(
        &self,
        fact: &str,
    ) -> Result<(), tandem_voice::conductor::memory::RememberRefusal> {
        use tandem_voice::conductor::memory::RememberRefusal as R;
        self.memory
            .remember(self.enabled().await, fact)
            .map_err(|e| match e {
                MemoryError::Off => R::Off,
                MemoryError::Credential => R::Credential,
                MemoryError::Empty | MemoryError::Io(_) => R::Failed,
            })
    }

    /// After a connected call (F-CN-06): reflection per "After each call"
    /// on what the user said about themselves, and one journal line (when,
    /// how long, which tools; nothing that was said) unless "Keep call logs"
    /// is "Don't keep". Both only while memory is on.
    pub async fn after_call(
        &self,
        proposals: &[String],
        ended_at: i64,
        seconds: u32,
        tools: &[String],
    ) {
        if !self.enabled().await {
            return;
        }
        if !proposals.is_empty() {
            self.reflect(proposals).await;
        }
        // "Don't keep" call logs promises the call goes once it's settled, so
        // no journal line outlives it (D-846).
        let logs = settings::get(&self.store, SettingKey::ConductorCallLogs)
            .await
            .unwrap_or(Value::Null);
        if crate::services::conductor::keep_days(&logs).is_none() {
            return;
        }
        let at = chrono::DateTime::from_timestamp_millis(ended_at)
            .unwrap_or_default()
            .with_timezone(&chrono::Local);
        let line = tandem_voice::conductor::memory::journal_line(
            &at.format("%H:%M").to_string(),
            seconds,
            tools,
        );
        self.journal(&at.format("%Y-%m-%d").to_string(), &line)
            .await;
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
    async fn f_cn_06_after_a_call_reflection_follows_the_setting_and_the_journal_keeps_no_words() {
        use tandem_voice::conductor::memory::RememberRefusal;
        let (dir, p, store) = rig().await;
        let conductor = dir.path().join("data/conductor");
        let proposals = vec!["I prefer pnpm over npm".to_string()];
        let tools = vec!["launch_agents".to_string(), "read_pane".to_string()];
        // Memory off: nothing at all, not even the journal.
        p.after_call(&proposals, 1_791_000_000_000, 65, &tools)
            .await;
        assert!(!conductor.exists());
        assert_eq!(
            p.remember_in_call("I deploy on Fridays").await,
            Err(RememberRefusal::Off)
        );

        settings::set(&store, SettingKey::ConductorMemory, json!(true))
            .await
            .expect("test");
        // "Ask me about new memories" (the default): Keep / Discard cards.
        p.after_call(&proposals, 1_791_000_000_000, 65, &tools)
            .await;
        let view = p.view().await.expect("view");
        assert_eq!(
            view.proposals
                .iter()
                .map(|c| c.text.clone())
                .collect::<Vec<_>>(),
            proposals
        );
        assert!(view.facts.is_empty());
        // The journal: when, how long, which tools; no words from the call.
        let journal: Vec<_> = std::fs::read_dir(conductor.join("journal"))
            .expect("journal")
            .collect();
        assert_eq!(journal.len(), 1);
        let text =
            std::fs::read_to_string(journal[0].as_ref().expect("entry").path()).expect("read");
        assert!(
            text.contains("call, 2 min, 2 actions: launch_agents, read_pane"),
            "{text}"
        );
        assert!(!text.contains("pnpm"));

        // "Save automatically".
        settings::set(&store, SettingKey::ConductorReflection, json!("auto"))
            .await
            .expect("test");
        p.after_call(
            &["I usually review in the morning".to_string()],
            1_791_000_100_000,
            30,
            &[],
        )
        .await;
        assert!(p
            .recall()
            .await
            .contains(&"I usually review in the morning".to_string()));
        // "Don't reflect": nothing proposed or saved.
        settings::set(&store, SettingKey::ConductorReflection, json!("off"))
            .await
            .expect("test");
        p.after_call(
            &["I never use yarn".to_string()],
            1_791_000_200_000,
            30,
            &[],
        )
        .await;
        assert!(!p.recall().await.contains(&"I never use yarn".to_string()));
        // In a call, "remember that…" saves; a secret is refused.
        assert_eq!(p.remember_in_call("I deploy on Fridays").await, Ok(()));
        assert_eq!(
            p.remember_in_call("my password is hunter22").await,
            Err(RememberRefusal::Credential)
        );
        assert!(p
            .recall()
            .await
            .contains(&"I deploy on Fridays".to_string()));
    }

    #[tokio::test]
    async fn f_cn_06_dont_keep_call_logs_leaves_no_journal_line_and_reflection_follows_its_own_setting(
    ) {
        let (dir, p, store) = rig().await;
        let journal = dir.path().join("data/conductor/journal");
        settings::set(&store, SettingKey::ConductorMemory, json!(true))
            .await
            .expect("test");
        settings::set(&store, SettingKey::ConductorCallLogs, json!("off"))
            .await
            .expect("test");
        let proposals = vec!["I usually review in the morning".to_string()];
        p.after_call(
            &proposals,
            1_791_000_000_000,
            65,
            &["read_pane".to_string()],
        )
        .await;
        // "Don't keep" promises the call goes once settled: no line outlives it.
        assert!(!journal.exists());
        // "After each call" decides reflection on its own ("Ask me", the default).
        let view = p.view().await.expect("view");
        assert_eq!(
            view.proposals
                .iter()
                .map(|c| c.text.clone())
                .collect::<Vec<_>>(),
            proposals
        );
        // Keeping logs again brings the journal back.
        settings::set(&store, SettingKey::ConductorCallLogs, json!("7"))
            .await
            .expect("test");
        p.after_call(&[], 1_791_000_100_000, 30, &[]).await;
        assert_eq!(std::fs::read_dir(&journal).expect("journal").count(), 1);
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

    #[tokio::test]
    async fn f_cn_06_a_call_reads_voice_speed_and_instructions_from_the_persona() {
        let (_dir, p, store) = rig().await;
        // Defaults: the provider's voice, normal speed, the default name.
        let b = p.call_brief().await;
        assert_eq!((b.voice.clone(), b.speed), (None, 1.0));
        assert!(b.instructions.contains("Conductor"), "{}", b.instructions);

        for (k, v) in [
            (SettingKey::ConductorName, json!("Ada")),
            (SettingKey::ConductorVoice, json!("sage")),
            (SettingKey::ConductorSpeed, json!(5)),
            (SettingKey::ConductorMemory, json!(true)),
        ] {
            settings::set(&store, k, v).await.expect("test");
        }
        p.remember("Works on the billing service")
            .await
            .expect("test");
        let b = p.call_brief().await;
        assert_eq!(b.voice.as_deref(), Some("sage"));
        assert_eq!(b.speed, 1.2);
        assert!(b.instructions.contains("Ada"), "{}", b.instructions);
        assert!(b.instructions.contains("- Works on the billing service"));

        // Memory off again: nothing remembered reaches the call.
        settings::set(&store, SettingKey::ConductorMemory, json!(false))
            .await
            .expect("test");
        assert!(!p
            .call_brief()
            .await
            .instructions
            .contains("billing service"));
    }
}
