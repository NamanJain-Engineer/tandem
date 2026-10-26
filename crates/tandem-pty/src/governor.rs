//! The resource governor (F-EX-06): each local agent pane's process tree in
//! its own resource group, its CPU and memory, an optional memory cap, and
//! Freeze/Thaw.
//!
//! One [`ResourceGroups`] per platform:
//! - Linux with a systemd user session: [`linux::SystemdScopes`], a
//!   transient user scope per pane (`busctl --user call … StartTransientUnit`
//!   with the pane's whole process tree once it exists, D-2545, and
//!   `AttachProcessesToUnit` for children that appeared meanwhile), read from
//!   its cgroup v2 files, frozen with `systemctl --user freeze` and stopped
//!   once the pane's processes are killed;
//! - everywhere else: [`Unplaced`], with the reason.
//!
//! An unplaced pane is measured over its process tree ([`tree_usage`]) and,
//! on Linux, frozen by signals ([`freeze_tree`], D-2546).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::suspend::{self, PausedJob, ProcessControl, SuspendError};

/// What this system can do (`resources_status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GovernorStatus {
    /// Panes get their own scope.
    pub grouping: bool,
    /// A memory cap can be set.
    pub capping: bool,
    /// Freeze is offered.
    pub freezing: bool,
    /// Why scopes or caps aren't available here.
    pub reason: Option<String>,
}

/// A scope's counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    /// CPU time used, all processes, in microseconds.
    pub cpu_usec: u64,
    /// Memory in use, in bytes (`None`: the memory controller isn't on).
    pub memory: Option<u64>,
    /// The cap, in bytes (`None`: no cap).
    pub memory_max: Option<u64>,
    /// Processes the kernel ended for going over the cap.
    pub oom_kills: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GovernorError {
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Failed(String),
}

pub const NO_SESSION: &str = "Memory caps need a systemd user session.";
pub const NO_MEMORY: &str = "This system's user session can't limit memory.";
pub const NOT_YET: &str = "Not available on this system yet";

pub trait ResourceGroups: Send + Sync {
    fn status(&self) -> GovernorStatus;
    /// Puts `pids` (the pane's process first) in a new scope `unit` for
    /// `pane`, capped at `memory_max` bytes.
    fn place(
        &self,
        unit: &str,
        pane: &str,
        pids: &[u32],
        memory_max: Option<u64>,
    ) -> Result<(), GovernorError>;
    /// The counters of `unit`, found through `pid`'s cgroup (`None` once
    /// `pid` has left it).
    fn usage(&self, unit: &str, pid: u32) -> Option<Usage>;
    fn freeze(&self, unit: &str) -> Result<(), GovernorError>;
    fn thaw(&self, unit: &str) -> Result<(), GovernorError>;
    /// Moves `pids` into `unit` (children that started while it was made).
    fn attach(&self, unit: &str, pids: &[u32]) -> Result<(), GovernorError>;
    /// Ends `unit` and anything still in it.
    fn stop(&self, unit: &str) -> Result<(), GovernorError>;
    /// Every pane scope the user manager has, from any launch.
    fn leftovers(&self) -> Vec<String>;
}

/// A unit name's prefix for every pane scope.
pub const UNIT_PREFIX: &str = "vibezeo-pane-";

/// `vibezeo-pane-<pane>-<launch>-<n>.scope`, with anything a unit name can't
/// hold replaced by `_`. `launch` tells this launch's scopes from those a
/// previous one left behind.
pub fn unit_name(pane: &str, launch: &str, n: u64) -> String {
    let safe: String = pane
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{UNIT_PREFIX}{safe}-{launch}-{n}.scope")
}

/// A launch's tag for [`unit_name`]: 8 hex digits from the pid and the clock.
pub fn launch_tag() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    format!("{:08x}", h.finish() as u32)
}

/// No scopes here.
pub struct Unplaced {
    pub reason: String,
    /// Freeze by signals (Linux).
    pub freezing: bool,
}

impl ResourceGroups for Unplaced {
    fn status(&self) -> GovernorStatus {
        GovernorStatus {
            grouping: false,
            capping: false,
            freezing: self.freezing,
            reason: Some(self.reason.clone()),
        }
    }
    fn place(&self, _: &str, _: &str, _: &[u32], _: Option<u64>) -> Result<(), GovernorError> {
        Err(GovernorError::Unavailable(self.reason.clone()))
    }
    fn usage(&self, _: &str, _: u32) -> Option<Usage> {
        None
    }
    fn freeze(&self, _: &str) -> Result<(), GovernorError> {
        Err(GovernorError::Unavailable(self.reason.clone()))
    }
    fn thaw(&self, _: &str) -> Result<(), GovernorError> {
        Err(GovernorError::Unavailable(self.reason.clone()))
    }
    fn attach(&self, _: &str, _: &[u32]) -> Result<(), GovernorError> {
        Err(GovernorError::Unavailable(self.reason.clone()))
    }
    fn stop(&self, _: &str) -> Result<(), GovernorError> {
        Err(GovernorError::Unavailable(self.reason.clone()))
    }
    fn leftovers(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Runs `program` with `args`; its stdout, or its stderr as the error.
pub type Runner = dyn Fn(&str, &[String]) -> Result<String, String> + Send + Sync;

/// A `busctl` or `systemctl` call that takes longer is ended and fails: a
/// stuck user manager never holds up a pane.
pub const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The real commands, ended after [`COMMAND_TIMEOUT`].
pub fn run_command(program: &str, args: &[String]) -> Result<String, String> {
    run_with_timeout(program, args, COMMAND_TIMEOUT)
}

pub fn run_with_timeout(
    program: &str,
    args: &[String],
    timeout: std::time::Duration,
) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} took longer than {}s", timeout.as_secs()));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success() {
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&stderr).trim().to_string())
    }
}

/// The platform's groups.
pub fn platform() -> Box<dyn ResourceGroups> {
    // A Flatpak build's pane pid is `flatpak-spawn`, not the pane's shell.
    if tandem_core::sandbox::in_flatpak() {
        return Box::new(Unplaced {
            reason: NOT_YET.to_string(),
            freezing: false,
        });
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: getuid(2) can't fail.
        let uid = unsafe { libc::getuid() };
        linux::SystemdScopes::detect(
            std::sync::Arc::new(run_command),
            "/sys/fs/cgroup",
            "/proc",
            uid,
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        Box::new(Unplaced {
            reason: NOT_YET.to_string(),
            freezing: false,
        })
    }
}

/// A pane's process tree, summed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TreeUsage {
    /// CPU time in milliseconds, of the processes alive now.
    pub cpu_ms: u64,
    /// Resident memory in bytes.
    pub rss: u64,
    /// Every process in the tree, the root first.
    pub pids: Vec<u32>,
}

/// Usage of the trees rooted at `roots` (owner → root pid), from one
/// process-table read.
pub fn tree_usage(roots: &[(String, u32)]) -> HashMap<String, TreeUsage> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, p) in sys.processes() {
        if let Some(parent) = p.parent() {
            children
                .entry(parent.as_u32())
                .or_default()
                .push(pid.as_u32());
        }
    }
    let mut out = HashMap::new();
    for (owner, root) in roots {
        let mut total = TreeUsage::default();
        let mut stack = vec![*root];
        while let Some(pid) = stack.pop() {
            if total.pids.contains(&pid) {
                continue;
            }
            let Some(p) = sys.process(sysinfo::Pid::from_u32(pid)) else {
                continue;
            };
            total.pids.push(pid);
            total.cpu_ms += p.accumulated_cpu_time();
            total.rss += p.memory();
            if let Some(kids) = children.get(&pid) {
                stack.extend(kids.iter().copied());
            }
        }
        if !total.pids.is_empty() {
            out.insert(owner.clone(), total);
        }
    }
    out
}

/// A pane frozen by signals: its shell's job as Pause stops it, and the
/// tree's other process groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenTree {
    pub job: PausedJob,
    pub others: Vec<i32>,
}

/// Stops a pane's tree without a freezer: the shell first and then its
/// foreground group, as Pause does (D-857, D-860), then every other group.
/// A group that can't be stopped (gone) is skipped.
pub fn freeze_tree(
    control: &dyn ProcessControl,
    shell: i32,
    foreground: Option<i32>,
    groups: &[i32],
) -> Result<FrozenTree, SuspendError> {
    let agent = foreground.filter(|&g| g > 1).unwrap_or(shell);
    let job = suspend::pause_or_hold_shell(control, agent, Some(shell))?;
    let mut others = Vec::new();
    for &g in groups {
        if g <= 1 || g == shell || g == agent || others.contains(&g) {
            continue;
        }
        if control.stop(g).is_ok() {
            others.push(g);
        }
    }
    Ok(FrozenTree { job, others })
}

/// Continues a tree [`freeze_tree`] stopped: the other groups, then the
/// job (agent first, then its shell).
pub fn thaw_tree(control: &dyn ProcessControl, frozen: &FrozenTree) -> Result<(), SuspendError> {
    for &g in &frozen.others {
        let _ = control.cont(g);
    }
    suspend::resume(control, &frozen.job)
}

#[cfg(target_os = "linux")]
pub mod linux {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    const DEST: &str = "org.freedesktop.systemd1";
    const PATH: &str = "/org/freedesktop/systemd1";
    const MANAGER: &str = "org.freedesktop.systemd1.Manager";
    /// `systemctl freeze` arrived in systemd 246.
    const FREEZE_SINCE: u32 = 246;
    /// Scopes take `OOMPolicy` from systemd 253; before, a scope whose
    /// process hit the cap was stopped whole.
    pub const OOM_POLICY_SINCE: u32 = 253;

    /// Transient systemd user scopes.
    pub struct SystemdScopes {
        run: Arc<Runner>,
        cgroup_root: PathBuf,
        proc_root: PathBuf,
        version: u32,
        capping: bool,
        freezer: bool,
        reason: Option<String>,
    }

    /// The major version in busctl's `s "255.4-1ubuntu8"`.
    pub fn parse_version(out: &str) -> Option<u32> {
        let v = out.trim().strip_prefix("s ")?.trim().trim_matches('"');
        v.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
    }

    /// The cgroup v2 path in `/proc/<pid>/cgroup` (`0::<path>`).
    pub fn cgroup_path(text: &str) -> Option<&str> {
        text.lines().find_map(|l| l.strip_prefix("0::"))
    }

    /// `pid`'s process group, from `/proc/<pid>/stat` (the field after the
    /// state and the parent, past the command's closing parenthesis).
    pub fn pgid(proc_root: &Path, pid: u32) -> Option<i32> {
        let stat = std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
        let rest = &stat[stat.rfind(')')? + 1..];
        rest.split_whitespace().nth(2)?.parse().ok()
    }

    /// The distinct process groups of `pids`.
    pub fn pgids(proc_root: &Path, pids: &[u32]) -> Vec<i32> {
        let mut out = Vec::new();
        for &pid in pids {
            if let Some(g) = pgid(proc_root, pid) {
                if !out.contains(&g) {
                    out.push(g);
                }
            }
        }
        out
    }

    fn read_u64(path: &Path) -> Option<u64> {
        std::fs::read_to_string(path).ok()?.trim().parse().ok()
    }

    /// `key value` lines (`cpu.stat`, `memory.events`).
    pub fn keyed(text: &str, key: &str) -> Option<u64> {
        text.lines().find_map(|l| {
            let (k, v) = l.split_once(' ')?;
            (k == key).then(|| v.trim().parse().ok()).flatten()
        })
    }

    impl SystemdScopes {
        /// Asks the user manager for its version; without one, or without
        /// cgroup v2, panes stay unplaced.
        pub fn detect(
            run: Arc<Runner>,
            cgroup_root: impl Into<PathBuf>,
            proc_root: impl Into<PathBuf>,
            uid: u32,
        ) -> Box<dyn ResourceGroups> {
            let cgroup_root = cgroup_root.into();
            let unplaced = |reason: &str| -> Box<dyn ResourceGroups> {
                Box::new(Unplaced {
                    reason: reason.to_string(),
                    freezing: true,
                })
            };
            if !cgroup_root.join("cgroup.controllers").is_file() {
                return unplaced(NO_MEMORY);
            }
            let args: Vec<String> = ["--user", "get-property", DEST, PATH, MANAGER, "Version"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let Some(version) = run("busctl", &args).ok().and_then(|o| parse_version(&o)) else {
                return unplaced(NO_SESSION);
            };
            // What's delegated to the user manager: it turns a controller on
            // below itself only once a unit asks for it.
            let manager = cgroup_root.join(format!(
                "user.slice/user-{uid}.slice/user@{uid}.service/cgroup.controllers"
            ));
            let capping = std::fs::read_to_string(manager)
                .map(|s| s.split_whitespace().any(|c| c == "memory"))
                .unwrap_or(false);
            Box::new(Self {
                run,
                cgroup_root,
                proc_root: proc_root.into(),
                version,
                capping,
                freezer: version >= FREEZE_SINCE,
                reason: (!capping).then(|| NO_MEMORY.to_string()),
            })
        }

        fn cgroup_dir(&self, unit: &str, pid: u32) -> Option<PathBuf> {
            let text = std::fs::read_to_string(self.proc_root.join(pid.to_string()).join("cgroup"))
                .ok()?;
            let path = cgroup_path(&text)?;
            if !path.ends_with(&format!("/{unit}")) {
                return None;
            }
            Some(self.cgroup_root.join(path.trim_start_matches('/')))
        }

        fn systemctl(&self, verb: &str, unit: &str) -> Result<(), GovernorError> {
            let args = vec!["--user".to_string(), verb.to_string(), unit.to_string()];
            (self.run)("systemctl", &args)
                .map(|_| ())
                .map_err(GovernorError::Failed)
        }
    }

    /// `StartTransientUnit`'s arguments for busctl, for a user manager at
    /// `version`.
    pub fn start_args(
        unit: &str,
        pane: &str,
        pids: &[u32],
        memory_max: Option<u64>,
        version: u32,
    ) -> Vec<String> {
        let mut props: Vec<String> = vec!["PIDs".into(), "au".into(), pids.len().to_string()];
        props.extend(pids.iter().map(|p| p.to_string()));
        props.extend([
            "CollectMode".into(),
            "s".into(),
            "inactive-or-failed".into(),
            "Description".into(),
            "s".into(),
            format!("Vibezeo pane {pane}"),
        ]);
        let mut count = 3;
        if version >= OOM_POLICY_SINCE {
            // A process over the cap is ended; the pane's shell stays.
            props.extend(["OOMPolicy".into(), "s".into(), "continue".into()]);
            count += 1;
        }
        if let Some(max) = memory_max {
            props.extend([
                "MemoryMax".into(),
                "t".into(),
                max.to_string(),
                "MemorySwapMax".into(),
                "t".into(),
                "0".into(),
            ]);
            count += 2;
        }
        let mut args: Vec<String> = [
            "--user",
            "call",
            DEST,
            PATH,
            MANAGER,
            "StartTransientUnit",
            "ssa(sv)a(sa(sv))",
            unit,
            "fail",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        args.push(count.to_string());
        args.extend(props);
        args.push("0".into());
        args
    }

    impl ResourceGroups for SystemdScopes {
        fn status(&self) -> GovernorStatus {
            GovernorStatus {
                grouping: true,
                capping: self.capping,
                freezing: true,
                reason: self.reason.clone(),
            }
        }

        fn place(
            &self,
            unit: &str,
            pane: &str,
            pids: &[u32],
            memory_max: Option<u64>,
        ) -> Result<(), GovernorError> {
            if memory_max.is_some() && !self.capping {
                return Err(GovernorError::Unavailable(NO_MEMORY.to_string()));
            }
            (self.run)(
                "busctl",
                &start_args(unit, pane, pids, memory_max, self.version),
            )
            .map(|_| ())
            .map_err(GovernorError::Failed)
        }

        fn attach(&self, unit: &str, pids: &[u32]) -> Result<(), GovernorError> {
            if pids.is_empty() {
                return Ok(());
            }
            let mut args: Vec<String> = [
                "--user",
                "call",
                DEST,
                PATH,
                MANAGER,
                "AttachProcessesToUnit",
                "ssau",
                unit,
                "",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            args.push(pids.len().to_string());
            args.extend(pids.iter().map(|p| p.to_string()));
            (self.run)("busctl", &args)
                .map(|_| ())
                .map_err(GovernorError::Failed)
        }

        fn stop(&self, unit: &str) -> Result<(), GovernorError> {
            self.systemctl("stop", unit)
        }

        fn leftovers(&self) -> Vec<String> {
            let args: Vec<String> = [
                "--user",
                "list-units",
                "--all",
                "--plain",
                "--no-legend",
                "--type=scope",
            ]
            .iter()
            .map(|s| s.to_string())
            .chain([format!("{UNIT_PREFIX}*")])
            .collect();
            (self.run)("systemctl", &args)
                .map(|out| {
                    out.lines()
                        .filter_map(|l| l.split_whitespace().next())
                        .filter(|u| u.starts_with(UNIT_PREFIX) && u.ends_with(".scope"))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        }

        fn usage(&self, unit: &str, pid: u32) -> Option<Usage> {
            let dir = self.cgroup_dir(unit, pid)?;
            let cpu = std::fs::read_to_string(dir.join("cpu.stat")).ok()?;
            let events = std::fs::read_to_string(dir.join("memory.events")).unwrap_or_default();
            let max = std::fs::read_to_string(dir.join("memory.max")).ok();
            Some(Usage {
                cpu_usec: keyed(&cpu, "usage_usec").unwrap_or(0),
                memory: read_u64(&dir.join("memory.current")),
                memory_max: max.and_then(|m| m.trim().parse().ok()),
                oom_kills: keyed(&events, "oom_kill").unwrap_or(0),
            })
        }

        fn freeze(&self, unit: &str) -> Result<(), GovernorError> {
            if !self.freezer {
                return Err(GovernorError::Unavailable(
                    "systemd is too old to freeze".into(),
                ));
            }
            self.systemctl("freeze", unit)
        }

        fn thaw(&self, unit: &str) -> Result<(), GovernorError> {
            if !self.freezer {
                return Err(GovernorError::Unavailable(
                    "systemd is too old to freeze".into(),
                ));
            }
            self.systemctl("thaw", unit)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn f_ex_06_unit_names_are_named_after_the_pane_and_the_launch() {
        assert_eq!(
            unit_name("p_01HX", "0a1b2c3d", 3),
            "vibezeo-pane-p_01HX-0a1b2c3d-3.scope"
        );
        assert_eq!(
            unit_name("a b/c.d", "0a1b2c3d", 1),
            "vibezeo-pane-a_b_c_d-0a1b2c3d-1.scope"
        );
        let tag = launch_tag();
        assert_eq!(tag.len(), 8);
        assert!(tag.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn f_ex_06_2_unplaced_says_why_for_everything() {
        let g = Unplaced {
            reason: NOT_YET.into(),
            freezing: false,
        };
        let s = g.status();
        assert!(!s.grouping && !s.capping && !s.freezing);
        assert_eq!(s.reason.as_deref(), Some(NOT_YET));
        for r in [
            g.place("u", "p", &[1], None),
            g.freeze("u"),
            g.thaw("u"),
            g.attach("u", &[1]),
            g.stop("u"),
        ] {
            assert_eq!(r, Err(GovernorError::Unavailable(NOT_YET.into())));
        }
        assert!(g.leftovers().is_empty());
        assert_eq!(g.usage("u", 1), None);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn f_ex_06_2_other_platforms_have_no_scopes_yet() {
        let s = platform().status();
        assert!(!s.grouping && !s.capping && !s.freezing);
        assert_eq!(s.reason.as_deref(), Some(NOT_YET));
    }

    #[derive(Default)]
    struct Fake {
        log: Mutex<Vec<String>>,
    }

    impl ProcessControl for Fake {
        fn stop(&self, g: i32) -> Result<(), SuspendError> {
            self.log
                .lock()
                .expect("test setup")
                .push(format!("stop {g}"));
            Ok(())
        }
        fn cont(&self, g: i32) -> Result<(), SuspendError> {
            self.log
                .lock()
                .expect("test setup")
                .push(format!("cont {g}"));
            Ok(())
        }
    }

    #[test]
    fn f_ex_06_4_freeze_by_signals_stops_the_shell_first_and_thaws_in_reverse() {
        let fake = Fake::default();
        let frozen =
            freeze_tree(&fake, 100, Some(200), &[100, 200, 300, 300, 400]).expect("test setup");
        assert_eq!(frozen.others, vec![300, 400]);
        thaw_tree(&fake, &frozen).expect("test setup");
        assert_eq!(
            *fake.log.lock().expect("test setup"),
            [
                "stop 100", "stop 200", "stop 300", "stop 400", "cont 300", "cont 400", "cont 200",
                "cont 100"
            ]
        );
    }

    #[test]
    fn f_ex_06_4_a_shell_at_its_prompt_is_stopped_alone() {
        let fake = Fake::default();
        let frozen = freeze_tree(&fake, 100, Some(100), &[100]).expect("test setup");
        assert!(frozen.others.is_empty());
        assert_eq!(*fake.log.lock().expect("test setup"), ["stop 100"]);
    }

    #[cfg(unix)]
    #[test]
    fn f_ex_06_1_tree_usage_sums_a_real_tree() {
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .expect("test setup");
        let me = std::process::id();
        let usage = tree_usage(&[("me".to_string(), me)]);
        let mine = &usage["me"];
        assert_eq!(mine.pids[0], me);
        assert!(mine.pids.contains(&child.id()));
        assert!(mine.rss > 0);
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::linux::*;
    use super::*;
    use std::sync::{Arc, Mutex};

    type Calls = Arc<Mutex<Vec<(String, Vec<String>)>>>;

    fn runner(version: Option<&'static str>) -> (Arc<Runner>, Calls) {
        let calls: Calls = Arc::default();
        let seen = calls.clone();
        let run: Arc<Runner> = Arc::new(move |program: &str, args: &[String]| {
            seen.lock()
                .expect("test setup")
                .push((program.to_string(), args.to_vec()));
            if args.get(1).map(String::as_str) == Some("get-property") {
                return version
                    .map(|v| format!("s \"{v}\"\n"))
                    .ok_or_else(|| "Failed to connect to bus: No medium found".to_string());
            }
            Ok(String::new())
        });
        (run, calls)
    }

    /// A cgroup v2 root with `controllers` delegated to the user manager.
    fn cgroup_root(controllers: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("test setup");
        std::fs::write(dir.path().join("cgroup.controllers"), "cpu memory pids")
            .expect("test setup");
        let manager = dir
            .path()
            .join("user.slice/user-1000.slice/user@1000.service");
        std::fs::create_dir_all(&manager).expect("test setup");
        std::fs::write(manager.join("cgroup.controllers"), controllers).expect("test setup");
        // Not turned on below the manager yet: no unit has asked for it.
        std::fs::write(manager.join("cgroup.subtree_control"), "").expect("test setup");
        dir
    }

    #[test]
    fn f_ex_06_2_without_a_user_manager_panes_stay_unplaced_and_caps_say_why() {
        let (run, _) = runner(None);
        let root = cgroup_root("memory pids");
        let g = SystemdScopes::detect(run, root.path(), "/proc", 1000);
        let s = g.status();
        assert!(!s.grouping && !s.capping && s.freezing);
        assert_eq!(s.reason.as_deref(), Some(NO_SESSION));
        assert!(matches!(
            g.place("u.scope", "p", &[1], None),
            Err(GovernorError::Unavailable(_))
        ));
    }

    #[test]
    fn f_ex_06_2_without_the_memory_controller_scopes_work_but_caps_dont() {
        let (run, _) = runner(Some("255.4-1ubuntu8"));
        let root = cgroup_root("pids");
        let g = SystemdScopes::detect(run, root.path(), "/proc", 1000);
        let s = g.status();
        assert!(s.grouping && !s.capping);
        assert_eq!(s.reason.as_deref(), Some(NO_MEMORY));
        assert!(g.place("u.scope", "p", &[1], None).is_ok());
        assert!(matches!(
            g.place("u.scope", "p", &[1], Some(512 << 20)),
            Err(GovernorError::Unavailable(_))
        ));
    }

    #[test]
    fn f_ex_06_1_place_starts_a_transient_scope_with_the_panes_processes() {
        let (run, calls) = runner(Some("252"));
        let root = cgroup_root("cpu memory pids");
        let g = SystemdScopes::detect(run, root.path(), "/proc", 1000);
        assert!(g.status().capping);
        g.place("vibezeo-pane-p-1.scope", "p", &[41, 42], Some(512 << 20))
            .expect("test setup");
        let calls = calls.lock().expect("test setup");
        let (program, args) = calls.last().expect("test setup");
        assert_eq!(program, "busctl");
        assert_eq!(
            args.join(" "),
            "--user call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
             org.freedesktop.systemd1.Manager StartTransientUnit ssa(sv)a(sa(sv)) \
             vibezeo-pane-p-1.scope fail 5 PIDs au 2 41 42 CollectMode s inactive-or-failed \
             Description s Vibezeo pane p MemoryMax t 536870912 MemorySwapMax t 0 0"
        );
    }

    #[test]
    fn f_ex_06_2_from_systemd_253_a_process_over_the_cap_ends_alone() {
        let root = cgroup_root("cpu memory pids");
        for (version, oom) in [("252", false), ("253", true), ("255.4", true)] {
            let (run, calls) = runner(Some(version));
            let g = SystemdScopes::detect(run, root.path(), "/proc", 1000);
            g.place("u.scope", "p", &[41], Some(1 << 30))
                .expect("test setup");
            let calls = calls.lock().expect("test setup");
            let args = calls.last().expect("test setup").1.join(" ");
            assert_eq!(
                args.contains("OOMPolicy s continue"),
                oom,
                "{version}: {args}"
            );
            let count = if oom { "6" } else { "5" };
            assert!(
                args.contains(&format!("u.scope fail {count} ")),
                "{version}: {args}"
            );
        }
    }

    #[test]
    fn f_ex_06_1_late_children_are_attached_and_scopes_stop() {
        let (run, calls) = runner(Some("255"));
        let root = cgroup_root("memory");
        let g = SystemdScopes::detect(run, root.path(), "/proc", 1000);
        g.attach("u.scope", &[]).expect("nothing to attach");
        g.attach("u.scope", &[43, 44]).expect("test setup");
        g.stop("u.scope").expect("test setup");
        let calls = calls.lock().expect("test setup");
        let got: Vec<String> = calls
            .iter()
            .filter(|(_, a)| a.get(1).map(String::as_str) != Some("get-property"))
            .map(|(p, a)| format!("{p} {}", a.join(" ")))
            .collect();
        assert_eq!(
            got,
            [
                "busctl --user call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
                 org.freedesktop.systemd1.Manager AttachProcessesToUnit ssau u.scope  2 43 44",
                "systemctl --user stop u.scope",
            ]
        );
    }

    #[test]
    fn f_ex_06_4_leftover_scopes_are_listed_by_prefix() {
        let calls: Calls = Arc::default();
        let seen = calls.clone();
        let run: Arc<Runner> = Arc::new(move |_: &str, args: &[String]| {
            seen.lock()
                .expect("test setup")
                .push((String::new(), args.to_vec()));
            if args.get(1).map(String::as_str) == Some("get-property") {
                return Ok("s \"255\"".into());
            }
            Ok(
                "vibezeo-pane-p_1-aaaa0000-1.scope loaded active running Vibezeo pane p_1\n\
                vibezeo-pane-p_2-bbbb1111-4.scope loaded active running Vibezeo pane p_2\n\
                other.scope loaded active running x\n"
                    .into(),
            )
        });
        let root = cgroup_root("memory");
        let g = SystemdScopes::detect(run, root.path(), "/proc", 1000);
        assert_eq!(
            g.leftovers(),
            [
                "vibezeo-pane-p_1-aaaa0000-1.scope",
                "vibezeo-pane-p_2-bbbb1111-4.scope"
            ]
        );
        let calls = calls.lock().expect("test setup");
        assert_eq!(
            calls.last().expect("test setup").1.join(" "),
            "--user list-units --all --plain --no-legend --type=scope vibezeo-pane-*"
        );
    }

    #[test]
    fn f_ex_06_8_a_stuck_command_is_ended() {
        let started = std::time::Instant::now();
        let out = run_with_timeout(
            "sleep",
            &["5".to_string()],
            std::time::Duration::from_millis(200),
        );
        assert!(out.is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
        assert_eq!(
            run_with_timeout("echo", &["hi".to_string()], COMMAND_TIMEOUT),
            Ok("hi\n".to_string())
        );
    }

    #[test]
    fn f_ex_06_3_usage_reads_the_scopes_cgroup_and_its_oom_kills() {
        let (run, _) = runner(Some("255"));
        let root = cgroup_root("cpu memory pids");
        let unit = "vibezeo-pane-p-1.scope";
        let rel = format!("user.slice/user-1000.slice/user@1000.service/app.slice/{unit}");
        let dir = root.path().join(&rel);
        std::fs::create_dir_all(&dir).expect("test setup");
        std::fs::write(
            dir.join("cpu.stat"),
            "usage_usec 2500000\nuser_usec 2000000\n",
        )
        .expect("test setup");
        std::fs::write(dir.join("memory.current"), "880803840\n").expect("test setup");
        std::fs::write(dir.join("memory.max"), "536870912\n").expect("test setup");
        std::fs::write(
            dir.join("memory.events"),
            "low 0\nhigh 0\nmax 4\noom 1\noom_kill 1\n",
        )
        .expect("test setup");
        let proc = tempfile::tempdir().expect("test setup");
        std::fs::create_dir_all(proc.path().join("41")).expect("test setup");
        std::fs::write(proc.path().join("41/cgroup"), format!("0::/{rel}\n")).expect("test setup");
        std::fs::create_dir_all(proc.path().join("50")).expect("test setup");
        std::fs::write(
            proc.path().join("50/cgroup"),
            "0::/user.slice/other.scope\n",
        )
        .expect("test setup");

        let g = SystemdScopes::detect(run, root.path(), proc.path(), 1000);
        assert_eq!(
            g.usage(unit, 41),
            Some(Usage {
                cpu_usec: 2_500_000,
                memory: Some(880_803_840),
                memory_max: Some(536_870_912),
                oom_kills: 1,
            })
        );
        // A pid that left the scope reads nothing.
        assert_eq!(g.usage(unit, 50), None);
        std::fs::write(dir.join("memory.max"), "max\n").expect("test setup");
        assert_eq!(g.usage(unit, 41).expect("test setup").memory_max, None);
    }

    #[test]
    fn f_ex_06_4_freeze_uses_systemds_freezer_from_246() {
        let root = cgroup_root("memory");
        let (run, calls) = runner(Some("246"));
        let g = SystemdScopes::detect(run, root.path(), "/proc", 1000);
        g.freeze("u.scope").expect("test setup");
        g.thaw("u.scope").expect("test setup");
        let calls = calls.lock().expect("test setup");
        let verbs: Vec<String> = calls
            .iter()
            .filter(|(p, _)| p == "systemctl")
            .map(|(_, a)| a.join(" "))
            .collect();
        assert_eq!(verbs, ["--user freeze u.scope", "--user thaw u.scope"]);
        let (old, _) = runner(Some("245"));
        let g = SystemdScopes::detect(old, root.path(), "/proc", 1000);
        assert!(matches!(
            g.freeze("u.scope"),
            Err(GovernorError::Unavailable(_))
        ));
    }

    #[test]
    fn f_ex_06_parsers() {
        assert_eq!(parse_version("s \"255.4-1ubuntu8\"\n"), Some(255));
        assert_eq!(parse_version("garbage"), None);
        assert_eq!(
            cgroup_path("0::/user.slice/x.scope\n"),
            Some("/user.slice/x.scope")
        );
        assert_eq!(keyed("oom 2\noom_kill 3\n", "oom_kill"), Some(3));
        let proc = tempfile::tempdir().expect("test setup");
        std::fs::create_dir_all(proc.path().join("7")).expect("test setup");
        // A command name with spaces and parentheses.
        std::fs::write(
            proc.path().join("7/stat"),
            "7 (my (odd) cmd) S 1 7000 7000 0 -1\n",
        )
        .expect("test setup");
        assert_eq!(pgid(proc.path(), 7), Some(7000));
        assert_eq!(pgids(proc.path(), &[7, 7, 8]), vec![7000]);
    }
}
