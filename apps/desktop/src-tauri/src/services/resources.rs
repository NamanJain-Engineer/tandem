//! The resource governor (F-EX-06): local agent panes in their own scope,
//! every live local pane's CPU and memory, the memory cap's OOM notice, and
//! Freeze/Thaw.
//!
//! The platform part is `tandem_pty::governor`; this service keeps what the
//! app knows about each pane (its scope, last CPU reading, frozen state and
//! chip) only while it runs.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// One pane's reading: CPU time (µs), memory, its cap, and its scope.
type Reading = (u64, Option<u64>, Option<u64>, Option<String>);

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::PaneId;
use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_pty::governor::{
    self, FrozenTree, GovernorError, GovernorStatus, ResourceGroups, TreeUsage,
};
use tandem_pty::suspend::ProcessControl;
use tandem_pty::PtyManager;
use tandem_store::repo::{panes, settings, workspaces};
use tandem_store::Store;

pub const NO_FREEZE: &str = "Freezing isn't available on this system yet.";
pub const NOT_FROZEN: &str = "This pane isn't frozen.";
pub const ALREADY_FROZEN: &str = "This pane is already frozen.";
pub const NO_PANE: &str = "This pane isn't running on this computer.";

/// `resources_list`: one live local pane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaneUsage {
    pub pane_id: String,
    pub pane_name: Option<String>,
    pub workspace_id: Option<String>,
    pub workspace_name: Option<String>,
    /// The worktree's branch when the pane is in one (grouping).
    pub lane_branch: Option<String>,
    /// Percent of one core; `None` on a pane's first reading.
    pub cpu_percent: Option<f64>,
    #[ts(type = "number | null")]
    pub memory_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub memory_cap_bytes: Option<u64>,
    /// The pane's scope, when it has one.
    pub scope: Option<String>,
    pub frozen: bool,
}

/// `resources:pane`: what a pane's header chip says. Neither flag nor note
/// clears it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaneResources {
    pub pane_id: String,
    pub frozen: bool,
    /// "512 MB": a process went over the pane's cap and was ended.
    pub capped_at: Option<String>,
    /// Why a cap that's set doesn't apply to this pane.
    pub not_capped: Option<String>,
}

impl PaneResources {
    fn clear(pane: &str) -> Self {
        Self {
            pane_id: pane.to_string(),
            frozen: false,
            capped_at: None,
            not_capped: None,
        }
    }
    fn is_clear(&self) -> bool {
        !self.frozen && self.capped_at.is_none() && self.not_capped.is_none()
    }
}

struct Placed {
    unit: String,
    pid: u32,
    cap: Option<u64>,
    oom_seen: u64,
}

enum Frozen {
    Scope(String),
    Signals(FrozenTree),
}

/// "512 MB", "1 GB".
pub fn cap_label(bytes: u64) -> String {
    let mb = bytes >> 20;
    if mb >= 1024 && mb.is_multiple_of(1024) {
        format!("{} GB", mb / 1024)
    } else {
        format!("{mb} MB")
    }
}

/// The `agent_memory_cap` setting in bytes (`"off"`: none).
pub fn cap_bytes(value: &serde_json::Value) -> Option<u64> {
    let mb: u64 = value.as_str()?.parse().ok()?;
    Some(mb << 20)
}

/// A pane's process tree, its root first.
pub type TreeReader = dyn Fn(u32) -> Vec<u32> + Send + Sync;

fn read_tree(pid: u32) -> Vec<u32> {
    governor::tree_usage(&[(String::new(), pid)])
        .remove("")
        .map(|t| t.pids)
        .unwrap_or_default()
}

pub struct ResourcesService {
    store: Store,
    bus: EventBus,
    groups: tokio::sync::OnceCell<Arc<dyn ResourceGroups>>,
    control: Arc<dyn ProcessControl>,
    tree: Arc<TreeReader>,
    /// This launch's tag in unit names (a previous launch's scopes differ).
    launch: String,
    /// Where process groups are read (Linux; a different root in tests).
    proc_root: PathBuf,
    next: AtomicU64,
    placed: Mutex<HashMap<String, Placed>>,
    frozen: Mutex<HashMap<String, Frozen>>,
    /// The last CPU reading per pane: when, and microseconds used.
    samples: Mutex<HashMap<String, (Instant, u64)>>,
    notes: Mutex<HashMap<String, PaneResources>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl ResourcesService {
    pub fn new(store: Store, bus: EventBus) -> Arc<Self> {
        Arc::new(Self::build(
            store,
            bus,
            Arc::new(tandem_pty::suspend::SignalControl),
        ))
    }

    fn build(store: Store, bus: EventBus, control: Arc<dyn ProcessControl>) -> Self {
        Self {
            store,
            bus,
            groups: tokio::sync::OnceCell::new(),
            control,
            tree: Arc::new(read_tree),
            launch: governor::launch_tag(),
            proc_root: PathBuf::from("/proc"),
            next: AtomicU64::new(1),
            placed: Mutex::default(),
            frozen: Mutex::default(),
            samples: Mutex::default(),
            notes: Mutex::default(),
        }
    }

    /// With fixed groups and signals (tests).
    pub fn with_parts(
        store: Store,
        bus: EventBus,
        groups: Arc<dyn ResourceGroups>,
        control: Arc<dyn ProcessControl>,
    ) -> Arc<Self> {
        Arc::new(Self::fixed(
            store,
            bus,
            groups,
            control,
            Arc::new(read_tree),
        ))
    }

    /// With fixed groups, signals and process trees (tests).
    pub fn fixed(
        store: Store,
        bus: EventBus,
        groups: Arc<dyn ResourceGroups>,
        control: Arc<dyn ProcessControl>,
        tree: Arc<TreeReader>,
    ) -> Self {
        let mut svc = Self::build(store, bus, control);
        svc.tree = tree;
        let _ = svc.groups.set(groups);
        svc
    }

    /// No scopes and no freezing: what tests of other services get, so
    /// they never make real scopes.
    pub fn inert(store: Store, bus: EventBus) -> Arc<Self> {
        Self::with_parts(
            store,
            bus,
            Arc::new(governor::Unplaced {
                reason: governor::NOT_YET.to_string(),
                freezing: false,
            }),
            Arc::new(tandem_pty::suspend::SignalControl),
        )
    }

    /// The platform's groups, detected once (it asks the user manager).
    async fn groups(&self) -> Arc<dyn ResourceGroups> {
        self.groups
            .get_or_init(|| async {
                let found: Arc<dyn ResourceGroups> =
                    match tokio::task::spawn_blocking(governor::platform).await {
                        Ok(groups) => Arc::from(groups),
                        Err(_) => Arc::new(governor::Unplaced {
                            reason: governor::NO_SESSION.to_string(),
                            freezing: false,
                        }),
                    };
                found
            })
            .await
            .clone()
    }

    pub async fn status(&self) -> GovernorStatus {
        self.groups().await.status()
    }

    fn note(&self, note: PaneResources) {
        {
            let mut notes = lock(&self.notes);
            if note.is_clear() {
                if notes.remove(&note.pane_id).is_none() {
                    return;
                }
            } else {
                notes.insert(note.pane_id.clone(), note.clone());
            }
        }
        self.bus
            .publish(BusEvent::new(Topic::Status, "resources:pane", note));
    }

    fn current(&self, pane: &str) -> PaneResources {
        lock(&self.notes)
            .get(pane)
            .cloned()
            .unwrap_or_else(|| PaneResources::clear(pane))
    }

    /// `resources_notes`.
    pub fn notes(&self) -> Vec<PaneResources> {
        let mut out: Vec<_> = lock(&self.notes).values().cloned().collect();
        out.sort_by(|a, b| a.pane_id.cmp(&b.pane_id));
        out
    }

    /// A local agent pane just started as `pid`: its own scope, under the
    /// cap when one is set. A pane whose scope can't be made runs anyway
    /// (D-2545); with a cap set, its chip says why.
    pub async fn place(&self, pane: &str, pid: u32) {
        let cap = settings::get(&self.store, SettingKey::AgentMemoryCap)
            .await
            .ok()
            .and_then(|v| cap_bytes(&v));
        let groups = self.groups().await;
        let status = groups.status();
        if !status.grouping {
            if cap.is_some() {
                self.note(PaneResources {
                    not_capped: status.reason,
                    ..PaneResources::clear(pane)
                });
            }
            return;
        }
        let unit = governor::unit_name(
            pane,
            &self.launch,
            self.next.fetch_add(1, Ordering::Relaxed),
        );
        let (u, p, tree) = (unit.clone(), pane.to_string(), self.tree.clone());
        // The whole tree goes in, and any child that started while the scope
        // was made follows it.
        let placed = tokio::task::spawn_blocking(move || {
            let mut pids = tree(pid);
            if pids.first() != Some(&pid) {
                pids.retain(|&p| p != pid);
                pids.insert(0, pid);
            }
            groups.place(&u, &p, &pids, cap)?;
            let late: Vec<u32> = tree(pid)
                .into_iter()
                .filter(|p| !pids.contains(p))
                .collect();
            if let Err(e) = groups.attach(&u, &late) {
                tracing::warn!(code = "resources.attach_failed", error = %e, "late children stay outside the scope");
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|e| Err(GovernorError::Failed(e.to_string())));
        match placed {
            Ok(()) => {
                tracing::info!(
                    code = "resources.placed",
                    capped = cap.is_some(),
                    "pane scope"
                );
                lock(&self.placed).insert(
                    pane.to_string(),
                    Placed {
                        unit,
                        pid,
                        cap,
                        oom_seen: 0,
                    },
                );
            }
            Err(e) => {
                tracing::warn!(code = "resources.place_failed", error = %e, "pane runs unplaced");
                if cap.is_some() {
                    self.note(PaneResources {
                        not_capped: Some(e.to_string()),
                        ..PaneResources::clear(pane)
                    });
                }
            }
        }
    }

    /// The pane is gone, or starts again: thaw it if it's frozen, and drop
    /// its readings and chip. Its scope, if it had one, is returned for
    /// [`Self::stop_scope`] once its processes are killed.
    pub async fn forget(&self, pane: &str) -> Option<String> {
        let frozen = lock(&self.frozen).remove(pane);
        if let Some(f) = frozen {
            if let Err(e) = self.thaw_with(&f).await {
                // Stopping the scope ends it frozen or not.
                tracing::warn!(code = "resources.thaw_failed", error = %e, "closing a frozen pane");
            }
        }
        let unit = lock(&self.placed).remove(pane).map(|p| p.unit);
        lock(&self.samples).remove(pane);
        self.note(PaneResources::clear(pane));
        unit
    }

    /// Ends a closed pane's scope and anything that outlived its kill.
    pub async fn stop_scope(&self, unit: String) {
        let groups = self.groups().await;
        let stopped = tokio::task::spawn_blocking(move || groups.stop(&unit))
            .await
            .unwrap_or_else(|e| Err(GovernorError::Failed(e.to_string())));
        if let Err(e) = stopped {
            tracing::debug!(code = "resources.stop_failed", error = %e, "scope already gone");
        }
    }

    /// At start: scopes an earlier launch left (it quit while a pane was
    /// frozen) are thawed, so their processes see the hangup and end.
    pub async fn thaw_leftovers(&self) {
        let groups = self.groups().await;
        let tag = format!("-{}-", self.launch);
        let _ = tokio::task::spawn_blocking(move || {
            for unit in groups.leftovers() {
                if unit.contains(&tag) {
                    continue;
                }
                if groups.thaw(&unit).is_ok() {
                    tracing::info!(
                        code = "resources.leftover_thawed",
                        "an earlier launch's scope"
                    );
                }
            }
        })
        .await;
    }

    async fn thaw_with(&self, frozen: &Frozen) -> Result<(), String> {
        match frozen {
            Frozen::Scope(unit) => {
                let (groups, unit) = (self.groups().await, unit.clone());
                tokio::task::spawn_blocking(move || groups.thaw(&unit))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())
            }
            Frozen::Signals(tree) => {
                let (control, tree) = (self.control.clone(), tree.clone());
                tokio::task::spawn_blocking(move || governor::thaw_tree(control.as_ref(), &tree))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())
            }
        }
    }

    /// Live panes in local workspaces, with their root pid.
    async fn local_roots(
        &self,
        pty: &PtyManager,
    ) -> Vec<(String, u32, Option<workspaces::Workspace>, Option<String>)> {
        let mut out = Vec::new();
        for pane in pty.panes() {
            if !pty.is_live(&pane) {
                continue;
            }
            let Some(pid) = pty.pid(&pane) else {
                continue;
            };
            let record = panes::get(&self.store, &PaneId::from_db(pane.clone()))
                .await
                .ok();
            let ws = match &record {
                Some(r) => workspaces::get(&self.store, &r.workspace_id).await.ok(),
                None => None,
            };
            if ws.as_ref().is_some_and(|w| w.kind != "local") {
                continue;
            }
            out.push((pane, pid, ws, record.and_then(|r| r.human_name)));
        }
        out
    }

    /// `resources_list`.
    pub async fn list(&self, pty: &PtyManager) -> Vec<PaneUsage> {
        let roots = self.local_roots(pty).await;
        if roots.is_empty() {
            return Vec::new();
        }
        let groups = self.groups().await;
        let scoped: HashMap<String, (String, u32)> = lock(&self.placed)
            .iter()
            .map(|(p, s)| (p.clone(), (s.unit.clone(), s.pid)))
            .collect();
        let tree_roots: Vec<(String, u32)> =
            roots.iter().map(|(p, pid, ..)| (p.clone(), *pid)).collect();
        let readings = tokio::task::spawn_blocking(move || {
            let trees = governor::tree_usage(&tree_roots);
            let mut out: HashMap<String, Reading> = HashMap::new();
            for (pane, _) in &tree_roots {
                let from_scope = scoped
                    .get(pane)
                    .and_then(|(unit, pid)| groups.usage(unit, *pid).map(|u| (unit.clone(), u)));
                let tree = trees.get(pane).cloned().unwrap_or(TreeUsage::default());
                let reading = match from_scope {
                    Some((unit, u)) => (
                        u.cpu_usec,
                        u.memory.or(Some(tree.rss)),
                        u.memory_max,
                        Some(unit),
                    ),
                    None => (tree.cpu_ms * 1000, Some(tree.rss), None, None),
                };
                out.insert(pane.clone(), reading);
            }
            out
        })
        .await
        .unwrap_or_default();
        let now = Instant::now();
        let frozen = lock(&self.frozen).keys().cloned().collect::<Vec<_>>();
        let mut samples = lock(&self.samples);
        let mut out = Vec::new();
        for (pane, _, ws, name) in roots {
            let Some((cpu_usec, memory, cap, scope)) = readings.get(&pane).cloned() else {
                continue;
            };
            let cpu_percent = samples.get(&pane).and_then(|(at, used)| {
                let wall = now.duration_since(*at).as_micros() as f64;
                (wall > 0.0).then(|| cpu_usec.saturating_sub(*used) as f64 / wall * 100.0)
            });
            samples.insert(pane.clone(), (now, cpu_usec));
            out.push(PaneUsage {
                frozen: frozen.contains(&pane),
                pane_id: pane,
                pane_name: name,
                workspace_id: ws.as_ref().map(|w| w.id.as_str().to_string()),
                lane_branch: ws.as_ref().and_then(|w| w.lane_branch.clone()),
                workspace_name: ws.map(|w| w.name),
                cpu_percent: cpu_percent.map(|c| (c * 10.0).round() / 10.0),
                memory_bytes: memory,
                memory_cap_bytes: cap,
                scope,
            });
        }
        out.sort_by(|a, b| {
            a.lane_branch
                .cmp(&b.lane_branch)
                .then(a.workspace_name.cmp(&b.workspace_name))
                .then(a.pane_id.cmp(&b.pane_id))
        });
        out
    }

    /// `resources_freeze`: the scope's freezer, else signals (Linux).
    pub async fn freeze(&self, pty: &PtyManager, pane: &str) -> Result<(), IpcError> {
        if lock(&self.frozen).contains_key(pane) {
            return Err(IpcError::new(ErrorCode::Conflict, ALREADY_FROZEN).with_stage("resources"));
        }
        let pid = pty
            .pid(pane)
            .filter(|_| pty.is_live(pane))
            .ok_or_else(|| IpcError::new(ErrorCode::NotFound, NO_PANE).with_stage("resources"))?;
        let groups = self.groups().await;
        if !groups.status().freezing {
            return Err(IpcError::new(ErrorCode::Unavailable, NO_FREEZE).with_stage("resources"));
        }
        let unit = lock(&self.placed).get(pane).map(|p| p.unit.clone());
        if let Some(unit) = unit {
            let (g, u) = (groups.clone(), unit.clone());
            let done = tokio::task::spawn_blocking(move || g.freeze(&u))
                .await
                .unwrap_or_else(|e| Err(GovernorError::Failed(e.to_string())));
            match done {
                Ok(()) => return self.frozen_as(pane, Frozen::Scope(unit)),
                Err(e) => {
                    tracing::warn!(code = "resources.freezer", error = %e, "freezing by signals")
                }
            }
        }
        let foreground = pty.foreground_group(pane);
        let (control, root) = (self.control.clone(), self.proc_root.clone());
        let pane_id = pane.to_string();
        let tree = tokio::task::spawn_blocking(move || {
            let pids = governor::tree_usage(&[(pane_id.clone(), pid)])
                .remove(&pane_id)
                .map(|t| t.pids)
                .unwrap_or_default();
            let groups = process_groups(&root, &pids);
            governor::freeze_tree(control.as_ref(), pid as i32, foreground, &groups)
        })
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?
        .map_err(|e| {
            IpcError::new(ErrorCode::Unavailable, e.to_string()).with_stage("resources")
        })?;
        self.frozen_as(pane, Frozen::Signals(tree))
    }

    fn frozen_as(&self, pane: &str, how: Frozen) -> Result<(), IpcError> {
        lock(&self.frozen).insert(pane.to_string(), how);
        tracing::info!(code = "resources.frozen", "pane frozen");
        self.note(PaneResources {
            frozen: true,
            ..self.current(pane)
        });
        Ok(())
    }

    /// `resources_thaw`. When thawing fails the pane stays frozen, and says
    /// so.
    pub async fn thaw(&self, pane: &str) -> Result<(), IpcError> {
        let frozen = lock(&self.frozen).remove(pane).ok_or_else(|| {
            IpcError::new(ErrorCode::Conflict, NOT_FROZEN).with_stage("resources")
        })?;
        if let Err(e) = self.thaw_with(&frozen).await {
            lock(&self.frozen).insert(pane.to_string(), frozen);
            return Err(IpcError::new(
                ErrorCode::Unavailable,
                format!("Couldn't thaw this pane: {e}"),
            )
            .with_stage("resources"));
        }
        tracing::info!(code = "resources.thawed", "pane thawed");
        self.note(PaneResources {
            frozen: false,
            ..self.current(pane)
        });
        Ok(())
    }

    /// Every 2 s: a capped scope whose `oom_kill` count rose had a process
    /// ended at its cap; the pane says so.
    pub async fn watch(&self, pty: &PtyManager) {
        let capped: Vec<(String, String, u32, u64, u64)> = lock(&self.placed)
            .iter()
            .filter_map(|(p, s)| {
                s.cap
                    .map(|cap| (p.clone(), s.unit.clone(), s.pid, cap, s.oom_seen))
            })
            .collect();
        if capped.is_empty() {
            return;
        }
        let groups = self.groups().await;
        let counts = tokio::task::spawn_blocking(move || {
            capped
                .into_iter()
                .filter_map(|(pane, unit, pid, cap, seen)| {
                    let kills = groups.usage(&unit, pid)?.oom_kills;
                    Some((pane, cap, seen, kills))
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        for (pane, cap, seen, kills) in counts {
            if kills <= seen {
                continue;
            }
            if let Some(p) = lock(&self.placed).get_mut(&pane) {
                p.oom_seen = kills;
            }
            let label = cap_label(cap);
            tracing::info!(
                code = "resources.oom",
                "a process went over its pane's memory cap"
            );
            let _ = pty.notice(&pane, &format!("Stopped: memory cap {label}"));
            self.note(PaneResources {
                capped_at: Some(label),
                ..self.current(&pane)
            });
        }
    }
}

/// The process groups of `pids` (Linux reads `/proc`).
fn process_groups(proc_root: &std::path::Path, pids: &[u32]) -> Vec<i32> {
    #[cfg(target_os = "linux")]
    {
        governor::linux::pgids(proc_root, pids)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (proc_root, pids);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_pty::governor::Usage;
    use tandem_pty::suspend::SuspendError;

    /// A `place` call: the pane, its PIDs and its cap.
    type Placed = (String, Vec<u32>, Option<u64>);

    #[derive(Default)]
    struct FakeGroups {
        grouping: bool,
        capping: bool,
        fail_place: bool,
        fail_thaw: bool,
        placed: Mutex<Vec<Placed>>,
        oom: Mutex<u64>,
        /// freeze, thaw, attach and stop calls, in order.
        frozen: Mutex<Vec<String>>,
        leftovers: Vec<String>,
    }

    impl ResourceGroups for FakeGroups {
        fn status(&self) -> GovernorStatus {
            GovernorStatus {
                grouping: self.grouping,
                capping: self.capping,
                freezing: true,
                reason: (!self.grouping).then(|| governor::NO_SESSION.to_string()),
            }
        }
        fn place(
            &self,
            unit: &str,
            _: &str,
            pids: &[u32],
            max: Option<u64>,
        ) -> Result<(), GovernorError> {
            if self.fail_place {
                return Err(GovernorError::Failed("Unit already exists".into()));
            }
            self.placed
                .lock()
                .expect("lock")
                .push((unit.to_string(), pids.to_vec(), max));
            Ok(())
        }
        fn usage(&self, _: &str, _: u32) -> Option<Usage> {
            Some(Usage {
                cpu_usec: 1_000,
                memory: Some(300 << 20),
                memory_max: Some(512 << 20),
                oom_kills: *self.oom.lock().expect("lock"),
            })
        }
        fn freeze(&self, unit: &str) -> Result<(), GovernorError> {
            self.frozen
                .lock()
                .expect("lock")
                .push(format!("freeze {unit}"));
            Ok(())
        }
        fn thaw(&self, unit: &str) -> Result<(), GovernorError> {
            self.frozen
                .lock()
                .expect("lock")
                .push(format!("thaw {unit}"));
            if self.fail_thaw {
                return Err(GovernorError::Failed("Unit is busy".into()));
            }
            Ok(())
        }
        fn attach(&self, unit: &str, pids: &[u32]) -> Result<(), GovernorError> {
            if !pids.is_empty() {
                self.frozen
                    .lock()
                    .expect("lock")
                    .push(format!("attach {unit} {pids:?}"));
            }
            Ok(())
        }
        fn stop(&self, unit: &str) -> Result<(), GovernorError> {
            self.frozen
                .lock()
                .expect("lock")
                .push(format!("stop {unit}"));
            Ok(())
        }
        fn leftovers(&self) -> Vec<String> {
            self.leftovers.clone()
        }
    }

    /// A process tree that grows by one child on every read after the first.
    fn growing_tree() -> Arc<TreeReader> {
        let reads = Arc::new(std::sync::atomic::AtomicU32::new(0));
        Arc::new(move |pid| {
            let n = reads.fetch_add(1, Ordering::SeqCst);
            let mut pids = vec![pid, pid + 1];
            pids.extend((0..n).map(|i| pid + 2 + i));
            pids
        })
    }

    #[derive(Default)]
    struct Signals(Mutex<Vec<String>>);
    impl ProcessControl for Signals {
        fn stop(&self, g: i32) -> Result<(), SuspendError> {
            self.0.lock().expect("lock").push(format!("stop {g}"));
            Ok(())
        }
        fn cont(&self, g: i32) -> Result<(), SuspendError> {
            self.0.lock().expect("lock").push(format!("cont {g}"));
            Ok(())
        }
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, store)
    }

    #[test]
    fn f_ex_06_cap_labels() {
        assert_eq!(cap_label(512 << 20), "512 MB");
        assert_eq!(cap_label(1024 << 20), "1 GB");
        assert_eq!(cap_label(16384 << 20), "16 GB");
        assert_eq!(cap_bytes(&serde_json::json!("512")), Some(512 << 20));
        assert_eq!(cap_bytes(&serde_json::json!("off")), None);
    }

    #[tokio::test]
    async fn f_ex_06_1_an_agent_pane_gets_its_scope_under_the_cap() {
        let (_d, store) = store();
        settings::set(&store, SettingKey::AgentMemoryCap, serde_json::json!("512"))
            .await
            .expect("set");
        let groups = Arc::new(FakeGroups {
            grouping: true,
            capping: true,
            ..Default::default()
        });
        let svc = ResourcesService::fixed(
            store,
            EventBus::new(),
            groups.clone(),
            Arc::new(Signals::default()),
            Arc::new(|pid| vec![pid]),
        );
        svc.place("p1", 4242).await;
        svc.place("p2", 4343).await;
        let placed = groups.placed.lock().expect("lock").clone();
        let tag = &svc.launch;
        assert_eq!(
            placed,
            vec![
                (
                    format!("vibezeo-pane-p1-{tag}-1.scope"),
                    vec![4242],
                    Some(512 << 20)
                ),
                (
                    format!("vibezeo-pane-p2-{tag}-2.scope"),
                    vec![4343],
                    Some(512 << 20)
                ),
            ]
        );
        assert!(svc.notes().is_empty());
    }

    #[tokio::test]
    async fn f_ex_06_6_a_pane_whose_scope_fails_runs_and_says_its_not_capped() {
        let (_d, store) = store();
        settings::set(
            &store,
            SettingKey::AgentMemoryCap,
            serde_json::json!("1024"),
        )
        .await
        .expect("set");
        let bus = EventBus::new();
        let mut events = bus.subscribe(Topic::Status);
        let svc = ResourcesService::with_parts(
            store,
            bus,
            Arc::new(FakeGroups {
                grouping: true,
                capping: true,
                fail_place: true,
                ..Default::default()
            }),
            Arc::new(Signals::default()),
        );
        svc.place("p1", 1).await;
        let notes = svc.notes();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].not_capped.as_deref(), Some("Unit already exists"));
        let ev = events.try_recv().expect("event");
        assert_eq!(ev.name, "resources:pane");
        // Closing it clears the chip.
        svc.forget("p1").await;
        assert!(svc.notes().is_empty());
    }

    #[tokio::test]
    async fn f_ex_06_2_without_scopes_a_set_cap_says_why_and_no_cap_says_nothing() {
        let (_d, store) = store();
        let unplaced = || -> Arc<dyn ResourceGroups> { Arc::new(FakeGroups::default()) };
        let svc = ResourcesService::with_parts(
            store.clone(),
            EventBus::new(),
            unplaced(),
            Arc::new(Signals::default()),
        );
        svc.place("p1", 1).await;
        assert!(svc.notes().is_empty());
        settings::set(&store, SettingKey::AgentMemoryCap, serde_json::json!("512"))
            .await
            .expect("set");
        svc.place("p1", 1).await;
        assert_eq!(
            svc.notes()[0].not_capped.as_deref(),
            Some(governor::NO_SESSION)
        );
    }

    #[tokio::test]
    async fn f_ex_06_3_an_oom_kill_in_a_capped_scope_marks_the_pane_once() {
        let (_d, store) = store();
        settings::set(&store, SettingKey::AgentMemoryCap, serde_json::json!("512"))
            .await
            .expect("set");
        let groups = Arc::new(FakeGroups {
            grouping: true,
            capping: true,
            ..Default::default()
        });
        let svc = ResourcesService::with_parts(
            store,
            EventBus::new(),
            groups.clone(),
            Arc::new(Signals::default()),
        );
        let pty = PtyManager::new();
        svc.place("p1", 1).await;
        svc.watch(&pty).await;
        assert!(svc.notes().is_empty());
        *groups.oom.lock().expect("lock") = 1;
        svc.watch(&pty).await;
        svc.watch(&pty).await;
        let notes = svc.notes();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].capped_at.as_deref(), Some("512 MB"));
    }

    #[tokio::test]
    async fn f_ex_06_4_freeze_needs_a_live_pane_and_thaw_a_frozen_one() {
        let (_d, store) = store();
        let svc = ResourcesService::with_parts(
            store,
            EventBus::new(),
            Arc::new(FakeGroups::default()),
            Arc::new(Signals::default()),
        );
        let pty = PtyManager::new();
        let err = svc.freeze(&pty, "nope").await.expect_err("no pane");
        assert_eq!(err.code, ErrorCode::NotFound);
        let err = svc.thaw("nope").await.expect_err("not frozen");
        assert_eq!(err.code, ErrorCode::Conflict);
    }

    #[tokio::test]
    async fn f_ex_06_1_the_whole_tree_goes_in_and_a_late_child_follows() {
        let (_d, store) = store();
        let groups = Arc::new(FakeGroups {
            grouping: true,
            capping: true,
            ..Default::default()
        });
        let svc = ResourcesService::fixed(
            store,
            EventBus::new(),
            groups.clone(),
            Arc::new(Signals::default()),
            growing_tree(),
        );
        svc.place("p1", 100).await;
        let unit = format!("vibezeo-pane-p1-{}-1.scope", svc.launch);
        // The shell had a child before the scope was made...
        assert_eq!(
            groups.placed.lock().expect("lock")[0],
            (unit.clone(), vec![100, 101], None)
        );
        // ...and another started while it was being made.
        assert_eq!(
            *groups.frozen.lock().expect("lock"),
            [format!("attach {unit} [102]")]
        );
    }

    #[tokio::test]
    async fn f_ex_06_4_a_thaw_that_fails_says_so_and_the_pane_stays_frozen() {
        let (_d, store) = store();
        let groups = Arc::new(FakeGroups {
            grouping: true,
            fail_thaw: true,
            ..Default::default()
        });
        let svc = ResourcesService::with_parts(
            store,
            EventBus::new(),
            groups.clone(),
            Arc::new(Signals::default()),
        );
        svc.frozen_as("p1", Frozen::Scope("u.scope".into()))
            .expect("frozen");
        let err = svc.thaw("p1").await.expect_err("thaw fails");
        assert_eq!(err.code, ErrorCode::Unavailable);
        assert!(err.message.contains("Unit is busy"), "{}", err.message);
        assert!(lock(&svc.frozen).contains_key("p1"));
        assert!(svc.notes()[0].frozen);
    }

    #[tokio::test]
    async fn f_ex_06_5_closing_thaws_first_and_hands_back_the_scope_to_stop() {
        let (_d, store) = store();
        let groups = Arc::new(FakeGroups {
            grouping: true,
            ..Default::default()
        });
        let svc = ResourcesService::fixed(
            store,
            EventBus::new(),
            groups.clone(),
            Arc::new(Signals::default()),
            Arc::new(|pid| vec![pid]),
        );
        svc.place("p1", 7).await;
        let unit = format!("vibezeo-pane-p1-{}-1.scope", svc.launch);
        svc.frozen_as("p1", Frozen::Scope(unit.clone()))
            .expect("frozen");
        let back = svc.forget("p1").await;
        assert_eq!(back.as_deref(), Some(unit.as_str()));
        svc.stop_scope(unit.clone()).await;
        assert_eq!(
            *groups.frozen.lock().expect("lock"),
            [format!("thaw {unit}"), format!("stop {unit}")]
        );
        assert!(svc.notes().is_empty());
        assert_eq!(svc.forget("p1").await, None);
    }

    #[tokio::test]
    async fn f_ex_06_4_an_earlier_launchs_scopes_are_thawed_at_start() {
        let (_d, store) = store();
        let mut svc = ResourcesService::build(store, EventBus::new(), Arc::new(Signals::default()));
        svc.launch = "cafe0001".into();
        let groups = Arc::new(FakeGroups {
            grouping: true,
            leftovers: vec![
                "vibezeo-pane-p1-0ld00000-3.scope".into(),
                "vibezeo-pane-p2-cafe0001-1.scope".into(),
            ],
            ..Default::default()
        });
        let _ = svc.groups.set(groups.clone());
        svc.thaw_leftovers().await;
        assert_eq!(
            *groups.frozen.lock().expect("lock"),
            ["thaw vibezeo-pane-p1-0ld00000-3.scope"]
        );
    }
}
