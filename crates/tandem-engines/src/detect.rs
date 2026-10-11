//! Engine detection (ARCHITECTURE §6.2, F-EN-01).
//!
//! Three sources, run concurrently and never on the UI thread:
//! 1. the login shell (`$SHELL -lic` + `command -v`), bounded by an 8 s
//!    deadline: a shell that takes longer gets its own diagnostic;
//! 2. the folders installers use, checked directly so a slow or broken
//!    shell never hides an engine that is plainly there;
//! 3. `<path> --version` for everything found (2 s each, in parallel).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use ts_rs::TS;

use crate::registry::EngineSpec;

/// The shell probe's budget (F-EN-01 AC2).
pub const DETECT_DEADLINE: Duration = Duration::from_secs(8);
/// Each `--version` call's budget.
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(2);
const MARKER: &str = "__TANDEM_DETECT__";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FoundSource {
    /// On the login shell's PATH.
    Shell,
    /// In a known install folder only.
    Known,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FoundEngine {
    pub engine_id: String,
    pub path: String,
    pub version: Option<String>,
    pub source: FoundSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SlowShell {
    pub shell: String,
    pub seconds: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DetectionReport {
    pub host: String,
    /// The shell commands asked, e.g. `/bin/zsh -lic`.
    pub shells_asked: Vec<String>,
    /// Folders checked directly (`~` for the home folder).
    pub folders_checked: Vec<String>,
    pub found: Vec<FoundEngine>,
    /// Engine ids not found anywhere.
    pub missing: Vec<String>,
    /// What the user can do about it, most likely first.
    pub fixes: Vec<String>,
    pub slow_shell: Option<SlowShell>,
}

impl DetectionReport {
    pub fn get(&self, engine_id: &str) -> Option<&FoundEngine> {
        self.found.iter().find(|f| f.engine_id == engine_id)
    }
}

/// Where and how to look.
#[derive(Debug, Clone)]
pub struct DetectEnv {
    pub host: String,
    /// The user's login shell.
    pub shell: PathBuf,
    pub home: PathBuf,
    /// The login environment (PATH for `npm`, and the Windows folders
    /// `APPDATA`, `LOCALAPPDATA`, `USERPROFILE`).
    pub vars: BTreeMap<String, String>,
    pub deadline: Duration,
    pub version_timeout: Duration,
    /// Skip the `npm prefix -g` probe (tests without npm).
    pub probe_npm: bool,
    /// Machine-wide folders (Homebrew, `/usr/local/bin`), after the user's.
    pub system_dirs: Vec<PathBuf>,
}

impl DetectEnv {
    pub fn local(shell: PathBuf, home: PathBuf, vars: BTreeMap<String, String>) -> Self {
        Self {
            host: "local".into(),
            shell,
            home,
            vars,
            deadline: DETECT_DEADLINE,
            version_timeout: VERSION_TIMEOUT,
            probe_npm: true,
            system_dirs: if cfg!(windows) {
                Vec::new()
            } else {
                [
                    "/opt/homebrew/bin",
                    "/usr/local/bin",
                    "/home/linuxbrew/.linuxbrew/bin",
                ]
                .map(PathBuf::from)
                .to_vec()
            },
        }
    }
}

/// Detects every engine in `engines` on this machine.
pub async fn detect(engines: &[EngineSpec], env: &DetectEnv) -> DetectionReport {
    let commands: Vec<&str> = {
        let mut seen = HashSet::new();
        engines
            .iter()
            .flat_map(|e| e.commands.iter().map(String::as_str))
            .filter(|c| seen.insert(*c))
            .collect()
    };
    let shell_cmd = probe_command(&env.shell, &commands);
    let shells_asked = vec![shell_cmd.describe()];
    let (shell, dirs) = tokio::join!(
        tokio::time::timeout(env.deadline, run_probe(shell_cmd, &env.vars)),
        known_dirs(env),
    );
    let (on_path, slow_shell) = match shell {
        Ok(found) => (found, None),
        Err(_) => (
            BTreeMap::new(),
            Some(SlowShell {
                shell: env.shell.to_string_lossy().into_owned(),
                seconds: env.deadline.as_secs() as u32,
            }),
        ),
    };

    let mut found = Vec::new();
    let mut missing = Vec::new();
    let mut known_only: Vec<PathBuf> = Vec::new();
    for engine in engines {
        let from_shell = engine
            .commands
            .iter()
            .find_map(|c| on_path.get(c.as_str()).cloned());
        let hit = match from_shell {
            Some(p) => Some((p, FoundSource::Shell)),
            None => engine
                .commands
                .iter()
                .find_map(|c| dirs.iter().find_map(|d| executable_in(d, c, &env.vars)))
                .map(|p| (p, FoundSource::Known)),
        };
        match hit {
            Some((path, source)) => {
                if source == FoundSource::Known {
                    if let Some(dir) = path.parent() {
                        known_only.push(dir.to_path_buf());
                    }
                }
                found.push((engine, path, source));
            }
            None => missing.push(engine.id.clone()),
        }
    }

    let versions = futures::future::join_all(
        found
            .iter()
            .map(|(engine, path, _)| version_of(engine, path, env.version_timeout)),
    )
    .await;
    let found: Vec<FoundEngine> = found
        .into_iter()
        .zip(versions)
        .map(|((engine, path, source), version)| FoundEngine {
            engine_id: engine.id.clone(),
            path: path.to_string_lossy().into_owned(),
            version,
            source,
        })
        .collect();

    let mut fixes = Vec::new();
    if let Some(slow) = &slow_shell {
        fixes.push(format!(
            "Your shell took more than {}s to start. Check {} for slow commands; engines in common folders were still found.",
            slow.seconds,
            rc_file(&env.shell)
        ));
    }
    known_only.dedup();
    for dir in known_only.iter().take(3) {
        fixes.push(path_fix(&tilde(dir, &env.home)));
    }
    if found.is_empty() {
        fixes.push(
            "Install an engine below, or make sure the folder it was installed to is on your shell's PATH, then Check again."
                .into(),
        );
    }

    DetectionReport {
        host: env.host.clone(),
        shells_asked,
        folders_checked: dirs.iter().map(|d| tilde(d, &env.home)).collect(),
        found,
        missing,
        fixes,
        slow_shell,
    }
}

fn rc_file(shell: &Path) -> &'static str {
    match shell.file_name().and_then(|n| n.to_str()) {
        Some("zsh") => "~/.zshrc and ~/.zprofile",
        Some("bash") => "~/.bashrc and ~/.bash_profile",
        Some("fish") => "~/.config/fish/config.fish",
        _ => "your shell's startup files",
    }
}

fn path_fix(dir: &str) -> String {
    if cfg!(target_os = "macos") {
        // The probe runs `$SHELL -lic`, so ~/.zprofile and ~/.zshrc both
        // count; ~/.zprofile also reaches non-interactive login shells.
        format!("{dir} isn't on your shell's PATH. Add the folder to PATH in ~/.zprofile.")
    } else if cfg!(windows) {
        format!("{dir} isn't on PATH. Add it to your user PATH in System Settings → Environment Variables.")
    } else {
        format!("{dir} isn't on your login PATH. Add the folder to PATH in ~/.profile.")
    }
}

fn tilde(p: &Path, home: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) if !home.as_os_str().is_empty() => {
            if rest.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~{}{}", std::path::MAIN_SEPARATOR, rest.to_string_lossy())
            }
        }
        _ => p.to_string_lossy().into_owned(),
    }
}

/// The login-shell probe for one shell.
struct Probe {
    program: PathBuf,
    args: Vec<String>,
}

impl Probe {
    fn describe(&self) -> String {
        let flags = self.args.first().map(String::as_str).unwrap_or("");
        format!("{} {flags}", self.program.to_string_lossy())
            .trim()
            .to_string()
    }
}

fn probe_command(shell: &Path, commands: &[&str]) -> Probe {
    let list = commands.join(" ");
    let name = shell
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if cfg!(windows) {
        // Each name through Get-Command; no profile, so it can't hang.
        let script = format!(
            "Write-Output '{MARKER}'; foreach ($c in '{}'.Split(' ')) {{ $p = Get-Command $c -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Source; Write-Output \"$c`t$p\" }}",
            list
        );
        return Probe {
            program: PathBuf::from("powershell.exe"),
            args: vec!["-NoProfile".into(), "-Command".into(), script],
        };
    }
    // Command names are registry-validated bare names: safe to inline.
    let (program, script) = if name == "fish" {
        (
            shell.to_path_buf(),
            format!(
                "printf '\\n%s\\n' {MARKER}; for c in {list}; printf '%s\\t%s\\n' $c (command -v $c 2>/dev/null); end"
            ),
        )
    } else {
        let program = if matches!(name, "bash" | "zsh" | "sh" | "dash" | "ksh" | "mksh") {
            shell.to_path_buf()
        } else {
            // An exotic shell (nu, xonsh…): ask a POSIX login shell instead.
            PathBuf::from("/bin/sh")
        };
        (
            program,
            format!(
                "printf '\\n%s\\n' {MARKER}; for c in {list}; do printf '%s\\t%s\\n' \"$c\" \"$(command -v \"$c\" 2>/dev/null)\"; done"
            ),
        )
    };
    Probe {
        program,
        args: vec!["-lic".into(), script],
    }
}

/// Runs the probe; returns command → absolute path for each one found.
async fn run_probe(probe: Probe, vars: &BTreeMap<String, String>) -> BTreeMap<String, PathBuf> {
    let mut cmd = if tandem_core::sandbox::in_flatpak() {
        // Engines are installed on the host (D-2032).
        let args: Vec<std::ffi::OsString> = probe.args.iter().map(Into::into).collect();
        let env: Vec<(&String, &String)> = vars.iter().collect();
        let argv = tandem_core::sandbox::flatpak_host_argv(
            std::ffi::OsStr::new(&probe.program),
            &args,
            None,
            (!env.is_empty()).then_some(env.as_slice()),
        );
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        cmd
    } else {
        let mut cmd = Command::new(&probe.program);
        cmd.args(&probe.args);
        // The login env, not the app's: what a terminal would see.
        if !vars.is_empty() {
            cmd.env_clear().envs(vars);
        }
        cmd
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let Ok(mut child) = cmd.spawn() else {
        return BTreeMap::new();
    };
    let guard = GroupGuard(child.id());
    let mut out = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_end(&mut out).await;
    }
    let _ = child.wait().await;
    drop(guard);
    parse_probe(&String::from_utf8_lossy(&out))
}

/// Kills the probe's whole process group when the deadline drops the probe
/// (an rc file's `sleep` would otherwise outlive it). Unix only: on Windows
/// the probe is a single PowerShell process and `kill_on_drop` ends it.
#[cfg_attr(not(unix), allow(dead_code))]
struct GroupGuard(Option<u32>);

impl Drop for GroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            // SAFETY: kill(2) on the process group created for the probe.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}

/// Lines after the marker: `name<TAB>/abs/path`. Aliases and functions
/// (`command -v` prints their definitions) aren't paths and are skipped.
pub fn parse_probe(out: &str) -> BTreeMap<String, PathBuf> {
    let mut found = BTreeMap::new();
    let Some(at) = out.find(MARKER) else {
        return found;
    };
    for line in out[at + MARKER.len()..].lines() {
        let Some((name, path)) = line.split_once('\t') else {
            continue;
        };
        let path = path.trim();
        if !name.is_empty() && Path::new(path).is_absolute() {
            found.insert(name.trim().to_string(), PathBuf::from(path));
        }
    }
    found
}

/// The folders installers put engines in, in the order they're checked.
pub async fn known_dirs(env: &DetectEnv) -> Vec<PathBuf> {
    let home = &env.home;
    let mut dirs = vec![home.join(".local/bin"), home.join(".claude/local")];
    if env.probe_npm {
        if let Some(prefix) = npm_prefix(&env.vars).await {
            dirs.push(if cfg!(windows) {
                prefix.clone()
            } else {
                prefix.join("bin")
            });
        }
    }
    // nvm: every installed Node, newest first.
    let mut nvm: Vec<PathBuf> = std::fs::read_dir(home.join(".nvm/versions/node"))
        .map(|rd| rd.flatten().map(|e| e.path().join("bin")).collect())
        .unwrap_or_default();
    nvm.sort_by_key(|p| std::cmp::Reverse(node_version(p)));
    dirs.extend(nvm);
    dirs.extend([
        home.join(".bun/bin"),
        home.join(".local/share/mise/shims"),
        home.join(".volta/bin"),
    ]);
    if cfg!(windows) {
        if let Some(appdata) = env.vars.get("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("npm"));
        }
        if let Some(local) = env.vars.get("LOCALAPPDATA") {
            let programs = PathBuf::from(local).join("Programs");
            if let Ok(rd) = std::fs::read_dir(&programs) {
                let mut sub: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
                sub.sort();
                dirs.extend(sub);
            }
        }
        if let Some(profile) = env.vars.get("USERPROFILE") {
            dirs.push(PathBuf::from(profile).join(".local").join("bin"));
        }
    }
    dirs.extend(env.system_dirs.iter().cloned());
    let mut seen = HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

/// `v20.11.1/bin` → (20, 11, 1), for newest-first ordering.
fn node_version(bin: &Path) -> (u32, u32, u32) {
    let name = bin
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let mut parts = name
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.parse::<u32>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

/// `npm prefix -g`, with 2 s to answer.
async fn npm_prefix(vars: &BTreeMap<String, String>) -> Option<PathBuf> {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let mut cmd = if tandem_core::sandbox::in_flatpak() {
        let path: Vec<(&str, &str)> = vars
            .get("PATH")
            .map(|p| ("PATH", p.as_str()))
            .into_iter()
            .collect();
        let argv = tandem_core::sandbox::flatpak_host_argv(
            std::ffi::OsStr::new(npm),
            &["prefix".into(), "-g".into()],
            None,
            (!path.is_empty()).then_some(path.as_slice()),
        );
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        cmd
    } else {
        let mut cmd = Command::new(npm);
        cmd.args(["prefix", "-g"]);
        cmd
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(path) = vars.get("PATH") {
        cmd.env("PATH", path);
    }
    let out = tokio::time::timeout(VERSION_TIMEOUT, cmd.output())
        .await
        .ok()?
        .ok()?;
    let prefix = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && Path::new(&prefix).is_absolute()).then(|| PathBuf::from(prefix))
}

/// `dir/name` when it's an executable file (with PATHEXT on Windows).
pub fn executable_in(dir: &Path, name: &str, vars: &BTreeMap<String, String>) -> Option<PathBuf> {
    if cfg!(windows) {
        let exts = vars
            .get("PATHEXT")
            .cloned()
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
        return exts
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| dir.join(format!("{name}{}", e.to_ascii_lowercase())))
            .find(|p| p.is_file());
    }
    let p = dir.join(name);
    is_executable(&p).then_some(p)
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// `<path> --version` (the registry's `version_cmd`), parsed with
/// `version_regex`; `None` when it fails, times out or doesn't match.
pub async fn version_of(engine: &EngineSpec, path: &Path, timeout: Duration) -> Option<String> {
    let mut cmd = if tandem_core::sandbox::in_flatpak() {
        let args: Vec<std::ffi::OsString> = engine.version_cmd.iter().map(Into::into).collect();
        let argv = tandem_core::sandbox::flatpak_host_argv::<&str, &str>(
            path.as_os_str(),
            &args,
            None,
            None,
        );
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        cmd
    } else {
        let mut cmd = Command::new(path);
        cmd.args(&engine.version_cmd);
        cmd
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .ok()?
        .ok()?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let re = regex::Regex::new(&engine.version_regex).ok()?;
    let caps = re.captures(&text)?;
    caps.get(1)
        .or_else(|| caps.get(0))
        .map(|m| m.as_str().to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::EngineRegistry;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn fake_bin(dir: &Path, name: &str, version: &str) -> PathBuf {
        std::fs::create_dir_all(dir).expect("dir");
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\necho \"{name} {version} (fake)\"\n"))
            .expect("write");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        p
    }

    /// A login shell that reads only the test home's profile files, so
    /// engines installed on the machine running the tests stay out of it.
    fn fake_shell(home: &Path, name: &str) -> PathBuf {
        let dir = home.join(".shell");
        std::fs::create_dir_all(&dir).expect("dir");
        let p = dir.join(name);
        std::fs::write(
            &p,
            "#!/bin/sh\n[ \"$1\" = -lic ] && shift\nfor f in .profile .bashrc; do [ -f \"$HOME/$f\" ] && . \"$HOME/$f\"; done\nexec /bin/sh -c \"$1\"\n",
        )
        .expect("write");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        p
    }

    fn env_for(home: &Path, shell: &str) -> DetectEnv {
        let mut vars = BTreeMap::new();
        vars.insert("HOME".into(), home.to_string_lossy().into_owned());
        // A PATH without any engine, like a GUI app's.
        vars.insert("PATH".into(), "/usr/bin:/bin".into());
        DetectEnv {
            probe_npm: false,
            system_dirs: Vec::new(),
            deadline: Duration::from_secs(8),
            ..DetectEnv::local(fake_shell(home, shell), home.to_path_buf(), vars)
        }
    }

    #[test]
    fn the_probe_output_is_read_after_the_marker_only() {
        let out = "motd: welcome\nclaude\t/fake\n__TANDEM_DETECT__\nclaude\t/home/u/.local/bin/claude\ncodex\t\nllm\talias llm='x'\n";
        let found = parse_probe(out);
        assert_eq!(found.len(), 1);
        assert_eq!(found["claude"], PathBuf::from("/home/u/.local/bin/claude"));
    }

    #[tokio::test]
    async fn f_en_01_1_an_engine_in_an_nvm_folder_is_found_with_its_version() {
        let home = tempfile::tempdir().expect("home");
        // nvm layout: the engine sits in an old Node's bin, which no PATH has.
        fake_bin(
            &home.path().join(".nvm/versions/node/v18.20.0/bin"),
            "claude",
            "1.0.9",
        );
        std::fs::create_dir_all(home.path().join(".nvm/versions/node/v22.3.0/bin")).expect("nvm");
        let registry = EngineRegistry::bundled();
        let report = detect(registry.all(), &env_for(home.path(), "sh")).await;
        let claude = report.get("claude").expect("claude found");
        assert_eq!(claude.source, FoundSource::Known);
        assert_eq!(claude.version.as_deref(), Some("1.0.9"));
        assert!(claude
            .path
            .ends_with(".nvm/versions/node/v18.20.0/bin/claude"));
        assert_eq!(report.missing.len(), 10);
        // The newest Node is checked first.
        let nvm: Vec<&String> = report
            .folders_checked
            .iter()
            .filter(|f| f.contains(".nvm"))
            .collect();
        assert!(nvm[0].contains("v22.3.0"), "{nvm:?}");
        assert!(
            report.fixes.iter().any(|f| f.contains("v18.20.0")),
            "{:?}",
            report.fixes
        );
    }

    #[tokio::test]
    async fn f_en_01_the_login_shell_path_wins_and_is_reported_as_shell() {
        let home = tempfile::tempdir().expect("home");
        let bin = home.path().join("tools");
        fake_bin(&bin, "codex", "0.40.1");
        std::fs::write(
            home.path().join(".profile"),
            format!("export PATH=\"{}:$PATH\"\n", bin.display()),
        )
        .expect("profile");
        let registry = EngineRegistry::bundled();
        let report = detect(registry.all(), &env_for(home.path(), "sh")).await;
        let codex = report.get("codex").expect("codex");
        assert_eq!(codex.source, FoundSource::Shell);
        assert_eq!(codex.version.as_deref(), Some("0.40.1"));
        assert!(
            report.shells_asked[0].ends_with("/sh -lic"),
            "{:?}",
            report.shells_asked
        );
        assert!(report.slow_shell.is_none());
    }

    #[tokio::test]
    async fn f_en_01_with_nothing_installed_the_report_says_what_was_probed() {
        let home = tempfile::tempdir().expect("home");
        let registry = EngineRegistry::bundled();
        let report = detect(registry.all(), &env_for(home.path(), "sh")).await;
        assert!(report.found.is_empty());
        assert_eq!(report.missing.len(), 11);
        assert!(report.folders_checked.iter().any(|f| f == "~/.local/bin"));
        assert!(!report.fixes.is_empty());
    }

    #[tokio::test]
    #[ignore = "waits out the 8 s deadline"]
    async fn f_en_01_2_a_slow_shell_gets_its_diagnostic_and_known_folders_still_answer() {
        let home = tempfile::tempdir().expect("home");
        std::fs::copy(
            fixtures().join("slowshell/.bashrc"),
            home.path().join(".bashrc"),
        )
        .expect("rc");
        std::fs::write(home.path().join(".bash_profile"), ". ~/.bashrc\n").expect("profile");
        fake_bin(&home.path().join(".local/bin"), "gemini", "0.9.0");
        let registry = EngineRegistry::bundled();
        let started = Instant::now();
        let report = detect(registry.all(), &env_for(home.path(), "bash")).await;
        let took = started.elapsed();
        assert!(
            took >= Duration::from_secs(8) && took < Duration::from_secs(12),
            "{took:?}"
        );
        let slow = report.slow_shell.as_ref().expect("slow shell diagnostic");
        assert_eq!(slow.seconds, 8);
        assert!(report.fixes[0].contains("more than 8s"));
        assert_eq!(
            report.get("gemini").map(|f| f.source),
            Some(FoundSource::Known)
        );
    }

    #[tokio::test]
    async fn a_version_that_hangs_is_abandoned() {
        let dir = tempfile::tempdir().expect("dir");
        let p = dir.path().join("claude");
        std::fs::write(&p, "#!/bin/sh\nsleep 30\n").expect("write");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let registry = EngineRegistry::bundled();
        let spec = registry.get("claude").expect("claude");
        let started = Instant::now();
        assert_eq!(version_of(spec, &p, Duration::from_millis(300)).await, None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
