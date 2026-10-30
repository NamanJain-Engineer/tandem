//! Provider homes (ARCHITECTURE §6.4, F-AC-01).
//!
//! Each account of an engine gets its own folder, `$PROVIDERS/<engine>/<id>/`
//! (mode 0700), which the engine is pointed at through its
//! `provider_home_env`. The engine signs in there by itself; Tandem only
//! writes the files that skip first-run screens and report status, and
//! every write merges into what's there so the user's own keys survive.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use serde_json::{json, Map, Value};

use crate::registry::{EngineSpec, HooksSupport};

/// Claude hook events and the status each reports (§6.6).
pub const CLAUDE_HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "Notification",
    "Stop",
    "SubagentStop",
    "SessionEnd",
];
/// Marks the hook groups Tandem owns, so re-writes replace only those.
pub const TANDEM_MARKER: &str = "_tandem";
const ONE_OFF_DIR: &str = "_oneoff";

#[derive(Debug, thiserror::Error)]
pub enum HomeError {
    #[error("couldn't write the provider home: {0}")]
    Io(#[from] io::Error),
    #[error("{file} isn't valid {kind}; leaving it as it is")]
    Parse { file: String, kind: &'static str },
    #[error("{file}'s {key} isn't a table; leaving it as it is")]
    Unexpected { file: String, key: &'static str },
    #[error("that folder isn't a Vibezeo provider home")]
    NotOwned,
}

/// One lock per home, held across each of Vibezeo's own reads, changes and
/// writes of an engine's settings there (D-3070, D-3073, D-3075): Codex's
/// `config.toml` (a local model's settings, the status hook, the tasks
/// server) and Claude Code's `.claude.json` and `settings.json` (onboarding,
/// a trusted folder, the tasks server, the hooks). Panes starting at once
/// then each keep what they wrote. The engine itself may still rewrite the
/// file, and no lock inside Vibezeo covers that.
///
/// The key is the home's path as given, not canonicalised. That's enough
/// because every writer's home comes from [`ProviderHomes`]: its root joined
/// with plain names ([`ProviderHomes::path`], `create`, a local model's
/// home), or a saved launch that stored one of those. [`ProviderHomes::owns`]
/// refuses any other spelling under the root (a `..`), and a local pane's
/// home must equal its model's own path. A symlink to a home would get a
/// lock of its own.
pub fn config_lock(home: &Path) -> Arc<Mutex<()>> {
    static HOMES: LazyLock<Mutex<BTreeMap<PathBuf, Arc<Mutex<()>>>>> =
        LazyLock::new(Default::default);
    let mut homes = HOMES.lock().unwrap_or_else(PoisonError::into_inner);
    Arc::clone(homes.entry(home.to_path_buf()).or_default())
}

/// What to put in a fresh home.
#[derive(Debug, Clone, Default)]
pub struct Seed {
    /// `dark` or `light`, matching the app.
    pub theme: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneOffHome {
    pub id: String,
    pub path: PathBuf,
}

/// All provider homes under one root (`$DATA/providers`).
#[derive(Debug, Clone)]
pub struct ProviderHomes {
    root: PathBuf,
}

impl ProviderHomes {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, engine_id: &str, account_id: &str) -> PathBuf {
        self.root.join(engine_id).join(account_id)
    }

    /// Creates (or re-seeds) an account's home: 0700, onboarding seeded.
    pub fn create(
        &self,
        engine: &EngineSpec,
        account_id: &str,
        seed: &Seed,
    ) -> Result<PathBuf, HomeError> {
        if account_id.is_empty() || account_id.contains(['/', '\\']) || account_id.starts_with('.')
        {
            return Err(HomeError::NotOwned);
        }
        let home = self.path(&engine.id, account_id);
        private_dir(&self.root)?;
        private_dir(&self.root.join(&engine.id))?;
        private_dir(&home)?;
        seed_onboarding(engine, &home, seed)?;
        Ok(home)
    }

    /// A throwaway home for one pane, deleted on close unless kept.
    pub fn create_one_off(
        &self,
        engine: &EngineSpec,
        seed: &Seed,
    ) -> Result<OneOffHome, HomeError> {
        let id = ulid::Ulid::new().to_string().to_ascii_lowercase();
        let path = self.root.join(ONE_OFF_DIR).join(&id);
        private_dir(&self.root)?;
        private_dir(&self.root.join(ONE_OFF_DIR))?;
        private_dir(&path)?;
        seed_onboarding(engine, &path, seed)?;
        Ok(OneOffHome { id, path })
    }

    /// Deletes a one-off home; refuses anything outside `_oneoff/`.
    pub fn remove_one_off(&self, path: &Path) -> Result<(), HomeError> {
        let base = self.root.join(ONE_OFF_DIR);
        let inside = path.parent() == Some(base.as_path())
            && path
                .file_name()
                .is_some_and(|n| !n.is_empty() && n != "." && n != "..");
        if !inside {
            return Err(HomeError::NotOwned);
        }
        match std::fs::remove_dir_all(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// Deletes an account's home (`<root>/<engine>/<account>`); refuses
    /// anything else, so a stored path can never point it at a user folder.
    pub fn remove(&self, path: &Path) -> Result<(), HomeError> {
        let rel = path
            .strip_prefix(&self.root)
            .map_err(|_| HomeError::NotOwned)?;
        let parts: Vec<_> = rel.components().collect();
        let plain = parts
            .iter()
            .all(|c| matches!(c, std::path::Component::Normal(n) if !n.is_empty()));
        if parts.len() != 2 || !plain || rel.starts_with(ONE_OFF_DIR) {
            return Err(HomeError::NotOwned);
        }
        match std::fs::remove_dir_all(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// True when `path` is one of this root's homes (never a user folder):
    /// exactly `<root>/<engine or _oneoff>/<home>`, plain names only, so a
    /// `..` can't lead out of it, a home has one spelling ([`config_lock`])
    /// and neither the root, an engine's folder nor a folder inside a home
    /// counts (D-3077).
    pub fn owns(&self, path: &Path) -> bool {
        path.strip_prefix(&self.root).is_ok_and(|rel| {
            rel.components().count() == 2
                && rel
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
        })
    }

    /// Points the engine's status hooks at `hook_bin` (§6.6), merging into
    /// the engine's own settings file.
    pub fn install_hooks(
        &self,
        engine: &EngineSpec,
        home: &Path,
        hook_bin: &Path,
    ) -> Result<(), HomeError> {
        if !self.owns(home) {
            return Err(HomeError::NotOwned);
        }
        let lock = config_lock(home);
        let _held = lock.lock().unwrap_or_else(PoisonError::into_inner);
        match engine.hooks_support {
            HooksSupport::ClaudeHooks => {
                let file = home.join("settings.json");
                let mut doc = read_json(&file)?;
                set_claude_hooks(&mut doc, hook_bin);
                write_json(&file, &doc)
            }
            HooksSupport::CodexNotify => {
                let file = home.join("config.toml");
                let mut doc = read_toml(&file)?;
                let mut notify = toml_edit::Array::new();
                notify.push(hook_bin.to_string_lossy().into_owned());
                notify.push("codex");
                doc["notify"] = toml_edit::value(notify);
                write_private(&file, doc.to_string().as_bytes())
            }
            HooksSupport::None => Ok(()),
        }
    }

    /// Adds (or with `None` removes) a stdio MCP server in the engine's own
    /// config in this home: `.claude.json` `mcpServers` for Claude Code,
    /// `config.toml` `[mcp_servers.<name>]` for Codex (F-TK-02, "Expose
    /// tasks to terminal agents"). Other engines are left alone.
    pub fn set_stdio_server(
        &self,
        engine: &EngineSpec,
        home: &Path,
        name: &str,
        server: Option<&crate::chat::StdioServer>,
    ) -> Result<(), HomeError> {
        if !self.owns(home) {
            return Err(HomeError::NotOwned);
        }
        let lock = config_lock(home);
        let _held = lock.lock().unwrap_or_else(PoisonError::into_inner);
        match engine.hooks_support {
            HooksSupport::ClaudeHooks => {
                let file = home.join(".claude.json");
                let mut doc = read_json(&file)?;
                let Some(root) = doc.as_object_mut() else {
                    return Ok(());
                };
                let servers = root.entry("mcpServers").or_insert_with(|| json!({}));
                let Some(servers) = servers.as_object_mut() else {
                    return Ok(());
                };
                match server {
                    Some(s) => {
                        servers.insert(
                            name.to_string(),
                            json!({
                                "type": "stdio",
                                "command": s.command,
                                "args": s.args,
                                "env": {},
                            }),
                        );
                    }
                    None => {
                        if servers.remove(name).is_none() {
                            return Ok(());
                        }
                    }
                }
                write_json(&file, &doc)
            }
            HooksSupport::CodexNotify => {
                let file = home.join("config.toml");
                let mut doc = read_toml(&file)?;
                let had = doc.get("mcp_servers").and_then(|t| t.get(name)).is_some();
                match server {
                    Some(s) => {
                        if doc.get("mcp_servers").is_none() {
                            let mut t = toml_edit::Table::new();
                            t.set_implicit(true);
                            doc["mcp_servers"] = toml_edit::Item::Table(t);
                        }
                        // A hand-edited `mcp_servers = 1` is refused, the file
                        // left as it is (D-3075, D-3077).
                        let Some(servers) = doc
                            .get_mut("mcp_servers")
                            .and_then(toml_edit::Item::as_table_like_mut)
                        else {
                            return Err(HomeError::Unexpected {
                                file: display_name(&file),
                                key: "mcp_servers",
                            });
                        };
                        let mut t = toml_edit::Table::new();
                        t["command"] = toml_edit::value(s.command.to_string_lossy().into_owned());
                        let mut args = toml_edit::Array::new();
                        for a in &s.args {
                            args.push(a.as_str());
                        }
                        t["args"] = toml_edit::value(args);
                        if !s.env_vars.is_empty() {
                            let mut vars = toml_edit::Array::new();
                            for v in &s.env_vars {
                                vars.push(v.as_str());
                            }
                            t["env_vars"] = toml_edit::value(vars);
                        }
                        servers.insert(name, toml_edit::Item::Table(t));
                    }
                    None => {
                        if !had {
                            return Ok(());
                        }
                        if let Some(t) = doc
                            .get_mut("mcp_servers")
                            .and_then(|t| t.as_table_like_mut())
                        {
                            t.remove(name);
                        }
                    }
                }
                write_private(&file, doc.to_string().as_bytes())
            }
            HooksSupport::None => Ok(()),
        }
    }

    /// Pre-answers the engine's "trust this folder" question for one
    /// workspace. Called only when "Trust workspaces I add" is on.
    pub fn trust_workspace(
        &self,
        engine: &EngineSpec,
        home: &Path,
        workspace: &Path,
    ) -> Result<(), HomeError> {
        if !self.owns(home) {
            return Err(HomeError::NotOwned);
        }
        if engine.hooks_support != HooksSupport::ClaudeHooks {
            return Ok(());
        }
        let file = home.join(".claude.json");
        let lock = config_lock(home);
        let _held = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut doc = read_json(&file)?;
        let key = workspace.to_string_lossy().into_owned();
        merge(
            &mut doc,
            &json!({ "projects": { key: { "hasTrustDialogAccepted": true } } }),
        );
        write_json(&file, &doc)
    }
}

fn seed_onboarding(engine: &EngineSpec, home: &Path, seed: &Seed) -> Result<(), HomeError> {
    let Some(onboarding) = &engine.onboarding_seed else {
        return Ok(());
    };
    let rel = Path::new(&onboarding.file);
    if rel.is_absolute()
        || rel
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(HomeError::NotOwned);
    }
    let file = home.join(rel);
    let lock = config_lock(home);
    let _held = lock.lock().unwrap_or_else(PoisonError::into_inner);
    let mut doc = read_json(&file)?;
    let theme = if seed.theme.is_empty() {
        "dark"
    } else {
        seed.theme.as_str()
    };
    merge(&mut doc, &fill(&onboarding.merge, theme));
    write_json(&file, &doc)
}

/// `{theme}` in string values becomes the app theme.
fn fill(v: &Value, theme: &str) -> Value {
    match v {
        Value::String(s) => Value::String(s.replace("{theme}", theme)),
        Value::Object(m) => {
            Value::Object(m.iter().map(|(k, v)| (k.clone(), fill(v, theme))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(|v| fill(v, theme)).collect()),
        other => other.clone(),
    }
}

/// Deep-merges `over` into `base`: objects merge key by key, everything
/// else is replaced. Keys only `base` has are kept.
/// The launch arguments that give a terminal engine one stdio MCP server
/// for this run only, where its config can't be written (a host's own
/// home, D-1606): `--mcp-config <json>` for Claude Code, `-c
/// mcp_servers.<name>.…` for Codex. Other engines get none, as locally.
pub fn inline_stdio_server(engine: &EngineSpec, server: &crate::chat::StdioServer) -> Vec<String> {
    let command = server.command.to_string_lossy().into_owned();
    match engine.hooks_support {
        HooksSupport::ClaudeHooks => vec![
            "--mcp-config".into(),
            json!({ "mcpServers": { server.name.as_str(): {
                "type": "stdio",
                "command": command,
                "args": server.args,
                "env": {},
            }}})
            .to_string(),
        ],
        HooksSupport::CodexNotify => {
            let mut args = toml_edit::Array::new();
            for a in &server.args {
                args.push(a.as_str());
            }
            vec![
                "-c".into(),
                format!(
                    "mcp_servers.{}.command={}",
                    server.name,
                    toml_edit::Value::from(command)
                ),
                "-c".into(),
                format!(
                    "mcp_servers.{}.args={}",
                    server.name,
                    toml_edit::Value::Array(args)
                ),
            ]
        }
        HooksSupport::None => Vec::new(),
    }
}

pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(existing) => merge(existing, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, o) => *b = o.clone(),
    }
}

/// Replaces Tandem's hook groups in a Claude `settings.json`, keeping the
/// user's own hooks and every other key.
pub fn set_claude_hooks(doc: &mut Value, hook_bin: &Path) {
    if !doc.is_object() {
        *doc = Value::Object(Map::new());
    }
    let Some(root) = doc.as_object_mut() else {
        return;
    };
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if !hooks.is_object() {
        *hooks = Value::Object(Map::new());
    }
    let Some(hooks) = hooks.as_object_mut() else {
        return;
    };
    for event in CLAUDE_HOOK_EVENTS {
        let groups = hooks
            .entry(*event)
            .or_insert_with(|| Value::Array(Vec::new()));
        if !groups.is_array() {
            *groups = Value::Array(Vec::new());
        }
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        groups.retain(|g| g.get(TANDEM_MARKER) != Some(&Value::Bool(true)));
        let command = format!("{} claude {event}", quote_path(hook_bin));
        let mut group = json!({
            TANDEM_MARKER: true,
            "hooks": [{ "type": "command", "command": command }],
        });
        if *event == "PreToolUse" {
            group["matcher"] = json!("*");
        }
        groups.push(group);
    }
}

/// Quotes the hook path for the shell the engine runs hooks with.
fn quote_path(p: &Path) -> String {
    let s = p.to_string_lossy();
    if cfg!(windows) {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

fn read_json(file: &Path) -> Result<Value, HomeError> {
    match std::fs::read(file) {
        Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => Ok(Value::Object(Map::new())),
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(v) if v.is_object() => Ok(v),
            _ => Err(HomeError::Parse {
                file: display_name(file),
                kind: "JSON",
            }),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(e) => Err(e.into()),
    }
}

fn write_json(file: &Path, doc: &Value) -> Result<(), HomeError> {
    let mut bytes = serde_json::to_vec_pretty(doc).map_err(io::Error::other)?;
    bytes.push(b'\n');
    write_private(file, &bytes)
}

fn read_toml(file: &Path) -> Result<toml_edit::DocumentMut, HomeError> {
    match std::fs::read_to_string(file) {
        Ok(text) => text.parse().map_err(|_| HomeError::Parse {
            file: display_name(file),
            kind: "TOML",
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(toml_edit::DocumentMut::new()),
        Err(e) => Err(e.into()),
    }
}

fn display_name(file: &Path) -> String {
    file.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Writes through a temp file and a rename, mode 0600.
fn write_private(file: &Path, bytes: &[u8]) -> Result<(), HomeError> {
    let dir = file.parent().ok_or(HomeError::NotOwned)?;
    std::fs::create_dir_all(dir)?;
    // Unique per write: panes launched together seed the same home.
    let tmp = dir.join(format!(
        ".{}.{}.tandem-tmp",
        file.file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default(),
        ulid::Ulid::new()
    ));
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, file) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// Makes `dir` if it's missing, and leaves it readable by its owner only.
pub fn private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    #[test]
    fn f_ac_02_1_removing_a_home_deletes_only_an_account_home() {
        let dir = tempfile::tempdir().expect("tmp");
        let homes = ProviderHomes::new(dir.path().join("providers"));
        let home = homes.path("claude", "acc_1");
        std::fs::create_dir_all(home.join("x")).expect("home");
        let outside = dir.path().join("mine");
        std::fs::create_dir_all(&outside).expect("outside");
        for refused in [
            outside.clone(),
            homes.root().to_path_buf(),
            homes.root().join("claude"),
            homes.root().join("claude/acc_1/x"),
            homes.root().join("claude/../../mine"),
        ] {
            assert!(homes.remove(&refused).is_err(), "{}", refused.display());
        }
        assert!(outside.is_dir());
        homes.remove(&home).expect("removed");
        assert!(!home.exists());
        homes.remove(&home).expect("already gone is fine");
    }
    use super::*;

    #[test]
    fn f_rm_01_f_tk_02_a_hosts_terminal_engine_gets_its_server_inline() {
        let reg = crate::registry::EngineRegistry::bundled();
        let server = crate::chat::StdioServer {
            name: "tandem_tasks".into(),
            command: PathBuf::from("/home/u/.tandem-host/1/tandem-host"),
            args: vec![
                "mcp".into(),
                "--channel".into(),
                "0123456789abcdef0123456789abcdef".into(),
            ],
            env_vars: Vec::new(),
        };
        let claude = inline_stdio_server(reg.get("claude").expect("claude"), &server);
        assert_eq!(claude[0], "--mcp-config");
        let v: Value = serde_json::from_str(&claude[1]).expect("json");
        assert_eq!(
            v["mcpServers"]["tandem_tasks"],
            json!({"type": "stdio", "command": "/home/u/.tandem-host/1/tandem-host",
                   "args": ["mcp", "--channel", "0123456789abcdef0123456789abcdef"], "env": {}})
        );
        let codex = inline_stdio_server(reg.get("codex").expect("codex"), &server);
        assert_eq!(
            codex,
            [
                "-c",
                "mcp_servers.tandem_tasks.command=\"/home/u/.tandem-host/1/tandem-host\"",
                "-c",
                "mcp_servers.tandem_tasks.args=[\"mcp\", \"--channel\", \"0123456789abcdef0123456789abcdef\"]",
            ]
        );
        // The channel id isn't taken for a secret, so the launch goes ahead.
        for a in claude.iter().chain(&codex) {
            assert!(!crate::launch::looks_secret(a), "{a}");
        }
        // Engines with no MCP config of their own get nothing, as locally.
        let other = reg
            .all()
            .iter()
            .find(|e| e.hooks_support == HooksSupport::None)
            .expect("one more engine");
        assert!(inline_stdio_server(other, &server).is_empty());
    }
    use crate::EngineRegistry;

    fn setup() -> (tempfile::TempDir, ProviderHomes, EngineRegistry) {
        let dir = tempfile::tempdir().expect("dir");
        let homes = ProviderHomes::new(dir.path().join("providers"));
        (dir, homes, EngineRegistry::bundled())
    }

    #[cfg(unix)]
    fn mode(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).expect("meta").permissions().mode() & 0o777
    }

    #[test]
    fn f_ac_01_1_each_account_gets_its_own_private_home_with_onboarding_seeded() {
        let (_d, homes, reg) = setup();
        let claude = reg.get("claude").expect("claude");
        let a = homes
            .create(
                claude,
                "acc_work",
                &Seed {
                    theme: "light".into(),
                },
            )
            .expect("a");
        let b = homes
            .create(claude, "acc_personal", &Seed::default())
            .expect("b");
        assert_ne!(a, b);
        assert!(a.ends_with("providers/claude/acc_work"));
        #[cfg(unix)]
        assert_eq!(mode(&a), 0o700);
        let seeded: Value =
            serde_json::from_slice(&std::fs::read(a.join(".claude.json")).expect("seed"))
                .expect("json");
        assert_eq!(seeded["hasCompletedOnboarding"], json!(true));
        assert_eq!(seeded["theme"], json!("light"));
        #[cfg(unix)]
        assert_eq!(mode(&a.join(".claude.json")), 0o600);
    }

    #[test]
    fn f_ac_01_4_hooks_are_merged_and_the_users_keys_survive() {
        let (_d, homes, reg) = setup();
        let claude = reg.get("claude").expect("claude");
        let home = homes
            .create(claude, "acc_1", &Seed::default())
            .expect("home");
        std::fs::write(
            home.join("settings.json"),
            r#"{"model":"opus","hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}}"#,
        )
        .expect("user settings");
        let hook = Path::new("/opt/Tandem App/tandem-hook");
        homes.install_hooks(claude, &home, hook).expect("hooks");
        // Twice: re-writes replace Tandem's entries, never duplicate them.
        homes
            .install_hooks(claude, &home, hook)
            .expect("hooks again");
        let doc: Value =
            serde_json::from_slice(&std::fs::read(home.join("settings.json")).expect("read"))
                .expect("json");
        assert_eq!(doc["model"], json!("opus"));
        let stop = doc["hooks"]["Stop"].as_array().expect("stop");
        assert_eq!(stop.len(), 2, "{stop:?}");
        assert_eq!(stop[0]["hooks"][0]["command"], json!("say done"));
        assert_eq!(stop[1][TANDEM_MARKER], json!(true));
        assert_eq!(
            stop[1]["hooks"][0]["command"],
            json!("'/opt/Tandem App/tandem-hook' claude Stop")
        );
        for event in CLAUDE_HOOK_EVENTS {
            assert!(
                doc["hooks"][event].as_array().map(Vec::len).unwrap_or(0) >= 1,
                "{event}"
            );
        }
        assert_eq!(doc["hooks"]["PreToolUse"][0]["matcher"], json!("*"));
    }

    #[test]
    fn f_ac_01_4_codex_notify_is_set_in_config_toml_keeping_user_keys() {
        let (_d, homes, reg) = setup();
        let codex = reg.get("codex").expect("codex");
        let home = homes
            .create(codex, "acc_1", &Seed::default())
            .expect("home");
        std::fs::write(
            home.join("config.toml"),
            "# mine\nmodel = \"o4\"\n\n[profiles.fast]\nmodel = \"mini\"\n",
        )
        .expect("user");
        homes
            .install_hooks(codex, &home, Path::new("/x/tandem-hook"))
            .expect("notify");
        let text = std::fs::read_to_string(home.join("config.toml")).expect("read");
        assert!(text.contains("# mine"), "{text}");
        assert!(text.contains("model = \"o4\""));
        assert!(text.contains("[profiles.fast]"));
        assert!(
            text.contains(r#"notify = ["/x/tandem-hook", "codex"]"#),
            "{text}"
        );
    }

    #[test]
    fn a_broken_settings_file_is_left_alone() {
        let (_d, homes, reg) = setup();
        let claude = reg.get("claude").expect("claude");
        let home = homes
            .create(claude, "acc_1", &Seed::default())
            .expect("home");
        std::fs::write(home.join("settings.json"), "{ not json").expect("broken");
        let err = homes
            .install_hooks(claude, &home, Path::new("/x/tandem-hook"))
            .expect_err("refused");
        assert!(matches!(err, HomeError::Parse { .. }));
        assert_eq!(
            std::fs::read_to_string(home.join("settings.json")).expect("read"),
            "{ not json"
        );
    }

    #[test]
    fn trust_is_written_per_workspace_only_when_asked() {
        let (_d, homes, reg) = setup();
        let claude = reg.get("claude").expect("claude");
        let home = homes
            .create(claude, "acc_1", &Seed::default())
            .expect("home");
        homes
            .trust_workspace(claude, &home, Path::new("/work/app"))
            .expect("trust");
        let doc: Value =
            serde_json::from_slice(&std::fs::read(home.join(".claude.json")).expect("read"))
                .expect("json");
        assert_eq!(
            doc["projects"]["/work/app"]["hasTrustDialogAccepted"],
            json!(true)
        );
        // Onboarding keys stay.
        assert_eq!(doc["hasCompletedOnboarding"], json!(true));
    }

    #[test]
    fn one_off_homes_are_deleted_and_nothing_else_can_be() {
        let (d, homes, reg) = setup();
        let claude = reg.get("claude").expect("claude");
        let one = homes
            .create_one_off(claude, &Seed::default())
            .expect("one-off");
        assert!(one.path.join(".claude.json").exists());
        homes.remove_one_off(&one.path).expect("removed");
        assert!(!one.path.exists());
        let account = homes
            .create(claude, "acc_1", &Seed::default())
            .expect("home");
        assert!(matches!(
            homes.remove_one_off(&account),
            Err(HomeError::NotOwned)
        ));
        assert!(matches!(
            homes.remove_one_off(d.path()),
            Err(HomeError::NotOwned)
        ));
        assert!(account.exists());
        // Hooks never go into a folder Tandem doesn't own.
        assert!(matches!(
            homes.install_hooks(claude, d.path(), Path::new("/x")),
            Err(HomeError::NotOwned)
        ));
        assert!(matches!(
            homes.create(claude, "../escape", &Seed::default()),
            Err(HomeError::NotOwned)
        ));
    }

    #[test]
    fn f_tk_02_the_tasks_server_goes_into_terminal_engines_native_config() {
        let (_d, homes, reg) = setup();
        let server = crate::chat::StdioServer {
            name: "tandem_tasks".into(),
            command: "/opt/v/tandem-tasks".into(),
            args: vec![
                "mcp".into(),
                "--dir".into(),
                "/d/tasks".into(),
                "--connection-env".into(),
                "TANDEM_TASKS_CONNECTION".into(),
            ],
            env_vars: vec!["TANDEM_TASKS_CONNECTION".into()],
        };
        let claude = reg.get("claude").expect("claude");
        let home = homes
            .create(claude, "acc_1", &Seed::default())
            .expect("home");
        homes
            .set_stdio_server(claude, &home, "tandem_tasks", Some(&server))
            .expect("set");
        let doc: Value =
            serde_json::from_slice(&std::fs::read(home.join(".claude.json")).expect("read"))
                .expect("json");
        assert_eq!(
            doc["mcpServers"]["tandem_tasks"]["command"],
            "/opt/v/tandem-tasks"
        );
        assert_eq!(
            doc["mcpServers"]["tandem_tasks"]["args"][3],
            "--connection-env"
        );
        // Seeded onboarding keys stay.
        assert!(doc.as_object().expect("obj").len() > 1);
        homes
            .set_stdio_server(claude, &home, "tandem_tasks", None)
            .expect("unset");
        let doc: Value =
            serde_json::from_slice(&std::fs::read(home.join(".claude.json")).expect("read"))
                .expect("json");
        assert!(doc["mcpServers"].get("tandem_tasks").is_none());

        let codex = reg.get("codex").expect("codex");
        let home = homes
            .create(codex, "acc_2", &Seed::default())
            .expect("home");
        std::fs::write(home.join("config.toml"), "model = \"o4\"\n").expect("seed");
        homes
            .set_stdio_server(codex, &home, "tandem_tasks", Some(&server))
            .expect("set");
        let text = std::fs::read_to_string(home.join("config.toml")).expect("read");
        let doc: toml_edit::DocumentMut = text.parse().expect("toml");
        assert_eq!(doc["model"].as_str(), Some("o4"));
        assert_eq!(
            doc["mcp_servers"]["tandem_tasks"]["command"].as_str(),
            Some("/opt/v/tandem-tasks")
        );
        assert_eq!(
            doc["mcp_servers"]["tandem_tasks"]["env_vars"]
                .as_array()
                .and_then(|a| a.get(0))
                .and_then(|v| v.as_str()),
            Some("TANDEM_TASKS_CONNECTION")
        );
        homes
            .set_stdio_server(codex, &home, "tandem_tasks", None)
            .expect("unset");
        let text = std::fs::read_to_string(home.join("config.toml")).expect("read");
        assert!(!text.contains("tandem_tasks"), "{text}");
        assert!(text.contains("model"));
        // Not a home Vibezeo owns: refused.
        let outside = tempfile::tempdir().expect("dir");
        assert!(homes
            .set_stdio_server(claude, outside.path(), "tandem_tasks", Some(&server))
            .is_err());
    }

    fn tasks_server() -> crate::chat::StdioServer {
        crate::chat::StdioServer {
            name: "tandem_tasks".into(),
            command: "/opt/v/tandem-tasks".into(),
            args: vec!["mcp".into()],
            env_vars: Vec::new(),
        }
    }

    /// F-EN-11 follow-up (D-3075): a hand-edited Codex `mcp_servers` that
    /// isn't a table is refused as unreadable and left as it is, rather
    /// than panicking with the home's lock held; an inline table takes the
    /// server beside its own.
    #[test]
    fn f_tk_02_a_codex_mcp_servers_that_isnt_a_table_is_refused() {
        let (_d, homes, reg) = setup();
        let codex = reg.get("codex").expect("codex");
        let home = homes
            .create(codex, "acc_1", &Seed::default())
            .expect("home");
        let server = tasks_server();
        for odd in [
            "mcp_servers = 1\n",
            "mcp_servers = \"x\"\n",
            "mcp_servers = [1]\n",
        ] {
            std::fs::write(home.join("config.toml"), odd).expect("seed");
            let err = homes
                .set_stdio_server(codex, &home, "tandem_tasks", Some(&server))
                .expect_err(odd);
            assert!(
                matches!(
                    err,
                    HomeError::Unexpected {
                        key: "mcp_servers",
                        ..
                    }
                ),
                "{odd}: {err}"
            );
            assert_eq!(
                err.to_string(),
                "config.toml's mcp_servers isn't a table; leaving it as it is"
            );
            assert_eq!(
                std::fs::read_to_string(home.join("config.toml")).expect("read"),
                odd
            );
            assert!(!config_lock(&home).is_poisoned(), "{odd}");
        }
        std::fs::write(
            home.join("config.toml"),
            "mcp_servers = { other = { command = \"x\" } }\n",
        )
        .expect("inline");
        homes
            .set_stdio_server(codex, &home, "tandem_tasks", Some(&server))
            .expect("inline table");
        let text = std::fs::read_to_string(home.join("config.toml")).expect("read");
        let doc: toml_edit::DocumentMut = text.parse().expect("toml");
        assert_eq!(
            doc["mcp_servers"]["tandem_tasks"]["command"].as_str(),
            Some("/opt/v/tandem-tasks"),
            "{text}"
        );
        assert_eq!(doc["mcp_servers"]["other"]["command"].as_str(), Some("x"));
    }

    /// F-EN-11 follow-up (D-3075): Claude Code panes on one account starting
    /// at once each keep their writes to the home's `.claude.json`
    /// (onboarding at each start, a trusted folder, the tasks server): each
    /// of Vibezeo's writers reads, changes and writes it one at a time.
    #[test]
    fn f_ac_01_4_claude_starts_at_once_keep_every_write_to_claude_json() {
        const PANES: usize = 24;
        let (_d, homes, reg) = setup();
        let homes = std::sync::Arc::new(homes);
        let reg = std::sync::Arc::new(reg);
        let server = std::sync::Arc::new(tasks_server());
        for round in 0..4 {
            let account = format!("acc_{round}");
            let home = homes
                .create(
                    reg.get("claude").expect("claude"),
                    &account,
                    &Seed::default(),
                )
                .expect("home");
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(PANES));
            let starts: Vec<_> = (0..PANES)
                .map(|i| {
                    let (homes, reg, server) = (
                        std::sync::Arc::clone(&homes),
                        std::sync::Arc::clone(&reg),
                        std::sync::Arc::clone(&server),
                    );
                    let (barrier, account, home) = (
                        std::sync::Arc::clone(&barrier),
                        account.clone(),
                        home.clone(),
                    );
                    std::thread::spawn(move || {
                        let claude = reg.get("claude").expect("claude");
                        barrier.wait();
                        // A start's writers, in its order.
                        homes
                            .create(claude, &account, &Seed::default())
                            .expect("seeded");
                        homes
                            .trust_workspace(claude, &home, Path::new(&format!("/work/pane-{i}")))
                            .expect("trusted");
                        homes
                            .set_stdio_server(claude, &home, "tandem_tasks", Some(&server))
                            .expect("tasks");
                    })
                })
                .collect();
            for start in starts {
                start.join().expect("thread");
            }
            let doc: Value =
                serde_json::from_slice(&std::fs::read(home.join(".claude.json")).expect("read"))
                    .expect("json");
            for i in 0..PANES {
                assert_eq!(
                    doc["projects"][format!("/work/pane-{i}")]["hasTrustDialogAccepted"],
                    json!(true),
                    "round {round}: pane-{i} lost"
                );
            }
            assert_eq!(
                doc["mcpServers"]["tandem_tasks"]["command"],
                json!("/opt/v/tandem-tasks"),
                "round {round}: the tasks server lost"
            );
            assert_eq!(doc["hasCompletedOnboarding"], json!(true), "round {round}");
        }
    }

    /// F-EN-11 follow-up (D-3075): a home is spelled one way only: a path
    /// under the root through `..` (or out of it) isn't one of its homes,
    /// so nothing is written there and no home has two locks.
    #[test]
    fn f_ac_01_4_a_home_reached_through_dotdot_is_not_owned() {
        let (_d, homes, reg) = setup();
        let claude = reg.get("claude").expect("claude");
        let home = homes
            .create(claude, "acc_1", &Seed::default())
            .expect("home");
        assert!(homes.owns(&home));
        let around = homes
            .root()
            .join("claude")
            .join("..")
            .join("claude")
            .join("acc_1");
        let out = homes.root().join("claude").join("..").join("..");
        // Only a home itself: not an engine's folder, nor one inside a home.
        let engine = homes.root().join("claude");
        let inside = home.join("projects");
        std::fs::create_dir_all(&inside).expect("inside");
        for path in [around, out, homes.root().to_path_buf(), engine, inside] {
            assert!(!homes.owns(&path), "{}", path.display());
            assert!(matches!(
                homes.install_hooks(claude, &path, Path::new("/x/tandem-hook")),
                Err(HomeError::NotOwned)
            ));
            assert!(matches!(
                homes.trust_workspace(claude, &path, Path::new("/work/app")),
                Err(HomeError::NotOwned)
            ));
        }
    }
}
