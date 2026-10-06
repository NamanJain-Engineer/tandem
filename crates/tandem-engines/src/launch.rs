//! Terminal launch specs (ARCHITECTURE §6.3, F-EN-04, F-AC-01 AC3).
//!
//! Turns "run this engine for this account in this folder" into the argv
//! and env a pane spawns with, and refuses what must never happen: a
//! secret-shaped argv, a local provider home handed to a remote host, an
//! inherited home of another account.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tandem_core::redact::Redactor;
use ts_rs::TS;

use crate::registry::{EngineSpec, HomeStrategy};

/// Inputs larger than this go through a file or stdin, never argv.
pub const MAX_ARG_INPUT: usize = 8 * 1024;

pub const STATUS_URL_VAR: &str = "TANDEM_STATUS_URL";
pub const STATUS_SECRET_VAR: &str = "TANDEM_STATUS_SECRET";

/// Where the pane's process runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HostKind {
    Local,
    Ssh,
    Wsl,
}

/// How the engine reports status back to this pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEnv {
    pub url: String,
    pub secret: String,
}

#[derive(Debug, Clone)]
pub struct TerminalLaunchRequest<'a> {
    pub engine: &'a EngineSpec,
    /// The detected executable.
    pub program: PathBuf,
    /// The user's extra launch args for this engine (Settings → Engines).
    pub extra_args: Vec<String>,
    /// Resume this engine session.
    pub resume: Option<String>,
    pub host: HostKind,
    /// The account's provider home; `None` = the engine's own default.
    pub home: Option<PathBuf>,
    /// Tandem's provider-home root, to tell local homes apart.
    pub providers_root: PathBuf,
    pub status: Option<StatusEnv>,
}

/// What a pane spawns: the engine argv (before shell wrapping, §5.2) and
/// the variables added to the login env.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl LaunchSpec {
    pub fn argv(&self) -> Vec<String> {
        std::iter::once(self.program.to_string_lossy().into_owned())
            .chain(self.args.iter().cloned())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LaunchError {
    #[error("The launch arguments look like they contain a secret. Remove it and use the engine's own sign-in instead.")]
    SecretInArgs,
    #[error("This account's sign-in lives on this computer and can't be used on a remote host.")]
    LocalHomeOnRemote,
    #[error("This engine can't resume sessions.")]
    NoResume,
}

/// True when `s` matches one of the redactor's secret patterns.
pub fn looks_secret(s: &str) -> bool {
    matches!(Redactor::global().redact(s), Cow::Owned(_))
}

/// Builds the launch for a terminal pane (§6.3).
pub fn terminal_launch(req: &TerminalLaunchRequest<'_>) -> Result<LaunchSpec, LaunchError> {
    let engine = req.engine;
    let mut args: Vec<String> = Vec::new();
    match (&req.resume, engine.id.as_str()) {
        (Some(id), "codex") => {
            args.push("resume".into());
            args.push(id.clone());
        }
        (Some(id), _) => {
            let template = engine
                .resume_cmd_template
                .as_ref()
                .filter(|_| engine.supports_resume)
                .ok_or(LaunchError::NoResume)?;
            args.extend(req.extra_args.iter().cloned());
            args.extend(template.iter().map(|a| a.replace("{session_id}", id)));
        }
        (None, _) => args.extend(req.extra_args.iter().cloned()),
    }
    if args.iter().any(|a| looks_secret(a)) {
        return Err(LaunchError::SecretInArgs);
    }

    let mut env = BTreeMap::new();
    if let Some(home) = &req.home {
        if req.host != HostKind::Local && home.starts_with(&req.providers_root) {
            return Err(LaunchError::LocalHomeOnRemote);
        }
        match (engine.home_strategy, &engine.provider_home_env) {
            (HomeStrategy::Env, Some(var)) => {
                env.insert(var.clone(), home.to_string_lossy().into_owned());
            }
            (HomeStrategy::Xdg, _) => {
                env.insert("XDG_CONFIG_HOME".into(), sub(home, "config"));
                env.insert("XDG_DATA_HOME".into(), sub(home, "data"));
            }
            _ => {}
        }
    }
    if let Some(status) = &req.status {
        env.insert(STATUS_URL_VAR.into(), status.url.clone());
        env.insert(STATUS_SECRET_VAR.into(), status.secret.clone());
    }
    Ok(LaunchSpec {
        program: req.program.clone(),
        args,
        env,
    })
}

fn sub(home: &Path, name: &str) -> String {
    home.join(name).to_string_lossy().into_owned()
}

/// An input for an engine: inline when small, else a private temp file the
/// engine reads (`@file`) or stdin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Inline(String),
    File(PathBuf),
}

/// Writes inputs over [`MAX_ARG_INPUT`] to `dir` (mode 0600).
pub fn input_for(text: &str, dir: &Path) -> std::io::Result<Input> {
    if text.len() <= MAX_ARG_INPUT {
        return Ok(Input::Inline(text.to_string()));
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!(
        "input-{}.md",
        ulid::Ulid::new().to_string().to_ascii_lowercase()
    ));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    opts.open(&path)?.write_all(text.as_bytes())?;
    Ok(Input::File(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineRegistry;

    fn req<'a>(engine: &'a EngineSpec, root: &Path) -> TerminalLaunchRequest<'a> {
        TerminalLaunchRequest {
            engine,
            program: PathBuf::from("/usr/local/bin/x"),
            extra_args: Vec::new(),
            resume: None,
            host: HostKind::Local,
            home: Some(root.join(&engine.id).join("acc_1")),
            providers_root: root.to_path_buf(),
            status: Some(StatusEnv {
                url: "unix:///tmp/s.sock".into(),
                secret: "ab".repeat(32),
            }),
        }
    }

    #[test]
    fn f_en_04_1_the_home_and_status_variables_are_set() {
        let reg = EngineRegistry::bundled();
        let root = Path::new("/data/providers");
        let claude = reg.get("claude").expect("claude");
        let spec = terminal_launch(&req(claude, root)).expect("spec");
        assert_eq!(
            spec.env["CLAUDE_CONFIG_DIR"],
            "/data/providers/claude/acc_1"
        );
        assert_eq!(spec.env[STATUS_URL_VAR], "unix:///tmp/s.sock");
        assert_eq!(spec.argv(), vec!["/usr/local/bin/x".to_string()]);
        let opencode = reg.get("opencode").expect("opencode");
        let spec = terminal_launch(&req(opencode, root)).expect("spec");
        assert_eq!(
            spec.env["XDG_CONFIG_HOME"],
            "/data/providers/opencode/acc_1/config"
        );
        assert_eq!(
            spec.env["XDG_DATA_HOME"],
            "/data/providers/opencode/acc_1/data"
        );
    }

    #[test]
    fn f_en_04_2_a_secret_in_the_args_refuses_the_spawn() {
        let reg = EngineRegistry::bundled();
        let claude = reg.get("claude").expect("claude");
        let mut r = req(claude, Path::new("/p"));
        r.extra_args = vec![
            "--api-key".into(),
            "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
        ];
        assert_eq!(terminal_launch(&r), Err(LaunchError::SecretInArgs));
        r.extra_args = vec!["--model".into(), "opus".into()];
        assert!(terminal_launch(&r).is_ok());
    }

    #[test]
    fn f_ac_01_3_a_local_home_is_never_given_to_a_remote_host() {
        let reg = EngineRegistry::bundled();
        let claude = reg.get("claude").expect("claude");
        for host in [HostKind::Ssh, HostKind::Wsl] {
            let mut r = req(claude, Path::new("/data/providers"));
            r.host = host;
            assert_eq!(terminal_launch(&r), Err(LaunchError::LocalHomeOnRemote));
            // The remote host's own default home is fine.
            r.home = None;
            let spec = terminal_launch(&r).expect("no home");
            assert!(!spec.env.contains_key("CLAUDE_CONFIG_DIR"));
        }
    }

    #[test]
    fn resume_uses_the_engine_template() {
        let reg = EngineRegistry::bundled();
        let claude = reg.get("claude").expect("claude");
        let mut r = req(claude, Path::new("/p"));
        r.resume = Some("s-1".into());
        assert_eq!(
            terminal_launch(&r).expect("claude").args,
            vec!["--resume", "s-1"]
        );
        let codex = reg.get("codex").expect("codex");
        let mut r = req(codex, Path::new("/p"));
        r.resume = Some("s-2".into());
        assert_eq!(
            terminal_launch(&r).expect("codex").args,
            vec!["resume", "s-2"]
        );
    }

    #[test]
    fn f_en_04_3_large_inputs_go_through_a_private_file() {
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(
            input_for("short", dir.path()).expect("inline"),
            Input::Inline("short".into())
        );
        let big = "x".repeat(MAX_ARG_INPUT + 1);
        let Input::File(path) = input_for(&big, dir.path()).expect("file") else {
            panic!("expected a file");
        };
        assert_eq!(std::fs::read_to_string(&path).expect("read"), big);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
