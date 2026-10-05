//! The bundled engine registry (`registry.json`, ARCHITECTURE §6.1).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineSpec {
    pub id: String,
    pub label: String,
    pub vendor: String,
    /// Executable names, first match on PATH wins.
    pub commands: Vec<String>,
    /// The variable that points the engine at a provider home (none: one
    /// sign-in per machine).
    pub provider_home_env: Option<String>,
    pub supports_resume: bool,
    /// Typed by Shift+Enter in this engine's TUI (newline without submit).
    pub newline_sequence: String,
}

#[derive(Debug, Clone)]
pub struct EngineRegistry {
    entries: Vec<EngineSpec>,
}

const BUNDLED: &str = include_str!("../registry.json");

impl EngineRegistry {
    pub fn bundled() -> Self {
        let entries = serde_json::from_str(BUNDLED).unwrap_or_else(|err| {
            tracing::error!(error = %err, "the bundled engine registry doesn't parse");
            Vec::new()
        });
        Self { entries }
    }

    pub fn all(&self) -> &[EngineSpec] {
        &self.entries
    }

    pub fn get(&self, id: &str) -> Option<&EngineSpec> {
        self.entries.iter().find(|e| e.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_registry_parses_with_unique_ids() {
        let reg = EngineRegistry::bundled();
        assert_eq!(reg.all().len(), 11);
        let mut ids: Vec<&str> = reg.all().iter().map(|e| e.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 11);
        let claude = reg.get("claude").expect("claude");
        assert_eq!(claude.commands, vec!["claude"]);
        assert_eq!(
            claude.provider_home_env.as_deref(),
            Some("CLAUDE_CONFIG_DIR")
        );
        assert!(reg.get("nope").is_none());
        for e in reg.all() {
            assert!(!e.commands.is_empty() && !e.label.is_empty(), "{}", e.id);
            assert!(
                e.commands
                    .iter()
                    .all(|c| c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')),
                "{} commands are bare names",
                e.id
            );
        }
    }
}
