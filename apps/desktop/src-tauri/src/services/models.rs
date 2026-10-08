//! Model catalog (F-TH-08, ARCHITECTURE `models_list`): what the engine
//! reports at runtime first, then the remote catalog (remote config
//! `models`, refreshed every 10 min), then the bundled one. The engine's own
//! default ("Default") is always the first row.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_core::remote_config::ConfigSource;
use ts_rs::TS;

/// The id of the engine's own default: no `--model` flag is passed.
pub const DEFAULT_MODEL: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ModelSource {
    Runtime,
    Remote,
    Bundled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelEntry {
    pub id: String,
    pub label: String,
    pub description: String,
    pub is_default: bool,
    pub effort_levels: Vec<String>,
    pub context_window: Option<u64>,
    pub source: ModelSource,
}

/// One catalog row as remote config and `resources/config.json` hold it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogRow {
    id: String,
    label: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    effort_levels: Vec<String>,
    #[serde(default)]
    context_window: Option<u64>,
    #[serde(default)]
    deprecated: bool,
}

fn rows(config: &Value, engine: &str) -> Vec<CatalogRow> {
    config
        .get("models")
        .and_then(|m| m.get(engine))
        .and_then(|list| serde_json::from_value::<Vec<Value>>(list.clone()).ok())
        .unwrap_or_default()
        .into_iter()
        // One malformed row never hides the others.
        .filter_map(|v| serde_json::from_value::<CatalogRow>(v).ok())
        .filter(|r| !r.deprecated && !r.id.trim().is_empty())
        .collect()
}

fn entry(row: CatalogRow, source: ModelSource) -> ModelEntry {
    ModelEntry {
        is_default: row.id == DEFAULT_MODEL,
        id: row.id,
        label: row.label,
        description: row.description,
        effort_levels: row.effort_levels,
        context_window: row.context_window,
        source,
    }
}

/// Merges the three sources: an id keeps its highest-priority row, and the
/// default row is always first.
pub fn merge(
    engine: &str,
    runtime: &[ModelEntry],
    config: &Value,
    config_source: ConfigSource,
    bundled: &Value,
) -> Vec<ModelEntry> {
    let remote_source = match config_source {
        ConfigSource::Remote => ModelSource::Remote,
        ConfigSource::Bundled => ModelSource::Bundled,
    };
    let mut seen = HashSet::new();
    let mut out: Vec<ModelEntry> = Vec::new();
    let candidates = runtime
        .iter()
        .cloned()
        .chain(
            rows(config, engine)
                .into_iter()
                .map(|r| entry(r, remote_source)),
        )
        .chain(
            rows(bundled, engine)
                .into_iter()
                .map(|r| entry(r, ModelSource::Bundled)),
        );
    for e in candidates {
        if seen.insert(e.id.clone()) {
            out.push(e);
        }
    }
    let default = match out.iter().position(|e| e.id == DEFAULT_MODEL) {
        Some(i) => out.remove(i),
        None => ModelEntry {
            id: DEFAULT_MODEL.into(),
            label: "Default".into(),
            description: "The engine's own default model".into(),
            is_default: true,
            effort_levels: Vec::new(),
            context_window: None,
            source: ModelSource::Bundled,
        },
    };
    out.insert(0, default);
    out
}

/// The model to launch with: the saved one when it's still offered, else the
/// default with the notice F-TH-08 AC2 asks for.
pub fn resolve(saved: Option<&str>, catalog: &[ModelEntry]) -> (Option<String>, Option<String>) {
    match saved {
        None | Some(DEFAULT_MODEL) | Some("") => (None, None),
        Some(id) if catalog.iter().any(|m| m.id == id) => (Some(id.to_string()), None),
        Some(id) => (
            None,
            Some(format!(
                "The saved model {id} isn't available — using Default"
            )),
        ),
    }
}

/// `resources/config.json`, parsed once.
pub fn bundled_config() -> &'static Value {
    static BUNDLED: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    BUNDLED.get_or_init(|| {
        serde_json::from_str(crate::services::config::BUNDLED_CONFIG).unwrap_or(Value::Null)
    })
}

/// An engine's catalog from every source (`models_list`).
pub fn catalog_for(
    engine: &str,
    runtime: &RuntimeModels,
    config: &crate::services::config::ConfigService,
) -> Vec<ModelEntry> {
    merge(
        engine,
        &runtime.get(engine),
        &config.value(),
        config.source().unwrap_or(ConfigSource::Bundled),
        bundled_config(),
    )
}

/// Models engines reported while running (ACP `session/new`, Codex
/// `model/list`); empty until an adapter reports some.
#[derive(Default)]
pub struct RuntimeModels {
    by_engine: RwLock<HashMap<String, Vec<ModelEntry>>>,
}

impl RuntimeModels {
    pub fn get(&self, engine: &str) -> Vec<ModelEntry> {
        self.by_engine
            .read()
            .ok()
            .and_then(|m| m.get(engine).cloned())
            .unwrap_or_default()
    }

    pub fn set(&self, engine: &str, models: Vec<ModelEntry>) {
        if let Ok(mut m) = self.by_engine.write() {
            m.insert(engine.to_string(), models);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bundled() -> Value {
        json!({"models": {"claude": [
            {"id": "default", "label": "Default", "description": "The engine's own default model"},
            {"id": "sonnet", "label": "Sonnet", "description": "Fast"}
        ]}})
    }

    #[test]
    fn f_th_08_default_first_then_runtime_remote_bundled_without_duplicates() {
        let remote = json!({"models": {"claude": [
            {"id": "opus", "label": "Opus", "description": "Deep"},
            {"id": "sonnet", "label": "Sonnet (remote)", "description": "Fast"},
            {"id": "old", "label": "Old", "deprecated": true},
            {"label": "no id"}
        ]}});
        let runtime = vec![ModelEntry {
            id: "sonnet".into(),
            label: "Sonnet (reported)".into(),
            description: String::new(),
            is_default: false,
            effort_levels: vec![],
            context_window: Some(200_000),
            source: ModelSource::Runtime,
        }];
        let list = merge(
            "claude",
            &runtime,
            &remote,
            ConfigSource::Remote,
            &bundled(),
        );
        let ids: Vec<_> = list.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["default", "sonnet", "opus"]);
        assert_eq!(list[1].label, "Sonnet (reported)");
        assert_eq!(list[2].source, ModelSource::Remote);
        assert!(list[0].is_default);
    }

    #[test]
    fn f_th_08_1_a_model_added_remotely_appears() {
        let before = merge("claude", &[], &bundled(), ConfigSource::Bundled, &bundled());
        assert!(!before.iter().any(|m| m.id == "haiku"));
        let remote = json!({"models": {"claude": [{"id": "haiku", "label": "Haiku"}]}});
        let after = merge("claude", &[], &remote, ConfigSource::Remote, &bundled());
        assert!(after
            .iter()
            .any(|m| m.id == "haiku" && m.source == ModelSource::Remote));
        // The bundled rows still back the remote list.
        assert!(after.iter().any(|m| m.id == "sonnet"));
    }

    #[test]
    fn f_th_08_2_a_saved_model_gone_from_every_source_falls_back_with_the_notice() {
        let list = merge("claude", &[], &bundled(), ConfigSource::Bundled, &bundled());
        assert_eq!(
            resolve(Some("sonnet"), &list),
            (Some("sonnet".into()), None)
        );
        assert_eq!(resolve(Some("default"), &list), (None, None));
        assert_eq!(resolve(None, &list), (None, None));
        let (model, notice) = resolve(Some("gone-1"), &list);
        assert_eq!(model, None);
        assert_eq!(
            notice.as_deref(),
            Some("The saved model gone-1 isn't available — using Default")
        );
        // An engine nobody lists still gets the default row.
        let none = merge("grok", &[], &json!({}), ConfigSource::Bundled, &json!({}));
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].id, "default");
    }
}
