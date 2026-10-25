//! Login environment for panes (ARCHITECTURE §5.2). Captured once from the
//! user's login shell so GUI-started apps see the same PATH as a terminal,
//! then sanitized so panes never inherit another account's provider home.

use std::collections::BTreeMap;
#[cfg(unix)]
use std::io::Read;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, Stdio};
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

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
            #[allow(unused_mut)]
            let mut vars: BTreeMap<String, String> = std::env::vars().collect();
            #[cfg(target_os = "macos")]
            {
                let current = vars.get("PATH").cloned().unwrap_or_default();
                let homebrew: Vec<PathBuf> = ["/opt/homebrew/bin", "/opt/homebrew/sbin"]
                    .iter()
                    .map(PathBuf::from)
                    .filter(|d| d.is_dir())
                    .collect();
                let path =
                    gui_fallback_path(&current, &path_helper_dirs(Path::new("/etc")), &homebrew);
                vars.insert("PATH".into(), path);
            }
            LoginEnv {
                vars: sanitize(vars, &[]),
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
    let args = ["-l", "-i", "-c", script];
    let mut cmd = if tandem_core::sandbox::in_flatpak() {
        // The host's login shell, not the sandbox's (D-2032).
        let host: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
        let argv = tandem_core::sandbox::flatpak_host_argv::<&str, &str>(
            shell.as_os_str(),
            &host,
            None,
            None,
        );
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        cmd
    } else {
        let mut cmd = Command::new(shell);
        cmd.args(args);
        cmd
    };
    let mut child = cmd
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

/// The folders macOS's `path_helper` puts on a login shell's PATH:
/// `<etc>/paths`, then each file in `<etc>/paths.d` in name order, one
/// folder per line.
pub fn path_helper_dirs(etc: &Path) -> Vec<String> {
    let mut files = vec![etc.join("paths")];
    if let Ok(rd) = std::fs::read_dir(etc.join("paths.d")) {
        let mut more: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        more.sort();
        files.extend(more);
    }
    files
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .flat_map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// macOS, when the login shell didn't answer (D-2110): an app started from
/// the Dock or Finder has launchd's PATH (`/usr/bin:/bin:/usr/sbin:/sbin`),
/// so panes would miss Homebrew and `/usr/local/bin`. This rebuilds what
/// the login shell would most likely have had: Homebrew's folders first (as
/// `brew shellenv` puts them), then `path_helper`'s, then the rest of the
/// current PATH, each folder once.
pub fn gui_fallback_path(current: &str, path_helper: &[String], homebrew: &[PathBuf]) -> String {
    let mut out: Vec<String> = Vec::new();
    let homebrew = homebrew.iter().map(|p| p.to_string_lossy().into_owned());
    for dir in homebrew
        .chain(path_helper.iter().cloned())
        .chain(current.split(':').map(str::to_string))
    {
        if !dir.is_empty() && !out.contains(&dir) {
            out.push(dir);
        }
    }
    out.join(":")
}

#[cfg(windows)]
fn run_capture(_shell: &Path, _timeout: Duration) -> Option<BTreeMap<String, String>> {
    // The process env, with the PATH Windows would build now from the
    // registry: Explorer-started apps keep the one from sign-in (§5.2).
    Some(crate::winpath::with_registry_path(
        std::env::vars().collect(),
        &crate::winpath::SystemRegistry,
    ))
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
    // A UTF-8 locale that exists: unset or empty LANG would leave programs
    // in ASCII (htop, less and TUIs draw differently).
    if env.get("LANG").is_none_or(|v| v.is_empty()) {
        env.insert("LANG".into(), DEFAULT_LANG.into());
    }
    env
}

/// `en_US.UTF-8` isn't installed on many Linux systems; `C.UTF-8` always is.
#[cfg(target_os = "linux")]
pub const DEFAULT_LANG: &str = "C.UTF-8";
#[cfg(not(target_os = "linux"))]
pub const DEFAULT_LANG: &str = "en_US.UTF-8";

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

/// The variable that carries a launch's mark token into the shell. The
/// wrapper copies it into an unexported shell variable and unsets it before
/// the engine starts, so neither argv nor the engine's environment holds it
/// (ARCHITECTURE §5.8).
pub const TOKEN_VAR: &str = "TANDEM_SHELL_TOKEN";

/// Arguments that run an agent as a child of the user's login shell and hand
/// the pane back to a fresh login shell when it exits (F-CD-03): POSIX
/// `$SHELL -lic '<engine>; <mark>; exec $SHELL -l'`, with fish and PowerShell
/// equivalents. The mark is `OSC 7770;handback;<token>;<exit code>`; the side
/// channel accepts it only with this launch's token, which the wrapper reads
/// from `TANDEM_SHELL_TOKEN`. With `integration` (the installed scripts'
/// folder), the shell that takes the pane back starts with shell integration
/// and the token (§5.8). Every argument is quoted; one that can't be
/// (control characters) refuses the launch.
pub fn agent_wrapper(
    shell: &Path,
    engine: &[String],
    integration: Option<&Path>,
) -> Result<Vec<String>, tandem_core::shellquote::UnsafeFilename> {
    use crate::integration::{handback_tail, IntegratedShell};
    use tandem_core::shellquote::{quote_path, quote_paths, ShellDialect};
    let shell_str = shell.to_string_lossy();
    let tail = match (IntegratedShell::detect(shell), integration) {
        (Some(kind), Some(dir)) => Some(handback_tail(kind, shell, dir)?),
        _ => None,
    };
    Ok(match shell_kind(shell) {
        ShellKind::Posix => {
            let cmd = quote_paths(engine, ShellDialect::Posix)?;
            let again = quote_path(&shell_str, ShellDialect::Posix)?;
            let tail = tail.unwrap_or_else(|| format!("unset t; exec {again} -l"));
            vec![
                "-lic".into(),
                format!(
                    "t=\"${TOKEN_VAR}\"; unset {TOKEN_VAR}; {cmd}; c=$?; printf '\\033]7770;handback;%s;%s\\007' \"$t\" \"$c\"; unset c; {tail}"
                ),
            ]
        }
        ShellKind::Fish => {
            let cmd = quote_paths(engine, ShellDialect::Posix)?;
            let again = quote_path(&shell_str, ShellDialect::Posix)?;
            let tail = tail.unwrap_or_else(|| format!("exec {again} -l"));
            vec![
                "-l".into(),
                "-i".into(),
                "-c".into(),
                format!(
                    "set -l t ${TOKEN_VAR}; set -e {TOKEN_VAR}; {cmd}; set -l c $status; printf '\\033]7770;handback;%s;%s\\007' $t $c; {tail}"
                ),
            ]
        }
        ShellKind::PowerShell => {
            let cmd = quote_paths(engine, ShellDialect::PowerShell)?;
            let tail = tail.map(|t| format!("{t}; ")).unwrap_or_default();
            vec![
                "-NoLogo".into(),
                "-NoExit".into(),
                "-Command".into(),
                format!(
                    "$t = $env:{TOKEN_VAR}; Remove-Item Env:{TOKEN_VAR}; & {cmd}; Write-Host -NoNewline \"$([char]27)]7770;handback;$t;$LASTEXITCODE$([char]7)\"; {tail}Remove-Variable t"
                ),
            ]
        }
        // cmd.exe can't print the mark; the agent still hands back to the shell.
        ShellKind::Cmd => vec!["/K".into(), quote_paths(engine, ShellDialect::Cmd)?],
    })
}

/// Runs a command line typed in Run with secrets (F-TL-12) under the login
/// shell, then unsets `names` and hands the pane to a fresh login shell: the
/// variables (and the secret values in them) reach the command only, never a
/// command line or what's typed afterwards. `None` for cmd.exe, which can't
/// unset them before handing over.
///
/// - The command is an argument (`$1`, `$argv[1]`, a quoted literal), never
///   pasted into the script, so a trailing `&&`, `|` or `\` can't swallow
///   the `unset` after it.
/// - The shell isn't interactive (`-lc`): rc files that start tmux or
///   re-source `.env` never see the values.
/// - Ctrl+C ends the command, not the wrapper: the variables are unset
///   whichever way the command stops.
/// - `TANDEM_SHELL_TOKEN` goes too: neither the command nor the shell after
///   it can forge shell-integration marks.
pub fn run_wrapper(shell: &Path, command: &str, names: &[String]) -> Option<Vec<String>> {
    use tandem_core::shellquote::{quote_path, ShellDialect};
    let shell_str = shell.to_string_lossy();
    // Names are `[A-Za-z_][A-Za-z0-9_]*` (`.env` parsing keeps only those).
    let mut all: Vec<&str> = names.iter().map(String::as_str).collect();
    all.push(TOKEN_VAR);
    let unset = all.join(" ");
    Some(match shell_kind(shell) {
        ShellKind::Posix => {
            let again = quote_path(&shell_str, ShellDialect::Posix).ok()?;
            vec![
                "-lc".into(),
                format!("trap : INT\n(eval \"$1\")\nunset {unset}\ntrap - INT\nexec {again} -l"),
                "vibezeo-run".into(),
                command.to_string(),
            ]
        }
        ShellKind::Fish => {
            let again = quote_path(&shell_str, ShellDialect::Posix).ok()?;
            vec![
                "-l".into(),
                "-c".into(),
                format!(
                    "function __vibezeo_int --on-signal INT; end\neval $argv[1]\nset -e {unset}\nfunctions -e __vibezeo_int\nexec {again} -l"
                ),
                command.to_string(),
            ]
        }
        ShellKind::PowerShell => {
            let literal = command.replace('\'', "''");
            let env = all
                .iter()
                .map(|n| format!("Env:{n}"))
                .collect::<Vec<_>>()
                .join(",");
            vec![
                "-NoLogo".into(),
                "-NoExit".into(),
                "-Command".into(),
                format!(
                    "$vibezeoRun = '{literal}'; try {{ Invoke-Expression $vibezeoRun }} finally {{ Remove-Item {env} -ErrorAction SilentlyContinue; Remove-Variable vibezeoRun }}"
                ),
            ]
        }
        ShellKind::Cmd => return None,
    })
}

/// Runs `argv` under the user's login shell (its profile's PATH and
/// variables) and nothing after it: the sign-in terminal of Settings ›
/// Accounts (F-AC-02), which closes when the engine exits.
pub fn login_exec(
    shell: &Path,
    argv: &[String],
) -> Result<Vec<String>, tandem_core::shellquote::UnsafeFilename> {
    use tandem_core::shellquote::{quote_paths, ShellDialect};
    Ok(match shell_kind(shell) {
        ShellKind::Posix => vec![
            "-lc".into(),
            format!("exec {}", quote_paths(argv, ShellDialect::Posix)?),
        ],
        ShellKind::Fish => vec![
            "-l".into(),
            "-c".into(),
            format!("exec {}", quote_paths(argv, ShellDialect::Posix)?),
        ],
        ShellKind::PowerShell => vec![
            "-NoLogo".into(),
            "-Command".into(),
            format!("& {}", quote_paths(argv, ShellDialect::PowerShell)?),
        ],
        ShellKind::Cmd => vec!["/C".into(), quote_paths(argv, ShellDialect::Cmd)?],
    })
}

/// Finds `program` on `path` only, never in the working directory (F-CD-03
/// AC6): a workspace that ships its own `claude.cmd` can't hijack a launch.
/// On Windows `pathext` lists the extensions to try.
pub fn resolve_on_path(program: &str, path: &str, pathext: Option<&str>) -> Option<PathBuf> {
    if program.contains(['/', '\\']) {
        // Explicit paths are the caller's decision, but never relative ones.
        let p = PathBuf::from(program);
        return p.is_absolute().then_some(p).filter(|p| p.is_file());
    }
    let sep = if cfg!(windows) { ';' } else { ':' };
    let exts: Vec<String> = match pathext {
        Some(list) => std::iter::once(String::new())
            .chain(
                list.split(';')
                    .filter(|e| !e.is_empty())
                    .map(|e| e.to_ascii_lowercase()),
            )
            .collect(),
        None => vec![String::new()],
    };
    for dir in path.split(sep) {
        // An empty or "." entry means the current directory: skip it.
        if dir.is_empty() || dir == "." || !Path::new(dir).is_absolute() {
            continue;
        }
        for ext in &exts {
            let candidate = Path::new(dir).join(format!("{program}{ext}"));
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

#[cfg(test)]
mod tests {
    #[test]
    fn f_tl_12_run_wrapper_unsets_before_handing_over() {
        let names = vec!["API_KEY".to_string(), "DB_URL".to_string()];
        let sh = run_wrapper(Path::new("/bin/zsh"), "pnpm dev &&", &names).expect("posix");
        assert_eq!(sh[0], "-lc");
        assert_eq!(
            sh[1],
            "trap : INT\n(eval \"$1\")\nunset API_KEY DB_URL TANDEM_SHELL_TOKEN\ntrap - INT\nexec '/bin/zsh' -l"
        );
        // The command is an argument, not part of the script.
        assert_eq!(
            &sh[2..],
            &["vibezeo-run".to_string(), "pnpm dev &&".to_string()]
        );
        let fish = run_wrapper(Path::new("/usr/bin/fish"), "pnpm dev", &names).expect("fish");
        assert_eq!(&fish[..2], &["-l".to_string(), "-c".to_string()]);
        assert!(fish[2].contains("eval $argv[1]\nset -e API_KEY DB_URL TANDEM_SHELL_TOKEN\n"));
        assert_eq!(fish[3], "pnpm dev");
        let ps = run_wrapper(Path::new("pwsh.exe"), "echo 'it''s'", &names).expect("pwsh");
        assert_eq!(
            ps[3],
            "$vibezeoRun = 'echo ''it''''s'''; try { Invoke-Expression $vibezeoRun } finally { Remove-Item Env:API_KEY,Env:DB_URL,Env:TANDEM_SHELL_TOKEN -ErrorAction SilentlyContinue; Remove-Variable vibezeoRun }"
        );
        assert!(run_wrapper(Path::new("cmd.exe"), "x", &names).is_none());
    }

    #[test]
    fn f_ac_02_the_sign_in_terminal_runs_the_engine_under_the_login_shell_and_exits_with_it() {
        let argv = vec!["/opt/my engine/codex".to_string(), "login".to_string()];
        assert_eq!(
            login_exec(Path::new("/bin/zsh"), &argv).expect("zsh"),
            ["-lc", "exec '/opt/my engine/codex' 'login'"]
        );
        assert_eq!(
            login_exec(Path::new("/usr/bin/fish"), &argv).expect("fish")[2],
            "exec '/opt/my engine/codex' 'login'"
        );
    }
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
        assert_eq!(env["LANG"], DEFAULT_LANG);
        // Set but empty is the same as unset: programs would fall back to ASCII.
        let empty = LoginEnv {
            vars: [("LANG".into(), String::new())].into(),
            ..Default::default()
        };
        assert_eq!(pane_env(&empty, "pn_3", "0.1.0")["LANG"], DEFAULT_LANG);
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
    fn f_cd_03_3_agents_run_under_the_login_shell_and_hand_back() {
        let args = agent_wrapper(
            Path::new("/bin/zsh"),
            &["claude".into(), "--model".into(), "it's".into()],
            None,
        )
        .expect("wrap");
        assert_eq!(args[0], "-lic");
        assert_eq!(
            args[1],
            "t=\"$TANDEM_SHELL_TOKEN\"; unset TANDEM_SHELL_TOKEN; 'claude' '--model' 'it'\\''s'; c=$?; printf '\\033]7770;handback;%s;%s\\007' \"$t\" \"$c\"; unset c; unset t; exec '/bin/zsh' -l"
        );
        // With integration the returning shell gets its scripts and the token.
        let args = agent_wrapper(
            Path::new("/bin/zsh"),
            &["claude".into()],
            Some(Path::new("/d/si")),
        )
        .expect("wrap");
        let s = std::path::MAIN_SEPARATOR;
        assert!(args[1].ends_with(&format!(
            "unset c; TANDEM_SHELL_TOKEN=\"$t\" TANDEM_REAL_ZDOTDIR=\"${{ZDOTDIR-}}\" ZDOTDIR='/d/si{s}zsh' exec '/bin/zsh' -l"
        )));
        let fish =
            agent_wrapper(Path::new("/usr/bin/fish"), &["codex".into()], None).expect("wrap");
        assert!(
            fish[3].contains("set -e TANDEM_SHELL_TOKEN")
                && fish[3].ends_with("exec '/usr/bin/fish' -l")
        );
        let pwsh = agent_wrapper(Path::new("C:/pwsh.exe"), &["claude".into()], None).expect("wrap");
        assert!(
            pwsh.contains(&"-NoExit".to_string())
                && pwsh[3].contains("Remove-Item Env:TANDEM_SHELL_TOKEN; & 'claude';")
        );
        assert!(agent_wrapper(Path::new("/bin/sh"), &["claude\nrm".into()], None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn f_cd_03_3_the_wrapper_marks_the_handback_and_hides_the_token() {
        // Run the real wrapper with /bin/sh and a tiny "engine".
        let args = agent_wrapper(
            Path::new("/bin/sh"),
            &[
                "sh".into(),
                "-c".into(),
                "env | grep -c tok42; exit 7".into(),
            ],
            None,
        )
        .expect("wrap");
        let out = Command::new("/bin/sh")
            .args(&args[..1])
            .arg(args[1].replace("; exec '/bin/sh' -l", ""))
            .env(TOKEN_VAR, "tok42")
            .stdin(Stdio::null())
            .output()
            .expect("run");
        let text = String::from_utf8_lossy(&out.stdout);
        // (dash exports `_=TANDEM_SHELL_TOKEN` after the unset: the name, not the value.)
        assert!(
            text.lines().any(|l| l.trim() == "0"),
            "the engine's env has no token: {text}"
        );
        assert!(
            text.contains("\u{1b}]7770;handback;tok42;7\u{7}"),
            "{text:?}"
        );
    }

    #[test]
    fn f_cd_03_6_commands_resolve_from_path_never_the_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&workspace).expect("ws");
        std::fs::create_dir_all(&bin).expect("bin");
        let make = |p: &Path| {
            std::fs::write(p, "#!/bin/sh\n").expect("write");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
            }
        };
        // The workspace ships its own `claude`; PATH has the real one.
        make(&workspace.join("claude"));
        make(&workspace.join("claude.cmd"));
        make(&bin.join("claude"));
        let sep = if cfg!(windows) { ";" } else { ":" };
        let path = format!(".{sep}{sep}relative{sep}{}", bin.display());
        // "." and relative PATH entries are skipped, so the workspace copy
        // never wins wherever the process happens to run.
        let found = resolve_on_path("claude", &path, Some(".COM;.EXE;.CMD"));
        assert_eq!(found, Some(bin.join("claude")));
        assert_eq!(resolve_on_path("nope", &path, None), None);
        assert_eq!(
            resolve_on_path("./claude", &path, None),
            None,
            "relative paths are refused"
        );
    }

    #[test]
    fn f_en_01_a_dock_launched_app_keeps_homebrew_when_the_shell_is_slow() {
        let etc = tempfile::tempdir().expect("etc");
        std::fs::write(
            etc.path().join("paths"),
            "/usr/local/bin\n/System/Cryptexes/App/usr/bin\n/usr/bin\n/bin\n/usr/sbin\n/sbin\n",
        )
        .expect("paths");
        std::fs::create_dir(etc.path().join("paths.d")).expect("paths.d");
        std::fs::write(etc.path().join("paths.d/20-tex"), "/Library/TeX/texbin\n").expect("tex");
        std::fs::write(
            etc.path().join("paths.d/10-go"),
            "# Go\n/usr/local/go/bin\n\n",
        )
        .expect("go");
        let dirs = path_helper_dirs(etc.path());
        assert_eq!(
            dirs,
            [
                "/usr/local/bin",
                "/System/Cryptexes/App/usr/bin",
                "/usr/bin",
                "/bin",
                "/usr/sbin",
                "/sbin",
                "/usr/local/go/bin",
                "/Library/TeX/texbin"
            ]
        );
        let launchd = "/usr/bin:/bin:/usr/sbin:/sbin";
        let homebrew = [
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/opt/homebrew/sbin"),
        ];
        assert_eq!(
            gui_fallback_path(launchd, &dirs, &homebrew),
            "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/System/Cryptexes/App/usr/bin:/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/go/bin:/Library/TeX/texbin"
        );
        // Anything else the app had stays, after them; nothing twice.
        assert_eq!(
            gui_fallback_path("/custom/bin:/usr/bin", &["/usr/bin".into()], &[]),
            "/usr/bin:/custom/bin"
        );
        assert!(path_helper_dirs(Path::new("/nonexistent")).is_empty());
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
