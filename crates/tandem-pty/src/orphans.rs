//! Orphaned children from a crashed run (F-CD-03 AC8, ARCHITECTURE §5.4).
//!
//! Every spawn is recorded in this run's `$DATA/run/children-<pid>.json`
//! (with the app's own PID and start time as its owner) and removed when it
//! exits. On the next launch, files whose owner is gone are collected, and
//! their records whose process is still alive with the same start time (so a
//! reused PID never matches) are orphans. A file whose owner is still alive
//! belongs to an instance that is shutting down: the scan waits for it, so
//! children that instance is about to stop aren't reported, and neither run
//! overwrites the other's records. On Unix the record's PID is also the session id of everything the
//! pane started, so a `nohup` child of a shell that died with the app is
//! found too. Windows trees die with their Job Object, so only the root is
//! checked there.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One recorded child (`run/children.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildRecord {
    pub pane_id: String,
    pub pid: u32,
    /// Seconds since the epoch, as the OS reports it.
    pub start_time: u64,
    /// Program file name only (no arguments: they can hold prompts).
    pub exe: String,
    pub cwd: String,
    pub started_at: i64,
}

/// What the "still running" dialog lists (UI-132).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OrphanInfo {
    pub pane_id: String,
    pub exe: String,
    pub cwd: String,
    pub started_at: i64,
    /// Live processes of that pane (root and session members).
    pub pids: Vec<u32>,
}

/// A live process as the platform table reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcRow {
    pub pid: u32,
    /// Unix session id; `None` where sessions don't apply (Windows).
    pub session: Option<u32>,
    pub start_time: u64,
}

/// The OS process table, behind a trait so scans and kills are tested with fakes.
pub trait ProcessTable: Send {
    fn rows(&mut self, pids: Option<&[u32]>) -> Vec<ProcRow>;
    /// Asks a process to end (SIGTERM on Unix); false if it's gone.
    fn terminate(&mut self, pid: u32) -> bool;
    fn kill(&mut self, pid: u32) -> bool;
}

/// The real table (`sysinfo`).
pub struct SystemTable(sysinfo::System);

impl Default for SystemTable {
    fn default() -> Self {
        Self(sysinfo::System::new())
    }
}

impl ProcessTable for SystemTable {
    fn rows(&mut self, pids: Option<&[u32]>) -> Vec<ProcRow> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate};
        let wanted: Vec<Pid> = pids
            .unwrap_or(&[])
            .iter()
            .map(|p| Pid::from_u32(*p))
            .collect();
        let which = if pids.is_some() {
            ProcessesToUpdate::Some(&wanted)
        } else {
            ProcessesToUpdate::All
        };
        self.0
            .refresh_processes_specifics(which, true, ProcessRefreshKind::nothing());
        self.0
            .processes()
            .values()
            .filter(|p| pids.is_none() || wanted.contains(&p.pid()))
            // A zombie has already exited (its parent just hasn't reaped it,
            // e.g. a container whose PID 1 doesn't); it can't be stopped.
            .filter(|p| {
                !matches!(
                    p.status(),
                    sysinfo::ProcessStatus::Zombie | sysinfo::ProcessStatus::Dead
                )
            })
            .map(|p| ProcRow {
                pid: p.pid().as_u32(),
                session: if cfg!(unix) {
                    p.session_id().map(|s| s.as_u32())
                } else {
                    None
                },
                start_time: p.start_time(),
            })
            .collect()
    }

    fn terminate(&mut self, pid: u32) -> bool {
        let pid = sysinfo::Pid::from_u32(pid);
        self.0
            .process(pid)
            .map(|p| {
                p.kill_with(sysinfo::Signal::Term)
                    .unwrap_or_else(|| p.kill())
            })
            .unwrap_or(false)
    }

    fn kill(&mut self, pid: u32) -> bool {
        self.0
            .process(sysinfo::Pid::from_u32(pid))
            .map(|p| p.kill())
            .unwrap_or(false)
    }
}

/// The app process that wrote a registry file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Owner {
    pub pid: u32,
    pub start_time: u64,
}

impl Owner {
    /// This process, as `table` reports it.
    pub fn current(table: &mut dyn ProcessTable) -> Self {
        let pid = std::process::id();
        let start_time = table
            .rows(Some(&[pid]))
            .first()
            .map(|r| r.start_time)
            .unwrap_or(0);
        Self { pid, start_time }
    }

    fn alive(&self, table: &mut dyn ProcessTable) -> bool {
        table
            .rows(Some(&[self.pid]))
            .iter()
            .any(|r| r.start_time == self.start_time)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryFile {
    owner: Option<Owner>,
    children: Vec<ChildRecord>,
}

/// A registry file: this format, or the older bare list (no owner).
fn read_file(path: &Path) -> Option<RegistryFile> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice::<RegistryFile>(&bytes)
        .ok()
        .or_else(|| {
            serde_json::from_slice::<Vec<ChildRecord>>(&bytes)
                .ok()
                .map(|children| RegistryFile {
                    owner: None,
                    children,
                })
        })
}

fn registry_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("children") && n.ends_with(".json"))
        })
        .collect();
    files.sort();
    files
}

/// Collects (and deletes) the registry files in `dir` whose owner has exited,
/// waiting up to `wait` for owners that are still shutting down. Files of an
/// owner still alive after that belong to a running instance and are left,
/// as is `own` (this run's file).
pub fn collect_stale(
    dir: &Path,
    own: &Path,
    table: &mut dyn ProcessTable,
    wait: Duration,
) -> Vec<ChildRecord> {
    let deadline = Instant::now() + wait;
    let mut out = Vec::new();
    for path in registry_files(dir) {
        if path == own {
            continue;
        }
        let Some(file) = read_file(&path) else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        if let Some(owner) = file.owner {
            while owner.alive(table) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            if owner.alive(table) {
                continue;
            }
        }
        out.extend(file.children);
        let _ = std::fs::remove_file(&path);
    }
    out
}

/// The children of this run, persisted on every change.
pub struct ChildRegistry {
    path: PathBuf,
    owner: Owner,
    entries: Mutex<Vec<ChildRecord>>,
    table: Mutex<Box<dyn ProcessTable>>,
}

impl ChildRegistry {
    /// Starts this run's empty registry in `dir` (`children-<pid>.json`).
    /// Collect last run's files first: a file left by a dead process with
    /// this PID is overwritten.
    pub fn new(dir: &Path, mut table: Box<dyn ProcessTable>) -> Self {
        let owner = Owner::current(table.as_mut());
        let path = dir.join(format!("children-{}.json", owner.pid));
        // A crashed run that had this PID: keep its records for the scan.
        if path.exists() {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let _ = std::fs::rename(
                &path,
                dir.join(format!("children-{}-{nanos}.json", owner.pid)),
            );
        }
        let reg = Self {
            path,
            owner,
            entries: Mutex::new(Vec::new()),
            table: Mutex::new(table),
        };
        reg.save();
        reg
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A clean exit: every child has been stopped, so nothing is left to find.
    pub fn close(&self) {
        let _ = std::fs::remove_file(&self.path);
    }

    pub fn add(&self, pane_id: &str, pid: u32, exe: &str, cwd: &str, started_at: i64) {
        let start_time = self
            .table
            .lock()
            .ok()
            .and_then(|mut t| t.rows(Some(&[pid])).first().map(|r| r.start_time))
            .unwrap_or(0);
        if let Ok(mut e) = self.entries.lock() {
            e.retain(|r| r.pane_id != pane_id);
            e.push(ChildRecord {
                pane_id: pane_id.to_string(),
                pid,
                start_time,
                exe: exe.to_string(),
                cwd: cwd.to_string(),
                started_at,
            });
        }
        self.save();
    }

    pub fn remove(&self, pane_id: &str) {
        if let Ok(mut e) = self.entries.lock() {
            e.retain(|r| r.pane_id != pane_id);
        }
        self.save();
    }

    pub fn records(&self) -> Vec<ChildRecord> {
        self.entries.lock().map(|e| e.clone()).unwrap_or_default()
    }

    fn save(&self) {
        let file = RegistryFile {
            owner: Some(self.owner),
            children: self.records(),
        };
        let Ok(body) = serde_json::to_vec_pretty(&file) else {
            return;
        };
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(err) = tandem_core::atomic::atomic_write(&self.path, &body, Some(0o600)) {
            tracing::warn!(code = "pty.children_write_failed", error = %err, "couldn't save the child registry");
        }
    }
}

/// The records in one registry file (either format).
pub fn read_records(path: &Path) -> Vec<ChildRecord> {
    read_file(path).map(|f| f.children).unwrap_or_default()
}

/// Records whose processes outlived the run that started them.
pub fn scan(records: &[ChildRecord], table: &mut dyn ProcessTable) -> Vec<OrphanInfo> {
    if records.is_empty() {
        return Vec::new();
    }
    let rows = table.rows(None);
    records
        .iter()
        .filter_map(|rec| {
            let pids: Vec<u32> = rows
                .iter()
                .filter(|r| {
                    let root = r.pid == rec.pid && r.start_time == rec.start_time;
                    let member = r.session == Some(rec.pid)
                        && r.start_time >= rec.start_time
                        && r.pid != rec.pid;
                    root || member
                })
                .map(|r| r.pid)
                .collect();
            (!pids.is_empty()).then(|| OrphanInfo {
                pane_id: rec.pane_id.clone(),
                exe: rec.exe.clone(),
                cwd: rec.cwd.clone(),
                started_at: rec.started_at,
                pids,
            })
        })
        .collect()
}

/// Ends an orphan's processes: terminate, then kill whatever is left after
/// `grace`. Only PIDs that still match the scan are signalled.
pub fn stop(orphans: &[OrphanInfo], table: &mut dyn ProcessTable, grace: Duration) {
    let pids: Vec<u32> = orphans.iter().flat_map(|o| o.pids.clone()).collect();
    if pids.is_empty() {
        return;
    }
    let known: Vec<(u32, u64)> = table
        .rows(Some(&pids))
        .iter()
        .map(|r| (r.pid, r.start_time))
        .collect();
    let still = |table: &mut dyn ProcessTable| -> Vec<u32> {
        table
            .rows(Some(&pids))
            .iter()
            .filter(|r| known.contains(&(r.pid, r.start_time)))
            .map(|r| r.pid)
            .collect()
    };
    for pid in still(table) {
        table.terminate(pid);
    }
    let until = Instant::now() + grace;
    while Instant::now() < until && !still(table).is_empty() {
        std::thread::sleep(Duration::from_millis(50));
    }
    for pid in still(table) {
        table.kill(pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeTable {
        procs: HashMap<u32, ProcRow>,
        /// PIDs that ignore SIGTERM.
        stubborn: Vec<u32>,
        signals: Vec<(u32, &'static str)>,
    }

    impl FakeTable {
        fn with(rows: &[(u32, Option<u32>, u64)]) -> Self {
            let mut t = Self::default();
            for (pid, session, start_time) in rows {
                t.procs.insert(
                    *pid,
                    ProcRow {
                        pid: *pid,
                        session: *session,
                        start_time: *start_time,
                    },
                );
            }
            t
        }
    }

    impl ProcessTable for FakeTable {
        fn rows(&mut self, pids: Option<&[u32]>) -> Vec<ProcRow> {
            self.procs
                .values()
                .filter(|r| pids.map(|p| p.contains(&r.pid)).unwrap_or(true))
                .cloned()
                .collect()
        }
        fn terminate(&mut self, pid: u32) -> bool {
            self.signals.push((pid, "term"));
            if !self.stubborn.contains(&pid) {
                self.procs.remove(&pid);
            }
            true
        }
        fn kill(&mut self, pid: u32) -> bool {
            self.signals.push((pid, "kill"));
            self.procs.remove(&pid).is_some()
        }
    }

    fn rec(pane: &str, pid: u32, start_time: u64) -> ChildRecord {
        ChildRecord {
            pane_id: pane.into(),
            pid,
            start_time,
            exe: "bash".into(),
            cwd: "/code".into(),
            started_at: 1,
        }
    }

    #[test]
    fn f_cd_03_8_live_children_with_matching_start_times_are_orphans() {
        let mut table = FakeTable::with(&[
            (100, Some(100), 50), // the shell, still alive
            (300, Some(300), 70), // pid reused by an unrelated process
            (400, Some(200), 90), // nohup child of pane 2, whose shell died
            (500, Some(500), 10), // unrelated
        ]);
        let records = [
            rec("pn_1", 100, 50),
            rec("pn_2", 200, 60),
            rec("pn_3", 300, 65),
        ];
        let found = scan(&records, &mut table);
        let by_pane: HashMap<&str, Vec<u32>> = found
            .iter()
            .map(|o| (o.pane_id.as_str(), o.pids.clone()))
            .collect();
        assert_eq!(by_pane.get("pn_1"), Some(&vec![100]));
        assert_eq!(by_pane.get("pn_2"), Some(&vec![400]));
        assert!(
            !by_pane.contains_key("pn_3"),
            "a reused pid isn't an orphan"
        );
    }

    #[test]
    fn f_cd_03_8_stop_terminates_then_kills_the_stubborn() {
        let mut table = FakeTable::with(&[(100, Some(100), 50), (101, Some(100), 51)]);
        table.stubborn = vec![101];
        let orphans = scan(&[rec("pn_1", 100, 50)], &mut table);
        stop(&orphans, &mut table, Duration::from_millis(100));
        assert!(table.procs.is_empty());
        assert!(table.signals.contains(&(100, "term")));
        assert!(table.signals.contains(&(101, "kill")));
        assert!(!table.signals.contains(&(100, "kill")));
    }

    #[test]
    fn registry_persists_adds_and_removes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let run = dir.path().join("run");
        let reg = ChildRegistry::new(&run, Box::new(FakeTable::with(&[(42, Some(42), 7)])));
        let path = reg.path().to_path_buf();
        assert_eq!(
            path,
            run.join(format!("children-{}.json", std::process::id()))
        );
        assert!(read_records(&path).is_empty());
        reg.add("pn_1", 42, "zsh", "/code", 5);
        let saved = read_records(&path);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].start_time, 7);
        reg.remove("pn_1");
        assert!(read_records(&path).is_empty());
        reg.close();
        assert!(!path.exists(), "a clean exit leaves no file");
    }

    #[test]
    fn a_file_left_by_a_dead_run_with_this_pid_is_kept_for_the_scan() {
        let dir = tempfile::tempdir().expect("tempdir");
        let me = std::process::id();
        let mut table = FakeTable::with(&[(me, Some(me), 9)]);
        let old = dir.path().join(format!("children-{me}.json"));
        write_file(
            &old,
            Some(Owner {
                pid: me,
                start_time: 2,
            }),
            vec![rec("pn_old", 500, 1)],
        );
        let reg = ChildRegistry::new(dir.path(), Box::new(FakeTable::with(&[(me, Some(me), 9)])));
        let got = collect_stale(
            dir.path(),
            reg.path(),
            &mut table,
            Duration::from_millis(100),
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].pane_id, "pn_old");
        assert!(reg.path().exists(), "this run's file is untouched");
    }

    fn write_file(path: &Path, owner: Option<Owner>, children: Vec<ChildRecord>) {
        std::fs::create_dir_all(path.parent().expect("dir")).expect("mkdir");
        let body = serde_json::to_vec(&RegistryFile { owner, children }).expect("json");
        std::fs::write(path, body).expect("write");
    }

    #[test]
    fn f_cd_03_8_only_files_of_exited_owners_are_collected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let run = dir.path();
        // Owner 10 crashed; owner 20 is a live instance; a legacy list has no owner.
        let mut table = FakeTable::with(&[(20, Some(20), 5)]);
        write_file(
            &run.join("children-10.json"),
            Some(Owner {
                pid: 10,
                start_time: 3,
            }),
            vec![rec("pn_a", 100, 50)],
        );
        write_file(
            &run.join("children-20.json"),
            Some(Owner {
                pid: 20,
                start_time: 5,
            }),
            vec![rec("pn_b", 200, 60)],
        );
        std::fs::write(
            run.join("children.json"),
            serde_json::to_vec(&vec![rec("pn_c", 300, 70)]).expect("json"),
        )
        .expect("write");
        let started = Instant::now();
        let got = collect_stale(
            run,
            &run.join("mine.json"),
            &mut table,
            Duration::from_millis(300),
        );
        let panes: Vec<&str> = got.iter().map(|r| r.pane_id.as_str()).collect();
        assert_eq!(panes, vec!["pn_a", "pn_c"]);
        assert!(
            started.elapsed() >= Duration::from_millis(300),
            "waited for the live owner"
        );
        assert!(
            run.join("children-20.json").exists(),
            "a live instance's file stays"
        );
        assert!(!run.join("children-10.json").exists() && !run.join("children.json").exists());
    }

    #[test]
    fn f_cd_03_8_an_owner_that_exits_during_the_wait_hands_over_its_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let run = dir.path().to_path_buf();
        // The owner has the same pid but a different start time: it's gone.
        let mut table = FakeTable::with(&[(30, Some(30), 99)]);
        write_file(
            &run.join("children-30.json"),
            Some(Owner {
                pid: 30,
                start_time: 8,
            }),
            vec![rec("pn_d", 400, 80)],
        );
        let started = Instant::now();
        let got = collect_stale(
            &run,
            &run.join("mine.json"),
            &mut table,
            Duration::from_secs(5),
        );
        assert_eq!(got.len(), 1);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "no wait for a dead owner"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn f_cd_03_8_an_exited_but_unreaped_child_is_not_an_orphan() {
        // A child that exited and wasn't reaped yet is a zombie.
        let mut child = std::process::Command::new("true").spawn().expect("spawn");
        let pid = child.id();
        std::thread::sleep(Duration::from_millis(200));
        let mut table = SystemTable::default();
        assert!(
            table.rows(Some(&[pid])).is_empty(),
            "zombies aren't live processes"
        );
        let _ = child.wait();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_system_table_sees_this_process() {
        let mut table = SystemTable::default();
        let me = std::process::id();
        let rows = table.rows(Some(&[me]));
        assert_eq!(rows.len(), 1);
        assert!(rows[0].start_time > 0);
        assert!(rows[0].session.is_some());
    }
}
