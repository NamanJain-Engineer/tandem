//! Orphaned children from a crashed run (F-CD-03 AC8, ARCHITECTURE §5.4).
//!
//! Every spawn is recorded in `$DATA/run/children.json` with its start time
//! and removed when it exits. On the next launch, records whose process is
//! still alive with the same start time (so a reused PID never matches) are
//! orphans. On Unix the record's PID is also the session id of everything the
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

/// The children of this run, persisted on every change.
pub struct ChildRegistry {
    path: PathBuf,
    entries: Mutex<Vec<ChildRecord>>,
    table: Mutex<Box<dyn ProcessTable>>,
}

impl ChildRegistry {
    /// Starts an empty registry at `path` (scan the old file first).
    pub fn new(path: PathBuf, table: Box<dyn ProcessTable>) -> Self {
        let reg = Self {
            path,
            entries: Mutex::new(Vec::new()),
            table: Mutex::new(table),
        };
        reg.save();
        reg
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
        let body = self
            .entries
            .lock()
            .ok()
            .and_then(|e| serde_json::to_vec_pretty(&*e).ok())
            .unwrap_or_else(|| b"[]".to_vec());
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(err) = tandem_core::atomic::atomic_write(&self.path, &body, Some(0o600)) {
            tracing::warn!(code = "pty.children_write_failed", error = %err, "couldn't save the child registry");
        }
    }
}

pub fn read_records(path: &Path) -> Vec<ChildRecord> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
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
        let path = dir.path().join("run/children.json");
        let reg = ChildRegistry::new(
            path.clone(),
            Box::new(FakeTable::with(&[(42, Some(42), 7)])),
        );
        assert!(read_records(&path).is_empty());
        reg.add("pn_1", 42, "zsh", "/code", 5);
        let saved = read_records(&path);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].start_time, 7);
        reg.remove("pn_1");
        assert!(read_records(&path).is_empty());
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
