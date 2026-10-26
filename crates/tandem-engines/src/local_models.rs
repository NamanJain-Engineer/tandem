//! Local model servers (F-EN-11, UI-164): Ollama, LM Studio and llama.cpp's
//! `llama-server`, found on this computer only, their models listed, and
//! Codex configured for one in a provider home of its own (D-2517).
//!
//! Nothing here talks to the network: the app asks the servers, through
//! [`url`], which only ever builds `http://127.0.0.1:<port>/…`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Which server software answers on a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ServerKind {
    Ollama,
    LmStudio,
    LlamaCpp,
}

impl ServerKind {
    pub const ALL: [ServerKind; 3] = [
        ServerKind::Ollama,
        ServerKind::LmStudio,
        ServerKind::LlamaCpp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ServerKind::Ollama => "Ollama",
            ServerKind::LmStudio => "LM Studio",
            ServerKind::LlamaCpp => "llama.cpp",
        }
    }

    /// Its documented default port.
    pub fn default_port(self) -> u16 {
        match self {
            ServerKind::Ollama => 11434,
            ServerKind::LmStudio => 1234,
            ServerKind::LlamaCpp => 8080,
        }
    }

    /// Where it lists its models.
    pub fn models_path(self) -> &'static str {
        match self {
            ServerKind::Ollama => "/api/tags",
            ServerKind::LmStudio | ServerKind::LlamaCpp => "/v1/models",
        }
    }

    /// Codex can use it: both document Codex through the Responses API
    /// (D-2518). llama.cpp's support isn't confirmed yet.
    pub fn startable(self) -> bool {
        matches!(self, ServerKind::Ollama | ServerKind::LmStudio)
    }

    /// As the IPC names it: settings entries and home folders use it too.
    pub fn slug(self) -> &'static str {
        match self {
            ServerKind::Ollama => "ollama",
            ServerKind::LmStudio => "lm_studio",
            ServerKind::LlamaCpp => "llama_cpp",
        }
    }
}

/// One model a server offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LocalModel {
    pub id: String,
    /// Bytes on disk, when the server says.
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    /// "30.5B", when the server says.
    pub parameters: Option<String>,
}

/// The only hosts a user may name for a server.
pub fn loopback_host(host: &str) -> bool {
    matches!(host.trim(), "127.0.0.1" | "localhost")
}

/// The one address the finder asks: always this computer.
pub fn url(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
}

fn text(v: &Value, key: &str, max: usize) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(|s| {
            s.chars()
                .filter(|c| !c.is_control())
                .take(max)
                .collect::<String>()
        })
        .filter(|s| !s.trim().is_empty())
}

/// Ollama's `/api/tags`: `{"models": [{"name", "size", "details": {"parameter_size"}}]}`.
pub fn parse_ollama(body: &Value) -> Option<Vec<LocalModel>> {
    let models = body.get("models")?.as_array()?;
    Some(
        models
            .iter()
            .filter_map(|m| {
                Some(LocalModel {
                    id: text(m, "name", 200).or_else(|| text(m, "model", 200))?,
                    size: m.get("size").and_then(Value::as_u64),
                    parameters: m.get("details").and_then(|d| text(d, "parameter_size", 16)),
                })
            })
            .take(200)
            .collect(),
    )
}

/// An OpenAI-style `/v1/models`: `{"data": [{"id"}]}`.
pub fn parse_openai(body: &Value) -> Option<Vec<LocalModel>> {
    let models = body.get("data")?.as_array()?;
    Some(
        models
            .iter()
            .filter_map(|m| {
                Some(LocalModel {
                    id: text(m, "id", 200)?,
                    size: None,
                    parameters: None,
                })
            })
            .take(200)
            .collect(),
    )
}

pub fn parse(kind: ServerKind, body: &Value) -> Option<Vec<LocalModel>> {
    match kind {
        ServerKind::Ollama => parse_ollama(body),
        ServerKind::LmStudio | ServerKind::LlamaCpp => parse_openai(body),
    }
}

/// One `local_model_ports` entry, `kind:port` (D-2517).
pub fn parse_port_entry(entry: &str) -> Option<(ServerKind, u16)> {
    let (kind, port) = entry.split_once(':')?;
    let kind = ServerKind::ALL.into_iter().find(|k| k.slug() == kind)?;
    let port: u16 = port.parse().ok().filter(|p| *p > 0)?;
    Some((kind, port))
}

pub fn port_entry(kind: ServerKind, port: u16) -> String {
    format!("{}:{port}", kind.slug())
}

/// A model id safe to write into a config and pass as an argument: never
/// one that reads as a flag.
pub fn model_ok(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && id.len() <= 200
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/' | '@'))
}

/// The provider home's folder name under `$PROVIDERS/codex/`: one per
/// server and model, so panes on two models of one server never share a
/// `config.toml` (D-2519). The model's letters and digits, then a hash of its
/// full name, so two names that slug alike stay apart.
pub fn home_name(kind: ServerKind, port: u16, model: &str) -> String {
    let slug: String = model
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(40)
        .collect();
    // FNV-1a: stable across runs and builds.
    let hash = model.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("local-{}-{port}-{slug}-{:08x}", kind.slug(), hash as u32)
}

/// What a local pane adds to `NO_PROXY` and `no_proxy`: this computer's
/// names, after whatever the login environment had, so no proxy ever
/// carries a prompt to the model server (D-2517).
pub fn no_proxy_env(upper: Option<&str>, lower: Option<&str>) -> Vec<(String, String)> {
    let with_local = |existing: Option<&str>| {
        let mut parts: Vec<String> = existing
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect();
        for host in ["127.0.0.1", "localhost", "::1"] {
            if !parts.iter().any(|p| p == host) {
                parts.push(host.to_string());
            }
        }
        parts.join(",")
    };
    vec![
        ("NO_PROXY".to_string(), with_local(upper)),
        ("no_proxy".to_string(), with_local(lower)),
    ]
}

/// Why a local config can't be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("That model's name can't be used")]
    Model,
    #[error("The home's config.toml can't be read as TOML: {0}")]
    Toml(String),
    #[error("The home's config.toml has an unexpected model_providers")]
    Providers,
}

/// Codex's `config.toml` for a local server, merged into what the home
/// already has (its status hook, D-2519): the model, a `vibezeo-local`
/// provider at `http://127.0.0.1:<port>/v1` over the Responses API, and no
/// update check at startup, which would go off this computer (D-2517).
/// Nothing about sandboxing or approvals is written: those stay Codex's
/// defaults and the user's own arguments.
pub fn codex_config(
    existing: &str,
    kind: ServerKind,
    port: u16,
    model: &str,
) -> Result<String, ConfigError> {
    if !model_ok(model) {
        return Err(ConfigError::Model);
    }
    let mut doc: toml_edit::DocumentMut = existing
        .parse()
        .map_err(|e: toml_edit::TomlError| ConfigError::Toml(e.to_string()))?;
    doc["model"] = toml_edit::value(model);
    doc["check_for_update_on_startup"] = toml_edit::value(false);
    doc["model_provider"] = toml_edit::value("vibezeo-local");
    let mut provider = toml_edit::Table::new();
    provider["name"] = toml_edit::value(format!("{} on this computer", kind.label()));
    provider["base_url"] = toml_edit::value(format!("{}/v1", url(port, "").trim_end_matches('/')));
    provider["wire_api"] = toml_edit::value("responses");
    let providers = doc
        .entry("model_providers")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let Some(table) = providers.as_table_mut() else {
        return Err(ConfigError::Providers);
    };
    table.set_implicit(true);
    table.insert("vibezeo-local", toml_edit::Item::Table(provider));
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn f_en_11_1_ollamas_and_openai_style_model_lists_parse() {
        let tags = json!({"models": [
            {"name": "qwen3-coder:30b", "size": 18_556_701_696u64, "details": {"parameter_size": "30.5B"}},
            {"name": "", "size": 1},
            {"model": "gemma3:4b"}
        ]});
        assert_eq!(
            parse(ServerKind::Ollama, &tags).expect("tags"),
            vec![
                LocalModel {
                    id: "qwen3-coder:30b".into(),
                    size: Some(18_556_701_696),
                    parameters: Some("30.5B".into())
                },
                LocalModel {
                    id: "gemma3:4b".into(),
                    size: None,
                    parameters: None
                },
            ]
        );
        let models =
            json!({"object": "list", "data": [{"id": "openai/gpt-oss-20b"}, {"object": "model"}]});
        assert_eq!(
            parse(ServerKind::LmStudio, &models).expect("models").len(),
            1
        );
        assert!(parse(ServerKind::Ollama, &json!({"data": []})).is_none());
    }

    #[test]
    fn f_en_11_3_no_proxy_names_this_computer_after_what_was_there() {
        assert_eq!(
            no_proxy_env(Some("corp.example, .internal"), None),
            vec![
                (
                    "NO_PROXY".to_string(),
                    "corp.example,.internal,127.0.0.1,localhost,::1".to_string()
                ),
                (
                    "no_proxy".to_string(),
                    "127.0.0.1,localhost,::1".to_string()
                ),
            ]
        );
        assert_eq!(
            no_proxy_env(Some("localhost"), Some("127.0.0.1"))[0].1,
            "localhost,127.0.0.1,::1"
        );
    }

    #[test]
    fn f_en_11_3_only_loopback_is_ever_asked() {
        for kind in ServerKind::ALL {
            assert!(url(kind.default_port(), kind.models_path()).starts_with("http://127.0.0.1:"));
        }
        assert!(loopback_host("127.0.0.1") && loopback_host(" localhost "));
        for host in [
            "192.168.1.5",
            "10.0.0.2",
            "example.com",
            "0.0.0.0",
            "::1",
            "127.0.0.2",
            "",
        ] {
            assert!(!loopback_host(host), "{host}");
        }
    }

    #[test]
    fn f_en_11_2_the_codex_config_names_the_model_and_a_loopback_provider_and_keeps_the_hook() {
        let existing = "notify = [\"/opt/vibezeo/tandem-hook\", \"codex\"]\n";
        let out =
            codex_config(existing, ServerKind::Ollama, 11434, "qwen3-coder:30b").expect("config");
        let doc: toml_edit::DocumentMut = out.parse().expect("toml");
        assert_eq!(doc["model"].as_str(), Some("qwen3-coder:30b"));
        assert_eq!(doc["model_provider"].as_str(), Some("vibezeo-local"));
        let p = &doc["model_providers"]["vibezeo-local"];
        assert_eq!(p["base_url"].as_str(), Some("http://127.0.0.1:11434/v1"));
        assert_eq!(p["wire_api"].as_str(), Some("responses"));
        assert!(doc.get("notify").is_some(), "the status hook stays");
        // Writing it again replaces, never duplicates.
        let again = codex_config(&out, ServerKind::Ollama, 11434, "gemma3:4b").expect("again");
        assert_eq!(again.matches("[model_providers.vibezeo-local]").count(), 1);
        assert!(codex_config("", ServerKind::Ollama, 11434, "bad\"name").is_err());
        assert!(codex_config(
            "",
            ServerKind::Ollama,
            11434,
            "x\nmodel_provider = \"openai\""
        )
        .is_err());
        assert_eq!(doc["check_for_update_on_startup"].as_bool(), Some(false));
        for key in ["sandbox_mode", "approval_policy"] {
            assert!(
                doc.get(key).is_none(),
                "{key} is left to Codex and the user"
            );
        }
        assert!(codex_config("", ServerKind::Ollama, 11434, "--yolo").is_err());
        let name = home_name(ServerKind::LmStudio, 1234, "openai/gpt-oss-20b");
        assert!(
            name.starts_with("local-lm_studio-1234-openai-gpt-oss-20b-"),
            "{name}"
        );
        // One home per model, and names that slug alike stay apart.
        assert_ne!(
            name,
            home_name(ServerKind::LmStudio, 1234, "openai/gpt-oss-120b")
        );
        assert_ne!(
            home_name(ServerKind::Ollama, 1, "a:b"),
            home_name(ServerKind::Ollama, 1, "a/b")
        );
        assert_eq!(
            parse_port_entry("ollama:11500"),
            Some((ServerKind::Ollama, 11500))
        );
        assert_eq!(port_entry(ServerKind::LmStudio, 1234), "lm_studio:1234");
        for bad in ["ollama", "ollama:0", "ollama:70000", "vllm:8000", ":1"] {
            assert_eq!(parse_port_entry(bad), None, "{bad}");
        }
        assert!(
            ServerKind::Ollama.startable()
                && ServerKind::LmStudio.startable()
                && !ServerKind::LlamaCpp.startable()
        );
    }
}
