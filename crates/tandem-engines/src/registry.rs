//! The engine registry (`registry.json`, ARCHITECTURE §6.1, F-EN-02).
//!
//! Every engine Tandem can detect and launch is data: the bundled list plus
//! entries from the signed remote config (`/v1/config` key `registry_overrides`), which
//! may override a bundled entry's fields by `id` or add a new engine. An
//! entry is checked against the schema's rules before it's used; a bad
//! remote entry is dropped with a warning and never breaks the bundled ones.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Install commands per OS, shown verbatim in the install sheet (F-EN-03).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InstallCommands {
    pub mac: Option<String>,
    pub lin: Option<String>,
    pub win: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LoginMode {
    /// A slash command typed into the running TUI.
    #[default]
    TuiSlash,
    /// A subcommand run as its own process (`codex login`).
    Argv,
}

/// How an account's provider home reaches the engine (F-AC-01).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HomeStrategy {
    /// `provider_home_env` points at the home.
    Env,
    /// `XDG_CONFIG_HOME` and `XDG_DATA_HOME` under the home.
    Xdg,
    /// One sign-in per machine (the user's own home).
    #[default]
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum KeychainIsolation {
    Supported,
    /// macOS keeps one keychain item per engine: one account there.
    UnsupportedMac,
    #[default]
    #[serde(rename = "n/a")]
    NotApplicable,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum HooksSupport {
    ClaudeHooks,
    CodexNotify,
    #[default]
    None,
}

/// A file merged into a fresh provider home so the engine skips first-run
/// screens (`{theme}` is replaced at write time).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OnboardingSeed {
    pub file: String,
    #[ts(type = "Record<string, unknown>")]
    pub merge: Value,
}

/// Where the engine keeps its own MCP server list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct McpNative {
    pub kind: String,
    pub file: String,
    pub key: String,
}

/// Whether an engine reads every instructions file that exists, or only the
/// first (F-EN-07).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum InstructionsMode {
    #[default]
    All,
    First,
}

/// A one-line file that makes the engine read `AGENTS.md`: its `file` holds
/// the `import` line (F-EN-07).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InstructionsPointer {
    pub file: String,
    pub import: String,
}

/// The instructions files an engine reads at a repository's root (F-EN-07,
/// D-2400), checked against the vendor's own documentation. Paths are
/// relative and `/`-separated; one ending in `/` is a folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstructionsSpec {
    pub files: Vec<String>,
    #[serde(default)]
    pub mode: InstructionsMode,
    /// Read only when none of `files` exist.
    #[serde(default)]
    pub fallback: Option<String>,
    #[serde(default)]
    pub pointer: Option<InstructionsPointer>,
}

/// How an engine was installed, read from where its command resolves
/// (F-EN-08, D-2406).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum InstallMethod {
    Npm,
    Homebrew,
    /// The engine's own installer (its versions folder).
    Native,
    Other,
}

/// The documented switch that stops an engine updating itself, set only in
/// its launch environment or arguments (F-EN-08).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UpdatePin {
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub args: Vec<String>,
}

/// How an engine is installed and updated (F-EN-08), from its vendor's docs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdatesSpec {
    /// Its npm package, for Check for updates and Roll back.
    #[serde(default)]
    pub npm: Option<String>,
    /// Its own installer's versions folder, relative to the user's home.
    #[serde(default)]
    pub native_dir: Option<String>,
    /// Its own installer for one version (`{version}` is replaced).
    #[serde(default)]
    pub native_install: Option<String>,
    #[serde(default)]
    pub pin: Option<UpdatePin>,
    /// The install methods with which it updates itself.
    #[serde(default)]
    pub self_updates: Vec<InstallMethod>,
}

fn default_version_cmd() -> Vec<String> {
    vec!["--version".into()]
}

fn default_version_regex() -> String {
    r"(\d+\.\d+\.\d+)".into()
}

fn default_newline() -> String {
    "\u{1b}\r".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineSpec {
    pub id: String,
    pub label: String,
    pub vendor: String,
    /// The letters of Tandem's own glyph for the engine (UI_SPEC §1.6);
    /// never a vendor logo.
    pub logo: String,
    /// Executable names, first match wins.
    pub commands: Vec<String>,
    #[serde(default = "default_version_cmd")]
    pub version_cmd: Vec<String>,
    /// Captures the version in `version_cmd`'s output.
    #[serde(default = "default_version_regex")]
    pub version_regex: String,
    #[serde(default)]
    pub install: InstallCommands,
    #[serde(default)]
    pub login_cmd: Option<Vec<String>>,
    #[serde(default)]
    pub login_mode: LoginMode,
    #[serde(default)]
    pub logout_cmd: Option<Vec<String>>,
    /// Probes whether the engine is signed in (exit code 0 = yes).
    #[serde(default)]
    pub status_cmd: Option<Vec<String>>,
    /// A file in the provider home that exists once the engine is signed in;
    /// only its presence is checked, never its contents (F-AC-01).
    #[serde(default)]
    pub signed_in_file: Option<String>,
    /// `claude`, `codex` or `acp`; none = terminal only.
    #[serde(default)]
    pub chat_adapter: Option<String>,
    #[serde(default)]
    pub acp_cmd: Option<Vec<String>>,
    #[serde(default)]
    pub adapter_versions: Option<String>,
    /// The variable that points the engine at a provider home.
    #[serde(default)]
    pub provider_home_env: Option<String>,
    #[serde(default)]
    pub home_strategy: HomeStrategy,
    #[serde(default)]
    pub keychain_isolation: KeychainIsolation,
    /// Arguments that resume a session (`{session_id}` is replaced).
    #[serde(default)]
    pub resume_cmd_template: Option<Vec<String>>,
    #[serde(default)]
    pub supports_resume: bool,
    /// Typed by Shift+Enter in this engine's TUI (newline without submit).
    #[serde(default = "default_newline")]
    pub newline_sequence: String,
    #[serde(default)]
    pub hooks_support: HooksSupport,
    /// Typed to stop the engine's current turn (F-AC-09); none = Esc.
    #[serde(default)]
    #[ts(skip)]
    pub interrupt: Option<String>,
    /// Tools whose use starts a subagent (F-AC-09), as the engine names them
    /// in its tool reports.
    #[serde(default)]
    #[ts(skip)]
    pub subagent_tools: Vec<String>,
    /// Usage logs under the provider home (globs).
    #[serde(default)]
    pub usage_log_globs: Vec<String>,
    #[serde(default)]
    pub skills_dir: Option<String>,
    /// The engine's cheapest model, for headless helper runs.
    #[serde(default)]
    pub cheap_model: Option<String>,
    /// Arguments for a one-shot run that reads its prompt on stdin and
    /// prints the answer, with no MCP servers (handoff briefs, F-CD-16).
    /// `{model}` becomes `cheap_model`; without one, it and the flag before
    /// it are left out.
    #[serde(default)]
    pub headless_cmd: Option<Vec<String>>,
    /// Where the engine reports its usage windows (F-AC-04):
    /// `codex_app_server`; none = "Usage unavailable".
    #[serde(default)]
    pub usage_probe: Option<String>,
    /// OSC titles: the first capture group is the work title.
    #[serde(default)]
    pub title_patterns: Vec<String>,
    /// Output that means the TUI is ready for input.
    #[serde(default)]
    pub ready_patterns: Vec<String>,
    /// Output that means the engine is asking the user something.
    #[serde(default)]
    pub waiting_patterns: Vec<String>,
    /// Output that means a usage or rate limit was hit.
    #[serde(default)]
    pub limit_patterns: Vec<String>,
    #[serde(default)]
    pub onboarding_seed: Option<OnboardingSeed>,
    #[serde(default)]
    pub mcp_native: Option<McpNative>,
    /// Its instructions files (F-EN-07); none = Unknown.
    #[serde(default)]
    pub instructions: Option<InstructionsSpec>,
    /// One line about who can use it, shown with its tile (F-EN-09).
    #[serde(default)]
    pub note: Option<String>,
    /// How it's installed and updated (F-EN-08).
    #[serde(default)]
    pub updates: Option<UpdatesSpec>,
}

/// Why a registry entry was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("engine {id:?}: {problem}")]
pub struct RegistryError {
    pub id: String,
    pub problem: String,
}

fn bare_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub(crate) fn env_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase() || c == '_')
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Variables a Pin may never set: any engine's provider home, and anything
/// that changes what a process loads or where it looks (F-EN-08).
pub(crate) fn pin_reserved(name: &str, own_home: Option<&str>) -> bool {
    const RESERVED: &[&str] = &[
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "GEMINI_CLI_HOME",
        "KIRO_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "HOME",
        "PATH",
        "SHELL",
        "NODE_OPTIONS",
        "NODE_PATH",
        "BASH_ENV",
        "ENV",
        "ZDOTDIR",
    ];
    RESERVED.contains(&name)
        || own_home == Some(name)
        || name.starts_with("LD_")
        || name.starts_with("DYLD_")
        || name.starts_with("TANDEM_")
}

impl EngineSpec {
    /// The schema's rules beyond its types (F-EN-02).
    pub fn validate(&self) -> Result<(), RegistryError> {
        let fail = |problem: &str| {
            Err(RegistryError {
                id: self.id.clone(),
                problem: problem.into(),
            })
        };
        if self.id.is_empty()
            || self.id.len() > 32
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return fail("ids are 1–32 lowercase letters, digits or dashes");
        }
        if self.label.trim().is_empty() || self.label.chars().count() > 40 {
            return fail("labels are 1–40 characters");
        }
        if self.logo.is_empty() || self.logo.chars().count() > 2 {
            return fail("the glyph is one or two letters");
        }
        if self.commands.is_empty() || !self.commands.iter().all(|c| bare_name(c)) {
            return fail("commands are bare executable names");
        }
        if self
            .version_cmd
            .iter()
            .chain(self.login_cmd.iter().flatten())
            .chain(self.headless_cmd.iter().flatten())
            .any(|a| a.chars().any(char::is_control))
        {
            return fail("arguments can't hold control characters");
        }
        for cmd in [&self.install.mac, &self.install.lin, &self.install.win]
            .into_iter()
            .flatten()
        {
            if cmd.trim().is_empty() || cmd.chars().any(char::is_control) || cmd.contains("sudo ") {
                return fail("install commands are one line, without sudo");
            }
        }
        if let Some(probe) = &self.usage_probe {
            if probe != "codex_app_server" {
                return fail("usage_probe is codex_app_server");
            }
        }
        if let Some(f) = &self.signed_in_file {
            if !bare_name(f) || f.starts_with("..") {
                return fail("signed_in_file is a file name in the provider home");
            }
        }
        if let Some(var) = &self.provider_home_env {
            if !env_name(var) {
                return fail("provider_home_env is an environment variable name");
            }
        }
        if self.home_strategy == HomeStrategy::Env && self.provider_home_env.is_none() {
            return fail("home_strategy env needs provider_home_env");
        }
        if self.chat_adapter.as_deref() == Some("acp") && self.acp_cmd.is_none() {
            return fail("an ACP engine needs acp_cmd");
        }
        if let Some(adapter) = &self.chat_adapter {
            if !matches!(adapter.as_str(), "claude" | "codex" | "acp") {
                return fail("chat_adapter is claude, codex or acp");
            }
        }
        if self.supports_resume != self.resume_cmd_template.is_some() {
            return fail("supports_resume goes with resume_cmd_template");
        }
        if let Some(t) = &self.resume_cmd_template {
            if !t.iter().any(|a| a.contains("{session_id}")) {
                return fail("resume_cmd_template needs {session_id}");
            }
        }
        match regex::Regex::new(&self.version_regex) {
            Ok(re) if re.captures_len() >= 2 => {}
            _ => return fail("version_regex is a regex with a capture group"),
        }
        for p in self
            .title_patterns
            .iter()
            .chain(&self.ready_patterns)
            .chain(&self.waiting_patterns)
            .chain(&self.limit_patterns)
        {
            if regex::Regex::new(p).is_err() {
                return fail("a pattern isn't a valid regex");
            }
        }
        if self.newline_sequence.is_empty() || self.newline_sequence.len() > 8 {
            return fail("newline_sequence is 1–8 bytes");
        }
        if self
            .note
            .as_deref()
            .is_some_and(|n| n.trim().is_empty() || n.chars().count() > 300 || n.contains('\n'))
        {
            return fail("a note is one line of at most 300 characters");
        }
        if let Some(u) = &self.updates {
            let npm_ok = u.npm.as_deref().is_none_or(|n| {
                !n.is_empty()
                    && n.len() <= 100
                    && n.chars().all(|c| {
                        c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '-' | '_' | '.')
                    })
            });
            let dir_ok = u.native_dir.as_deref().is_none_or(instructions_path);
            let install_ok = u
                .native_install
                .as_deref()
                .is_none_or(|i| i.contains("{version}") && !i.contains('\n'));
            let pin_ok = u.pin.as_ref().is_none_or(|p| {
                p.env
                    .keys()
                    .all(|k| env_name(k) && !pin_reserved(k, self.provider_home_env.as_deref()))
                    && p.env
                        .values()
                        .chain(&p.args)
                        .all(|v| !crate::launch::looks_secret(v) && !v.contains('\n'))
            });
            if !(npm_ok && dir_ok && install_ok && pin_ok) {
                return fail("updates: an npm package name, a folder in the home, an install command with {version}, and plain pin values");
            }
        }
        if let Some(ins) = &self.instructions {
            let pointer = ins.pointer.as_ref().map(|p| p.file.as_str());
            if ins.files.is_empty()
                || ins.files.len() > 16
                || !ins
                    .files
                    .iter()
                    .map(String::as_str)
                    .chain(ins.fallback.as_deref())
                    .chain(pointer)
                    .all(instructions_path)
            {
                return fail("instructions files are 1–16 relative paths inside the repository");
            }
            if let Some(p) = &ins.pointer {
                if p.import.trim().is_empty()
                    || p.import.len() > 200
                    || p.import.contains(['\n', '\r'])
                    || p.file.ends_with('/')
                {
                    return fail("an instructions pointer is one line in a file");
                }
            }
        }
        Ok(())
    }
}

/// A relative, `/`-separated path with no `.` or `..` parts (F-EN-07).
fn instructions_path(p: &str) -> bool {
    let body = p.strip_suffix('/').unwrap_or(p);
    !body.is_empty()
        && p.len() <= 200
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains(':')
        && body
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != "..")
}

#[derive(Debug, Clone)]
pub struct EngineRegistry {
    entries: Vec<EngineSpec>,
}

const BUNDLED: &str = include_str!("../registry.json");

/// Recursive JSON merge: objects merge key by key, anything else replaces.
fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                merge(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (b, o) => *b = o.clone(),
    }
}

impl EngineRegistry {
    pub fn bundled() -> Self {
        let entries: Vec<EngineSpec> = serde_json::from_str(BUNDLED).unwrap_or_else(|err| {
            tracing::error!(error = %err, "the bundled engine registry doesn't parse");
            Vec::new()
        });
        Self { entries }
    }

    /// The bundled list with remote entries applied: an entry whose `id`
    /// matches overrides that engine's fields; a new `id` adds an engine.
    /// Entries that don't parse or validate are skipped.
    pub fn with_overrides(overrides: &[Value]) -> Self {
        let mut reg = Self::bundled();
        for over in overrides {
            let Some(id) = over.get("id").and_then(Value::as_str) else {
                tracing::warn!(
                    code = "engines.override_skipped",
                    "a remote engine entry has no id"
                );
                continue;
            };
            let base = reg
                .entries
                .iter()
                .position(|e| e.id == id)
                .and_then(|i| serde_json::to_value(&reg.entries[i]).ok().map(|v| (i, v)));
            let (index, mut value) = match base {
                Some((i, v)) => (Some(i), v),
                None => (None, Value::Object(Default::default())),
            };
            merge(&mut value, over);
            match serde_json::from_value::<EngineSpec>(value)
                .map_err(|e| RegistryError {
                    id: id.into(),
                    problem: e.to_string(),
                })
                .and_then(|spec| spec.validate().map(|()| spec))
            {
                Ok(spec) => match index {
                    Some(i) => reg.entries[i] = spec,
                    None => reg.entries.push(spec),
                },
                Err(err) => {
                    tracing::warn!(code = "engines.override_skipped", error = %err, "a remote engine entry was refused")
                }
            }
        }
        reg
    }

    /// The remote config's `registry_overrides`, if any.
    pub fn from_config(config: &Value) -> Self {
        match config.get("registry_overrides").and_then(Value::as_array) {
            Some(list) => Self::with_overrides(list),
            None => Self::bundled(),
        }
    }

    pub fn all(&self) -> &[EngineSpec] {
        &self.entries
    }

    /// Adds an entry unless its id is taken (ACP agents the user added,
    /// F-EN-10).
    pub fn add(&mut self, spec: EngineSpec) {
        if self.get(&spec.id).is_none() {
            self.entries.push(spec);
        }
    }

    pub fn get(&self, id: &str) -> Option<&EngineSpec> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Every entry against the schema; ids are unique.
    pub fn validate(&self) -> Result<(), RegistryError> {
        let mut seen = HashSet::new();
        for e in &self.entries {
            e.validate()?;
            if !seen.insert(e.id.as_str()) {
                return Err(RegistryError {
                    id: e.id.clone(),
                    problem: "duplicate id".into(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn f_en_02_the_bundled_registry_has_thirteen_valid_engines() {
        let reg = EngineRegistry::bundled();
        assert_eq!(reg.all().len(), 13);
        reg.validate().expect("valid");
        let claude = reg.get("claude").expect("claude");
        assert_eq!(claude.commands, vec!["claude"]);
        assert_eq!(
            claude.provider_home_env.as_deref(),
            Some("CLAUDE_CONFIG_DIR")
        );
        assert_eq!(claude.keychain_isolation, KeychainIsolation::UnsupportedMac);
        assert_eq!(claude.hooks_support, HooksSupport::ClaudeHooks);
        assert_eq!(
            reg.get("opencode").map(|e| e.home_strategy),
            Some(HomeStrategy::Xdg)
        );
        assert_eq!(
            reg.get("codex").map(|e| e.login_mode),
            Some(LoginMode::Argv)
        );
        for e in reg.all() {
            assert!(
                e.install.mac.is_some() && e.install.lin.is_some() && e.install.win.is_some(),
                "{} install",
                e.id
            );
        }
        assert!(reg.get("nope").is_none());
    }

    #[test]
    fn f_en_02_1_a_remote_entry_adds_an_engine_with_no_code_change() {
        let reg = EngineRegistry::with_overrides(&[json!({
            "id": "zed-agent", "label": "Zed Agent", "vendor": "Zed", "logo": "Z",
            "commands": ["zed-agent"], "install": { "lin": "curl -fsSL https://example.test/i.sh | sh" }
        })]);
        let zed = reg.get("zed-agent").expect("added");
        assert_eq!(zed.version_cmd, vec!["--version"]);
        assert_eq!(zed.newline_sequence, "\u{1b}\r");
        assert_eq!(reg.all().len(), 14);
    }

    #[test]
    fn a_remote_entry_overrides_fields_of_a_bundled_one() {
        let reg = EngineRegistry::with_overrides(&[
            json!({ "id": "gemini", "install": { "lin": "npm install -g @google/gemini-cli@latest" } }),
        ]);
        let g = reg.get("gemini").expect("gemini");
        assert_eq!(
            g.install.lin.as_deref(),
            Some("npm install -g @google/gemini-cli@latest")
        );
        // Untouched fields stay.
        assert_eq!(
            g.install.mac.as_deref(),
            Some("npm install -g @google/gemini-cli")
        );
        assert_eq!(g.label, "Gemini CLI");
    }

    #[test]
    fn bad_remote_entries_are_dropped_and_never_break_the_bundled_ones() {
        let reg = EngineRegistry::with_overrides(&[
            json!({ "id": "claude", "commands": ["claude; rm -rf ~"] }),
            json!({ "id": "x", "label": "X", "vendor": "X", "logo": "X", "commands": ["x"], "install": { "lin": "sudo apt install x" } }),
            json!({ "id": "y", "label": "Y" }),
            json!({ "label": "no id" }),
        ]);
        assert_eq!(
            reg.get("claude").map(|e| e.commands.clone()),
            Some(vec!["claude".to_string()])
        );
        assert!(reg.get("x").is_none() && reg.get("y").is_none());
        assert_eq!(reg.all().len(), 13);
    }

    #[test]
    fn the_schema_rules() {
        let base = EngineRegistry::bundled()
            .get("claude")
            .cloned()
            .expect("claude");
        let bad = |f: fn(&mut EngineSpec)| {
            let mut e = base.clone();
            f(&mut e);
            e.validate().is_err()
        };
        assert!(bad(|e| e.id = "Claude".into()));
        assert!(bad(|e| e.logo = "ABC".into()));
        assert!(bad(|e| e.provider_home_env = Some("claude-dir".into())));
        assert!(bad(|e| e.version_regex = r"\d+".into()));
        assert!(bad(|e| e.ready_patterns.push("(".into())));
        assert!(bad(
            |e| e.resume_cmd_template = Some(vec!["--resume".into()])
        ));
        assert!(bad(|e| e.chat_adapter = Some("magic".into())));
        assert!(bad(|e| e.install.mac = Some("a\nb".into())));
        base.validate().expect("the original is fine");
    }

    #[test]
    fn f_en_07_the_instructions_table_names_six_engines_and_rejects_paths_outside_the_repository() {
        let reg = EngineRegistry::bundled();
        let with: Vec<&str> = reg
            .all()
            .iter()
            .filter(|e| e.instructions.is_some())
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(
            with,
            vec![
                "claude",
                "codex",
                "cursor-agent",
                "gemini",
                "copilot",
                "antigravity"
            ]
        );
        let claude = reg
            .get("claude")
            .and_then(|e| e.instructions.clone())
            .expect("claude");
        assert_eq!(claude.fallback.as_deref(), Some("AGENTS.md"));
        assert_eq!(
            claude.pointer.map(|p| (p.file, p.import)),
            Some(("CLAUDE.md".into(), "@AGENTS.md".into()))
        );
        let codex = reg
            .get("codex")
            .and_then(|e| e.instructions.clone())
            .expect("codex");
        assert_eq!(codex.mode, InstructionsMode::First);
        assert!(reg
            .get("aider")
            .and_then(|e| e.instructions.clone())
            .is_none());
        for bad in [
            "../AGENTS.md",
            "/etc/passwd",
            "a/../b",
            "C:\\x",
            "./AGENTS.md",
            "",
        ] {
            let mut spec = reg.get("gemini").cloned().expect("gemini");
            spec.instructions = Some(InstructionsSpec {
                files: vec![bad.into()],
                mode: InstructionsMode::All,
                fallback: None,
                pointer: None,
            });
            assert!(spec.validate().is_err(), "{bad:?}");
        }
        let mut spec = reg.get("gemini").cloned().expect("gemini");
        if let Some(ins) = spec.instructions.as_mut() {
            ins.pointer = Some(InstructionsPointer {
                file: "GEMINI.md".into(),
                import: "@AGENTS.md\nrm -rf ~".into(),
            });
        }
        assert!(spec.validate().is_err());
    }

    #[test]
    fn f_en_09_antigravity_and_kiro_are_terminal_engines_from_their_vendors_docs() {
        let reg = EngineRegistry::bundled();
        let agy = reg.get("antigravity").expect("antigravity");
        assert_eq!(agy.commands, vec!["agy"]);
        assert_eq!(agy.vendor, "Google");
        assert_eq!(
            agy.home_strategy,
            HomeStrategy::None,
            "one account per computer"
        );
        assert!(agy
            .install
            .win
            .as_deref()
            .is_some_and(|w| w.contains("install.ps1")));
        let kiro = reg.get("kiro").expect("kiro");
        assert_eq!(kiro.commands, vec!["kiro-cli"]);
        assert_eq!(kiro.provider_home_env.as_deref(), Some("KIRO_HOME"));
        assert_eq!(kiro.home_strategy, HomeStrategy::Env);
        assert_eq!(kiro.login_cmd, Some(vec!["login".to_string()]));
        assert_eq!(kiro.login_mode, LoginMode::Argv);
        // F-EN-09.3: terminal only, and never headless.
        for e in [agy, kiro] {
            assert!(e.chat_adapter.is_none() && e.acp_cmd.is_none(), "{}", e.id);
            assert!(e.headless_cmd.is_none(), "{}", e.id);
        }
        // F-EN-09.2: Gemini CLI stays, with its note.
        let gemini = reg.get("gemini").expect("gemini");
        assert!(gemini
            .note
            .as_deref()
            .is_some_and(|n| n.contains("API keys") && n.contains("Antigravity CLI")));
        let mut bad = gemini.clone();
        bad.note = Some("two\nlines".into());
        assert!(bad.validate().is_err());
    }
}
