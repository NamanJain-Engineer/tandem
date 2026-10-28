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
            // A cloud model runs on ollama.com, not here: never listed.
            .filter(|m| !ollama_remote(m))
            .filter_map(|m| {
                Some(LocalModel {
                    id: text(m, "name", 200)
                        .or_else(|| text(m, "model", 200))
                        .filter(|id| !cloud_name(id))?,
                    size: m.get("size").and_then(Value::as_u64),
                    parameters: m.get("details").and_then(|d| text(d, "parameter_size", 16)),
                })
            })
            .take(200)
            .collect(),
    )
}

/// An Ollama model that runs on Ollama's servers (`remote_host` or
/// `remote_model`, or a `cloud` tag such as `gpt-oss:120b-cloud`): Ollama
/// passes prompts on to ollama.com, so it isn't a local model.
fn ollama_remote(m: &Value) -> bool {
    let set = |k: &str| {
        m.get(k)
            .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
    };
    // As listed: control characters removed, so a name can't hide its tag.
    let cloud_tag = ["name", "model"]
        .iter()
        .any(|k| text(m, k, 200).is_some_and(|n| cloud_name(&n)));
    set("remote_host") || set("remote_model") || cloud_tag
}

/// A model name with Ollama's cloud tag (`:cloud`, `:120b-cloud`), in any
/// case.
fn cloud_name(name: &str) -> bool {
    name.rsplit_once(':').is_some_and(|(_, tag)| {
        let tag = tag.trim().to_ascii_lowercase();
        tag == "cloud" || tag.ends_with("-cloud")
    })
}

/// The models an Ollama `/api/tags` answer says run on Ollama's servers, by
/// the names its OpenAI-style list uses: a server added as another kind may
/// be Ollama, and `/v1/models` doesn't say which models are remote.
pub fn ollama_remote_ids(tags: &Value) -> std::collections::HashSet<String> {
    tags.get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter(|m| ollama_remote(m))
                .flat_map(|m| [text(m, "name", 200), text(m, "model", 200)])
                .flatten()
                .collect()
        })
        .unwrap_or_default()
}

/// Whether an answer is Ollama's `/api/version` (`{"version": "0.12.6"}`):
/// a server added as another kind that answers it is Ollama, and its tags
/// must be read before its models are listed (D-2517).
pub fn ollama_version(v: &Value) -> bool {
    v.get("version")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty())
}

/// An OpenAI-style `/v1/models`: `{"data": [{"id"}]}`.
pub fn parse_openai(body: &Value) -> Option<Vec<LocalModel>> {
    let models = body.get("data")?.as_array()?;
    Some(
        models
            .iter()
            .filter_map(|m| {
                Some(LocalModel {
                    // Ollama lists its cloud models here too (an Ollama
                    // server added as another kind): they run on ollama.com,
                    // never listed.
                    id: text(m, "id", 200).filter(|id| !cloud_name(id))?,
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
    format!("local-{}-{port}-{slug}-{hash:016x}", kind.slug())
}

/// `NO_PROXY` for a local pane: what the login environment had in
/// `NO_PROXY` and `no_proxy` (one variable on Windows), then this
/// computer's names, so no proxy ever carries a prompt to the model server
/// (D-2517).
pub fn no_proxy_value(upper: Option<&str>, lower: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for p in [upper, lower]
        .into_iter()
        .flatten()
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .chain(["127.0.0.1", "localhost", "::1"])
    {
        if !parts.iter().any(|q| q == p) {
            parts.push(p.to_string());
        }
    }
    parts.join(",")
}

/// Why Start isn't offered on Windows yet: its shells run their startup
/// files before the command, and there's no wrapper to set the pane's
/// environment after them (D-2517).
pub const NOT_ON_WINDOWS: &str = "Local models run on Linux and macOS for now";

/// The proxy variables a local pane runs without.
pub const PROXY_VARS: [&str; 6] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
];

/// The variables that would point Codex at an OpenAI account or endpoint: a
/// local pane runs without them too.
pub const ACCOUNT_VARS: [&str; 3] = ["OPENAI_API_KEY", "OPENAI_BASE_URL", "CODEX_API_KEY"];

/// Codex's command-line settings for a local pane, on every launch, fresh
/// or `codex resume` (D-2519): command-line settings come before every
/// configuration file, a trusted project's `.codex/config.toml` included,
/// so no project can name another model or provider. The same as the
/// home's `config.toml`. `None` for a model name that can't be written.
pub fn codex_overrides(kind: ServerKind, port: u16, model: &str) -> Option<Vec<String>> {
    if !model_ok(model) {
        return None;
    }
    let base = format!("{}/v1", url(port, "").trim_end_matches('/'));
    Some(
        [
            format!("model=\"{model}\""),
            format!("model_provider=\"{PROVIDER_ID}\""),
            format!(
                "model_providers.{PROVIDER_ID}.name=\"{} on this computer\"",
                kind.label()
            ),
            format!("model_providers.{PROVIDER_ID}.base_url=\"{base}\""),
            format!("model_providers.{PROVIDER_ID}.wire_api=\"responses\""),
            "check_for_update_on_startup=false".to_string(),
        ]
        .into_iter()
        .flat_map(|kv| ["-c".to_string(), kv])
        .collect(),
    )
}

/// The provider id a local home and its overrides name.
pub const PROVIDER_ID: &str = "vibezeo-local";

/// `argv` with `overrides` right after its program: before a subcommand
/// (`resume`) and before the user's own arguments.
pub fn with_overrides(argv: Vec<String>, overrides: Vec<String>) -> Vec<String> {
    let mut it = argv.into_iter();
    let mut out: Vec<String> = it.next().into_iter().collect();
    out.extend(overrides);
    out.extend(it);
    out
}

/// A local pane's command with its environment set inside it (unix): the
/// login shell's startup files run before the command, so a variable they
/// export (a proxy, `NO_PROXY`, `CODEX_HOME`) can't reach Codex (D-2517).
/// `None` when the program's path has an `=`: `env` would read it as a
/// variable and print the whole environment instead of running it.
pub fn env_argv(argv: Vec<String>, codex_home: &str, no_proxy: &str) -> Option<Vec<String>> {
    if argv.first().is_none_or(|p| p.contains('=')) {
        return None;
    }
    let mut out = vec![ENV_PROGRAM.to_string()];
    for var in PROXY_VARS.iter().chain(ACCOUNT_VARS.iter()) {
        out.push("-u".into());
        out.push((*var).into());
    }
    out.push(format!("NO_PROXY={no_proxy}"));
    out.push(format!("no_proxy={no_proxy}"));
    out.push(format!("CODEX_HOME={codex_home}"));
    out.extend(argv);
    Some(out)
}

/// The program [`env_argv`] wraps.
pub const ENV_PROGRAM: &str = "/usr/bin/env";

/// The program a command runs: past [`env_argv`]'s wrapper when it has one,
/// so the sandbox finds the engine itself.
pub fn wrapped_program(argv: &[String]) -> Option<&str> {
    if argv.first().map(String::as_str) != Some(ENV_PROGRAM) {
        return argv.first().map(String::as_str);
    }
    let mut rest = argv.iter().skip(1);
    while let Some(a) = rest.next() {
        match a.as_str() {
            "-u" => {
                rest.next();
            }
            a if a.contains('=') => {}
            a => return Some(a),
        }
    }
    None
}

/// A Codex flag that takes a value: its short and long spelling, and
/// whether a value may be kept.
type Flag = (&'static str, &'static str, fn(&str) -> bool);

/// The user's Codex arguments a local pane keeps (D-2519): an allowlist.
/// Only sandbox and approval flags, and `-c` settings whose key is in
/// [`safe_setting`], carry over, in every spelling Codex's parser takes
/// (`-cK=V`, `-c=K=V`, `-c K=V`, `--config K=V`, `--config=K=V`, and the
/// same for each flag). Everything else is dropped: a model, provider or
/// profile, in any form, could send prompts off this computer.
pub fn local_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let next = args.get(i + 1).map(String::as_str);
        // Flags without a value.
        if matches!(
            a,
            "--full-auto" | "--dangerously-bypass-approvals-and-sandbox"
        ) {
            out.push(a.to_string());
            i += 1;
            continue;
        }
        // Flags with a value: (short, long, keep the value if).
        let flags: [Flag; 3] = [
            ("-s", "--sandbox", |v| {
                matches!(v, "read-only" | "workspace-write" | "danger-full-access")
            }),
            ("-a", "--ask-for-approval", |v| {
                matches!(v, "untrusted" | "on-failure" | "on-request" | "never")
            }),
            ("-c", "--config", safe_setting),
        ];
        let mut handled = false;
        for (short, long, keep) in flags {
            let (value, used) = if a == short || a == long {
                (next, 2)
            } else if let Some(v) = a.strip_prefix(&format!("{long}=")) {
                (Some(v), 1)
            } else if let Some(v) = a.strip_prefix(short).filter(|_| a.len() > short.len()) {
                (Some(v.strip_prefix('=').unwrap_or(v)), 1)
            } else {
                continue;
            };
            handled = true;
            if let Some(v) = value.filter(|v| keep(v)) {
                out.push(long.to_string());
                out.push(v.to_string());
            }
            i += used;
            break;
        }
        if !handled {
            // Dropped, and a value it may take with it is dropped too: an
            // unknown flag's next argument is read as a flag on its own.
            i += 1;
        }
    }
    out
}

/// A `-c key=value` a local pane may keep: sandbox, approval and reasoning
/// settings only. Never a model, provider or profile, nor a `profiles.*`
/// table that could name one.
pub fn safe_setting(kv: &str) -> bool {
    let Some((key, _)) = kv.split_once('=') else {
        return false;
    };
    let key = key.trim();
    key == "approval_policy" || key.starts_with("sandbox_") || key.starts_with("model_reasoning_")
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
    // A profile could name another provider: none in a local home.
    doc.remove("profile");
    doc.remove("profiles");
    doc["model_provider"] = toml_edit::value(PROVIDER_ID);
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
    table.insert(PROVIDER_ID, toml_edit::Item::Table(provider));
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
    fn f_en_11_3_no_proxy_is_both_spellings_and_this_computer() {
        assert_eq!(
            no_proxy_value(Some("corp.example, .internal"), Some(".lan,corp.example")),
            "corp.example,.internal,.lan,127.0.0.1,localhost,::1"
        );
        assert_eq!(no_proxy_value(None, None), "127.0.0.1,localhost,::1");
        let argv = env_argv(
            vec!["/bin/codex".into(), "--sandbox".into()],
            "/h",
            "127.0.0.1",
        )
        .expect("argv");
        assert_eq!(wrapped_program(&argv), Some("/bin/codex"));
        assert_eq!(
            wrapped_program(&["/bin/claude".to_string()]),
            Some("/bin/claude")
        );
        // An `=` in the path would make env print the environment.
        assert_eq!(env_argv(vec!["/opt/a=b/codex".into()], "/h", "x"), None);
        assert_eq!(argv[0], "/usr/bin/env");
        for var in PROXY_VARS.iter().chain(ACCOUNT_VARS.iter()) {
            assert!(
                argv.windows(2).any(|w| w[0] == "-u" && w[1] == *var),
                "{var}"
            );
        }
        assert!(argv.contains(&"NO_PROXY=127.0.0.1".to_string()));
        assert!(argv.contains(&"CODEX_HOME=/h".to_string()));
        assert_eq!(&argv[argv.len() - 2..], ["/bin/codex", "--sandbox"]);
    }

    #[test]
    fn f_en_11_1_ollamas_cloud_models_are_not_listed() {
        let tags = json!({"models": [
            {"name": "gpt-oss:120b-cloud", "model": "gpt-oss:120b-cloud", "remote_model": "gpt-oss:120b",
             "remote_host": "https://ollama.com:443", "size": 384, "details": {"parameter_size": "116.8B"}},
            {"name": "deepseek-v3.1:671b-cloud", "size": 384},
            {"name": "kimi-k2:cloud", "size": 384},
            {"name": "glm-4.6:CLOUD\u{7}", "size": 384},
            {"name": "gemma3:4b", "size": 3_338_801_804u64, "details": {"parameter_size": "4.3B"}},
            {"name": "qwen3:8b", "remote_host": ""}
        ]});
        let ids: Vec<String> = parse(ServerKind::Ollama, &tags)
            .expect("tags")
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, vec!["gemma3:4b", "qwen3:8b"]);
        assert_eq!(
            ollama_remote_ids(&tags),
            [
                "gpt-oss:120b-cloud",
                "deepseek-v3.1:671b-cloud",
                "kimi-k2:cloud",
                "glm-4.6:CLOUD"
            ]
            .into_iter()
            .map(String::from)
            .collect()
        );
        assert!(ollama_version(&json!({"version": "0.12.6"})));
        assert!(!ollama_version(&json!({"data": []})));
        assert!(!ollama_version(&json!({"version": ""})));
        // Ollama's OpenAI-style list has them too, as another kind would ask.
        let v1 = json!({"object": "list", "data": [
            {"id": "gpt-oss:120b-cloud", "object": "model", "owned_by": "library"},
            {"id": "kimi-k2:cloud", "object": "model"},
            {"id": "kimi-k2:Cloud", "object": "model"},
            {"id": "qwen3-coder:480B-CLOUD", "object": "model"},
            {"id": "gpt-oss:120b-cloud\u{7}", "object": "model"},
            {"id": "gemma3:4b", "object": "model", "owned_by": "library"}
        ]});
        for kind in [ServerKind::LmStudio, ServerKind::LlamaCpp] {
            let ids: Vec<String> = parse(kind, &v1)
                .expect("models")
                .into_iter()
                .map(|m| m.id)
                .collect();
            assert_eq!(ids, vec!["gemma3:4b"], "{kind:?}");
        }
    }

    /// F-EN-11 review: every launch names the model and the local provider
    /// on the command line, before a subcommand and the user's arguments.
    #[test]
    fn f_en_11_2_every_launch_names_the_model_and_provider_on_the_command_line() {
        let o = codex_overrides(ServerKind::Ollama, 11434, "qwen3:8b").expect("overrides");
        let pairs: Vec<&str> = o
            .chunks(2)
            .map(|c| {
                assert_eq!(c[0], "-c");
                c[1].as_str()
            })
            .collect();
        assert_eq!(
            pairs,
            vec![
                "model=\"qwen3:8b\"",
                "model_provider=\"vibezeo-local\"",
                "model_providers.vibezeo-local.name=\"Ollama on this computer\"",
                "model_providers.vibezeo-local.base_url=\"http://127.0.0.1:11434/v1\"",
                "model_providers.vibezeo-local.wire_api=\"responses\"",
                "check_for_update_on_startup=false",
            ]
        );
        // Each value is TOML, as Codex reads it.
        for kv in &pairs {
            let doc: toml_edit::DocumentMut = kv.parse().expect("toml");
            assert_eq!(doc.len(), 1, "{kv}");
        }
        let resume = with_overrides(
            vec!["/bin/codex".into(), "resume".into(), "t1".into()],
            o.clone(),
        );
        assert_eq!(resume[0], "/bin/codex");
        assert_eq!(&resume[1..3], ["-c", "model=\"qwen3:8b\""]);
        assert_eq!(&resume[resume.len() - 2..], ["resume", "t1"]);
        assert_eq!(codex_overrides(ServerKind::Ollama, 1, "-m"), None);
        assert_eq!(codex_overrides(ServerKind::Ollama, 1, "a\"b"), None);
    }

    #[test]
    fn f_en_11_2_only_sandbox_approval_and_reasoning_arguments_carry_over_in_any_spelling() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Kept, normalized to the long form.
        assert_eq!(
            local_args(&v(&[
                "--sandbox",
                "read-only",
                "-auntrusted",
                "-c",
                "model_reasoning_effort=high",
                "-csandbox_workspace_write.network_access=false",
                "--config=approval_policy=never",
                "--full-auto",
            ])),
            v(&[
                "--sandbox",
                "read-only",
                "--ask-for-approval",
                "untrusted",
                "--config",
                "model_reasoning_effort=high",
                "--config",
                "sandbox_workspace_write.network_access=false",
                "--config",
                "approval_policy=never",
                "--full-auto",
            ])
        );
        // Dropped, every spelling the review found and more.
        for bad in [
            v(&["-cmodel_providers.vibezeo-local.base_url=https://gw.example/v1"]),
            v(&[
                "-c=model_provider=corp",
                "-c=model_providers.corp.base_url=https://gw.example/v1",
            ]),
            v(&["-c", "profiles.w.model_provider=openai", "-pw"]),
            v(&["-p=w"]),
            v(&["--profile", "w"]),
            v(&["--profile=w"]),
            v(&["-mNAME"]),
            v(&["-m", "gpt-5"]),
            v(&["--model=o3"]),
            v(&["--oss"]),
            v(&["--local-provider", "lmstudio"]),
            v(&["--config", "model=o4"]),
            v(&["--config=model_provider=openai"]),
            v(&["-c", "base_url=https://gw.example"]),
            v(&["--sandbox", "anything"]),
            v(&["-sfull"]),
            v(&["-a", "always"]),
            v(&["--ask-for-approval=yolo"]),
        ] {
            assert_eq!(local_args(&bad), Vec::<String>::new(), "{bad:?}");
        }
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
        let profiled = codex_config(
            "profile = \"w\"\n[profiles.w]\nmodel_provider = \"openai\"\n",
            ServerKind::Ollama,
            11434,
            "qwen3:8b",
        )
        .expect("config");
        assert!(!profiled.contains("profile"), "{profiled}");
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
