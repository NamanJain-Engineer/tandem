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
use tandem_git::{FileDiff, ForceConfirm, Repo};
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
pub const JJ_UNTRUSTED: &str =
    "Vibezeo runs jj only in repositories you trust: trust this repository first";
/// A jj workspace being made that never got its row is forgotten by the
/// sweep after this long.
pub const ORPHAN_AGE: std::time::Duration = std::time::Duration::from_secs(10 * 60);
/// Every pending record goes after this long, whatever it names: the sweep
/// acts on one within minutes, so an older one is for a repository it
/// never reaches.
pub const PRUNE_AGE: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 3600);
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
    /// The repository is trusted (F-CD-23's trust): jj runs only then.
    pub trusted: bool,
    /// Why a `.jj` here isn't used (committed to git, or not this
    /// repository's).
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
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
    #[cfg(test)]
    let vars = isolated(vars);
    Jj::locate(&path, home.as_deref(), vars).map(|j| Arc::new(j) as Arc<dyn JjRunner>)
}

/// Tests' jj never reads the developer's config: a home and a config of
/// its own (an identity only). One folder for every run (not one per
/// process, which would pile up), its config written whole by rename so
/// runs side by side never see half of it.
#[cfg(test)]
fn isolated(mut vars: Vec<(String, String)>) -> Vec<(String, String)> {
    static HOME: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    let home = HOME.get_or_init(|| {
        let dir = std::env::temp_dir().join("tandem-jj-test-home");
        std::fs::create_dir_all(&dir).ok()?;
        let part = dir.join(format!("jj-config.toml.{}", std::process::id()));
        std::fs::write(
            &part,
            "[user]\nname = \"Test\"\nemail = \"test@example.invalid\"\n",
        )
        .ok()?;
        std::fs::rename(&part, dir.join("jj-config.toml")).ok()?;
        Some(dir)
    });
    vars.retain(|(k, _)| k != "HOME" && !k.starts_with("JJ_"));
    if let Some(dir) = home {
        vars.push(("HOME".into(), dir.to_string_lossy().into_owned()));
        vars.push((
            "JJ_CONFIG".into(),
            dir.join("jj-config.toml").to_string_lossy().into_owned(),
        ));
    }
    vars
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

/// The repository at `root` is trusted for its code to run (F-CD-23's
/// trust, which covers jj too).
fn trusted(root: &Path) -> bool {
    crate::services::lsp::data_dir().is_some_and(|d| crate::services::lsp::is_trusted(d, root))
}

/// What every jj command in the repository at `root` needs first: a `.jj`
/// of jj's own beside `.git`, and the person's trust. jj reads the
/// repository's config, so an untrusted clone never gets that far.
pub fn gate(root: &Path) -> Result<(), IpcError> {
    if jj::jj_without_git(root) {
        return Err(IpcError::new(ErrorCode::Invalid, JJ_ONLY).with_stage("jj"));
    }
    jj::check_repo(root).map_err(|e| match e {
        tandem_git::GitError::Invalid(m) => IpcError::new(ErrorCode::Invalid, m).with_stage("jj"),
        other => jj_err(other),
    })?;
    if !trusted(root) {
        return Err(IpcError::new(ErrorCode::PermissionDenied, JJ_UNTRUSTED).with_stage("jj"));
    }
    Ok(())
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
            trusted: false,
            refused: None,
        });
    }
    let root = PathBuf::from(&top.root);
    let jj_only = jj::jj_without_git(&root);
    let mut version = None;
    let check = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || jj::check_repo(&root))
            .await
            .unwrap_or_else(|e| Err(tandem_git::GitError::Invalid(e.to_string())))
    };
    let refused = match (&check, jj::colocated(&root)) {
        (Err(e), true) => Some(match e {
            tandem_git::GitError::Invalid(m) => m.clone(),
            other => other.to_string(),
        }),
        _ => None,
    };
    if check.is_ok() {
        // `--version` reads no repository.
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
        trusted: trusted(&root),
        refused,
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
    gate(&root)?;
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
    // F-CD-23: no trust is left at a path the new workspace may take.
    crate::services::lsp::untrust_missing_under(&root.join(tandem_git::lanes::WORKTREES_DIR));
    // One at a time per repository, with the sweep: held until the row
    // exists and the record is gone.
    #[cfg(test)]
    lock_probe::waiting(&root);
    let _repo = repo_lock(&root).lock_owned().await;
    let lane = jj::add_workspace(
        runner.clone(),
        &repo,
        slug_source,
        &Default::default(),
        start,
        pending_dir(),
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
    // The row exists: the sweep's marker goes.
    jj::done_adding(&lane);
    tabs::create(store, &row.id, None).await?;
    tracing::info!(code = "lane.created_jj", "jj workspace created");
    Ok(row)
}

/// Nothing half-made stays behind: the workspace (and its empty first
/// commit), its folder, and a handoff's copy of its source.
async fn undo(runner: Arc<dyn JjRunner>, root: &Path, lane: &jj::JjLane) {
    // The record stays when the forget failed, so the sweep tries again.
    if jj::forget_workspace(
        runner.clone(),
        root,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        None,
    )
    .await
    .is_ok()
    {
        jj::done_adding(lane);
    }
    if let Some(copy) = lane.copy.clone() {
        let root = root.to_path_buf();
        let _ = tokio::task::spawn_blocking(move || {
            jj::abandon_leftover(runner.as_ref(), &root, &copy, false)
        })
        .await;
    }
}

/// A jj lane's changes against the commit it started from.
pub async fn diff(lane: &Workspace, parent: &Workspace) -> Result<Vec<FileDiff>, IpcError> {
    gate(Path::new(&parent.root))?;
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
    jj::diff(runner, Path::new(&parent.root), &root, &base)
        .await
        .map_err(jj_err)
}

/// A jj lane's changed files since its base (the overlap check).
pub async fn changed_files(lane: &Workspace, parent: &Workspace) -> Result<Vec<String>, IpcError> {
    gate(Path::new(&parent.root))?;
    let Some(runner) = runner().await else {
        return Err(IpcError::new(ErrorCode::EngineMissing, JJ_MISSING).with_stage("jj"));
    };
    let base = lane.lane_base.clone().unwrap_or_default();
    jj::changed_files(
        runner,
        Path::new(&parent.root),
        Path::new(&lane.root),
        &base,
    )
    .await
    .map_err(jj_err)
}

fn lane_name(lane: &Workspace) -> String {
    lane.lane_branch
        .clone()
        .unwrap_or_else(|| lane.name.clone())
}

/// Discard's checks for a jj lane, before anything stops: everything
/// [`jj::forget_checks`] refuses (even forced), and without `force`
/// anything jj didn't keep whole ([`jj::kept_whole`]: Dirty is Conflict, so
/// the dialog asks for the force as for a git lane's uncommitted changes; a
/// stale working copy is Invalid, saying what to run, never forced past).
pub async fn preflight(lane: &Workspace, parent: &Workspace, force: bool) -> Result<(), IpcError> {
    gate(Path::new(&parent.root))?;
    let (main, root, name) = (
        PathBuf::from(&parent.root),
        PathBuf::from(&lane.root),
        lane_name(lane),
    );
    if jj::forget_checks(&main, &root, &name).map_err(jj_err)? == jj::Folder::Gone || force {
        return Ok(());
    }
    let Some(runner) = runner().await else {
        return Err(IpcError::new(ErrorCode::EngineMissing, JJ_MISSING).with_stage("jj"));
    };
    tokio::task::spawn_blocking(move || jj::kept_whole(runner.as_ref(), &main, &root, &name))
        .await
        .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()).with_stage("jj"))?
        .map_err(jj_err)
}

/// Discard's last step for a jj lane: `jj workspace forget` and the folder,
/// checked again now its panes have stopped. The workspace's commits stay
/// in the repository.
pub async fn forget(
    lane: &Workspace,
    parent: &Workspace,
    force: Option<ForceConfirm>,
) -> Result<(), IpcError> {
    gate(Path::new(&parent.root))?;
    let Some(runner) = runner().await else {
        return Err(IpcError::new(ErrorCode::EngineMissing, JJ_MISSING).with_stage("jj"));
    };
    jj::forget_workspace(
        runner,
        Path::new(&parent.root),
        Path::new(&lane.root),
        &lane_name(lane),
        lane.lane_base.as_deref(),
        force,
    )
    .await
    .map_err(jj_err)
}

/// The sweep: jj workspaces in the repository at `root` whose making was
/// cut short (no row was written) are forgotten, their commits kept. Only
/// in a repository jj may run in, never while a workspace is being made in
/// it (the repository's lock is held throughout), and never for a name any
/// lane row has.
pub async fn forget_orphans(store: &Store, root: &Path) -> usize {
    let Some(pending) = pending_dir() else {
        return 0;
    };
    if !pending.is_dir() || gate(root).is_err() {
        return 0;
    }
    let Ok(_repo) = repo_lock(root).try_lock_owned() else {
        return 0;
    };
    let Some(runner) = runner().await else {
        return 0;
    };
    let rows = match row_names(store, root).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(code = "jj.orphans_failed", error = %e.message, "the sweep couldn't read the worktrees");
            return 0;
        }
    };
    match jj::forget_orphans(runner, &pending, root, ORPHAN_AGE, &rows).await {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!(code = "jj.orphans_failed", error = %e, "unfinished jj workspaces weren't forgotten");
            0
        }
    }
}

/// The folder names of every lane row in `<root>/.tandem/worktrees`, jj or
/// git, removed rows included (a workspace removed from the rail keeps its
/// worktrees).
async fn row_names(
    store: &Store,
    root: &Path,
) -> Result<std::collections::HashSet<String>, IpcError> {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let dir = canon(&root.join(tandem_git::lanes::WORKTREES_DIR));
    let mut rows = Vec::new();
    for id in lane_vcs::jj_lanes(store).await? {
        // Only a row that's gone is skipped: one that can't be read fails
        // the sweep, which then forgets nothing.
        match workspaces::get(store, &id).await {
            Ok(ws) => rows.push(ws),
            Err(tandem_store::StoreError::NotFound(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    for top in workspaces::list(store).await? {
        if top.parent_id.is_none() && canon(Path::new(&top.root)) == canon(root) {
            rows.extend(workspaces::lanes(store, &top.id).await?);
        }
    }
    Ok(rows
        .iter()
        .map(|w| PathBuf::from(&w.root))
        .filter(|p| p.parent().map(canon).as_deref() == Some(dir.as_path()))
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect())
}

/// Records older than [`PRUNE_AGE`] go, whatever they name: the sweep acts
/// on a record within minutes, so one that old is for a repository it
/// never reaches (removed from the rail, untrusted, without jj) or from an
/// older version. Nothing is forgotten for them.
pub fn prune_pending() -> usize {
    let Some(pending) = pending_dir() else {
        return 0;
    };
    jj::prune_pending(&pending, PRUNE_AGE)
}

/// Where records of jj workspaces being made are kept: `$DATA/jj-pending`,
/// a folder of Vibezeo's own (a repository's content can't name one).
pub fn pending_dir() -> Option<PathBuf> {
    crate::services::lsp::data_dir().map(|d| d.root().join("jj-pending"))
}

/// One lock per repository (canonical folder): a jj workspace being made
/// holds it until its row exists, and the sweep holds it throughout.
static LOCKS: std::sync::Mutex<BTreeMap<PathBuf, Arc<tokio::sync::Mutex<()>>>> =
    std::sync::Mutex::new(BTreeMap::new());

pub(crate) fn repo_lock(root: &Path) -> Arc<tokio::sync::Mutex<()>> {
    let key = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    LOCKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(key)
        .or_default()
        .clone()
}

/// Tests see a create reach the repository's lock (and wait there) without
/// sleeping: one signal per repository.
#[cfg(test)]
pub(crate) mod lock_probe {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    static WAITING: std::sync::Mutex<BTreeMap<PathBuf, Arc<tokio::sync::Notify>>> =
        std::sync::Mutex::new(BTreeMap::new());

    pub(crate) fn signal(root: &Path) -> Arc<tokio::sync::Notify> {
        let key = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        WAITING
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key)
            .or_default()
            .clone()
    }

    pub(super) fn waiting(root: &Path) {
        signal(root).notify_one();
    }
}
