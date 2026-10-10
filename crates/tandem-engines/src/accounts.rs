//! Signing an engine account in and out with the engine's own login, in its
//! provider home (F-AC-02, ARCHITECTURE §6). Tandem never sees the token:
//! completion is the engine's status command or a file's presence.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use crate::registry::{EngineSpec, LoginMode};

/// How a sign-in or sign-out runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Start the engine with these arguments (`codex login`).
    Argv(Vec<String>),
    /// Start the engine's TUI, then type this once it's ready (`/login`).
    Slash(String),
    /// The engine has no such command: start its TUI and let the user do it.
    Manual,
}

fn step(mode: LoginMode, cmd: Option<&Vec<String>>) -> Step {
    match (mode, cmd) {
        (LoginMode::Argv, Some(args)) if !args.is_empty() => Step::Argv(args.clone()),
        (LoginMode::TuiSlash, Some(args)) if !args.is_empty() => Step::Slash(args.join(" ")),
        _ => Step::Manual,
    }
}

pub fn login_step(engine: &EngineSpec) -> Step {
    step(engine.login_mode, engine.login_cmd.as_ref())
}

pub fn logout_step(engine: &EngineSpec) -> Step {
    step(engine.login_mode, engine.logout_cmd.as_ref())
}

/// Whether the account in `home` is signed in: `Some` when the engine can
/// tell (its `signed_in_file` exists, or its `status_cmd` exits 0), `None`
/// when it can't.
pub async fn signed_in(
    engine: &EngineSpec,
    program: &Path,
    home: &Path,
    env: &[(String, String)],
) -> Option<bool> {
    if let Some(file) = &engine.signed_in_file {
        return Some(home.join(file).exists());
    }
    let args = engine.status_cmd.as_ref()?;
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(Duration::from_secs(15), cmd.status())
        .await
        .ok()?
        .ok()?;
    Some(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineRegistry;

    #[test]
    fn f_ac_02_each_engine_signs_in_its_own_way() {
        let reg = EngineRegistry::bundled();
        let get = |id: &str| reg.get(id).expect(id).clone();
        assert_eq!(login_step(&get("claude")), Step::Slash("/login".into()));
        assert_eq!(logout_step(&get("claude")), Step::Slash("/logout".into()));
        assert_eq!(login_step(&get("codex")), Step::Argv(vec!["login".into()]));
        assert_eq!(
            login_step(&get("opencode")),
            Step::Argv(vec!["auth".into(), "login".into()])
        );
        assert_eq!(login_step(&get("gemini")), Step::Manual);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_ac_02_signed_in_is_a_file_or_the_status_command_never_the_token() {
        let reg = EngineRegistry::bundled();
        let dir = tempfile::tempdir().expect("tmp");
        let claude = reg.get("claude").expect("claude").clone();
        let sh = Path::new("/bin/sh");
        assert_eq!(signed_in(&claude, sh, dir.path(), &[]).await, Some(false));
        std::fs::write(dir.path().join(".credentials.json"), "{}").expect("write");
        assert_eq!(signed_in(&claude, sh, dir.path(), &[]).await, Some(true));

        // A status command: `sh -c 'test -f marker'` stands in for `codex login status`.
        let mut codex = reg.get("codex").expect("codex").clone();
        codex.status_cmd = Some(vec!["-c".into(), "test -f marker".into()]);
        let path = [("PATH".to_string(), "/usr/bin:/bin".to_string())];
        assert_eq!(signed_in(&codex, sh, dir.path(), &path).await, Some(false));
        std::fs::write(dir.path().join("marker"), "").expect("marker");
        assert_eq!(signed_in(&codex, sh, dir.path(), &path).await, Some(true));

        let gemini = reg.get("gemini").expect("gemini").clone();
        assert_eq!(signed_in(&gemini, sh, dir.path(), &[]).await, None);
    }
}
