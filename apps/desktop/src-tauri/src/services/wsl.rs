//! WSL workspaces (F-RM-02, ARCHITECTURE §13.2). Windows only: a WSL
//! workspace is a Linux folder in a distro; its terminals, agents and
//! threads run there through `wsl.exe -d <distro> --cd <path> -- …`, its
//! files are read from Windows at `\\wsl.localhost\<distro>\…`, and its
//! engines are detected inside the distro ("Claude Code (in Ubuntu)").
//! `WslSystem` is the one platform piece; it's faked in tests, and on other
//! systems there is none.

use std::sync::Arc;

use tandem_core::wslpath::{self, Distro};
use tandem_core::{ErrorCode, IpcError};
use tandem_host::protocol::DetectWanted;

/// What runs `wsl.exe`.
pub trait WslSystem: Send + Sync {
    /// `wsl.exe -l -v`'s raw output.
    fn list(&self) -> std::io::Result<Vec<u8>>;
    /// `wsl.exe -d <distro> -- <argv>`: stdout, or the error it printed.
    fn run(&self, distro: &str, argv: &[String]) -> std::io::Result<Result<Vec<u8>, String>>;
}

#[cfg(target_os = "windows")]
mod windows {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    /// No console window flashes up for each call.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub struct WslExe;

    impl super::WslSystem for WslExe {
        fn list(&self) -> std::io::Result<Vec<u8>> {
            let out = Command::new("wsl.exe")
                .args(["-l", "-v"])
                .stdin(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .output()?;
            Ok(out.stdout)
        }

        fn run(&self, distro: &str, argv: &[String]) -> std::io::Result<Result<Vec<u8>, String>> {
            let out = Command::new("wsl.exe")
                .args(["-d", distro, "--"])
                .args(argv)
                .stdin(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .output()?;
            Ok(if out.status.success() {
                Ok(out.stdout)
            } else {
                Err(tandem_core::wslpath::decode_wsl_output(&out.stderr)
                    .trim()
                    .to_string())
            })
        }
    }
}

/// This system's `wsl.exe`, when there is one.
pub fn system() -> Option<Arc<dyn WslSystem>> {
    #[cfg(target_os = "windows")]
    {
        Some(Arc::new(windows::WslExe))
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

fn unavailable() -> IpcError {
    IpcError::new(ErrorCode::Unavailable, "WSL folders open on Windows only").with_stage("wsl")
}

pub struct WslService {
    sys: Option<Arc<dyn WslSystem>>,
}

impl WslService {
    pub fn new(sys: Option<Arc<dyn WslSystem>>) -> Self {
        Self { sys }
    }

    fn sys(&self) -> Result<&Arc<dyn WslSystem>, IpcError> {
        self.sys.as_ref().ok_or_else(unavailable)
    }

    /// `wsl_distros`.
    pub fn distros(&self) -> Result<Vec<Distro>, IpcError> {
        let raw = self.sys()?.list().map_err(|e| {
            IpcError::new(ErrorCode::Unavailable, format!("Couldn't run wsl.exe: {e}"))
                .with_stage("wsl")
        })?;
        Ok(wslpath::parse_distros_verbose(&raw))
    }

    fn checked(&self, distro: &str) -> Result<(), IpcError> {
        if !wslpath::valid_distro(distro) {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "That isn't a WSL distribution name",
            ));
        }
        if !self.distros()?.iter().any(|d| d.name == distro) {
            return Err(IpcError::new(
                ErrorCode::NotFound,
                format!("WSL has no distribution called {distro}"),
            ));
        }
        Ok(())
    }

    /// A new WSL workspace's folder: it exists in the distro, and this is
    /// its real path. A Windows path (`C:\…`, `\\wsl$\…`) is translated
    /// first.
    pub fn checked_root(&self, distro: &str, root: &str) -> Result<String, IpcError> {
        self.checked(distro)?;
        let linux = wslpath::to_wsl_path(root.trim())
            .ok_or_else(|| IpcError::new(ErrorCode::Invalid, "Choose a folder with a full path"))?;
        let script = "cd -- \"$1\" && pwd -P".to_string();
        let out = self
            .sys()?
            .run(
                distro,
                &["sh".into(), "-c".into(), script, "sh".into(), linux],
            )
            .map_err(|e| {
                IpcError::new(ErrorCode::Unavailable, format!("Couldn't run wsl.exe: {e}"))
                    .with_stage("wsl")
            })?
            .map_err(|_| {
                IpcError::new(
                    ErrorCode::NotFound,
                    format!("That folder doesn't exist in {distro}."),
                )
                .with_stage("folder_missing")
            })?;
        let path = String::from_utf8_lossy(&out).trim().to_string();
        if !path.starts_with('/') {
            return Err(IpcError::new(
                ErrorCode::NotFound,
                format!("That folder doesn't exist in {distro}."),
            ));
        }
        Ok(path)
    }

    /// Engines found inside the distro's login shell (F-EN-01 per host):
    /// `(engine id, path, version line)`.
    pub fn detect(
        &self,
        distro: &str,
        wanted: &[DetectWanted],
    ) -> Result<Vec<(String, String, Option<String>)>, IpcError> {
        self.checked(distro)?;
        let script = tandem_host::detect::script(wanted);
        let out = self
            .sys()?
            .run(distro, &["sh".into(), "-lc".into(), script])
            .map_err(|e| {
                IpcError::new(ErrorCode::Unavailable, format!("Couldn't run wsl.exe: {e}"))
                    .with_stage("wsl")
            })?
            .unwrap_or_default();
        let mut found = Vec::new();
        for (id, path) in tandem_host::detect::parse(&String::from_utf8_lossy(&out)) {
            let version = wanted
                .iter()
                .find(|w| w.id == id)
                .filter(|w| !w.version_args.is_empty())
                .and_then(|w| {
                    let mut argv = vec![path.clone()];
                    argv.extend(w.version_args.iter().cloned());
                    self.sys().ok()?.run(distro, &argv).ok()?.ok()
                })
                .and_then(|v| {
                    String::from_utf8_lossy(&v)
                        .lines()
                        .next()
                        .map(|l| l.trim().chars().take(200).collect())
                });
            found.push((id, path, version));
        }
        Ok(found)
    }
}

/// How a WSL pane starts: `wsl.exe` with the distro, the folder and, for an
/// agent, its command in the distro's login shell. Only `WSLENV`'s
/// variables cross.
pub fn pane_command(distro: &str, linux_cwd: &str, command: Option<&str>) -> (String, Vec<String>) {
    let argv: Vec<String> = match command {
        Some(cmd) => vec![
            "sh".into(),
            "-lc".into(),
            format!("{cmd}\nexec \"${{SHELL:-/bin/sh}}\" -l"),
        ],
        None => Vec::new(),
    };
    (
        "wsl.exe".into(),
        wslpath::spawn_args(distro, linux_cwd, &argv),
    )
}

/// The Windows variables `wsl.exe` itself needs. Nothing else of the app's
/// environment reaches it, and an empty `WSLENV` lets none of these cross.
const WINDOWS_ENV: &[&str] = &[
    "SystemRoot",
    "SYSTEMROOT",
    "windir",
    "PATH",
    "Path",
    "USERPROFILE",
    "TEMP",
    "TMP",
    "LOCALAPPDATA",
];

/// A thread's engine inside the distro (F-RM-02 "Run agents inside WSL"):
/// `wsl.exe -d <distro> --cd <folder> -- env K=V … <engine> <args>`. The
/// engine's own variables go through `env` in the distro; `wsl.exe` gets
/// only what Windows needs to start it.
pub fn thread_spec(
    distro: &str,
    linux_cwd: &str,
    spec: tandem_engines::chat::process::ProcessSpec,
    windows_env: impl Iterator<Item = (String, String)>,
) -> tandem_engines::chat::process::ProcessSpec {
    let mut argv = vec!["env".to_string()];
    argv.extend(spec.env.iter().map(|(k, v)| format!("{k}={v}")));
    argv.push(spec.program.to_string_lossy().into_owned());
    argv.extend(spec.args.iter().map(|a| a.to_string_lossy().into_owned()));
    let mut env: Vec<(String, String)> = windows_env
        .filter(|(k, _)| WINDOWS_ENV.contains(&k.as_str()))
        .collect();
    env.push(("WSLENV".into(), String::new()));
    let cwd = env
        .iter()
        .find(|(k, _)| k == "USERPROFILE")
        .map(|(_, v)| std::path::PathBuf::from(v))
        .unwrap_or_else(std::env::temp_dir);
    tandem_engines::chat::process::ProcessSpec {
        program: "wsl.exe".into(),
        args: wslpath::spawn_args(distro, linux_cwd, &argv)
            .into_iter()
            .map(Into::into)
            .collect(),
        env,
        cwd,
        ..spec
    }
}

/// The engines wanted from a distro's detection.
pub fn wanted(engines: &[tandem_engines::EngineSpec]) -> Vec<DetectWanted> {
    engines
        .iter()
        .map(|e| DetectWanted {
            id: e.id.clone(),
            commands: e.commands.clone(),
            version_args: e.version_cmd.clone(),
        })
        .collect()
}

impl WslService {
    /// Detection inside a WSL host's distro, as the host's report.
    pub async fn detect_report(
        self: &Arc<Self>,
        host: &tandem_core::ids::HostId,
        distro: &str,
        engines: &[tandem_engines::EngineSpec],
    ) -> Result<tandem_engines::detect::DetectionReport, IpcError> {
        let want = wanted(engines);
        let me = self.clone();
        let d = distro.to_string();
        let found = tokio::task::spawn_blocking(move || me.detect(&d, &want))
            .await
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))??;
        Ok(crate::services::remote::report_from(
            host,
            &format!("sh -lc (in {distro})"),
            engines,
            found,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A distro whose `sh` answers from a table.
    struct FakeWsl {
        list: Vec<u8>,
        calls: Mutex<Vec<(String, Vec<String>)>>,
        folders: Vec<&'static str>,
        engines: &'static str,
    }

    fn utf16(s: &str) -> Vec<u8> {
        let mut out = vec![0xFF, 0xFE];
        for u in s.encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out
    }

    impl WslSystem for FakeWsl {
        fn list(&self) -> std::io::Result<Vec<u8>> {
            Ok(self.list.clone())
        }
        fn run(&self, distro: &str, argv: &[String]) -> std::io::Result<Result<Vec<u8>, String>> {
            self.calls
                .lock()
                .expect("calls")
                .push((distro.into(), argv.to_vec()));
            if argv.get(1).map(String::as_str) == Some("-c") {
                let dir = argv.last().cloned().unwrap_or_default();
                return Ok(if self.folders.contains(&dir.as_str()) {
                    Ok(format!("{dir}\n").into_bytes())
                } else {
                    Err("sh: cd: can't cd".into())
                });
            }
            if argv.get(1).map(String::as_str) == Some("-lc") {
                return Ok(Ok(self.engines.as_bytes().to_vec()));
            }
            Ok(Ok(b"2.0.1 (Claude Code)\n".to_vec()))
        }
    }

    fn service() -> (Arc<FakeWsl>, WslService) {
        let fake = Arc::new(FakeWsl {
            list: utf16("  NAME            STATE           VERSION\r\n* Ubuntu-22.04    Running         2\r\n  Debian          Stopped         1\r\n"),
            calls: Mutex::default(),
            folders: vec!["/home/ana/shop", "/mnt/c/x y/z"],
            engines: "claude\t/home/ana/.local/bin/claude\n",
        });
        (fake.clone(), WslService::new(Some(fake)))
    }

    #[test]
    fn f_rm_02_distros_from_wsl_l_v() {
        let (_, s) = service();
        let d = s.distros().expect("distros");
        assert_eq!(d.len(), 2);
        assert_eq!(
            (d[0].name.as_str(), d[0].is_default, d[0].version),
            ("Ubuntu-22.04", true, Some(2))
        );
        assert_eq!(
            (d[1].name.as_str(), d[1].is_default, d[1].version),
            ("Debian", false, Some(1))
        );
        assert_eq!(
            WslService::new(None)
                .distros()
                .expect_err("not windows")
                .code,
            ErrorCode::Unavailable
        );
    }

    #[test]
    fn f_rm_02_a_workspace_folder_is_checked_inside_the_distro() {
        let (fake, s) = service();
        assert_eq!(
            s.checked_root("Ubuntu-22.04", "/home/ana/shop")
                .expect("root"),
            "/home/ana/shop"
        );
        // A Windows path is the distro's /mnt one.
        assert_eq!(
            s.checked_root("Ubuntu-22.04", r"C:\x y\z").expect("root"),
            "/mnt/c/x y/z"
        );
        let err = s
            .checked_root("Ubuntu-22.04", "/nope")
            .expect_err("missing");
        assert_eq!(err.stage.as_deref(), Some("folder_missing"));
        assert_eq!(
            s.checked_root("Arch", "/home").expect_err("unknown").code,
            ErrorCode::NotFound
        );
        assert_eq!(
            s.checked_root("-x", "/home").expect_err("bad").code,
            ErrorCode::Invalid
        );
        // The folder is an argument, never part of the script.
        let calls = fake.calls.lock().expect("calls");
        assert!(calls
            .iter()
            .all(|(_, argv)| argv.get(2).is_none_or(|script| !script.contains("shop"))));
    }

    #[test]
    fn f_rm_02_engines_are_detected_inside_each_distro() {
        let (_, s) = service();
        let wanted = vec![DetectWanted {
            id: "claude".into(),
            commands: vec!["claude".into()],
            version_args: vec!["--version".into()],
        }];
        let found = s.detect("Ubuntu-22.04", &wanted).expect("detect");
        assert_eq!(
            found,
            [(
                "claude".to_string(),
                "/home/ana/.local/bin/claude".to_string(),
                Some("2.0.1 (Claude Code)".to_string())
            )]
        );
    }

    #[test]
    fn f_rm_02_panes_and_agents_run_inside_the_distro() {
        let (program, args) = pane_command("Ubuntu", "/home/ana/my shop", None);
        assert_eq!(program, "wsl.exe");
        assert_eq!(args, ["-d", "Ubuntu", "--cd", "/home/ana/my shop"]);
        let (_, args) = pane_command("Ubuntu", "/home/ana", Some("/home/ana/.local/bin/claude"));
        assert_eq!(&args[..5], ["-d", "Ubuntu", "--cd", "/home/ana", "--"]);
        assert_eq!(&args[5..7], ["sh", "-lc"]);
        assert!(args[7].starts_with("/home/ana/.local/bin/claude\nexec"));
    }

    #[test]
    fn f_rm_02_threads_run_the_distros_engine_with_nothing_of_windows() {
        let spec = tandem_engines::chat::process::ProcessSpec::new(
            "/home/ana/.local/bin/claude".into(),
            vec!["-p".into(), "--output-format".into(), "stream-json".into()],
            vec![("TERM".into(), "dumb".into())],
            r"C:\Users\ana".into(),
        );
        let windows = vec![
            ("SystemRoot".to_string(), r"C:\Windows".to_string()),
            ("USERPROFILE".to_string(), r"C:\Users\ana".to_string()),
            ("ANTHROPIC_API_KEY".to_string(), "sk-local".to_string()),
            (
                "CLAUDE_CONFIG_DIR".to_string(),
                r"C:\Users\ana\.tandem\providers\claude\a".to_string(),
            ),
        ];
        let out = thread_spec("Ubuntu", "/home/ana/shop", spec, windows.into_iter());
        assert_eq!(out.program, std::path::PathBuf::from("wsl.exe"));
        let args: Vec<String> = out
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-d",
                "Ubuntu",
                "--cd",
                "/home/ana/shop",
                "--",
                "env",
                "TERM=dumb",
                "/home/ana/.local/bin/claude",
                "-p",
                "--output-format",
                "stream-json"
            ]
        );
        // No local credentials or provider home cross (invariant: nothing
        // local is forwarded), and WSLENV lets nothing through.
        let keys: Vec<&str> = out.env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["SystemRoot", "USERPROFILE", "WSLENV"]);
        assert!(out.env.iter().any(|(k, v)| k == "WSLENV" && v.is_empty()));
        assert_eq!(out.cwd, std::path::PathBuf::from(r"C:\Users\ana"));
    }
}
