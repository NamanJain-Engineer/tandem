//! OS launchers for background routine runs (F-RT-03, ARCHITECTURE §10.3).
//! Each runs `tandem --run-routines` every 5 minutes: a launchd LaunchAgent
//! on macOS, a Task Scheduler task on Windows, a systemd `--user` timer on
//! Linux with a crontab line when systemd's user manager isn't there.
//!
//! Every command goes through [`CommandRunner`] (a recording fake in tests),
//! and every file lives under a home root (`TANDEM_LAUNCHER_ROOT` in tests),
//! so each implementation is tested on any OS.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const LAUNCHD_LABEL: &str = "app.tandem.routines";
pub const SCHTASKS_NAME: &str = "Tandem\\Routines";
pub const SYSTEMD_UNIT: &str = "tandem-routines";
/// Marks Vibezeo's crontab line.
pub const CRON_TAG: &str = "# tandem-routines";
/// How often the launcher runs the headless pass.
pub const EVERY_MINUTES: u32 = 5;

#[derive(Debug, thiserror::Error)]
pub enum LauncherError {
    #[error("couldn't write the launcher file: {0}")]
    Io(#[from] std::io::Error),
    #[error("{program} failed: {message}")]
    Command { program: String, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LauncherKind {
    Launchd,
    TaskScheduler,
    Systemd,
    Cron,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LauncherStatus {
    pub installed: bool,
    pub kind: LauncherKind,
    /// The unit, plist or crontab the entry is in.
    pub path: Option<String>,
}

/// The App Execution Alias a Microsoft Store (MSIX) install declares
/// (`packaging/msix/AppxManifest.xml.in`).
pub const STORE_ALIAS: &str = "vibezeo.exe";

/// The program the OS launcher runs: the AppImage when the app runs from
/// one; for a Microsoft Store install the execution alias in
/// `%LOCALAPPDATA%\Microsoft\WindowsApps`, since a packaged app can't be
/// started by its path inside `WindowsApps` (D-1713); else this executable.
pub fn launcher_program(
    current_exe: PathBuf,
    appimage: Option<PathBuf>,
    store_package: bool,
    local_app_data: Option<PathBuf>,
) -> PathBuf {
    if let Some(appimage) = appimage.filter(|p| !p.as_os_str().is_empty()) {
        return appimage;
    }
    match local_app_data {
        Some(local) if store_package => local
            .join("Microsoft")
            .join("WindowsApps")
            .join(STORE_ALIAS),
        _ => current_exe,
    }
}

/// What the entry runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherSpec {
    /// The app's executable.
    pub exe: PathBuf,
    /// `TANDEM_HOME`, passed on when the app runs with one.
    pub tandem_home: Option<PathBuf>,
    /// `$DATA/logs`, for launchd's output files.
    pub logs: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs OS tools (`launchctl`, `schtasks`, `systemctl`, `crontab`).
pub trait CommandRunner: Send + Sync {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&str>,
    ) -> std::io::Result<CommandOutput>;
}

/// The real thing: no window on Windows, stdin when given.
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&str>,
    ) -> std::io::Result<CommandOutput> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn()?;
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(text.as_bytes())?;
        }
        let out = child.wait_with_output()?;
        Ok(CommandOutput {
            success: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

/// One recorded command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedCall {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

type Responder = dyn Fn(&RecordedCall) -> CommandOutput + Send + Sync;

/// A fake that records every command and answers from a closure (success
/// with no output by default). Test-only in spirit; public so the app's
/// tests can use it too.
pub struct RecordingRunner {
    pub calls: Mutex<Vec<RecordedCall>>,
    respond: Box<Responder>,
}

impl Default for RecordingRunner {
    fn default() -> Self {
        Self::answering(|_| CommandOutput {
            success: true,
            ..Default::default()
        })
    }
}

impl RecordingRunner {
    pub fn answering(f: impl Fn(&RecordedCall) -> CommandOutput + Send + Sync + 'static) -> Self {
        Self {
            calls: Mutex::default(),
            respond: Box::new(f),
        }
    }

    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// `program arg arg` per call.
    pub fn lines(&self) -> Vec<String> {
        self.calls()
            .iter()
            .map(|c| {
                std::iter::once(c.program.clone())
                    .chain(c.args.iter().cloned())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }
}

impl CommandRunner for RecordingRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&str>,
    ) -> std::io::Result<CommandOutput> {
        let call = RecordedCall {
            program: program.to_string(),
            args: args.to_vec(),
            stdin: stdin.map(str::to_string),
        };
        let out = (self.respond)(&call);
        if let Ok(mut c) = self.calls.lock() {
            c.push(call);
        }
        Ok(out)
    }
}

pub trait OsLauncher: Send + Sync {
    fn kind(&self) -> LauncherKind;
    /// Writes the entry and registers it; installing again replaces it.
    fn install(&self, spec: &LauncherSpec) -> Result<LauncherStatus, LauncherError>;
    /// Removes the entry; nothing to remove is fine.
    fn uninstall(&self) -> Result<(), LauncherError>;
    fn status(&self) -> LauncherStatus;
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn check(
    program: &str,
    out: std::io::Result<CommandOutput>,
) -> Result<CommandOutput, LauncherError> {
    match out {
        Ok(o) if o.success => Ok(o),
        Ok(o) => Err(LauncherError::Command {
            program: program.into(),
            message: o
                .stderr
                .lines()
                .next()
                .unwrap_or("exited with an error")
                .to_string(),
        }),
        Err(e) => Err(LauncherError::Command {
            program: program.into(),
            message: e.to_string(),
        }),
    }
}

fn write_file(path: &Path, body: &str) -> Result<(), LauncherError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    tandem_core::atomic::atomic_write(path, body.as_bytes(), Some(0o644))?;
    Ok(())
}

fn remove_file(path: &Path) -> Result<(), LauncherError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── macOS: launchd ───────────────────────────────────────────────────────

pub struct LaunchdLauncher {
    pub runner: Arc<dyn CommandRunner>,
    /// The user's home (`~/Library/LaunchAgents/…`).
    pub home: PathBuf,
    pub uid: u32,
}

impl LaunchdLauncher {
    pub fn plist_path(&self) -> PathBuf {
        self.home
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{LAUNCHD_LABEL}.plist"))
    }

    pub fn plist(spec: &LauncherSpec) -> String {
        let exe = xml_escape(&spec.exe.to_string_lossy());
        let log = xml_escape(&spec.logs.join("routines-launchd.log").to_string_lossy());
        let env = match &spec.tandem_home {
            Some(h) => format!(
                "  <key>EnvironmentVariables</key>\n  <dict>\n    <key>TANDEM_HOME</key>\n    <string>{}</string>\n  </dict>\n",
                xml_escape(&h.to_string_lossy())
            ),
            None => String::new(),
        };
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>--run-routines</string>
  </array>
{env}  <key>StartInterval</key>
  <integer>{secs}</integer>
  <key>RunAtLoad</key>
  <false/>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#,
            secs = EVERY_MINUTES * 60
        )
    }

    fn domain(&self) -> String {
        format!("gui/{}", self.uid)
    }
}

impl OsLauncher for LaunchdLauncher {
    fn kind(&self) -> LauncherKind {
        LauncherKind::Launchd
    }

    fn install(&self, spec: &LauncherSpec) -> Result<LauncherStatus, LauncherError> {
        let path = self.plist_path();
        write_file(&path, &Self::plist(spec))?;
        // A loaded older copy goes first (its failure means none was loaded).
        let _ = self.runner.run(
            "launchctl",
            &args(&["bootout", &format!("{}/{LAUNCHD_LABEL}", self.domain())]),
            None,
        );
        let out = self.runner.run(
            "launchctl",
            &[
                "bootstrap".to_string(),
                self.domain(),
                path.to_string_lossy().into_owned(),
            ],
            None,
        );
        if let Err(e) = check("launchctl", out) {
            let _ = remove_file(&path);
            return Err(e);
        }
        Ok(self.status())
    }

    fn uninstall(&self) -> Result<(), LauncherError> {
        let _ = self.runner.run(
            "launchctl",
            &args(&["bootout", &format!("{}/{LAUNCHD_LABEL}", self.domain())]),
            None,
        );
        remove_file(&self.plist_path())
    }

    fn status(&self) -> LauncherStatus {
        let path = self.plist_path();
        LauncherStatus {
            installed: path.is_file(),
            kind: LauncherKind::Launchd,
            path: Some(path.to_string_lossy().into_owned()),
        }
    }
}

// ── Windows: Task Scheduler ──────────────────────────────────────────────

pub struct TaskSchedulerLauncher {
    pub runner: Arc<dyn CommandRunner>,
}

impl TaskSchedulerLauncher {
    /// The task's command line (`/TR`).
    pub fn command_line(spec: &LauncherSpec) -> String {
        let run = format!("\"{}\" --run-routines", spec.exe.to_string_lossy());
        match &spec.tandem_home {
            // Task Scheduler sets no env of its own: cmd sets it first.
            Some(h) => format!(
                "cmd /c \"set \"TANDEM_HOME={}\" && {run}\"",
                h.to_string_lossy()
            ),
            None => run,
        }
    }
}

impl OsLauncher for TaskSchedulerLauncher {
    fn kind(&self) -> LauncherKind {
        LauncherKind::TaskScheduler
    }

    fn install(&self, spec: &LauncherSpec) -> Result<LauncherStatus, LauncherError> {
        let out = self.runner.run(
            "schtasks",
            &[
                "/Create".into(),
                "/F".into(),
                "/TN".into(),
                SCHTASKS_NAME.into(),
                "/SC".into(),
                "MINUTE".into(),
                "/MO".into(),
                EVERY_MINUTES.to_string(),
                "/TR".into(),
                Self::command_line(spec),
            ],
            None,
        );
        check("schtasks", out)?;
        Ok(self.status())
    }

    fn uninstall(&self) -> Result<(), LauncherError> {
        if !self.status().installed {
            return Ok(());
        }
        let out = self.runner.run(
            "schtasks",
            &args(&["/Delete", "/F", "/TN", SCHTASKS_NAME]),
            None,
        );
        check("schtasks", out).map(|_| ())
    }

    fn status(&self) -> LauncherStatus {
        let installed = self
            .runner
            .run("schtasks", &args(&["/Query", "/TN", SCHTASKS_NAME]), None)
            .map(|o| o.success)
            .unwrap_or(false);
        LauncherStatus {
            installed,
            kind: LauncherKind::TaskScheduler,
            path: Some(SCHTASKS_NAME.into()),
        }
    }
}

// ── Linux: systemd --user, else crontab ─────────────────────────────────

/// A single-quoted shell word.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A systemd `ExecStart` word: quoted, with `%` and `\` escaped.
fn systemd_quote(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    )
}

pub struct CronLauncher {
    pub runner: Arc<dyn CommandRunner>,
}

impl CronLauncher {
    pub fn line(spec: &LauncherSpec) -> String {
        let env = spec
            .tandem_home
            .as_ref()
            .map(|h| format!("TANDEM_HOME={} ", sh_quote(&h.to_string_lossy())))
            .unwrap_or_default();
        format!(
            "*/{EVERY_MINUTES} * * * * {env}{} --run-routines {CRON_TAG}",
            sh_quote(&spec.exe.to_string_lossy())
        )
    }

    fn current(&self) -> Result<Vec<String>, LauncherError> {
        // No crontab yet is an error exit with nothing to keep.
        let out = self
            .runner
            .run("crontab", &args(&["-l"]), None)
            .map_err(|e| LauncherError::Command {
                program: "crontab".into(),
                message: e.to_string(),
            })?;
        if !out.success {
            return Ok(Vec::new());
        }
        Ok(out.stdout.lines().map(str::to_string).collect())
    }

    fn write(&self, lines: &[String]) -> Result<(), LauncherError> {
        let mut body = lines.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        let out = self.runner.run("crontab", &args(&["-"]), Some(&body));
        check("crontab", out).map(|_| ())
    }
}

impl OsLauncher for CronLauncher {
    fn kind(&self) -> LauncherKind {
        LauncherKind::Cron
    }

    fn install(&self, spec: &LauncherSpec) -> Result<LauncherStatus, LauncherError> {
        let mut lines: Vec<String> = self
            .current()?
            .into_iter()
            .filter(|l| !l.trim_end().ends_with(CRON_TAG))
            .collect();
        lines.push(Self::line(spec));
        self.write(&lines)?;
        Ok(self.status())
    }

    fn uninstall(&self) -> Result<(), LauncherError> {
        let lines = self.current()?;
        let kept: Vec<String> = lines
            .iter()
            .filter(|l| !l.trim_end().ends_with(CRON_TAG))
            .cloned()
            .collect();
        if kept.len() == lines.len() {
            return Ok(());
        }
        self.write(&kept)
    }

    fn status(&self) -> LauncherStatus {
        let installed = self
            .current()
            .map(|l| l.iter().any(|l| l.trim_end().ends_with(CRON_TAG)))
            .unwrap_or(false);
        LauncherStatus {
            installed,
            kind: LauncherKind::Cron,
            path: Some("crontab".into()),
        }
    }
}

pub struct SystemdUserLauncher {
    pub runner: Arc<dyn CommandRunner>,
    /// `~/.config/systemd/user`.
    pub unit_dir: PathBuf,
    /// Where entries go when `systemctl --user` doesn't work.
    pub fallback: CronLauncher,
}

impl SystemdUserLauncher {
    pub fn new(runner: Arc<dyn CommandRunner>, home: &Path) -> Self {
        Self {
            unit_dir: home.join(".config/systemd/user"),
            fallback: CronLauncher {
                runner: runner.clone(),
            },
            runner,
        }
    }

    pub fn service_path(&self) -> PathBuf {
        self.unit_dir.join(format!("{SYSTEMD_UNIT}.service"))
    }

    pub fn timer_path(&self) -> PathBuf {
        self.unit_dir.join(format!("{SYSTEMD_UNIT}.timer"))
    }

    pub fn service(spec: &LauncherSpec) -> String {
        let env = spec
            .tandem_home
            .as_ref()
            .map(|h| {
                format!(
                    "Environment={}\n",
                    systemd_quote(&format!("TANDEM_HOME={}", h.to_string_lossy()))
                )
            })
            .unwrap_or_default();
        format!(
            "[Unit]\nDescription=Vibezeo routines (runs due routines while the app is closed)\n\n[Service]\nType=oneshot\n{env}ExecStart={} --run-routines\n",
            systemd_quote(&spec.exe.to_string_lossy())
        )
    }

    pub fn timer() -> String {
        format!(
            "[Unit]\nDescription=Run Vibezeo routines every {EVERY_MINUTES} minutes\n\n[Timer]\nOnBootSec=2min\nOnUnitActiveSec={EVERY_MINUTES}min\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"
        )
    }

    fn systemctl(&self, rest: &[&str]) -> Result<CommandOutput, LauncherError> {
        let mut a = vec!["--user".to_string()];
        a.extend(rest.iter().map(|s| s.to_string()));
        check("systemctl", self.runner.run("systemctl", &a, None))
    }
}

impl OsLauncher for SystemdUserLauncher {
    fn kind(&self) -> LauncherKind {
        LauncherKind::Systemd
    }

    fn install(&self, spec: &LauncherSpec) -> Result<LauncherStatus, LauncherError> {
        write_file(&self.service_path(), &Self::service(spec))?;
        write_file(&self.timer_path(), &Self::timer())?;
        let timer = format!("{SYSTEMD_UNIT}.timer");
        let enabled = self
            .systemctl(&["daemon-reload"])
            .and_then(|_| self.systemctl(&["enable", "--now", &timer]));
        match enabled {
            Ok(_) => {
                // Only one entry runs: a crontab line from an earlier
                // fallback goes.
                let _ = self.fallback.uninstall();
                Ok(self.status())
            }
            Err(e) => {
                tracing::info!(code = "routines.systemd_unavailable", error = %e, "using a crontab line instead");
                remove_file(&self.service_path())?;
                remove_file(&self.timer_path())?;
                self.fallback.install(spec)
            }
        }
    }

    fn uninstall(&self) -> Result<(), LauncherError> {
        if self.timer_path().exists() || self.service_path().exists() {
            let timer = format!("{SYSTEMD_UNIT}.timer");
            let _ = self.systemctl(&["disable", "--now", &timer]);
            remove_file(&self.timer_path())?;
            remove_file(&self.service_path())?;
            let _ = self.systemctl(&["daemon-reload"]);
        }
        // A crontab line, if that's where it went (missing crontab is fine).
        match self.fallback.uninstall() {
            Ok(()) | Err(LauncherError::Command { .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn status(&self) -> LauncherStatus {
        if self.timer_path().is_file() {
            return LauncherStatus {
                installed: true,
                kind: LauncherKind::Systemd,
                path: Some(self.timer_path().to_string_lossy().into_owned()),
            };
        }
        let cron = self.fallback.status();
        if cron.installed {
            return cron;
        }
        LauncherStatus {
            installed: false,
            kind: LauncherKind::Systemd,
            path: Some(self.timer_path().to_string_lossy().into_owned()),
        }
    }
}

/// The user's home for launcher files: `TANDEM_LAUNCHER_ROOT` (tests), else
/// the real home.
pub fn launcher_home() -> Option<PathBuf> {
    std::env::var_os("TANDEM_LAUNCHER_ROOT")
        .map(PathBuf::from)
        .or_else(tandem_core::paths::user_home)
}

/// This OS's launcher.
pub fn platform_launcher(runner: Arc<dyn CommandRunner>, home: &Path) -> Box<dyn OsLauncher> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::MetadataExt;
        let uid = std::fs::metadata(home).map(|m| m.uid()).unwrap_or(501);
        Box::new(LaunchdLauncher {
            runner,
            home: home.to_path_buf(),
            uid,
        })
    }
    #[cfg(windows)]
    {
        let _ = home;
        Box::new(TaskSchedulerLauncher { runner })
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Box::new(SystemdUserLauncher::new(runner, home))
    }
}
