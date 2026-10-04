//! Login environment for panes (ARCHITECTURE §5.2). Captured once from the
//! user's login shell so GUI-started apps see the same PATH as a terminal,
//! then sanitized so panes never inherit another account's provider home.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MARKER: &[u8] = b"\0__TANDEM_ENV__\0";
pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(3);

/// Variables that point an engine at a specific account. Panes get their
/// own value from the launch config, never from the captured env.
pub const PROVIDER_HOME_VARS: &[&str] = &["CLAUDE_CONFIG_DIR", "CODEX_HOME", "GEMINI_CLI_HOME"];

#[derive(Debug, Clone, Default)]
pub struct LoginEnv {
    pub vars: BTreeMap<String, String>,
    pub shell: PathBuf,
    /// True when the login shell didn't answer in time and the GUI process
    /// env was used instead (shown in detection diagnostics).
    pub degraded: bool,
}

/// The user's shell: `$SHELL`, then the passwd entry, then `/bin/sh`.
pub fn default_shell() -> PathBuf {
    if let Some(shell) = std::env::var_os("SHELL").filter(|s| !s.is_empty()) {
        return PathBuf::from(shell);
    }
    #[cfg(unix)]
    if let Some(shell) = passwd_shell() {
        return shell;
    }
    #[cfg(windows)]
    {
        return PathBuf::from(std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()));
    }
    #[allow(unreachable_code)]
    PathBuf::from("/bin/sh")
}

#[cfg(unix)]
fn passwd_shell() -> Option<PathBuf> {
    // SAFETY: getpwuid returns a pointer into static storage or null; we copy
    // the string out before any other passwd call can overwrite it.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() || (*pw).pw_shell.is_null() {
            return None;
        }
        let shell = std::ffi::CStr::from_ptr((*pw).pw_shell)
            .to_string_lossy()
            .into_owned();
        (!shell.is_empty()).then(|| PathBuf::from(shell))
    }
}

/// Parses `env -0` output that follows [`MARKER`]; rc-file noise before the
/// marker is ignored. `None` when the marker never appeared.
pub fn parse_env_dump(bytes: &[u8]) -> Option<BTreeMap<String, String>> {
    let start = bytes.windows(MARKER.len()).position(|w| w == MARKER)? + MARKER.len();
    let mut vars = BTreeMap::new();
    for entry in bytes[start..].split(|b| *b == 0) {
        let entry = String::from_utf8_lossy(entry);
        if let Some((k, v)) = entry.split_once('=') {
            if !k.is_empty() && !k.contains(char::is_whitespace) {
                vars.insert(k.to_string(), v.to_string());
            }
        }
    }
    Some(vars)
}

/// Runs the login shell once and returns its environment, or the process
/// env marked `degraded` if it doesn't finish within `timeout`.
pub fn capture(shell: &Path, timeout: Duration) -> LoginEnv {
    match run_capture(shell, timeout) {
        Some(vars) => LoginEnv {
            vars: sanitize(vars, &[]),
            shell: shell.to_path_buf(),
            degraded: false,
        },
        None => {
            tracing::warn!("login shell env capture failed; using the app's own environment");
            LoginEnv {
                vars: sanitize(std::env::vars().collect(), &[]),
                shell: shell.to_path_buf(),
                degraded: true,
            }
        }
    }
}

#[cfg(unix)]
fn run_capture(shell: &Path, timeout: Duration) -> Option<BTreeMap<String, String>> {
    use std::os::unix::process::CommandExt;
    let script = r#"printf '\000__TANDEM_ENV__\000'; env -0"#;
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let until = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                // SAFETY: kill(2) on the process group we just created.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = reader.join().ok()?;
    parse_env_dump(&out)
}

#[cfg(windows)]
fn run_capture(_shell: &Path, _timeout: Duration) -> Option<BTreeMap<String, String>> {
    // Windows reads the registry PATH (M3); the process env is the base.
    Some(std::env::vars().collect())
}

/// Removes what panes must not inherit (§5.2).
pub fn sanitize(
    mut vars: BTreeMap<String, String>,
    extra_provider_vars: &[&str],
) -> BTreeMap<String, String> {
    vars.retain(|k, v| {
        !(k.starts_with("TANDEM_")
            || PROVIDER_HOME_VARS.contains(&k.as_str())
            || extra_provider_vars.contains(&k.as_str())
            || k == "ELECTRON_RUN_AS_NODE"
            || (k == "NODE_OPTIONS" && v.contains("--inspect"))
            || (k == "TERM" && v == "dumb"))
    });
    vars
}

/// The env for one pane: the login env plus the variables every pane gets.
pub fn pane_env(login: &LoginEnv, pane_id: &str, app_version: &str) -> BTreeMap<String, String> {
    let mut env = login.vars.clone();
    for (k, v) in [
        ("TERM", "xterm-256color"),
        ("COLORTERM", "truecolor"),
        ("TERM_PROGRAM", "Tandem"),
        ("TERM_PROGRAM_VERSION", app_version),
        ("FORCE_COLOR", "3"),
        ("CLICOLOR", "1"),
        ("TANDEM_PANE_ID", pane_id),
    ] {
        env.insert(k.into(), v.into());
    }
    env.entry("LANG".into())
        .or_insert_with(|| "en_US.UTF-8".into());
    env
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Posix,
    Fish,
    PowerShell,
    Cmd,
}

pub fn shell_kind(shell: &Path) -> ShellKind {
    let name = shell
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        "fish" => ShellKind::Fish,
        "pwsh" | "powershell" => ShellKind::PowerShell,
        "cmd" => ShellKind::Cmd,
        _ => ShellKind::Posix,
    }
}

/// Arguments that start `shell` as an interactive login shell.
pub fn login_args(shell: &Path) -> Vec<String> {
    match shell_kind(shell) {
        ShellKind::Posix | ShellKind::Fish => vec!["-l".into()],
        ShellKind::PowerShell => vec!["-NoLogo".into()],
        ShellKind::Cmd => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_after_marker_and_ignores_rc_noise() {
        let mut dump = b"Welcome to my rc file!\nPATH=/wrong\0".to_vec();
        dump.extend_from_slice(MARKER);
        dump.extend_from_slice(b"PATH=/usr/bin:/bin\0MULTI=line1\nline2\0EMPTY=\0");
        let vars = parse_env_dump(&dump).expect("marker");
        assert_eq!(vars["PATH"], "/usr/bin:/bin");
        assert_eq!(vars["MULTI"], "line1\nline2");
        assert_eq!(vars["EMPTY"], "");
        assert!(parse_env_dump(b"no marker here").is_none());
    }

    #[test]
    fn sanitize_strips_provider_homes_and_tandem_vars() {
        let vars: BTreeMap<String, String> = [
            ("PATH", "/bin"),
            ("CLAUDE_CONFIG_DIR", "/other/account"),
            ("CODEX_HOME", "/other/codex"),
            ("TANDEM_STATUS_SECRET", "x"),
            ("TERM", "dumb"),
            ("NODE_OPTIONS", "--inspect=9229"),
            ("ELECTRON_RUN_AS_NODE", "1"),
            ("CUSTOM_HOME", "/custom"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let clean = sanitize(vars, &["CUSTOM_HOME"]);
        assert_eq!(clean.keys().collect::<Vec<_>>(), vec!["PATH"]);
    }

    #[test]
    fn pane_env_sets_terminal_identity() {
        let login = LoginEnv {
            vars: [("LANG".into(), "de_DE.UTF-8".into())].into(),
            ..Default::default()
        };
        let env = pane_env(&login, "pn_1", "0.1.0");
        assert_eq!(env["TERM"], "xterm-256color");
        assert_eq!(env["TERM_PROGRAM"], "Tandem");
        assert_eq!(env["TANDEM_PANE_ID"], "pn_1");
        assert_eq!(env["LANG"], "de_DE.UTF-8", "an existing LANG is kept");
        let env = pane_env(&LoginEnv::default(), "pn_2", "0.1.0");
        assert_eq!(env["LANG"], "en_US.UTF-8");
    }

    #[cfg(unix)]
    #[test]
    fn captures_a_real_login_shell() {
        let login = capture(Path::new("/bin/sh"), CAPTURE_TIMEOUT);
        assert!(!login.degraded);
        assert!(login.vars.contains_key("PATH"));
    }

    #[cfg(unix)]
    #[test]
    fn falls_back_to_process_env_when_the_shell_is_missing() {
        let login = capture(Path::new("/nonexistent/shell"), Duration::from_millis(200));
        assert!(login.degraded);
    }

    #[test]
    fn login_args_per_shell() {
        assert_eq!(login_args(Path::new("/bin/zsh")), vec!["-l"]);
        assert_eq!(login_args(Path::new("/usr/bin/fish")), vec!["-l"]);
        assert_eq!(
            login_args(Path::new("C:/Program Files/PowerShell/7/pwsh.exe")),
            vec!["-NoLogo"]
        );
    }
}
