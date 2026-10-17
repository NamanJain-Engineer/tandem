//! Engine processes running in the background (F-EN-08 "Running in the
//! background"): this user's processes of an installed engine with no
//! terminal, outside the app's own process tree (D-2409). A process started
//! in a terminal, the app's or the user's own, is never listed, nor is
//! another user's, so Stop can't end someone's session.
//!
//! The process table is platform code behind [`ProcessTable`] (CLAUDE.md
//! "Platform code"): `sysinfo` for the processes everywhere it works, plus
//! each one's terminal from `/proc` on Linux and `ps` on macOS; Windows has
//! no listing yet.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One process as the table reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: u32,
    /// Its effective user.
    pub uid: u32,
    /// When it started, as the OS reports it (with the pid, its identity:
    /// a reused pid never matches).
    pub started: u64,
    /// It has a controlling terminal.
    pub tty: bool,
    pub exe: Option<PathBuf>,
    pub args: Vec<String>,
}

/// The OS's process table.
pub trait ProcessTable: Send + Sync {
    /// `None`: this platform can't list them (Windows).
    fn list(&self) -> Option<Vec<ProcInfo>>;
    /// The app's own effective user.
    fn current_uid(&self) -> Option<u32>;
    /// Asks the process to end (SIGTERM).
    fn terminate(&self, pid: u32) -> std::io::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackgroundProcess {
    pub pid: u32,
    pub engine_id: String,
    /// The process's start time; Stop sends it back so a reused pid is
    /// never signalled.
    #[ts(type = "number")]
    pub started: u64,
    /// The engine's command name and its subcommand, if any: never its
    /// other arguments, which can hold prompts or keys.
    pub command: String,
}

/// An installed engine: its id and where its command is (as found, and
/// resolved through links).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnginePaths {
    pub id: String,
    pub paths: Vec<PathBuf>,
}

fn descends_from(pid: u32, ancestor: u32, parents: &HashMap<u32, u32>) -> bool {
    let mut at = pid;
    for _ in 0..256 {
        if at == ancestor {
            return true;
        }
        match parents.get(&at) {
            Some(&p) if p != at && p != 0 => at = p,
            _ => return false,
        }
    }
    false
}

/// Runtimes that run an engine's script as their first argument.
fn is_runtime(path: &Path) -> bool {
    path.file_stem()
        .and_then(|n| n.to_str())
        .is_some_and(|n| matches!(n, "node" | "nodejs" | "bun"))
}

/// Which engine the process is, with the index of the argument that names
/// it: the executable itself, or a runtime's script (`node …/codex.js`).
/// A wrapper (`timeout 5 codex`, `flock … claude`) is neither.
fn engine_of<'a>(p: &ProcInfo, engines: &'a [EnginePaths]) -> Option<(&'a str, usize)> {
    let is = |path: &Path| engines.iter().find(|e| e.paths.iter().any(|q| q == path));
    let exe = p
        .exe
        .clone()
        .or_else(|| p.args.first().map(PathBuf::from))?;
    if let Some(e) = is(&exe) {
        return Some((e.id.as_str(), 0));
    }
    if is_runtime(&exe) {
        if let Some(e) = p.args.get(1).and_then(|a| is(Path::new(a))) {
            return Some((e.id.as_str(), 1));
        }
    }
    None
}

/// `codex app-server`: the command's name and a subcommand-like word after
/// it (letters, digits and dashes, not an option), nothing else.
fn summary(p: &ProcInfo, at: usize) -> String {
    let name = p
        .args
        .get(at)
        .map(|a| {
            Path::new(a)
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .filter(|n| !n.is_empty())
        .or_else(|| {
            p.exe
                .as_ref()
                .and_then(|e| e.file_stem())
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let sub = p.args.get(at + 1).filter(|a| {
        !a.starts_with('-')
            && a.len() <= 32
            && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    });
    match sub {
        Some(sub) => format!("{name} {sub}"),
        None => name,
    }
}

/// This user's engine processes with no terminal, outside `app_pid`'s tree.
pub fn background(
    procs: &[ProcInfo],
    engines: &[EnginePaths],
    app_pid: u32,
    uid: u32,
) -> Vec<BackgroundProcess> {
    let parents: HashMap<u32, u32> = procs.iter().map(|p| (p.pid, p.ppid)).collect();
    let mut out: Vec<BackgroundProcess> = procs
        .iter()
        .filter(|p| p.uid == uid)
        .filter(|p| !p.tty && p.pid != app_pid && !descends_from(p.pid, app_pid, &parents))
        .filter_map(|p| {
            engine_of(p, engines).map(|(id, at)| BackgroundProcess {
                pid: p.pid,
                engine_id: id.to_string(),
                started: p.started,
                command: summary(p, at),
            })
        })
        .collect();
    out.sort_by_key(|b| b.pid);
    out
}

/// The listed process Stop may signal: the same pid, engine and start time.
pub fn still_listed<'a>(
    listed: &'a [BackgroundProcess],
    pid: u32,
    engine_id: &str,
    started: u64,
) -> Option<&'a BackgroundProcess> {
    listed
        .iter()
        .find(|b| b.pid == pid && b.engine_id == engine_id && b.started == started)
}

fn unavailable() -> tandem_core::IpcError {
    tandem_core::IpcError::new(
        tandem_core::ErrorCode::Unavailable,
        "Listing background processes isn't available on this system yet",
    )
}

/// The background list from `table`.
pub fn list(
    table: &dyn ProcessTable,
    engines: &[EnginePaths],
    app_pid: u32,
) -> Result<Vec<BackgroundProcess>, tandem_core::IpcError> {
    let uid = table.current_uid().ok_or_else(unavailable)?;
    let procs = table.list().ok_or_else(unavailable)?;
    Ok(background(&procs, engines, app_pid, uid))
}

/// Stop: lists again and signals only a process that is still this user's
/// engine process with the same pid, engine and start time.
pub fn stop(
    table: &dyn ProcessTable,
    engines: &[EnginePaths],
    app_pid: u32,
    pid: u32,
    engine_id: &str,
    started: u64,
) -> Result<(), tandem_core::IpcError> {
    let listed = list(table, engines, app_pid)?;
    if still_listed(&listed, pid, engine_id, started).is_none() {
        return Err(tandem_core::IpcError::new(
            tandem_core::ErrorCode::NotFound,
            "That process isn't running in the background any more",
        ));
    }
    table.terminate(pid).map_err(|e| {
        tandem_core::IpcError::new(tandem_core::ErrorCode::Io, format!("Couldn't stop it: {e}"))
    })
}

#[cfg(unix)]
mod platform {
    use super::*;

    pub struct Table;

    impl ProcessTable for Table {
        fn list(&self) -> Option<Vec<ProcInfo>> {
            use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, UpdateKind};
            let ttys = terminals()?;
            let mut sys = sysinfo::System::new();
            sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing()
                    .with_exe(UpdateKind::Always)
                    .with_cmd(UpdateKind::Always)
                    .with_user(UpdateKind::Always),
            );
            Some(
                sys.processes()
                    .values()
                    .filter(|p| {
                        !matches!(
                            p.status(),
                            sysinfo::ProcessStatus::Zombie | sysinfo::ProcessStatus::Dead
                        )
                    })
                    .filter_map(|p| {
                        let pid = p.pid().as_u32();
                        Some(ProcInfo {
                            pid,
                            ppid: p.parent().map(|q| q.as_u32()).unwrap_or(0),
                            uid: **p.effective_user_id()?,
                            started: p.start_time(),
                            // Unknown counts as a terminal: never listed.
                            tty: ttys.get(&pid).copied().unwrap_or(true),
                            exe: p.exe().map(Path::to_path_buf),
                            args: p
                                .cmd()
                                .iter()
                                .map(|a| a.to_string_lossy().into_owned())
                                .collect(),
                        })
                    })
                    .collect(),
            )
        }

        fn current_uid(&self) -> Option<u32> {
            use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, UpdateKind};
            let me = sysinfo::get_current_pid().ok()?;
            let mut sys = sysinfo::System::new();
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[me]),
                true,
                ProcessRefreshKind::nothing().with_user(UpdateKind::Always),
            );
            sys.process(me)?.effective_user_id().map(|u| **u)
        }

        fn terminate(&self, pid: u32) -> std::io::Result<()> {
            let out = std::process::Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .output()?;
            if out.status.success() {
                Ok(())
            } else {
                Err(std::io::Error::other(
                    String::from_utf8_lossy(&out.stderr).trim().to_string(),
                ))
            }
        }
    }

    /// Whether each process has a controlling terminal.
    #[cfg(target_os = "linux")]
    fn terminals() -> Option<HashMap<u32, bool>> {
        let dir = std::fs::read_dir("/proc").ok()?;
        Some(
            dir.flatten()
                .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
                .filter_map(|pid| {
                    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                    Some((pid, parse_stat_tty(&stat)?))
                })
                .collect(),
        )
    }

    #[cfg(target_os = "macos")]
    fn terminals() -> Option<HashMap<u32, bool>> {
        let out = std::process::Command::new("/bin/ps")
            .args(["-axo", "pid=,tty="])
            .output()
            .ok()?;
        Some(parse_ps_tty(&String::from_utf8_lossy(&out.stdout)))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn terminals() -> Option<HashMap<u32, bool>> {
        None
    }
}

/// `/proc/<pid>/stat`: the 5th field after the command's closing `)` is the
/// controlling terminal (0: none).
pub fn parse_stat_tty(stat: &str) -> Option<bool> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let tty: i64 = rest.split_whitespace().nth(4)?.parse().ok()?;
    Some(tty != 0)
}

/// `ps -axo pid=,tty=` (macOS): `??` is no terminal.
pub fn parse_ps_tty(text: &str) -> HashMap<u32, bool> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let pid = it.next()?.parse().ok()?;
            Some((pid, it.next()? != "??"))
        })
        .collect()
}

#[cfg(not(unix))]
mod platform {
    use super::*;

    pub struct Table;

    impl ProcessTable for Table {
        fn list(&self) -> Option<Vec<ProcInfo>> {
            None
        }

        fn current_uid(&self) -> Option<u32> {
            None
        }

        fn terminate(&self, _pid: u32) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "not available on this platform yet",
            ))
        }
    }
}

/// This platform's table.
pub fn system() -> Box<dyn ProcessTable> {
    Box::new(platform::Table)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: u32 = 1000;

    fn p(pid: u32, ppid: u32, tty: bool, exe: &str, args: &[&str]) -> ProcInfo {
        ProcInfo {
            pid,
            ppid,
            uid: ME,
            started: u64::from(pid) * 10,
            tty,
            exe: Some(PathBuf::from(exe)),
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    fn codex() -> Vec<EnginePaths> {
        vec![EnginePaths {
            id: "codex".into(),
            paths: vec![
                PathBuf::from("/usr/local/bin/codex"),
                PathBuf::from("/usr/local/lib/node_modules/@openai/codex/bin/codex.js"),
            ],
        }]
    }

    #[test]
    fn f_en_08_4_an_engine_process_with_no_terminal_outside_the_app_is_listed() {
        let app = 100;
        let script = "/usr/local/lib/node_modules/@openai/codex/bin/codex.js";
        let procs = vec![
            p(1, 0, false, "/sbin/init", &["/sbin/init"]),
            p(app, 1, false, "/opt/vibezeo/vibezeo", &["vibezeo"]),
            // The app's own pane: under the app, with a terminal.
            p(200, app, true, "/usr/bin/node", &["node", script]),
            // A thread's app-server: under the app, no terminal.
            p(
                201,
                app,
                false,
                "/usr/local/bin/codex",
                &["/usr/local/bin/codex", "app-server"],
            ),
            // The user's own terminal session of it.
            p(
                300,
                299,
                true,
                "/usr/local/bin/codex",
                &["/usr/local/bin/codex"],
            ),
            // Left behind: reparented to init, no terminal.
            p(
                400,
                1,
                false,
                "/usr/bin/node",
                &["node", script, "daemon", "--prompt", "deploy with sk-ant-x"],
            ),
            // Something else with no terminal.
            p(500, 1, false, "/usr/bin/sshd", &["sshd"]),
        ];
        let got = background(&procs, &codex(), app, ME);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].pid, 400);
        assert_eq!(got[0].engine_id, "codex");
        assert_eq!(got[0].started, 4000);
        // The name and subcommand only: never the prompt.
        assert_eq!(got[0].command, "codex daemon");
    }

    #[test]
    fn f_en_08_4_another_users_process_is_never_listed() {
        let mut theirs = p(
            400,
            1,
            false,
            "/usr/local/bin/codex",
            &["/usr/local/bin/codex", "exec", "their prompt"],
        );
        theirs.uid = 1001;
        let mine = p(
            401,
            1,
            false,
            "/usr/local/bin/codex",
            &["/usr/local/bin/codex"],
        );
        let got = background(&[theirs, mine], &codex(), 100, ME);
        assert_eq!(got.iter().map(|b| b.pid).collect::<Vec<_>>(), vec![401]);
    }

    #[test]
    fn f_en_08_4_a_wrapper_isnt_the_engine() {
        let procs = vec![
            p(
                10,
                1,
                false,
                "/usr/bin/timeout",
                &["timeout", "60", "/usr/local/bin/codex"],
            ),
            p(
                11,
                1,
                false,
                "/usr/bin/flock",
                &["flock", "/tmp/l", "/usr/local/bin/codex"],
            ),
            // A runtime whose script isn't the engine's.
            p(
                12,
                1,
                false,
                "/usr/bin/node",
                &["node", "server.js", "/usr/local/bin/codex"],
            ),
            // Bun running the engine's script is the engine.
            p(
                13,
                1,
                false,
                "/home/u/.bun/bin/bun",
                &[
                    "bun",
                    "/usr/local/lib/node_modules/@openai/codex/bin/codex.js",
                ],
            ),
        ];
        let got = background(&procs, &codex(), 100, ME);
        assert_eq!(got.iter().map(|b| b.pid).collect::<Vec<_>>(), vec![13]);
        assert_eq!(got[0].command, "codex");
    }

    #[test]
    fn f_en_08_4_a_path_with_spaces_matches_exactly() {
        let engines = vec![EnginePaths {
            id: "claude".into(),
            paths: vec![PathBuf::from("/Users/a b/.local/bin/claude")],
        }];
        let procs = vec![p(
            20,
            1,
            false,
            "/Users/a b/.local/bin/claude",
            &["/Users/a b/.local/bin/claude", "mcp", "serve"],
        )];
        let got = background(&procs, &engines, 100, ME);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].command, "claude mcp");
    }

    #[test]
    fn f_en_08_4_stop_needs_the_same_pid_engine_and_start_time() {
        let listed = background(
            &[p(
                400,
                1,
                false,
                "/usr/local/bin/codex",
                &["/usr/local/bin/codex"],
            )],
            &codex(),
            100,
            ME,
        );
        assert!(still_listed(&listed, 400, "codex", 4000).is_some());
        // The pid was reused by another process of the engine since.
        assert!(still_listed(&listed, 400, "codex", 3999).is_none());
        assert!(still_listed(&listed, 400, "claude", 4000).is_none());
        assert!(still_listed(&listed, 401, "codex", 4000).is_none());
    }

    #[test]
    fn f_en_08_4_a_deep_descendant_of_the_app_is_still_the_apps() {
        let procs = vec![
            p(10, 1, false, "/app", &["app"]),
            p(11, 10, false, "/bin/sh", &["sh"]),
            p(
                12,
                11,
                false,
                "/usr/local/bin/codex",
                &["/usr/local/bin/codex"],
            ),
        ];
        assert!(background(&procs, &codex(), 10, ME).is_empty());
        // A loop in the table doesn't hang.
        let looped = vec![
            p(20, 21, false, "/usr/local/bin/codex", &[]),
            p(21, 20, false, "/x", &[]),
        ];
        assert_eq!(background(&looped, &codex(), 10, ME).len(), 1);
    }

    #[test]
    fn f_en_08_4_the_terminal_columns_parse_on_both_platforms() {
        let with = "4321 (codex (beta)) S 1 4321 4321 34816 4321 4194304";
        assert_eq!(parse_stat_tty(with), Some(true));
        let without = "77 (node) S 1 77 77 0 -1 4194560";
        assert_eq!(parse_stat_tty(without), Some(false));
        assert_eq!(parse_stat_tty("garbage"), None);
        let ps = "  412 ??\n  900 ttys001\nnot a row\n";
        let ttys = parse_ps_tty(ps);
        assert_eq!(ttys.get(&412), Some(&false));
        assert_eq!(ttys.get(&900), Some(&true));
        assert_eq!(ttys.len(), 2);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn f_en_08_4_the_real_table_lists_this_process_with_its_user() {
        let table = system();
        let me = std::process::id();
        let all = table.list().expect("linux lists");
        let row = all.iter().find(|p| p.pid == me).expect("this process");
        assert_eq!(Some(row.uid), table.current_uid());
        assert!(row.started > 0);
    }

    struct Fake {
        procs: Vec<ProcInfo>,
        stopped: std::sync::Mutex<Vec<u32>>,
    }

    impl ProcessTable for Fake {
        fn list(&self) -> Option<Vec<ProcInfo>> {
            Some(self.procs.clone())
        }
        fn current_uid(&self) -> Option<u32> {
            Some(ME)
        }
        fn terminate(&self, pid: u32) -> std::io::Result<()> {
            self.stopped.lock().expect("lock").push(pid);
            Ok(())
        }
    }

    #[test]
    fn f_en_08_4_stop_rechecks_the_table_before_signalling() {
        let fake = Fake {
            procs: vec![p(
                400,
                1,
                false,
                "/usr/local/bin/codex",
                &["/usr/local/bin/codex"],
            )],
            stopped: std::sync::Mutex::new(Vec::new()),
        };
        // The list showed pid 400 started at 3999: it's another process now.
        let err = stop(&fake, &codex(), 100, 400, "codex", 3999).expect_err("reused pid");
        assert_eq!(err.code, tandem_core::ErrorCode::NotFound);
        // A pane's process, never listed, is never stopped.
        let err = stop(&fake, &codex(), 1, 400, "codex", 4000).expect_err("the app's own");
        assert_eq!(err.code, tandem_core::ErrorCode::NotFound);
        assert!(fake.stopped.lock().expect("lock").is_empty());
        stop(&fake, &codex(), 100, 400, "codex", 4000).expect("stopped");
        assert_eq!(*fake.stopped.lock().expect("lock"), vec![400]);
    }
}
