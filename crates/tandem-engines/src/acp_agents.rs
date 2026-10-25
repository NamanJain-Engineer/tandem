//! ACP agents the user added (F-EN-10, UI-163): a label, a command, its
//! arguments and environment, run as a Thread engine through the ACP
//! adapter (F-TH-06). The agent signs in its own way; Vibezeo never holds
//! its credentials, so an environment value that looks like a secret is
//! refused (D-2507).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use crate::launch::looks_secret;
use crate::registry::{env_name, pin_reserved, EngineSpec, RegistryError};

/// The vendor of an added agent's registry entry.
pub const VENDOR: &str = "ACP agent";

/// Every added agent's id starts with it; no bundled engine's does.
pub const ID_PREFIX: &str = "acp-";

/// Whether an engine id is an added agent's (one that's still saved or not).
pub fn is_added_id(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

/// Saved agents at most.
pub const MAX_AGENTS: usize = 16;

/// What the user types to add an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AcpAgentInput {
    pub label: String,
    /// A name on the login PATH, or an absolute path.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// A saved agent: its command resolved to an absolute path when added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AcpAgent {
    /// `acp-` and a slug of the label.
    pub id: String,
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

/// Why an agent can't be added, as the form shows it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    #[error("Give the agent a name of 1 to 40 characters")]
    Label,
    #[error("Type the command that starts the agent")]
    Command,
    #[error("Arguments are one line each, at most 20 of them")]
    Args,
    #[error("{0} isn't a variable name (capital letters, digits and _)")]
    EnvName(String),
    #[error("{0} steers other engines or what a program loads, so an agent can't set it")]
    EnvReserved(String),
    #[error("{0} looks like a key or token. Keep keys in the agent's own sign-in")]
    EnvSecret(String),
}

/// The rules a form's values must meet before anything runs.
pub fn check(input: &AcpAgentInput) -> Result<(), AgentError> {
    let label = input.label.trim();
    if label.is_empty() || label.chars().count() > 40 || label.contains(['\n', '\r']) {
        return Err(AgentError::Label);
    }
    let command = input.command.trim();
    if command.is_empty() || command.len() > 1024 || command.chars().any(char::is_control) {
        return Err(AgentError::Command);
    }
    let arg_ok = |a: &String| a.len() <= 1024 && !a.chars().any(char::is_control);
    if input.args.len() > 20 || !input.args.iter().all(arg_ok) {
        return Err(AgentError::Args);
    }
    if looks_secret(&input.args.join(" ")) {
        return Err(AgentError::EnvSecret("An argument".into()));
    }
    for (k, v) in &input.env {
        if !env_name(k) {
            return Err(AgentError::EnvName(k.clone()));
        }
        if pin_reserved(k, None) || k.ends_with("_BASE_URL") {
            return Err(AgentError::EnvReserved(k.clone()));
        }
        if looks_secret(v) || looks_secret(&format!("{k}={v}")) || v.chars().any(char::is_control) {
            return Err(AgentError::EnvSecret(k.clone()));
        }
    }
    Ok(())
}

/// `acp-`, the label's letters and digits, and `nonce` in hex, made unique
/// among `taken`. The nonce is random, so an id isn't given again after its
/// agent is removed: its old threads never run a later agent (D-2511).
pub fn id_for(label: &str, taken: &[String], nonce: u32) -> String {
    let mut slug: String = label
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    let slug: String = if slug.is_empty() {
        "agent".into()
    } else {
        slug.chars().take(24).collect()
    };
    let mut n = nonce;
    loop {
        let id = format!("{ID_PREFIX}{}-{n:08x}", slug.trim_end_matches('-'));
        if !taken.contains(&id) {
            return id;
        }
        n = n.wrapping_add(1);
    }
}

impl AcpAgent {
    /// The form's rules, checked again on a saved agent: whenever the file is
    /// read and before a thread runs it, so an edited file can't slip past
    /// them.
    pub fn check(&self) -> Result<(), AgentError> {
        check(&AcpAgentInput {
            label: self.label.clone(),
            command: self.program.clone(),
            args: self.args.clone(),
            env: self.env.clone(),
        })?;
        if !std::path::Path::new(&self.program).is_absolute() {
            return Err(AgentError::Command);
        }
        Ok(())
    }

    /// The agent as a registry entry: a Thread engine through the ACP
    /// adapter, with no terminal launch, provider home or install command.
    pub fn spec(&self) -> Result<EngineSpec, RegistryError> {
        let program = std::path::Path::new(&self.program);
        let name: String = program
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .take(64)
            .collect();
        let logo: String = self
            .label
            .chars()
            .filter(char::is_ascii_alphabetic)
            .take(2)
            .collect::<String>()
            .to_ascii_uppercase();
        let mut acp_cmd = vec![if name.is_empty() {
            "agent".to_string()
        } else {
            name.clone()
        }];
        acp_cmd.extend(self.args.iter().cloned());
        let spec: EngineSpec = serde_json::from_value(json!({
            "id": self.id,
            "label": self.label,
            "vendor": VENDOR,
            "logo": if logo.is_empty() { "A".to_string() } else { logo },
            "commands": [acp_cmd[0]],
            "install": {},
            "chatAdapter": "acp",
            "acpCmd": acp_cmd,
        }))
        .map_err(|e| RegistryError {
            id: self.id.clone(),
            problem: e.to_string(),
        })?;
        spec.validate()?;
        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(env: &[(&str, &str)]) -> AcpAgentInput {
        AcpAgentInput {
            label: "Goose".into(),
            command: "goose".into(),
            args: vec!["acp".into()],
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn f_en_10_4_secrets_and_reserved_variables_are_refused_before_anything_runs() {
        assert_eq!(check(&input(&[("GOOSE_MODE", "auto")])), Ok(()));
        assert_eq!(
            check(&input(&[(
                "OPENAI_API_KEY",
                "sk-proj-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789"
            )])),
            Err(AgentError::EnvSecret("OPENAI_API_KEY".into()))
        );
        assert_eq!(
            check(&input(&[(
                "GITHUB_TOKEN",
                "ghp_0123456789abcdefghijABCDEFGHIJ0123456789"
            )])),
            Err(AgentError::EnvSecret("GITHUB_TOKEN".into()))
        );
        for reserved in [
            "CLAUDE_CONFIG_DIR",
            "LD_PRELOAD",
            "NODE_OPTIONS",
            "PATH",
            "ANTHROPIC_BASE_URL",
        ] {
            assert_eq!(
                check(&input(&[(reserved, "/tmp/x")])),
                Err(AgentError::EnvReserved(reserved.into())),
                "{reserved}"
            );
        }
        assert_eq!(
            check(&input(&[("lower", "x")])),
            Err(AgentError::EnvName("lower".into()))
        );
        let mut keyed = input(&[]);
        keyed.args = vec![
            "--api-key".into(),
            "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789".into(),
        ];
        assert!(matches!(check(&keyed), Err(AgentError::EnvSecret(_))));
        let mut empty = input(&[]);
        empty.label = " ".into();
        assert_eq!(check(&empty), Err(AgentError::Label));
    }

    #[test]
    fn f_en_10_1_a_saved_agent_is_a_thread_engine_through_acp() {
        let agent = AcpAgent {
            id: id_for("My Goose!", &[], 0x2a),
            label: "My Goose!".into(),
            program: "/opt/goose/bin/goose".into(),
            args: vec!["acp".into(), "--with-builtin".into(), "developer".into()],
            env: BTreeMap::new(),
        };
        assert_eq!(agent.id, "acp-my-goose-0000002a");
        let spec = agent.spec().expect("spec");
        assert_eq!(spec.chat_adapter.as_deref(), Some("acp"));
        assert_eq!(
            spec.acp_cmd,
            Some(vec![
                "goose".into(),
                "acp".into(),
                "--with-builtin".into(),
                "developer".into()
            ])
        );
        assert_eq!(spec.logo, "MY");
        assert!(spec.provider_home_env.is_none() && spec.headless_cmd.is_none());
        assert_eq!(id_for("!!!", &[], 1), "acp-agent-00000001");
    }

    #[test]
    fn f_en_10_3_an_id_is_never_given_to_another_agent() {
        // Different nonces: the same label never gets the same id twice.
        assert_ne!(id_for("Goose", &[], 7), id_for("Goose", &[], 8));
        assert_eq!(
            id_for("Goose", &["acp-goose-00000007".into()], 7),
            "acp-goose-00000008"
        );
    }

    #[test]
    fn f_en_10_4_a_saved_agent_is_checked_again() {
        let mut agent = AcpAgent {
            id: "acp-goose-00000001".into(),
            label: "Goose".into(),
            program: "/usr/local/bin/goose".into(),
            args: vec!["acp".into()],
            env: BTreeMap::new(),
        };
        assert_eq!(agent.check(), Ok(()));
        agent.env.insert("LD_PRELOAD".into(), "/tmp/x.so".into());
        assert_eq!(
            agent.check(),
            Err(AgentError::EnvReserved("LD_PRELOAD".into()))
        );
        agent.env.clear();
        agent.program = "goose".into();
        assert_eq!(agent.check(), Err(AgentError::Command));
    }
}
