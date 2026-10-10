//! Learn after a turn (F-AG-08, ARCHITECTURE §8.4). When a teammate chat's
//! turn completes, and both "Learn after each turn" (Settings › Teammates)
//! and the teammate's own Learn mode allow it, a headless follow-up runs on
//! the chat's engine and account, on the engine's cheapest model, with no
//! MCP servers and no plugins, and suggests memory changes.
//! - Ask: a "Memory suggestions" card in the chat (`memory:suggestions`);
//!   Keep applies the kept ones.
//! - Auto: applied at once, with a "Memory updated" note and Undo.
//! - Off: nothing runs.
//!
//! Unattended turns (mail) learn only in Auto. It uses no Vibezeo credits.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::{TeammateId, ThreadId, TurnId};
use tandem_core::settings::SettingKey;
use tandem_core::teammates::LearnMode;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::events::TurnEndReason;
use tandem_engines::learn::{self, Suggestion};
use tandem_store::memory::MemoryFile;
use tandem_store::repo::threads::ThreadRecord;
use tandem_store::Store;
use ts_rs::TS;

use crate::services::teammates::TeammatesService;

/// How long the follow-up may take.
pub const LEARN_WAIT: Duration = Duration::from_secs(90);

/// `memory:suggestions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MemorySuggestions {
    pub teammate_id: TeammateId,
    pub thread_id: ThreadId,
    pub set_id: String,
    pub items: Vec<Suggestion>,
    pub rationale: String,
    /// Auto mode: already applied; `change_id` undoes it.
    pub applied: bool,
    pub change_id: Option<String>,
}

/// Runs the follow-up for a chat: its engine headless, no MCP servers.
#[async_trait]
pub trait LearnRunner: Send + Sync {
    async fn run(&self, chat: &ThreadRecord, prompt: &str) -> Option<String>;
}

struct Pending {
    teammate: TeammateId,
    items: Vec<Suggestion>,
}

pub struct LearnService {
    store: Store,
    bus: EventBus,
    teammates: Arc<TeammatesService>,
    runner: Arc<dyn LearnRunner>,
    pending: Mutex<HashMap<String, Pending>>,
}

impl LearnService {
    pub fn new(
        store: Store,
        bus: EventBus,
        teammates: Arc<TeammatesService>,
        runner: Arc<dyn LearnRunner>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            bus,
            teammates,
            runner,
            pending: Mutex::default(),
        })
    }

    async fn global_on(&self) -> bool {
        tandem_store::repo::settings::get(&self.store, SettingKey::LearnAfterTurn)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    }

    /// Whether a finished turn gets a follow-up, and in which mode.
    pub async fn mode_for(&self, chat: &ThreadRecord, reason: TurnEndReason) -> Option<LearnMode> {
        if reason != TurnEndReason::Completed {
            return None;
        }
        let teammate = self.teammates.get(chat.teammate_id.as_ref()?).await.ok()?;
        let mode = teammate.settings.learn_mode;
        if mode == LearnMode::Off || !self.global_on().await {
            return None;
        }
        // Mail runs nobody watches: only Auto learns from them.
        if !chat.attended && mode != LearnMode::Auto {
            return None;
        }
        Some(mode)
    }

    /// The turn's user message and the assistant's text.
    async fn turn_text(&self, chat: &ThreadId, turn: &TurnId) -> (String, String) {
        let rows = tandem_store::repo::threads::all_messages(&self.store, chat)
            .await
            .unwrap_or_default();
        let (mut user, mut assistant) = (String::new(), Vec::new());
        for r in rows.iter().filter(|r| &r.turn_id == turn) {
            let v: serde_json::Value = serde_json::from_str(&r.payload_json).unwrap_or_default();
            match r.kind.as_str() {
                "user" => user = v["text"].as_str().unwrap_or("").to_string(),
                "assistant" => assistant.push(v["markdown"].as_str().unwrap_or("").to_string()),
                _ => {}
            }
        }
        (user, assistant.join("\n\n"))
    }

    /// After a teammate chat's turn (F-AG-08): run the follow-up if the
    /// settings allow it.
    pub async fn after_turn(&self, chat: ThreadRecord, turn: TurnId, reason: TurnEndReason) {
        let Some(mode) = self.mode_for(&chat, reason).await else {
            return;
        };
        let Some(teammate_id) = chat.teammate_id.clone() else {
            return;
        };
        let Ok(teammate) = self.teammates.get(&teammate_id).await else {
            return;
        };
        let (user, assistant) = self.turn_text(&chat.id, &turn).await;
        let Ok(memory) = self.teammates.memory(&teammate_id).await else {
            return;
        };
        let prompt = learn::prompt(&teammate.name, &user, &assistant, &memory.notes);
        let Some(out) = self.runner.run(&chat, &prompt).await else {
            return;
        };
        let Some(learned) = learn::parse(&out) else {
            tracing::info!(
                code = "learn.unparsed",
                "the learn follow-up's answer wasn't JSON"
            );
            return;
        };
        if learned.items.is_empty() {
            return;
        }
        let set_id = format!("ms_{}", uuid::Uuid::new_v4().simple());
        let mut event = MemorySuggestions {
            teammate_id: teammate_id.clone(),
            thread_id: chat.id.clone(),
            set_id: set_id.clone(),
            items: learned.items.clone(),
            rationale: learned.rationale,
            applied: false,
            change_id: None,
        };
        match mode {
            LearnMode::Auto => {
                let next = learn::apply(&memory.notes, &learned.items);
                match self
                    .teammates
                    .write_memory(&teammate, MemoryFile::Notes, &next)
                    .await
                {
                    Ok(change) => {
                        event.applied = true;
                        event.change_id = Some(change);
                    }
                    Err(e) => {
                        tracing::info!(code = "learn.refused", error = %e.message, "learned notes didn't fit");
                        return;
                    }
                }
            }
            _ => {
                if let Ok(mut p) = self.pending.lock() {
                    p.insert(
                        set_id,
                        Pending {
                            teammate: teammate_id,
                            items: learned.items,
                        },
                    );
                }
            }
        }
        self.bus
            .publish(BusEvent::new(Topic::Threads, "memory:suggestions", event));
    }

    /// `memory_suggestions_resolve`: applies the kept items (Ask mode); an
    /// empty `keep` discards the set.
    pub async fn resolve(&self, set_id: &str, keep: &[u32]) -> Result<Option<String>, IpcError> {
        let pending = self
            .pending
            .lock()
            .ok()
            .and_then(|mut p| p.remove(set_id))
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "These suggestions are gone"))?;
        let kept: Vec<Suggestion> = keep
            .iter()
            .filter_map(|i| pending.items.get(*i as usize).cloned())
            .collect();
        if kept.is_empty() {
            return Ok(None);
        }
        let teammate = self.teammates.get(&pending.teammate).await?;
        let memory = self.teammates.memory(&pending.teammate).await?;
        let next = learn::apply(&memory.notes, &kept);
        let change = self
            .teammates
            .write_memory(&teammate, MemoryFile::Notes, &next)
            .await?;
        Ok(Some(change))
    }
}

/// The app's runner: the chat's engine and account, headless.
pub struct AppLearnRunner {
    pub terminals: Arc<crate::services::terminal::TerminalService>,
}

#[async_trait]
impl LearnRunner for AppLearnRunner {
    async fn run(&self, chat: &ThreadRecord, prompt: &str) -> Option<String> {
        let engines = &self.terminals.engines;
        let engine = engines.get(&chat.engine_id)?;
        // Engines without a headless mode (ACP agents) don't learn.
        let args = tandem_engines::handoff::headless_args(&engine)?;
        let program = engines.installed(&chat.engine_id)?;
        let cwd = std::path::PathBuf::from(&chat.cwd);
        let launch = engines
            .agent_launch(
                &engine,
                program.clone(),
                &cwd,
                None,
                chat.account_id.as_ref(),
            )
            .await
            .ok()?;
        let login = self.terminals.login_env().await;
        let mut env: Vec<(String, String)> = login
            .vars
            .iter()
            .chain(launch.spec.env.iter())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        env.extend(crate::services::fake_engine_env());
        match tandem_engines::handoff::run_headless(&program, &args, &env, &cwd, prompt, LEARN_WAIT)
            .await
        {
            Ok(out) => Some(out),
            Err(e) => {
                tracing::warn!(code = "learn.failed", error = %e, "the learn follow-up failed");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::teammates::tests_support::{input, service};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tandem_core::access::{Access, ThreadMode};
    use tandem_core::teammates::TeammateSettings;
    use tandem_store::repo::threads::{self, ChatExtra, NewThread, ThreadKind};

    struct Scripted {
        runs: AtomicUsize,
        reply: String,
    }

    #[async_trait]
    impl LearnRunner for Scripted {
        async fn run(&self, _chat: &ThreadRecord, prompt: &str) -> Option<String> {
            assert!(prompt.contains("<user>use pnpm</user>"));
            self.runs.fetch_add(1, Ordering::SeqCst);
            Some(self.reply.clone())
        }
    }

    async fn setup(
        mode: LearnMode,
        attended: bool,
    ) -> (
        tempfile::TempDir,
        Arc<LearnService>,
        Arc<Scripted>,
        ThreadRecord,
        TurnId,
        Arc<TeammatesService>,
    ) {
        let (dir, svc) = service(None).await;
        let mut i = input("Scout", "claude");
        i.settings = Some(TeammateSettings {
            learn_mode: mode,
            ..TeammateSettings::default()
        });
        let scout = svc.create(i).await.expect("scout");
        let store = svc.store().clone();
        let chat = threads::insert_chat(
            &store,
            NewThread {
                id: ThreadId::new(),
                kind: ThreadKind::TeammateChat,
                workspace_id: None,
                cwd: scout.home.clone(),
                engine_id: "claude".into(),
                account_id: None,
                model: None,
                effort: None,
                access: Access::Edits,
                mode: ThreadMode::Act,
                attended,
            },
            ChatExtra {
                teammate_id: Some(scout.id.clone()),
                ..Default::default()
            },
        )
        .await
        .expect("chat");
        let turn = TurnId::new();
        threads::start_turn(&store, &chat.id, &turn, "claude", None)
            .await
            .expect("turn");
        threads::append(
            &store,
            &chat.id,
            &turn,
            0,
            "user",
            serde_json::json!({"type": "user", "text": "use pnpm", "attachments": []}).to_string(),
        )
        .await
        .expect("user");
        threads::append(
            &store,
            &chat.id,
            &turn,
            1,
            "assistant",
            serde_json::json!({"type": "assistant", "markdown": "Okay."}).to_string(),
        )
        .await
        .expect("reply");
        let runner = Arc::new(Scripted {
            runs: AtomicUsize::new(0),
            reply: "{\"add\": [\"Prefers pnpm.\"], \"remove\": [], \"rationale\": \"Said so.\"}"
                .into(),
        });
        let learn = LearnService::new(store, EventBus::new(), svc.clone(), runner.clone());
        (dir, learn, runner, chat, turn, svc)
    }

    #[tokio::test]
    async fn f_ag_08_1_ask_suggests_and_keep_appends_to_notes() {
        let (_d, learn, runner, chat, turn, svc) = setup(LearnMode::Ask, true).await;
        let mut events = learn.bus.subscribe(Topic::Threads);
        learn
            .after_turn(chat.clone(), turn, TurnEndReason::Completed)
            .await;
        assert_eq!(runner.runs.load(Ordering::SeqCst), 1);
        let ev = events.recv().await.expect("event");
        let set: MemorySuggestions = serde_json::from_value(ev.payload).expect("payload");
        assert!(!set.applied);
        let tm = chat.teammate_id.clone().expect("teammate");
        assert_eq!(
            svc.memory(&tm).await.expect("m").notes,
            "",
            "nothing until Keep"
        );
        learn.resolve(&set.set_id, &[0]).await.expect("keep");
        assert_eq!(svc.memory(&tm).await.expect("m").notes, "- Prefers pnpm.\n");
        assert!(learn.resolve(&set.set_id, &[0]).await.is_err(), "once");
    }

    #[tokio::test]
    async fn f_ag_08_auto_applies_with_undo() {
        let (_d, learn, _runner, chat, turn, svc) = setup(LearnMode::Auto, false).await;
        let mut events = learn.bus.subscribe(Topic::Threads);
        learn
            .after_turn(chat.clone(), turn, TurnEndReason::Completed)
            .await;
        let ev = events.recv().await.expect("event");
        let set: MemorySuggestions = serde_json::from_value(ev.payload).expect("payload");
        assert!(set.applied);
        let tm = chat.teammate_id.clone().expect("teammate");
        assert_eq!(svc.memory(&tm).await.expect("m").notes, "- Prefers pnpm.\n");
        svc.undo_memory(set.change_id.as_deref().expect("change"))
            .await
            .expect("undo");
        assert_eq!(svc.memory(&tm).await.expect("m").notes, "");
    }

    #[tokio::test]
    async fn f_ag_08_3_off_unattended_ask_and_unfinished_turns_run_nothing() {
        let (_d, learn, runner, chat, turn, _svc) = setup(LearnMode::Off, true).await;
        learn.after_turn(chat, turn, TurnEndReason::Completed).await;
        assert_eq!(runner.runs.load(Ordering::SeqCst), 0);
        let (_d, learn, runner, chat, turn, _svc) = setup(LearnMode::Ask, false).await;
        learn.after_turn(chat, turn, TurnEndReason::Completed).await;
        assert_eq!(
            runner.runs.load(Ordering::SeqCst),
            0,
            "mail runs learn only in Auto"
        );
        let (_d, learn, runner, chat, turn, _svc) = setup(LearnMode::Ask, true).await;
        learn.after_turn(chat, turn, TurnEndReason::Error).await;
        assert_eq!(runner.runs.load(Ordering::SeqCst), 0);
    }
}
