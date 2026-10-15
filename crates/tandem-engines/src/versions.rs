//! Engine versions under the user's control (F-EN-08, ARCHITECTURE §2.5):
//! how an engine was installed, read from where its command resolves; the
//! versions Vibezeo has seen and the last one that ran a Thread turn
//! without an error; and the command that rolls an engine back.
//!
//! Nothing here touches the network or the engine's own files.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::registry::{EngineSpec, InstallMethod, UpdatesSpec};

/// How an engine was installed (D-2406): its command, resolved through any
/// links, under `node_modules` is npm; under a Homebrew `Cellar` or
/// `Caskroom` is Homebrew; under the engine's own versions folder in the
/// user's home is its own installer; anything else is Other.
pub fn install_method(
    resolved: &Path,
    home: &Path,
    updates: Option<&UpdatesSpec>,
) -> InstallMethod {
    let parts: Vec<String> = resolved
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.iter().any(|p| p == "node_modules") {
        return InstallMethod::Npm;
    }
    if parts.iter().any(|p| p == "Cellar" || p == "Caskroom") {
        return InstallMethod::Homebrew;
    }
    if let Some(dir) = updates.and_then(|u| u.native_dir.as_deref()) {
        if resolved.starts_with(home.join(dir)) {
            return InstallMethod::Native;
        }
    }
    InstallMethod::Other
}

/// Whether the engine updates itself when installed this way.
pub fn self_updates(spec: &EngineSpec, method: InstallMethod) -> bool {
    spec.updates
        .as_ref()
        .is_some_and(|u| u.self_updates.contains(&method))
}

/// A version as engines print them: digits and dots, at most four parts,
/// with an optional `-pre.1` style suffix.
pub fn valid_version(v: &str) -> bool {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    (1..=4).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 9 && p.chars().all(|c| c.is_ascii_digit()))
        && pre.is_none_or(|p| {
            !p.is_empty()
                && p.len() <= 32
                && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
        })
}

/// The command that installs `version` the way the engine is installed now,
/// or `None` when that install method can't (Homebrew, Other).
pub fn rollback_command(spec: &EngineSpec, method: InstallMethod, version: &str) -> Option<String> {
    if !valid_version(version) {
        return None;
    }
    let u = spec.updates.as_ref()?;
    match method {
        InstallMethod::Npm => u
            .npm
            .as_ref()
            .map(|pkg| format!("npm install -g {pkg}@{version}")),
        InstallMethod::Native => u
            .native_install
            .as_ref()
            .map(|cmd| cmd.replace("{version}", version)),
        InstallMethod::Homebrew | InstallMethod::Other => None,
    }
}

/// What updates the engine when Vibezeo doesn't check for it.
pub fn update_hint(spec: &EngineSpec, method: InstallMethod) -> Option<String> {
    match method {
        InstallMethod::Npm => spec
            .updates
            .as_ref()
            .and_then(|u| u.npm.as_ref())
            .map(|pkg| format!("npm install -g {pkg}@latest")),
        InstallMethod::Homebrew => Some("brew upgrade".into()),
        InstallMethod::Native => spec.commands.first().map(|c| format!("{c} update")),
        InstallMethod::Other => None,
    }
}

/// One engine's history in `$DATA/engine-versions.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seen {
    pub current: Option<String>,
    pub previous: Option<String>,
    /// The version that last finished a Thread turn without an error.
    pub last_good: Option<String>,
    /// A Thread turn of the current version failed since it was last good.
    #[serde(default)]
    pub failing: bool,
}

/// Versions Vibezeo has seen, per engine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionBook {
    #[serde(default)]
    pub engines: BTreeMap<String, Seen>,
}

impl VersionBook {
    pub fn parse(bytes: &[u8]) -> Self {
        serde_json::from_slice(bytes).unwrap_or_default()
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).unwrap_or_default()
    }

    /// Detection saw `version`. A new one moves the old to `previous` and
    /// starts with no failures. Returns whether anything changed.
    pub fn observe(&mut self, engine: &str, version: &str) -> bool {
        let seen = self.engines.entry(engine.to_string()).or_default();
        if seen.current.as_deref() == Some(version) {
            return false;
        }
        seen.previous = seen.current.take();
        seen.current = Some(version.to_string());
        seen.failing = false;
        true
    }

    /// A Thread turn of the engine ended: `ok` marks the current version
    /// good; a failure is remembered for the banner.
    pub fn record(&mut self, engine: &str, ok: bool) -> bool {
        let Some(seen) = self.engines.get_mut(engine) else {
            return false;
        };
        let Some(current) = seen.current.clone() else {
            return false;
        };
        if ok {
            let changed = seen.last_good.as_deref() != Some(current.as_str()) || seen.failing;
            seen.last_good = Some(current);
            seen.failing = false;
            changed
        } else {
            let changed = !seen.failing;
            seen.failing = true;
            changed
        }
    }

    /// `(last good, current)` when threads have failed since an update.
    pub fn broken(&self, engine: &str) -> Option<(String, String)> {
        let seen = self.engines.get(engine)?;
        let (good, current) = (seen.last_good.as_ref()?, seen.current.as_ref()?);
        (seen.failing && good != current).then(|| (good.clone(), current.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::EngineRegistry;

    #[test]
    fn f_en_08_1_the_install_method_comes_from_where_the_command_resolves() {
        let reg = EngineRegistry::bundled();
        let claude = reg.get("claude").expect("claude");
        let home = Path::new("/home/ana");
        let m = |p: &str| install_method(Path::new(p), home, claude.updates.as_ref());
        assert_eq!(
            m("/home/ana/.npm-global/lib/node_modules/@anthropic-ai/claude-code/bin/claude"),
            InstallMethod::Npm
        );
        assert_eq!(
            m("/opt/homebrew/Caskroom/claude-code/2.1.0/claude"),
            InstallMethod::Homebrew
        );
        assert_eq!(
            m("/usr/local/Cellar/x/1.0/bin/claude"),
            InstallMethod::Homebrew
        );
        assert_eq!(
            m("/home/ana/.local/share/claude/versions/2.1.89"),
            InstallMethod::Native
        );
        assert_eq!(m("/usr/bin/claude"), InstallMethod::Other);
        // Another user's versions folder isn't this one's.
        assert_eq!(
            m("/home/bo/.local/share/claude/versions/2.1.89"),
            InstallMethod::Other
        );
        assert!(self_updates(claude, InstallMethod::Native));
        assert!(self_updates(claude, InstallMethod::Npm));
        assert!(!self_updates(claude, InstallMethod::Homebrew));
        let codex = reg.get("codex").expect("codex");
        assert!(!self_updates(codex, InstallMethod::Npm));
    }

    #[test]
    fn f_en_08_3_a_failure_after_an_update_names_both_versions_until_a_turn_works() {
        let mut book = VersionBook::default();
        book.observe("codex", "1.2.0");
        book.record("codex", true);
        assert_eq!(book.broken("codex"), None);
        book.observe("codex", "1.3.0");
        assert_eq!(book.broken("codex"), None, "no failure yet");
        book.record("codex", false);
        assert_eq!(book.broken("codex"), Some(("1.2.0".into(), "1.3.0".into())));
        // It survives a restart.
        let again = VersionBook::parse(&book.to_json());
        assert_eq!(
            again.broken("codex"),
            Some(("1.2.0".into(), "1.3.0".into()))
        );
        assert_eq!(again.engines["codex"].previous.as_deref(), Some("1.2.0"));
        // A turn that works on 1.3.0 makes it the last good one.
        book.record("codex", true);
        assert_eq!(book.broken("codex"), None);
        assert_eq!(book.engines["codex"].last_good.as_deref(), Some("1.3.0"));
        // A failure on the last good version itself isn't an update's fault.
        book.record("codex", false);
        assert_eq!(book.broken("codex"), None);
        // Seeing the same version again changes nothing.
        assert!(!book.observe("codex", "1.3.0"));
        assert!(!VersionBook::default().record("nope", false));
        assert_eq!(VersionBook::parse(b"not json"), VersionBook::default());
    }

    #[test]
    fn f_en_08_3_rollback_uses_the_same_install_method_and_refuses_odd_versions() {
        let reg = EngineRegistry::bundled();
        let codex = reg.get("codex").expect("codex");
        assert_eq!(
            rollback_command(codex, InstallMethod::Npm, "1.2.0").as_deref(),
            Some("npm install -g @openai/codex@1.2.0")
        );
        let claude = reg.get("claude").expect("claude");
        assert_eq!(
            rollback_command(claude, InstallMethod::Native, "2.1.89").as_deref(),
            Some("curl -fsSL https://claude.ai/install.sh | bash -s 2.1.89")
        );
        assert_eq!(
            rollback_command(claude, InstallMethod::Homebrew, "2.1.89"),
            None
        );
        assert_eq!(
            rollback_command(codex, InstallMethod::Native, "1.2.0"),
            None
        );
        for bad in [
            "1.2.0; rm -rf ~",
            "$(id)",
            "1..2",
            "",
            "1.2.3.4.5",
            "latest",
            "1.2.0 && x",
        ] {
            assert_eq!(
                rollback_command(codex, InstallMethod::Npm, bad),
                None,
                "{bad:?}"
            );
        }
        assert!(valid_version("0.40.1-alpha.2"));
        assert_eq!(
            update_hint(codex, InstallMethod::Npm).as_deref(),
            Some("npm install -g @openai/codex@latest")
        );
        assert_eq!(
            update_hint(claude, InstallMethod::Native).as_deref(),
            Some("claude update")
        );
    }

    #[test]
    fn f_en_08_2_the_registry_has_claude_and_codex_pins_from_their_docs() {
        let reg = EngineRegistry::bundled();
        let pin = |id: &str| {
            reg.get(id)
                .and_then(|e| e.updates.clone())
                .and_then(|u| u.pin)
        };
        let claude = pin("claude").expect("claude pin");
        assert_eq!(
            claude.env.get("DISABLE_AUTOUPDATER").map(String::as_str),
            Some("1")
        );
        let codex = pin("codex").expect("codex pin");
        assert_eq!(codex.args, vec!["-c", "check_for_update_on_startup=false"]);
        assert!(pin("gemini").is_none(), "no documented switch, no Pin");
        let mut bad = reg.get("codex").cloned().expect("codex");
        if let Some(u) = bad.updates.as_mut() {
            u.npm = Some("@openai/codex; rm -rf ~".into());
        }
        assert!(bad.validate().is_err());
    }
}
