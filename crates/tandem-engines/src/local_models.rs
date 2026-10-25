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

    pub fn slug(self) -> &'static str {
        match self {
            ServerKind::Ollama => "ollama",
            ServerKind::LmStudio => "lmstudio",
            ServerKind::LlamaCpp => "llamacpp",
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

/// A model id safe to write into a config and pass as an argument.
pub fn model_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/' | '@'))
}

/// The provider home's folder name under `$PROVIDERS/codex/`.
pub fn home_name(kind: ServerKind, port: u16) -> String {
    format!("local-{}-{port}", kind.slug())
}

/// Codex's `config.toml` for a local server, merged into what the home
/// already has (its status hook, D-2519): the model and a `vibezeo-local`
/// provider at `http://127.0.0.1:<port>/v1` over the Responses API.
pub fn codex_config(
    existing: &str,
    kind: ServerKind,
    port: u16,
    model: &str,
) -> Result<String, String> {
    if !model_ok(model) {
        return Err("That model's name can't be used".into());
    }
    let mut doc: toml_edit::DocumentMut = existing.parse().map_err(|e| format!("{e}"))?;
    doc["model"] = toml_edit::value(model);
    doc["model_provider"] = toml_edit::value("vibezeo-local");
    let mut provider = toml_edit::Table::new();
    provider["name"] = toml_edit::value(format!("{} on this computer", kind.label()));
    provider["base_url"] = toml_edit::value(format!("{}/v1", url(port, "").trim_end_matches('/')));
    provider["wire_api"] = toml_edit::value("responses");
    let providers = doc
        .entry("model_providers")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    let Some(table) = providers.as_table_mut() else {
        return Err("The home's config.toml has an unexpected model_providers".into());
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
        assert_eq!(home_name(ServerKind::LmStudio, 1234), "local-lmstudio-1234");
        assert_eq!(
            parse_port_entry("ollama:11500"),
            Some((ServerKind::Ollama, 11500))
        );
        assert_eq!(port_entry(ServerKind::LmStudio, 1234), "lmstudio:1234");
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
