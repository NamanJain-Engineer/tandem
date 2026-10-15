//! Engine processes running in the background (F-EN-08 "Running in the
//! background"): an installed engine's processes with no terminal, outside
//! the app's own process tree. A process started in a terminal, the app's
//! or the user's own, is never listed, so Stop can't end someone's session.
//!
//! The process table is platform code behind [`ProcessTable`] (CLAUDE.md
//! "Platform code"): `/proc` on Linux, `ps` on macOS, nothing on Windows.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One process as the table reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: u32,
    /// It has a controlling terminal.
    pub tty: bool,
    pub exe: Option<PathBuf>,
    pub args: Vec<String>,
}

/// The OS's process table.
pub trait ProcessTable: Send + Sync {
    /// `None`: this platform can't list them (Windows).
    fn list(&self) -> Option<Vec<ProcInfo>>;
    /// Asks the process to end (SIGTERM).
    fn terminate(&self, pid: u32) -> std::io::Result<()>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackgroundProcess {
    pub pid: u32,
    pub engine_id: String,
    /// Its command line, cut to 300 characters.
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

fn engine_of<'a>(p: &ProcInfo, engines: &'a [EnginePaths]) -> Option<&'a str> {
    engines
        .iter()
        .find(|e| {
            e.paths.iter().any(|path| {
                p.exe.as_deref() == Some(path.as_path())
                    || p.args.iter().any(|a| Path::new(a) == path.as_path())
            })
        })
        .map(|e| e.id.as_str())
}

fn clip(s: &str) -> String {
    match s.char_indices().nth(300) {
        Some((cut, _)) => format!("{}…", &s[..cut]),
        None => s.to_string(),
    }
}

/// The engines' processes with no terminal, outside `app_pid`'s tree.
pub fn background(
    procs: &[ProcInfo],
    engines: &[EnginePaths],
    app_pid: u32,
) -> Vec<BackgroundProcess> {
    let parents: HashMap<u32, u32> = procs.iter().map(|p| (p.pid, p.ppid)).collect();
    let mut out: Vec<BackgroundProcess> = procs
        .iter()
        .filter(|p| !p.tty && p.pid != app_pid && !descends_from(p.pid, app_pid, &parents))
        .filter_map(|p| {
            engine_of(p, engines).map(|id| BackgroundProcess {
                pid: p.pid,
                engine_id: id.to_string(),
                command: clip(&p.args.join(" ")),
            })
        })
        .collect();
    out.sort_by_key(|b| b.pid);
    out
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;

    pub struct Table;

    impl ProcessTable for Table {
        fn list(&self) -> Option<Vec<ProcInfo>> {
            let dir = std::fs::read_dir("/proc").ok()?;
            Some(
                dir.flatten()
                    .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
                    .filter_map(read)
                    .collect(),
            )
        }

        fn terminate(&self, pid: u32) -> std::io::Result<()> {
            terminate_unix(pid)
        }
    }

    /// `/proc/<pid>/stat`: the fields after the command's closing `)`; the
    /// parent is the 2nd and the controlling terminal the 5th of those.
    fn read(pid: u32) -> Option<ProcInfo> {
        let base = PathBuf::from(format!("/proc/{pid}"));
        let stat = std::fs::read_to_string(base.join("stat")).ok()?;
        let (ppid, tty) = parse_stat(&stat)?;
        let args = std::fs::read(base.join("cmdline"))
            .ok()
            .map(|b| {
                b.split(|c| *c == 0)
                    .filter(|a| !a.is_empty())
                    .map(|a| String::from_utf8_lossy(a).into_owned())
                    .collect()
            })
            .unwrap_or_default();
        Some(ProcInfo {
            pid,
            ppid,
            tty,
            exe: std::fs::read_link(base.join("exe")).ok(),
            args,
        })
    }

    pub fn parse_stat(stat: &str) -> Option<(u32, bool)> {
        let rest = &stat[stat.rfind(')')? + 1..];
        let fields: Vec<&str> = rest.split_whitespace().collect();
        let ppid = fields.get(1)?.parse().ok()?;
        let tty: i64 = fields.get(4)?.parse().ok()?;
        Some((ppid, tty != 0))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    pub struct Table;

    impl ProcessTable for Table {
        fn list(&self) -> Option<Vec<ProcInfo>> {
            let out = std::process::Command::new("/bin/ps")
                .args(["-axo", "pid=,ppid=,tty=,command="])
                .output()
                .ok()?;
            Some(parse_ps(&String::from_utf8_lossy(&out.stdout)))
        }

        fn terminate(&self, pid: u32) -> std::io::Result<()> {
            terminate_unix(pid)
        }
    }

    /// `ps -axo pid=,ppid=,tty=,command=`; `??` is no terminal.
    pub fn parse_ps(text: &str) -> Vec<ProcInfo> {
        text.lines()
            .filter_map(|l| {
                let mut it = l.split_whitespace();
                let pid = it.next()?.parse().ok()?;
                let ppid = it.next()?.parse().ok()?;
                let tty = it.next()? != "??";
                let args: Vec<String> = it.map(str::to_string).collect();
                Some(ProcInfo {
                    pid,
                    ppid,
                    tty,
                    exe: args.first().map(PathBuf::from),
                    args,
                })
            })
            .collect()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    use super::*;

    pub struct Table;

    impl ProcessTable for Table {
        fn list(&self) -> Option<Vec<ProcInfo>> {
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

#[cfg(unix)]
fn terminate_unix(pid: u32) -> std::io::Result<()> {
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

/// This platform's table.
pub fn system() -> Box<dyn ProcessTable> {
    Box::new(platform::Table)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, tty: bool, exe: &str, args: &[&str]) -> ProcInfo {
        ProcInfo {
            pid,
            ppid,
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
        let procs = vec![
            p(1, 0, false, "/sbin/init", &["/sbin/init"]),
            p(app, 1, false, "/opt/vibezeo/vibezeo", &["vibezeo"]),
            // The app's own pane: under the app, with a terminal.
            p(
                200,
                app,
                true,
                "/usr/bin/node",
                &[
                    "node",
                    "/usr/local/lib/node_modules/@openai/codex/bin/codex.js",
                ],
            ),
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
                &[
                    "node",
                    "/usr/local/lib/node_modules/@openai/codex/bin/codex.js",
                    "daemon",
                ],
            ),
            // Something else with no terminal.
            p(500, 1, false, "/usr/bin/sshd", &["sshd"]),
        ];
        let got = background(&procs, &codex(), app);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].pid, 400);
        assert_eq!(got[0].engine_id, "codex");
        assert!(got[0].command.ends_with("codex.js daemon"));
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
        assert!(background(&procs, &codex(), 10).is_empty());
        // A loop in the table doesn't hang.
        let looped = vec![
            p(20, 21, false, "/usr/local/bin/codex", &[]),
            p(21, 20, false, "/x", &[]),
        ];
        assert_eq!(background(&looped, &codex(), 10).len(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn f_en_08_4_linux_stat_gives_the_parent_and_the_terminal() {
        let with = "4321 (codex (beta)) S 1 4321 4321 34816 4321 4194304";
        assert_eq!(platform::parse_stat(with), Some((1, true)));
        let without = "77 (node) S 1 77 77 0 -1 4194560";
        assert_eq!(platform::parse_stat(without), Some((1, false)));
        assert_eq!(platform::parse_stat("garbage"), None);
        // This process is in the table.
        let me = std::process::id();
        let all = system().list().expect("linux lists");
        assert!(all.iter().any(|p| p.pid == me));
    }
}
