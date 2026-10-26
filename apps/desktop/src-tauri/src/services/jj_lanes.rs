//! jj workspaces as worktrees (F-CD-24): in a repository co-located with
//! jj, a lane can be a jj workspace at the usual
//! `<repo>/.tandem/worktrees/<slug>`. jj itself runs in `tandem_git::jj`;
//! this finds it on the login PATH, remembers the person's choice per
//! repository, and makes, diffs and discards jj lanes for `lanes`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::ids::WorkspaceId;
use tandem_core::{DataDir, ErrorCode, IpcError};
use tandem_git::jj::{self, Jj, JjRunner};
use tandem_git::{FileDiff, Repo};
use tandem_store::repo::lane_vcs;
use tandem_store::repo::tabs;
use tandem_store::repo::workspaces::{self, NewLane, Workspace};
use tandem_store::Store;

use crate::services::terminal::TerminalService;

pub const JJ_NOT_YET: &str = "Not available for jj workspaces yet";
pub const JJ_ONLY: &str =
    "This repository uses jj without git. Vibezeo works with jj in repositories co-located with git.";
pub const JJ_REMOTE: &str = "jj workspaces work on folders on this computer";
pub const JJ_MISSING: &str = "jj isn't installed";
pub const NOT_COLOCATED: &str = "This repository isn't co-located with jj";
pub const NO_FORECAST: &str = "No forecast for jj workspaces yet";
/// The choice New worktree remembers per repository.
pub const KINDS_FILE: &str = "lane-kinds.json";

/// What New worktree makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LaneKind {
    #[default]
    Git,
    Jj,
}

/// `lane_kind_get`: whether New worktree offers a jj workspace here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LaneKinds {
    /// `.jj` beside `.git` at the repository's root, and jj found.
    pub colocated: bool,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jj_version: Option<String>,
    /// `.jj` without `.git`: out of scope, and said so.
    pub jj_only: bool,
    /// A folder on an SSH host or in WSL: no jj workspaces.
    pub remote: bool,
    pub preferred: LaneKind,
}

/// The terminals, for the login environment jj runs in. Set at startup;
/// without it (tests) jj runs in this process's environment.
static TERMINALS: OnceLock<Weak<TerminalService>> = OnceLock::new();

pub fn set_terminals(terms: &Arc<TerminalService>) {
    let _ = TERMINALS.set(Arc::downgrade(terms));
}

/// jj on the login PATH or in the usual places, with the login environment.
pub async fn runner() -> Option<Arc<dyn JjRunner>> {
    let terms = TERMINALS.get().and_then(Weak::upgrade);
    let vars: Vec<(String, String)> = match terms {
        Some(t) => t.login_env().await.vars.clone().into_iter().collect(),
        None => std::env::vars().collect(),
    };
    let get = |k: &str| {
        vars.iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let path = get("PATH");
    let home = Some(get("HOME"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from);
    Jj::locate(&path, home.as_deref(), vars).map(|j| Arc::new(j) as Arc<dyn JjRunner>)
}

/// Whether a worktree is a jj workspace.
pub async fn is_jj(store: &Store, ws: &Workspace) -> Result<bool, IpcError> {
    if ws.parent_id.is_none() {
        return Ok(false);
    }
    Ok(lane_vcs::is_jj(store, &ws.id).await?)
}

fn not_yet() -> IpcError {
    IpcError::new(ErrorCode::Invalid, JJ_NOT_YET).with_stage("jj")
}

/// Refuses what isn't available for jj workspaces yet, before it runs.
pub async fn refuse_jj(store: &Store, ws: &Workspace) -> Result<(), IpcError> {
    if is_jj(store, ws).await? {
        Err(not_yet())
    } else {
        Ok(())
    }
}

fn jj_err(e: tandem_git::GitError) -> IpcError {
    IpcError::from(e).with_stage("jj")
}

fn read_kinds(data: &DataDir) -> BTreeMap<String, LaneKind> {
    std::fs::read(data.root().join(KINDS_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn kind_key(root: &Path) -> String {
    std::fs::canonicalize(root)
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// Remembers what New worktree made in the repository at `root`.
pub fn remember(data: &DataDir, root: &Path, kind: LaneKind) {
    let mut all = read_kinds(data);
    all.insert(kind_key(root), kind);
    match serde_json::to_vec_pretty(&all) {
        Ok(bytes) => {
            if let Err(e) =
                tandem_core::atomic::atomic_write(&data.root().join(KINDS_FILE), &bytes, None)
            {
                tracing::warn!(code = "jj.kind_not_saved", error = %e, "the worktree kind wasn't saved");
            }
        }
        Err(e) => {
            tracing::warn!(code = "jj.kind_not_saved", error = %e, "the worktree kind wasn't saved")
        }
    }
}

/// The repository's row: the workspace itself or a worktree's parent.
async fn top_of(store: &Store, ws: &Workspace) -> Result<Workspace, IpcError> {
    Ok(match &ws.parent_id {
        Some(parent) => workspaces::get(store, parent).await?,
        None => ws.clone(),
    })
}

/// `lane_kind_get`.
pub async fn kinds(store: &Store, data: &DataDir, id: &WorkspaceId) -> Result<LaneKinds, IpcError> {
    let ws = workspaces::get(store, id).await?;
    let top = top_of(store, &ws).await?;
    if top.kind != "local" {
        return Ok(LaneKinds {
            colocated: false,
            jj_version: None,
            jj_only: false,
            remote: true,
            preferred: LaneKind::Git,
        });
    }
    let root = PathBuf::from(&top.root);
    let jj_only = jj::jj_without_git(&root);
    let mut version = None;
    if jj::colocated(&root) {
        if let Some(j) = runner().await {
            version = tokio::task::spawn_blocking(move || jj::version(j.as_ref()))
                .await
                .ok()
                .flatten();
        }
    }
    let colocated = version.is_some();
    let preferred = if !colocated {
        LaneKind::Git
    } else if is_jj(store, &ws).await? {
        // From a jj workspace, New worktree makes another (F-CD-24).
        LaneKind::Jj
    } else {
        read_kinds(data)
            .get(&kind_key(&root))
            .copied()
            .unwrap_or(LaneKind::Jj)
    };
    Ok(LaneKinds {
        colocated,
        jj_version: version,
        jj_only,
        remote: false,
        preferred,
    })
}

/// A jj workspace for `slug_source`: from the repository's default
/// workspace, or, made from a jj lane `from`, from that one's working copy.
pub async fn create(
    store: &Store,
    from: &Workspace,
    top: &Workspace,
    slug_source: &str,
) -> Result<Workspace, IpcError> {
    if top.kind != "local" {
        return Err(IpcError::new(ErrorCode::Invalid, JJ_REMOTE).with_stage("jj"));
    }
    let root = PathBuf::from(&top.root);
    if jj::jj_without_git(&root) {
        return Err(IpcError::new(ErrorCode::Invalid, JJ_ONLY).with_stage("jj"));
    }
    if !jj::colocated(&root) {
        return Err(IpcError::new(ErrorCode::Invalid, NOT_COLOCATED).with_stage("jj"));
    }
    let Some(runner) = runner().await else {
        return Err(IpcError::new(ErrorCode::EngineMissing, JJ_MISSING).with_stage("jj"));
    };
    let repo = Repo::discover(&root)?
        .ok_or_else(|| IpcError::new(ErrorCode::Invalid, NOT_COLOCATED).with_stage("jj"))?;
    let start = if is_jj(store, from).await? {
        from.lane_branch.clone()
    } else {
        None
    };
    let lane = jj::add_workspace(
        runner.clone(),
        &repo,
        slug_source,
        &Default::default(),
        start,
    )
    .await
    .map_err(jj_err)?;
    // F-CD-23: no trust left at a reused path reaches it.
    crate::services::lsp::untrust_new_worktree(&lane.path).await;
    let Some(path) = lane.path.to_str().map(str::to_string) else {
        undo(runner, &root, &lane).await;
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Vibezeo can't open folders whose names aren't valid UTF-8",
        ));
    };
    let row = lane_vcs::add_jj_lane(
        store,
        NewLane {
            parent_id: top.id.clone(),
            root: path,
            name: lane.slug.clone(),
            branch: lane.slug.clone(),
            base: lane.base.clone(),
        },
    )
    .await;
    let row = match row {
        Ok(row) => row,
        Err(e) => {
            undo(runner, &root, &lane).await;
            return Err(e.into());
        }
    };
    tabs::create(store, &row.id, None).await?;
    tracing::info!(code = "lane.created_jj", "jj workspace created");
    Ok(row)
}

/// Nothing half-made stays behind.
async fn undo(runner: Arc<dyn JjRunner>, root: &Path, lane: &jj::JjLane) {
    let _ = jj::forget_workspace(runner, root, &lane.path, &lane.slug).await;
}

/// A jj lane's changes against the commit it started from.
pub async fn diff(lane: &Workspace) -> Result<Vec<FileDiff>, IpcError> {
    let root = PathBuf::from(&lane.root);
    if !root.is_dir() {
        return Err(
            IpcError::new(ErrorCode::NotFound, crate::services::lanes::FOLDER_MISSING)
                .with_stage("jj"),
        );
    }
    let Some(runner) = runner().await else {
        return Err(IpcError::new(ErrorCode::EngineMissing, JJ_MISSING).with_stage("jj"));
    };
    let base = lane.lane_base.clone().unwrap_or_default();
    jj::diff(runner, &root, &base).await.map_err(jj_err)
}

/// A jj lane's changed files since its base (the overlap check).
pub async fn changed_files(lane: &Workspace) -> Result<Vec<String>, IpcError> {
    let Some(runner) = runner().await else {
        return Err(IpcError::new(ErrorCode::EngineMissing, JJ_MISSING).with_stage("jj"));
    };
    let base = lane.lane_base.clone().unwrap_or_default();
    jj::changed_files(runner, Path::new(&lane.root), &base)
        .await
        .map_err(jj_err)
}

/// Discard's last step for a jj lane: `jj workspace forget` and the folder.
/// The workspace's commits stay in the repository.
pub async fn forget(lane: &Workspace, parent: &Workspace) -> Result<(), IpcError> {
    let Some(runner) = runner().await else {
        return Err(IpcError::new(ErrorCode::EngineMissing, JJ_MISSING).with_stage("jj"));
    };
    let name = lane
        .lane_branch
        .clone()
        .unwrap_or_else(|| lane.name.clone());
    jj::forget_workspace(
        runner,
        Path::new(&parent.root),
        Path::new(&lane.root),
        &name,
    )
    .await
    .map_err(jj_err)
}
