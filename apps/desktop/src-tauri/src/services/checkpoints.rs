//! Checkpoints and rewind (F-EX-01, ARCHITECTURE §7, D-701). Before each
//! agent turn the folder's files go into its private store
//! (`tandem_git::checkpoint`); the timeline shows them against now and puts
//! files back. One queue per folder keeps snapshots and restores in order.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::clock::Clock;
use tandem_core::ids::{new_id, WorkspaceId};
use tandem_core::settings::SettingKey;
use tandem_core::{DataDir, ErrorCode, IpcError};
use tandem_git::checkpoint::{self, RestoreFailure, Side, Snapshot, Store as CpStore, StoreKind};
use tandem_git::FileDiff;
use tandem_store::repo::checkpoints::{self as rows, Checkpoint, CheckpointReason};
use tandem_store::repo::{settings, workspaces};
use tandem_store::Store;

use crate::services::lanes::git_for;
use crate::services::terminal::TerminalService;

/// A restore's own checkpoint survives pruning this long.
const PROTECT_RESTORE_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;

pub const GIT_MISSING: &str = "Checkpoints need git. Install git to turn them on.";
/// Checkpoints save and restore folders on this computer. A folder on an SSH
/// host isn't one, even when a folder here has the same path (D-833).
pub const ON_HOST: &str = "Checkpoints aren't available in folders on SSH hosts yet.";

/// `checkpoints_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CheckpointTimeline {
    pub folder: String,
    pub store: StoreKind,
    pub enabled: bool,
    pub items: Vec<Checkpoint>,
}

/// `checkpoint_restore`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RestoreReport {
    pub restored: Vec<String>,
    pub failed: Vec<RestoreFailure>,
    /// The checkpoint of how things were just before the restore.
    pub undo_id: Option<String>,
}

/// `checkpoints_usage`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CheckpointUsage {
    #[ts(type = "number")]
    pub bytes: u64,
    pub folders: u32,
}

/// What a checkpoint is taken for.
#[derive(Debug, Clone, Default)]
pub struct Request {
    pub folder: PathBuf,
    pub workspace_id: Option<String>,
    pub thread_id: Option<String>,
    pub pane_id: Option<String>,
    pub turn_id: Option<String>,
    pub label: String,
    /// Set when the prompt went ahead before this checkpoint was written.
    pub late: Arc<std::sync::atomic::AtomicBool>,
}

pub struct CheckpointService {
    data: DataDir,
    store: Store,
    bus: EventBus,
    clock: Arc<dyn Clock>,
    terminals: Arc<TerminalService>,
    queues: Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
}

fn io(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Io, e.to_string()).with_stage("checkpoints")
}

impl CheckpointService {
    pub fn new(
        data: DataDir,
        store: Store,
        bus: EventBus,
        clock: Arc<dyn Clock>,
        terminals: Arc<TerminalService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            data,
            store,
            bus,
            clock,
            terminals,
            queues: Mutex::default(),
        })
    }

    fn root(&self) -> PathBuf {
        self.data.checkpoints()
    }

    fn queue(&self, folder: &Path) -> Arc<tokio::sync::Mutex<()>> {
        let mut q = self.queues.lock().unwrap_or_else(|e| e.into_inner());
        q.entry(folder.to_path_buf()).or_default().clone()
    }

    async fn setting_i64(&self, key: SettingKey, fallback: i64) -> i64 {
        settings::get(&self.store, key)
            .await
            .ok()
            .and_then(|v| v.as_i64())
            .unwrap_or(fallback)
    }

    pub async fn enabled(&self) -> bool {
        settings::get(&self.store, SettingKey::CheckpointsEnabled)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    }

    async fn open(&self, folder: &Path) -> Result<CpStore, IpcError> {
        let login = self.terminals.login_env().await;
        let git = git_for(&self.data, &login).map_err(|_| {
            IpcError::new(ErrorCode::Unavailable, GIT_MISSING).with_stage("checkpoints")
        })?;
        let root = self.root();
        let folder = folder.to_path_buf();
        tokio::task::spawn_blocking(move || CpStore::open(git, &root, &folder))
            .await
            .map_err(io)?
            .map_err(IpcError::from)
    }

    async fn max_bytes(&self) -> u64 {
        let mb = self.setting_i64(SettingKey::CheckpointsMaxFileMb, 5).await;
        u64::try_from(mb.max(1)).unwrap_or(5) * 1024 * 1024
    }

    /// Whether `workspace_id` names a folder on an SSH host.
    async fn on_host(&self, workspace_id: Option<&str>) -> bool {
        let Some(id) = workspace_id else {
            return false;
        };
        workspaces::get(&self.store, &WorkspaceId::from_db(id.to_string()))
            .await
            .is_ok_and(|ws| ws.kind == "ssh")
    }

    /// Takes a checkpoint unless checkpoints are off, the folder is on an SSH
    /// host, or nothing changed since the folder's last one. Errors are
    /// logged by code, never with paths or contents, and never stop the turn.
    pub async fn before_turn(&self, req: Request, reason: CheckpointReason) -> Option<Checkpoint> {
        if !self.enabled().await
            || self.on_host(req.workspace_id.as_deref()).await
            || !req.folder.is_dir()
        {
            return None;
        }
        match self.take(req, reason).await {
            Ok(c) => c,
            Err(err) => {
                tracing::warn!(code = "checkpoint.failed", error = %err.message, "checkpoint skipped");
                None
            }
        }
    }

    /// Saves `req.folder` as a new checkpoint; `None` when it equals the
    /// newest one.
    pub async fn take(
        &self,
        req: Request,
        reason: CheckpointReason,
    ) -> Result<Option<Checkpoint>, IpcError> {
        if self.on_host(req.workspace_id.as_deref()).await {
            return Err(IpcError::new(ErrorCode::Unavailable, ON_HOST).with_stage("checkpoints"));
        }
        let folder = std::fs::canonicalize(&req.folder).map_err(|_| {
            IpcError::new(ErrorCode::NotFound, "The folder is missing").with_stage("checkpoints")
        })?;
        let queue = self.queue(&folder);
        let _turn = queue.lock().await;
        let cp = self.open(&folder).await?;
        let max = self.max_bytes().await;
        let (snap, _) = cp.snapshot(max).await?;
        let made = self.record(&cp, &folder, &snap, req, reason).await?;
        drop(_turn);
        if made.is_some() {
            self.prune(&folder).await;
        }
        Ok(made)
    }

    async fn record(
        &self,
        cp: &CpStore,
        folder: &Path,
        snap: &Snapshot,
        req: Request,
        reason: CheckpointReason,
    ) -> Result<Option<Checkpoint>, IpcError> {
        let key = folder.to_string_lossy().into_owned();
        if let Some(last) = rows::latest(&self.store, key.clone()).await? {
            if last.tree == snap.tree {
                return Ok(None);
            }
        }
        let id = new_id("cp");
        cp.pin(&id, &snap.tree)?;
        let row = rows::insert(
            &self.store,
            Checkpoint {
                id,
                folder: key.clone(),
                workspace_id: req.workspace_id,
                thread_id: req.thread_id,
                pane_id: req.pane_id,
                turn_id: req.turn_id,
                reason,
                label: req.label,
                tree: snap.tree.clone(),
                files: snap.files,
                skipped: snap.skipped.clone(),
                partial: snap.partial,
                late: req.late.load(std::sync::atomic::Ordering::SeqCst),
                created_at: self.clock.now_ms(),
                differs: 0,
            },
        )
        .await?;
        self.bus.publish(BusEvent::new(
            Topic::App,
            "checkpoint:created",
            serde_json::json!({ "folder": key, "checkpoint": row }),
        ));
        Ok(Some(row))
    }

    /// Drops what the settings no longer keep, and frees its space.
    pub async fn prune(&self, folder: &Path) {
        let keep = self.setting_i64(SettingKey::CheckpointsKeep, 100).await;
        let days = self.setting_i64(SettingKey::CheckpointsDays, 14).await;
        let now = self.clock.now_ms();
        let key = folder.to_string_lossy().into_owned();
        let Ok(ids) = rows::prunable(
            &self.store,
            key,
            u32::try_from(keep).unwrap_or(100),
            now - days * DAY_MS,
            now - PROTECT_RESTORE_MS,
        )
        .await
        else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        let queue = self.queue(folder);
        let _turn = queue.lock().await;
        if let Ok(cp) = self.open(folder).await {
            if cp.unpin(&ids).is_ok() {
                let _ = rows::delete(&self.store, ids).await;
                if let Err(err) = cp.gc().await {
                    tracing::warn!(code = "checkpoint.gc", error = %err, "checkpoint gc failed");
                }
            }
        }
    }

    /// Startup: every folder's old checkpoints go; a folder that's gone
    /// loses its checkpoints and its store.
    pub async fn prune_all(&self) {
        let Ok(folders) = rows::folders(&self.store).await else {
            return;
        };
        for folder in folders {
            let path = PathBuf::from(&folder);
            if path.is_dir() {
                self.prune(&path).await;
            }
        }
    }

    fn now_side(snap: &Snapshot) -> Side<'_> {
        Side {
            tree: &snap.tree,
            skipped: &snap.skipped,
            partial: snap.partial,
        }
    }

    /// `checkpoints_list`.
    pub async fn timeline(
        &self,
        folder: &Path,
        thread_id: Option<String>,
    ) -> Result<CheckpointTimeline, IpcError> {
        let folder = std::fs::canonicalize(folder).map_err(|_| {
            IpcError::new(ErrorCode::NotFound, "The folder is missing").with_stage("checkpoints")
        })?;
        let key = folder.to_string_lossy().into_owned();
        let enabled = self.enabled().await;
        let cp = match self.open(&folder).await {
            Ok(cp) => cp,
            Err(err) if err.code == ErrorCode::Unavailable => {
                return Ok(CheckpointTimeline {
                    folder: key,
                    store: StoreKind::NoGit,
                    enabled,
                    items: Vec::new(),
                })
            }
            Err(err) => return Err(err),
        };
        let mut items = rows::list(&self.store, key.clone(), thread_id).await?;
        let queue = self.queue(&folder);
        let _turn = queue.lock().await;
        let max = self.max_bytes().await;
        let (now, kind) = cp.snapshot(max).await?;
        for item in &mut items {
            item.differs = cp.count_changes(&item.tree, &now.tree).unwrap_or(0);
        }
        Ok(CheckpointTimeline {
            folder: key,
            store: kind,
            enabled,
            items,
        })
    }

    async fn row(&self, id: &str) -> Result<Checkpoint, IpcError> {
        rows::get(&self.store, id.to_string())
            .await?
            .ok_or_else(|| {
                IpcError::new(ErrorCode::NotFound, "That checkpoint is gone")
                    .with_stage("checkpoints")
            })
    }

    /// `checkpoint_diff`: the checkpoint → the folder now.
    pub async fn diff(&self, id: &str) -> Result<Vec<FileDiff>, IpcError> {
        let row = self.row(id).await?;
        let folder = PathBuf::from(&row.folder);
        let cp = self.open(&folder).await?;
        let queue = self.queue(&folder);
        let _turn = queue.lock().await;
        let (now, _) = cp.snapshot(self.max_bytes().await).await?;
        let tree = row.tree.clone();
        tokio::task::spawn_blocking(move || cp.diff(&tree, &now.tree))
            .await
            .map_err(io)?
            .map_err(IpcError::from)
    }

    /// `checkpoint_restore`: saves how things are now, then puts back
    /// `paths` (every differing file when `None`).
    pub async fn restore(
        &self,
        id: &str,
        paths: Option<Vec<String>>,
    ) -> Result<RestoreReport, IpcError> {
        let row = self.row(id).await?;
        let folder = PathBuf::from(&row.folder);
        let cp = self.open(&folder).await?;
        let queue = self.queue(&folder);
        let guard = queue.lock().await;
        let (now, _) = cp.snapshot(self.max_bytes().await).await?;
        let req = Request {
            folder: folder.clone(),
            workspace_id: row.workspace_id.clone(),
            thread_id: row.thread_id.clone(),
            pane_id: None,
            turn_id: None,
            label: row.label.clone(),
            late: Arc::default(),
        };
        let undo = match self
            .record(&cp, &folder, &now, req, CheckpointReason::Restore)
            .await?
        {
            Some(c) => Some(c.id),
            // Nothing changed since the newest checkpoint: it is "now".
            None => rows::latest(&self.store, row.folder.clone())
                .await?
                .map(|c| c.id),
        };
        let then_tree = row.tree.clone();
        let then_skipped = row.skipped.clone();
        let then_partial = row.partial;
        let restored = tokio::task::spawn_blocking(move || {
            cp.restore(
                Side {
                    tree: &then_tree,
                    skipped: &then_skipped,
                    partial: then_partial,
                },
                Self::now_side(&now),
                paths.as_deref(),
            )
        })
        .await
        .map_err(io)??;
        drop(guard);
        tracing::info!(
            code = "checkpoint.restored",
            files = restored.restored.len(),
            failed = restored.failed.len(),
            "checkpoint restored"
        );
        Ok(RestoreReport {
            restored: restored.restored,
            failed: restored.failed,
            undo_id: undo,
        })
    }

    /// `checkpoints_usage`.
    pub async fn usage(&self) -> CheckpointUsage {
        let root = self.root();
        tokio::task::spawn_blocking(move || {
            let mut folders = 0;
            if let Ok(entries) = std::fs::read_dir(&root) {
                folders = entries.flatten().filter(|e| e.path().is_dir()).count();
            }
            CheckpointUsage {
                bytes: checkpoint::disk_usage(&root),
                folders: u32::try_from(folders).unwrap_or(u32::MAX),
            }
        })
        .await
        .unwrap_or(CheckpointUsage {
            bytes: 0,
            folders: 0,
        })
    }

    /// `checkpoints_delete_all`.
    pub async fn delete_all(&self) -> Result<(), IpcError> {
        rows::delete_all(&self.store).await?;
        let root = self.root();
        tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&root) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
            _ => Ok(()),
        })
        .await
        .map_err(io)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Duration;
    use tandem_core::clock::MockClock;

    fn git(cwd: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .status()
            .expect("git")
            .success();
        assert!(ok, "git {args:?}");
    }

    async fn service() -> (
        tempfile::TempDir,
        Arc<CheckpointService>,
        PathBuf,
        MockClock,
    ) {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(tmp.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let terminals = TerminalService::new(tandem_pty::PtyManager::new(), store.clone());
        terminals.set_login_env(tandem_pty::env::LoginEnv {
            vars: std::env::vars().collect(),
            shell: PathBuf::from("/bin/sh"),
            degraded: false,
        });
        let clock = MockClock::new(chrono::Utc::now());
        let svc = CheckpointService::new(
            data,
            store,
            EventBus::new(),
            Arc::new(clock.clone()),
            terminals,
        );
        let folder = tmp.path().join("work");
        std::fs::create_dir_all(&folder).expect("mkdir");
        git(&folder, &["init", "-q", "-b", "main"]);
        (tmp, svc, folder, clock)
    }

    fn req(folder: &Path) -> Request {
        Request {
            folder: folder.to_path_buf(),
            thread_id: Some("th_x".into()),
            label: "Fix it".into(),
            ..Request::default()
        }
    }

    #[tokio::test]
    async fn f_ex_01_4_a_turn_that_changed_nothing_adds_no_checkpoint() {
        let (_t, svc, folder, clock) = service().await;
        std::fs::write(folder.join("a.txt"), "a").expect("write");
        assert!(svc
            .before_turn(req(&folder), CheckpointReason::Turn)
            .await
            .is_some());
        clock.advance(Duration::from_secs(1));
        assert!(svc
            .before_turn(req(&folder), CheckpointReason::Turn)
            .await
            .is_none());
        std::fs::write(folder.join("a.txt"), "b").expect("write");
        clock.advance(Duration::from_secs(1));
        assert!(svc
            .before_turn(req(&folder), CheckpointReason::Turn)
            .await
            .is_some());
        let tl = svc.timeline(&folder, None).await.expect("timeline");
        assert_eq!(tl.store, StoreKind::Repo);
        assert_eq!(tl.items.len(), 2);
        assert_eq!(tl.items[0].differs, 0);
        assert_eq!(tl.items[1].differs, 1);
        // The thread's own view lists the same two.
        assert_eq!(
            svc.timeline(&folder, Some("th_x".into()))
                .await
                .expect("tl")
                .items
                .len(),
            2
        );
        assert_eq!(
            svc.timeline(&folder, Some("th_other".into()))
                .await
                .expect("tl")
                .items
                .len(),
            0
        );
    }

    #[tokio::test]
    async fn f_ex_01_10_checkpoints_off_takes_none() {
        let (_t, svc, folder, _clock) = service().await;
        std::fs::write(folder.join("a.txt"), "a").expect("write");
        settings::set(
            &svc.store,
            SettingKey::CheckpointsEnabled,
            serde_json::json!(false),
        )
        .await
        .expect("set");
        assert!(svc
            .before_turn(req(&folder), CheckpointReason::Turn)
            .await
            .is_none());
        let tl = svc.timeline(&folder, None).await.expect("timeline");
        assert!(!tl.enabled);
        assert!(tl.items.is_empty());
    }

    #[tokio::test]
    async fn f_ex_01_a_folder_on_an_ssh_host_never_gets_a_checkpoint() {
        let (_t, svc, folder, _clock) = service().await;
        std::fs::write(folder.join("a.txt"), "a").expect("write");
        // The host's folder has the same path as a folder on this computer:
        // that local folder must not be what's saved (D-833).
        let host = tandem_store::repo::hosts::add_ssh(
            &svc.store,
            tandem_store::repo::hosts::NewSshHost {
                label: "box".into(),
                target: "dev@box".into(),
                port: None,
                identity_file: None,
            },
        )
        .await
        .expect("host");
        let (ws, _) = workspaces::add(
            &svc.store,
            workspaces::NewWorkspace {
                kind: "ssh".into(),
                host_id: host.id.as_str().to_string(),
                root: folder.to_string_lossy().into_owned(),
                name: "work".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("workspace");
        let on_host = Request {
            workspace_id: Some(ws.id.as_str().to_string()),
            ..req(&folder)
        };
        assert!(svc
            .before_turn(on_host.clone(), CheckpointReason::Turn)
            .await
            .is_none());
        let err = svc
            .take(on_host, CheckpointReason::Manual)
            .await
            .expect_err("refused");
        assert_eq!(err.code, ErrorCode::Unavailable);
        assert_eq!(err.message, ON_HOST);
        assert!(svc
            .timeline(&folder, None)
            .await
            .expect("timeline")
            .items
            .is_empty());
    }

    #[tokio::test]
    async fn f_ex_01_2_restore_saves_an_undo_checkpoint_first() {
        let (_t, svc, folder, clock) = service().await;
        std::fs::write(folder.join("a.txt"), "before").expect("write");
        let cp = svc
            .before_turn(req(&folder), CheckpointReason::Turn)
            .await
            .expect("checkpoint");
        std::fs::write(folder.join("a.txt"), "agent").expect("write");
        std::fs::write(folder.join("new.txt"), "new").expect("write");
        clock.advance(Duration::from_secs(1));
        let diff = svc.diff(&cp.id).await.expect("diff");
        assert_eq!(diff.len(), 2);
        let report = svc.restore(&cp.id, None).await.expect("restore");
        assert_eq!(report.restored, vec!["a.txt", "new.txt"]);
        assert_eq!(
            std::fs::read_to_string(folder.join("a.txt")).expect("read"),
            "before"
        );
        assert!(!folder.join("new.txt").exists());
        let undo = report.undo_id.expect("undo");
        clock.advance(Duration::from_secs(1));
        let back = svc.restore(&undo, None).await.expect("undo");
        assert_eq!(back.restored.len(), 2);
        assert_eq!(
            std::fs::read_to_string(folder.join("a.txt")).expect("read"),
            "agent"
        );
        let tl = svc.timeline(&folder, None).await.expect("timeline");
        assert!(tl
            .items
            .iter()
            .any(|c| c.reason == CheckpointReason::Restore));
    }

    #[tokio::test]
    async fn f_ex_01_5_keep_limits_the_timeline() {
        let (_t, svc, folder, clock) = service().await;
        settings::set(
            &svc.store,
            SettingKey::CheckpointsKeep,
            serde_json::json!(10),
        )
        .await
        .expect("set");
        for i in 0..11 {
            std::fs::write(folder.join("a.txt"), format!("{i}")).expect("write");
            clock.advance(Duration::from_secs(1));
            assert!(svc
                .before_turn(req(&folder), CheckpointReason::Turn)
                .await
                .is_some());
        }
        let tl = svc.timeline(&folder, None).await.expect("timeline");
        assert_eq!(tl.items.len(), 10);
        let cp = svc.open(&folder).await.expect("open");
        assert_eq!(cp.pinned().expect("pinned").len(), 10);
        // And by age, at startup.
        clock.advance(Duration::from_secs(15 * 86_400));
        svc.prune_all().await;
        assert!(svc
            .timeline(&folder, None)
            .await
            .expect("tl")
            .items
            .is_empty());
    }

    #[tokio::test]
    async fn f_ex_01_9_a_prompt_waits_for_its_checkpoint_at_most_two_seconds() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let terminals = TerminalService::new(
            tandem_pty::PtyManager::new(),
            Store::open(&DataDir::at(tempfile::tempdir().expect("tmp").keep()).expect("data"))
                .expect("store"),
        );
        let done = Arc::new(AtomicBool::new(false));
        let seen_late = Arc::new(AtomicBool::new(false));
        let (d, l) = (done.clone(), seen_late.clone());
        terminals.set_prompt_hook(Arc::new(move |_pane, late, _turn| {
            let (d, l) = (d.clone(), l.clone());
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(2500)).await;
                l.store(late.load(Ordering::SeqCst), Ordering::SeqCst);
                d.store(true, Ordering::SeqCst);
            })
        }));
        let pane = tandem_core::ids::PaneId::new();
        let started = std::time::Instant::now();
        terminals.before_prompt(&pane, false).await;
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_millis(1900) && waited < Duration::from_millis(2400),
            "{waited:?}"
        );
        assert!(!done.load(Ordering::SeqCst));
        // The checkpoint carries on and is marked late.
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert!(done.load(Ordering::SeqCst));
        assert!(seen_late.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn f_ex_01_usage_and_delete_all() {
        let (_t, svc, folder, _clock) = service().await;
        std::fs::write(folder.join("a.txt"), "a").expect("write");
        svc.before_turn(req(&folder), CheckpointReason::Turn)
            .await
            .expect("cp");
        let u = svc.usage().await;
        assert_eq!(u.folders, 1);
        assert!(u.bytes > 0);
        svc.delete_all().await.expect("delete");
        assert_eq!(svc.usage().await.folders, 0);
        assert!(svc
            .timeline(&folder, None)
            .await
            .expect("tl")
            .items
            .is_empty());
    }
}
