//! Worktree lanes (F-CD-15, ARCHITECTURE §7). Each lane is a child
//! workspace on its own `tandem/<slug>` branch under
//! `<repo>/.tandem/worktrees/<slug>`. Copy the user sees says "worktree";
//! "lane" is the internal word (UI_SPEC §6.2).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::ids::{TabId, WorkspaceId};
use tandem_core::{DataDir, ErrorCode, IpcError};
use tandem_git::{ForceConfirm, Git, GitError, LaneManager, MergeReport, Repo, StaleLane};
use tandem_pty::env::{resolve_on_path, LoginEnv};
use tandem_pty::PtyManager;
use tandem_store::repo::lane_repos;
use tandem_store::repo::panes::{self, PaneRecord};
use tandem_store::repo::tabs;
use tandem_store::repo::workspaces::{self, NewLane, Workspace};
use tandem_store::Store;

use crate::services::worktree_setup::{self, RemoveReport};

pub const NOT_A_REPO: &str = "Worktrees need a git repository";
pub const GIT_MISSING: &str = "Git isn't installed";
pub const GH_MISSING: &str = "GitHub CLI (gh) isn't installed";
pub const FOLDER_MISSING: &str = "The worktree folder is missing";
/// The sweep runs at startup and then this often.
pub const SWEEP_EVERY: Duration = Duration::from_secs(6 * 3600);
/// "Create pull request" types into its shell once the prompt is up.
pub const PR_COMMAND: &str = "gh pr create";
pub const PR_READY_TIMEOUT: Duration = Duration::from_secs(30);

/// Lanes in a folder on an SSH host are made there, by its helper with the
/// host's own git (D-1602). Set once at startup.
static REMOTE: std::sync::OnceLock<std::sync::Weak<crate::services::remote::RemoteService>> =
    std::sync::OnceLock::new();

pub fn set_remote(remote: &std::sync::Arc<crate::services::remote::RemoteService>) {
    let _ = REMOTE.set(std::sync::Arc::downgrade(remote));
}

type Remote = (
    std::sync::Arc<crate::services::remote::RemoteService>,
    tandem_core::ids::HostId,
);

/// The host a workspace's lanes are made on: `None` for a local folder.
/// A WSL folder has none yet (its git runs inside the distro).
fn remote_for(ws: &Workspace) -> Result<Option<Remote>, IpcError> {
    match ws.kind.as_str() {
        "ssh" => REMOTE
            .get()
            .and_then(std::sync::Weak::upgrade)
            .map(|r| Some((r, tandem_core::ids::HostId::from_db(ws.host_id.clone()))))
            .ok_or_else(|| {
                IpcError::new(
                    ErrorCode::Unavailable,
                    "Remote hosts aren't available in this build",
                )
            }),
        "wsl" => Err(IpcError::new(
            ErrorCode::Unavailable,
            "Worktrees aren't available in WSL folders yet",
        )),
        _ => Ok(None),
    }
}

/// `lanes:stale`: a repository's merged worktrees idle for over 7 days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StaleLanes {
    pub workspace_id: WorkspaceId,
    pub lanes: Vec<StaleLane>,
}

/// The git runner, from the login environment's PATH. Reads point
/// `core.hooksPath` at an empty folder Tandem owns.
pub fn git_for(data: &DataDir, login: &LoginEnv) -> Result<Git, IpcError> {
    let hooks = data.run().join("empty-hooks");
    std::fs::create_dir_all(&hooks)
        .map_err(|e| IpcError::new(ErrorCode::Io, e.to_string()).with_stage("git"))?;
    Git::locate(login.vars.get("PATH").map(OsStr::new), hooks)
        .map_err(|_| IpcError::new(ErrorCode::EngineMissing, GIT_MISSING).with_stage("git"))
}

fn discover(root: &str, missing: &str) -> Result<Repo, IpcError> {
    if !Path::new(root).is_dir() {
        return Err(IpcError::new(ErrorCode::NotFound, missing).with_stage("git"));
    }
    Repo::discover(Path::new(root))?
        .ok_or_else(|| IpcError::new(ErrorCode::Invalid, NOT_A_REPO).with_stage("git"))
}

/// A lane row: a child workspace with its branch and base.
async fn lane_row(store: &Store, id: &WorkspaceId) -> Result<(Workspace, Workspace), IpcError> {
    let lane = workspaces::get(store, id).await?;
    let (Some(parent_id), Some(_), Some(_)) = (&lane.parent_id, &lane.lane_branch, &lane.lane_base)
    else {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "That workspace isn't a worktree",
        ));
    };
    let parent = workspaces::get(store, parent_id).await?;
    Ok((lane, parent))
}

/// `lane_create`: a worktree from the workspace's current HEAD, registered
/// under the repository's workspace with one tab. Called on a worktree, the
/// new one starts from that worktree's HEAD and sits beside it.
pub async fn create(
    store: &Store,
    git: &Git,
    workspace: &WorkspaceId,
    slug_source: &str,
) -> Result<Workspace, IpcError> {
    let ws = workspaces::get(store, workspace).await?;
    let top = match &ws.parent_id {
        Some(parent) => workspaces::get(store, parent).await?,
        None => ws.clone(),
    };
    if let Some((remote, host)) = remote_for(&ws)? {
        return create_on_host(store, &remote, &host, &ws, &top, slug_source).await;
    }
    // F-CD-22: a folder holding several repositories.
    let names = crate::services::workspaces::multi_repos(Path::new(&top.root));
    if !names.is_empty() {
        return create_multi(store, git, &ws, &top, &names, slug_source).await;
    }
    let repo = discover(&ws.root, "Folder not found")?;
    let lane = LaneManager::create(git, &repo, slug_source, &HashSet::new()).await?;
    let root = lane.path.to_str().map(str::to_string).ok_or_else(|| {
        IpcError::new(
            ErrorCode::Invalid,
            "Vibezeo can't open folders whose names aren't valid UTF-8",
        )
    });
    let row = match root {
        Ok(root) => workspaces::add_lane(
            store,
            NewLane {
                parent_id: top.id.clone(),
                root,
                name: lane.slug.clone(),
                branch: lane.branch.clone(),
                base: lane.base.clone(),
            },
        )
        .await
        .map_err(IpcError::from),
        Err(e) => Err(e),
    };
    let row = match row {
        Ok(row) => row,
        Err(err) => {
            // Nothing half-made stays behind: the new worktree is clean.
            let _ = LaneManager::remove(git, &repo, &lane.path, None).await;
            return Err(err);
        }
    };
    tabs::create(store, &row.id, None).await?;
    tracing::info!(code = "lane.created", "worktree created");
    // F-WT-01: the repository's ignored files, for every new worktree.
    crate::services::worktree_setup::copy_for_new_lane(&row).await;
    // F-WT-06: its index and port block, reported by `lane_setup`.
    crate::services::worktree_runtime::note_new_lane(&row);
    Ok(row)
}

/// F-CD-22: a multi-repo worktree (it has `lane_repos` rows), or a
/// multi-repo folder.
pub async fn is_multi(store: &Store, ws: &Workspace) -> Result<bool, IpcError> {
    if !lane_repos::list(store, &ws.id).await?.is_empty() {
        return Ok(true);
    }
    Ok(ws.kind == "local"
        && ws.parent_id.is_none()
        && !crate::services::workspaces::multi_repos(Path::new(&ws.root)).is_empty())
}

/// Refuses, up front, what needs one repository for a multi-repo worktree
/// or folder (F-CD-22, D-2534).
pub async fn single_repo(store: &Store, ws: &Workspace) -> Result<(), IpcError> {
    if is_multi(store, ws).await? {
        Err(IpcError::new(ErrorCode::Invalid, MULTI_NOT_YET).with_stage("git"))
    } else {
        Ok(())
    }
}
pub const MULTI_NOT_YET: &str = "Not available for multi-repo worktrees yet";

/// F-CD-22: a worktree across a multi-repo folder: the parent folder's row,
/// each repository's worktree in `lane_repos`, and a tab. Setup (F-WT-01)
/// and the runtime (F-WT-06) don't apply to it yet.
async fn create_multi(
    store: &Store,
    git: &Git,
    from: &Workspace,
    top: &Workspace,
    names: &[String],
    slug_source: &str,
) -> Result<Workspace, IpcError> {
    let folder = Path::new(&top.root);
    // Called on a multi-repo worktree (a handoff to a new one), each
    // repository starts from that worktree's HEAD, not the main checkout's.
    let starts: HashMap<String, String> = lane_repos::list(store, &from.id)
        .await?
        .into_iter()
        .map(|r| (r.name, r.root))
        .collect();
    let mut repos = Vec::new();
    for name in names {
        let start = starts
            .get(name)
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| folder.join(name));
        let repo = Repo::discover(&start)?.ok_or_else(|| {
            IpcError::new(ErrorCode::Invalid, format!("{name} isn't a git repository"))
        })?;
        repos.push((name.clone(), repo));
    }
    let lane = LaneManager::create_multi(git, folder, &repos, slug_source, &HashSet::new()).await?;
    let utf8 = |p: &Path| {
        p.to_str().map(str::to_string).ok_or_else(|| {
            IpcError::new(
                ErrorCode::Invalid,
                "Vibezeo can't open folders whose names aren't valid UTF-8",
            )
        })
    };
    let undo = |lane: &tandem_git::lanes::MultiLane| {
        let lane = lane.clone();
        let repos: Vec<(String, Repo)> =
            repos.iter().map(|(n, r)| (n.clone(), r.clone())).collect();
        async move {
            // Force-remove what was just made, and delete its new branches.
            for ((_, wt), (_, repo)) in lane.repos.iter().zip(&repos) {
                LaneManager::discard_new(git, repo, &wt.path, &wt.branch).await;
            }
            let _ = std::fs::remove_dir_all(&lane.path);
        }
    };
    let made = async {
        let mut rows = Vec::new();
        for ((name, wt), (_, repo)) in lane.repos.iter().zip(&repos) {
            rows.push(lane_repos::LaneRepo {
                name: name.clone(),
                // The repository's own checkout, even when the worktree
                // started from another worktree's HEAD.
                repo_root: utf8(&repo.main_root())?,
                root: utf8(&wt.path)?,
                base: wt.base.clone(),
            });
        }
        // The row and its repositories together, or neither.
        let row = lane_repos::add_lane(
            store,
            NewLane {
                parent_id: top.id.clone(),
                root: utf8(&lane.path)?,
                name: lane.slug.clone(),
                branch: lane.branch.clone(),
                base: String::new(),
            },
            rows,
        )
        .await?;
        if let Err(e) = tabs::create(store, &row.id, None).await {
            let _ = workspaces::remove(store, &row.id).await;
            let _ = lane_repos::clear(store, &row.id).await;
            return Err(e.into());
        }
        Ok::<Workspace, IpcError>(row)
    };
    let row = match made.await {
        Ok(row) => row,
        Err(err) => {
            undo(&lane).await;
            return Err(err);
        }
    };
    tracing::info!(
        code = "lane.created_multi",
        repos = lane.repos.len(),
        "multi-repo worktree created"
    );
    Ok(row)
}

/// [`create`] on an SSH host (D-1602): the helper makes the worktree with
/// the host's git; the row and its tab are as locally.
async fn create_on_host(
    store: &Store,
    remote: &std::sync::Arc<crate::services::remote::RemoteService>,
    host: &tandem_core::ids::HostId,
    ws: &Workspace,
    top: &Workspace,
    slug_source: &str,
) -> Result<Workspace, IpcError> {
    use tandem_host::protocol::{method, LaneCreated};
    let lane: LaneCreated = remote
        .call(
            host,
            method::LANE_CREATE,
            serde_json::json!({"root": ws.root, "slug_source": slug_source, "also_taken": []}),
        )
        .await?;
    let row = workspaces::add_lane(
        store,
        NewLane {
            parent_id: top.id.clone(),
            root: lane.path.clone(),
            name: lane.slug.clone(),
            branch: lane.branch.clone(),
            base: lane.base.clone(),
        },
    )
    .await;
    let row = match row {
        Ok(row) => row,
        Err(err) => {
            // Nothing half-made stays behind: the new worktree is clean.
            let _ = remote
                .call::<serde_json::Value>(
                    host,
                    method::LANE_REMOVE,
                    serde_json::json!({"root": top.root, "path": lane.path, "force": false}),
                )
                .await;
            return Err(err.into());
        }
    };
    tabs::create(store, &row.id, None).await?;
    tracing::info!(code = "lane.created", remote = true, "worktree created");
    Ok(row)
}

/// A unified patch's file headers with `name/` leading each path, so a
/// multi-repo diff's patches say which repository they're in (F-CD-22).
pub fn prefix_patch(patch: &str, name: &str) -> String {
    let mut out = String::with_capacity(patch.len() + 64);
    let mut in_hunk = false;
    for line in patch.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            in_hunk = false;
            out.push_str("diff --git ");
            out.push_str(&rest.replacen("a/", &format!("a/{name}/"), 1).replacen(
                " b/",
                &format!(" b/{name}/"),
                1,
            ));
            continue;
        }
        if line.starts_with("@@") {
            in_hunk = true;
        }
        if !in_hunk {
            for (head, side) in [("--- ", "a/"), ("+++ ", "b/")] {
                if let Some(rest) = line.strip_prefix(head).and_then(|r| r.strip_prefix(side)) {
                    out.push_str(&format!("{head}{side}{name}/{rest}"));
                    break;
                }
            }
            if line.starts_with("--- a/") || line.starts_with("+++ b/") {
                continue;
            }
            for head in ["rename from ", "rename to ", "copy from ", "copy to "] {
                if let Some(rest) = line.strip_prefix(head) {
                    out.push_str(&format!("{head}{name}/{rest}"));
                    break;
                }
            }
            if ["rename from ", "rename to ", "copy from ", "copy to "]
                .iter()
                .any(|h| line.starts_with(h))
            {
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

/// `lane_diff`: the worktree's changes against the commit it started from,
/// including uncommitted work.
pub async fn diff(store: &Store, id: &WorkspaceId) -> Result<Vec<tandem_git::FileDiff>, IpcError> {
    let (lane, _) = lane_row(store, id).await?;
    if let Some((remote, host)) = remote_for(&lane)? {
        return remote
            .call(
                &host,
                tandem_host::protocol::method::LANE_DIFF,
                serde_json::json!({"root": lane.root, "base": lane.lane_base.unwrap_or_default()}),
            )
            .await;
    }
    // F-CD-22: each repository's changes, its name leading every path.
    let repos = lane_repos::list(store, &lane.id).await?;
    if !repos.is_empty() {
        let mut out = Vec::new();
        for r in repos {
            let repo = discover(&r.root, FOLDER_MISSING)?;
            for mut f in repo.diff_since(&r.base).await? {
                f.path = format!("{}/{}", r.name, f.path);
                f.old_path = f.old_path.map(|p| format!("{}/{p}", r.name));
                f.patch = prefix_patch(&f.patch, &r.name);
                out.push(f);
            }
        }
        return Ok(out);
    }
    let repo = discover(&lane.root, FOLDER_MISSING)?;
    let base = lane.lane_base.unwrap_or_default();
    Ok(repo.diff_since(&base).await?)
}

/// `lane_merge`: `git merge --no-ff` in the parent checkout. A conflict stops
/// it and is reported; nothing is committed and nothing is resolved.
pub async fn merge(store: &Store, git: &Git, id: &WorkspaceId) -> Result<MergeReport, IpcError> {
    let (lane, parent) = lane_row(store, id).await?;
    single_repo(store, &lane).await?;
    let dirty = || {
        IpcError::new(
            ErrorCode::Conflict,
            format!(
                "{} has uncommitted changes. Commit or stash them, then merge.",
                parent.name
            ),
        )
        .with_stage("git")
    };
    if let Some((remote, host)) = remote_for(&parent)? {
        let report: Result<MergeReport, IpcError> = remote
            .call(
                &host,
                tandem_host::protocol::method::LANE_MERGE,
                serde_json::json!({"root": parent.root, "branch": lane.lane_branch.unwrap_or_default()}),
            )
            .await;
        return match report {
            Err(e) if e.code == ErrorCode::Conflict => Err(dirty()),
            other => other,
        };
    }
    let repo = discover(&parent.root, "Folder not found")?;
    let branch = lane.lane_branch.unwrap_or_default();
    match LaneManager::merge_into_base(git, &repo, &branch).await {
        Ok(report) => {
            tracing::info!(
                code = "lane.merged",
                merged = report.merged,
                conflicts = report.conflicts.len(),
                "worktree merge"
            );
            Ok(report)
        }
        Err(GitError::Dirty) => Err(IpcError::new(
            ErrorCode::Conflict,
            format!(
                "{} has uncommitted changes. Commit or stash them, then merge.",
                parent.name
            ),
        )
        .with_stage("git")),
        Err(e) => Err(e.into()),
    }
}

/// Whether `gh` is on the login PATH ("Create pull request" needs it).
pub fn gh_available(login: &LoginEnv) -> bool {
    on_login_path(login, "gh")
}

fn on_login_path(login: &LoginEnv, program: &str) -> bool {
    let path = login.vars.get("PATH").map(String::as_str).unwrap_or("");
    let pathext = login.vars.get("PATHEXT").map(String::as_str);
    resolve_on_path(program, path, pathext).is_some()
}

pub const GLAB_MISSING: &str = "GitLab CLI (glab) isn't installed";
/// "Create merge request" types this for a GitLab remote (F-WT-04).
pub const MR_COMMAND: &str = "glab mr create";

/// Where a worktree's `origin` is hosted, for Create pull/merge request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Forge {
    Github,
    Gitlab,
}

/// The host in a remote URL: `git@host:path`, `ssh://user@host:port/path`,
/// `https://host/path`.
pub fn remote_host(url: &str) -> Option<String> {
    let rest = match url.split_once("://") {
        Some((_, rest)) => rest,
        // scp-like `user@host:path`; a plain local path has no host.
        None if url.contains(':') && !url.starts_with('/') => url,
        None => return None,
    };
    let rest = rest.rsplit_once('@').map_or(rest, |(_, h)| h);
    let host = rest.split(['/', ':']).next().unwrap_or("");
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// GitLab when `origin`'s host name contains "gitlab", else GitHub (D-2207).
pub fn forge_for(url: Option<&str>) -> Forge {
    match url.and_then(remote_host) {
        Some(host) if host.contains("gitlab") => Forge::Gitlab,
        _ => Forge::Github,
    }
}

/// The worktree folder's `origin` URL.
fn origin_url(root: &str) -> Option<String> {
    tandem_git::lanes::origin_url(Path::new(root))
}

/// `lane_forge` (F-WT-04).
pub async fn forge(store: &Store, id: &WorkspaceId) -> Result<Forge, IpcError> {
    let (lane, _) = lane_row(store, id).await?;
    let root = lane.root.clone();
    let url = tokio::task::spawn_blocking(move || origin_url(&root))
        .await
        .ok()
        .flatten();
    Ok(forge_for(url.as_deref()))
}

/// `lane_create_pr`: a shell pane in the worktree, in `tab`, and the command
/// to type once the view has started it: [`PR_COMMAND`], or [`MR_COMMAND`]
/// for a GitLab remote (F-WT-04).
pub async fn create_pr_pane(
    store: &Store,
    login: &LoginEnv,
    id: &WorkspaceId,
    tab: &TabId,
) -> Result<(PaneRecord, &'static str), IpcError> {
    let (lane, _) = lane_row(store, id).await?;
    single_repo(store, &lane).await?;
    if let Some((remote, host)) = remote_for(&lane)? {
        // The host's own gh, signed in there; its pane runs on the host.
        let (ssh, target) = remote.transport(&host).await?;
        let found = ssh
            .exec(
                &target,
                "\"${SHELL:-sh}\" -lc 'command -v gh' >/dev/null",
                None,
            )
            .await
            .is_ok();
        if !found {
            return Err(IpcError::new(ErrorCode::EngineMissing, GH_MISSING).with_stage("gh"));
        }
        // The host's lanes get gh only: telling GitLab apart there needs the
        // host's origin, so merge requests stay local-only for now.
        return Ok((
            panes::create_with(store, tab, "terminal", Some(lane.root.clone()), None).await?,
            PR_COMMAND,
        ));
    }
    if !Path::new(&lane.root).is_dir() {
        return Err(IpcError::new(ErrorCode::NotFound, FOLDER_MISSING));
    }
    // F-WT-04: a GitLab remote gets a merge request through glab.
    let command = match forge_for(origin_url(&lane.root).as_deref()) {
        Forge::Gitlab => {
            if !on_login_path(login, "glab") {
                return Err(
                    IpcError::new(ErrorCode::EngineMissing, GLAB_MISSING).with_stage("glab")
                );
            }
            MR_COMMAND
        }
        Forge::Github => {
            if !gh_available(login) {
                return Err(IpcError::new(ErrorCode::EngineMissing, GH_MISSING).with_stage("gh"));
            }
            PR_COMMAND
        }
    };
    let pane = panes::create_with(store, tab, "terminal", Some(lane.root.clone()), None).await?;
    Ok((pane, command))
}

/// Stops every open pane in a workspace's tabs.
pub async fn stop_panes(store: &Store, pty: &PtyManager, ws: &WorkspaceId) -> Result<(), IpcError> {
    for tab in tabs::list(store, ws).await? {
        for pane in panes::list_open(store, &tab.id).await? {
            let _ = pty.kill(pane.id.as_str());
            panes::close(store, &pane.id).await?;
        }
    }
    Ok(())
}

/// `lane_remove`. Without `force`, uncommitted or untracked files refuse
/// with Conflict before anything stops. With it (a [`ForceConfirm`], which
/// only a person in the main window can give), the worktree folder and its
/// changes go. Either way the branch and its commits stay.
pub async fn remove(
    store: &Store,
    pty: &PtyManager,
    git: &Git,
    id: &WorkspaceId,
    force: Option<ForceConfirm>,
) -> Result<(), IpcError> {
    remove_with_teardown(store, pty, git, id, force, None)
        .await
        .map(|_| ())
}

/// [`remove`], running the repository's trusted teardown (F-WT-01) in the
/// worktree after its panes stop and before the folder goes. The teardown's
/// result never blocks the removal.
pub async fn remove_with_teardown(
    store: &Store,
    pty: &PtyManager,
    git: &Git,
    id: &WorkspaceId,
    force: Option<ForceConfirm>,
    teardown: Option<(&DataDir, &LoginEnv)>,
) -> Result<RemoveReport, IpcError> {
    let (lane, parent) = lane_row(store, id).await?;
    if let Some((remote, host)) = remote_for(&parent)? {
        use tandem_host::protocol::method;
        // Dirty refuses before anything stops, as locally.
        if force.is_none() {
            let dirty = remote
                .files
                .git_status(
                    &remote,
                    &crate::services::remote_files::RemoteRoot {
                        host: host.clone(),
                        root: lane.root.clone(),
                    },
                )
                .await?
                .is_some_and(|(_, _, s)| !s.staged.is_empty() || !s.unstaged.is_empty());
            if dirty {
                return Err(
                    IpcError::new(ErrorCode::Conflict, "Uncommitted changes").with_stage("git")
                );
            }
        }
        stop_panes(store, pty, &lane.id).await?;
        remote
            .call::<serde_json::Value>(
                &host,
                method::LANE_REMOVE,
                serde_json::json!({"root": parent.root, "path": lane.root, "force": force.is_some()}),
            )
            .await
            .map_err(|e| {
                if e.code == ErrorCode::Conflict {
                    IpcError::new(ErrorCode::Conflict, "Uncommitted changes").with_stage("git")
                } else {
                    e
                }
            })?;
        workspaces::remove(store, &lane.id).await?;
        tracing::info!(code = "lane.removed", remote = true, "worktree removed");
        // The repository's teardown is a local command; a host's lane has none.
        return Ok(RemoveReport::default());
    }
    let repos = lane_repos::list(store, &lane.id).await?;
    if !repos.is_empty() {
        return remove_multi(store, pty, git, &lane, &repos, force).await;
    }
    let lane_path = Path::new(&lane.root);
    if force.is_none() && lane_path.is_dir() {
        if let Some(repo) = Repo::discover(lane_path)? {
            if !repo.status().await?.is_clean() {
                return Err(
                    IpcError::new(ErrorCode::Conflict, "Uncommitted changes").with_stage("git")
                );
            }
        }
    }
    stop_panes(store, pty, &lane.id).await?;
    // F-CD-23: its language servers stop before the folder goes (open
    // handles block `git worktree remove` on Windows; cargo check would
    // recreate target/ in it).
    crate::services::lsp::global()
        .forget(&crate::services::files::local_root(&lane))
        .await;
    let mut report = RemoveReport::default();
    if let Some((data, login)) = teardown {
        // F-WT-06: the teardown sees the worktree's variables.
        let vars = crate::services::worktree_runtime::vars_for(data, &lane);
        report.teardown = worktree_setup::teardown(
            data,
            login,
            &vars,
            &lane.root,
            worktree_setup::TEARDOWN_LIMIT,
        )
        .await;
    }
    let repo = discover(&parent.root, "Folder not found")?;
    LaneManager::remove(git, &repo, lane_path, force).await?;
    workspaces::remove(store, &lane.id).await?;
    crate::services::worktree_runtime::free_global(lane.id.as_str());
    tracing::info!(code = "lane.removed", "worktree removed");
    Ok(report)
}

/// F-CD-22: Discard for a multi-repo worktree: every repository's worktree
/// (each as an ordinary Discard, branches kept), then the parent folder.
/// Without `force`, uncommitted changes in any repository, or files the
/// agent left in the parent folder outside them, refuse it first.
async fn remove_multi(
    store: &Store,
    pty: &PtyManager,
    git: &Git,
    lane: &Workspace,
    repos: &[lane_repos::LaneRepo],
    force: Option<ForceConfirm>,
) -> Result<RemoveReport, IpcError> {
    let dirty = || IpcError::new(ErrorCode::Conflict, "Uncommitted changes").with_stage("git");
    let parent = Path::new(&lane.root);
    if force.is_none() {
        for r in repos {
            // A repository's worktree folder that's already gone has
            // nothing to lose; its registration is pruned below.
            if !Path::new(&r.root).exists() {
                continue;
            }
            if let Some(repo) = Repo::discover(Path::new(&r.root))? {
                if !repo.status().await?.is_clean() {
                    return Err(dirty());
                }
            }
        }
        let extra = std::fs::read_dir(parent)
            .map(|d| {
                d.flatten().any(|e| {
                    !repos
                        .iter()
                        .any(|r| e.file_name().to_string_lossy() == r.name)
                })
            })
            .unwrap_or(false);
        if extra {
            return Err(dirty());
        }
    }
    stop_panes(store, pty, &lane.id).await?;
    // F-CD-23: its language servers stop before the folder goes (open
    // handles block `git worktree remove` on Windows; cargo check would
    // recreate target/ in it).
    crate::services::lsp::global()
        .forget(&crate::services::files::local_root(lane))
        .await;
    let forced = force.is_some();
    let mut each = Vec::new();
    for r in repos {
        // A repository that's gone from the folder has no worktree to remove.
        match Repo::discover(Path::new(&r.repo_root)) {
            Ok(Some(repo)) => each.push((repo, PathBuf::from(&r.root))),
            _ => tracing::warn!(
                code = "lane.multi_repo_missing",
                "a multi-repo worktree's repository is gone; skipped"
            ),
        }
    }
    LaneManager::remove_many(git, &each, force).await?;
    let gone = if forced {
        std::fs::remove_dir_all(parent)
    } else {
        std::fs::remove_dir(parent)
    };
    if let Err(e) = gone {
        match e.kind() {
            std::io::ErrorKind::NotFound => {}
            // Something besides the worktrees is in it: Discard with force.
            std::io::ErrorKind::DirectoryNotEmpty => return Err(dirty()),
            _ => return Err(IpcError::new(ErrorCode::Io, e.to_string()).with_stage("git")),
        }
    }
    workspaces::remove(store, &lane.id).await?;
    // The soft removal keeps the row; its repositories go now.
    lane_repos::clear(store, &lane.id).await?;
    tracing::info!(code = "lane.removed_multi", "multi-repo worktree removed");
    Ok(RemoveReport::default())
}

/// Top-level workspaces whose repository has a `.tandem/worktrees` folder.
async fn swept(store: &Store) -> Result<Vec<Workspace>, IpcError> {
    Ok(workspaces::list(store)
        .await?
        .into_iter()
        .filter(|w| w.parent_id.is_none())
        .filter(|w| {
            Path::new(&w.root)
                .join(tandem_git::lanes::WORKTREES_DIR)
                .is_dir()
        })
        .collect())
}

/// `lanes_sweep`: `git worktree prune` in each repository with worktrees,
/// then the merged ones idle for over 7 days. Never deletes a worktree.
pub async fn sweep(
    store: &Store,
    git: &Git,
    now: DateTime<Utc>,
) -> Result<Vec<StaleLanes>, IpcError> {
    let mut out = Vec::new();
    for ws in swept(store).await? {
        let Ok(Some(repo)) = Repo::discover(Path::new(&ws.root)) else {
            continue;
        };
        match LaneManager::sweep(git, &repo, now).await {
            Ok(lanes) if !lanes.is_empty() => out.push(StaleLanes {
                workspace_id: ws.id.clone(),
                lanes,
            }),
            Ok(_) => {}
            Err(err) => tracing::warn!(error = %err, "worktree sweep failed for one workspace"),
        }
    }
    // Repositories on SSH hosts, by their helper (D-1602): only hosts
    // connected now, so the sweep never opens a connection (or a password
    // prompt) by itself.
    let remote = REMOTE.get().and_then(std::sync::Weak::upgrade);
    if let Some(remote) = remote {
        for ws in workspaces::list(store)
            .await?
            .into_iter()
            .filter(|w| w.parent_id.is_none() && w.kind == "ssh")
        {
            let host = tandem_core::ids::HostId::from_db(ws.host_id.clone());
            if remote.state(&host) != crate::services::remote::HostState::Connected {
                continue;
            }
            let found: Result<Vec<StaleLane>, IpcError> = remote
                .call(
                    &host,
                    tandem_host::protocol::method::LANE_SWEEP,
                    serde_json::json!({"root": ws.root}),
                )
                .await;
            match found {
                Ok(lanes) if !lanes.is_empty() => out.push(StaleLanes {
                    workspace_id: ws.id.clone(),
                    lanes,
                }),
                Ok(_) => {}
                // Not a repository, or no lanes there: nothing to say.
                Err(err) => tracing::debug!(error = %err, "remote worktree sweep skipped"),
            }
        }
    }
    Ok(out)
}

/// What the last sweep reported per workspace, so an unchanged list isn't
/// raised again every 6 hours.
#[derive(Default)]
pub struct SweepMemory(Mutex<BTreeMap<String, Vec<String>>>);

impl SweepMemory {
    /// The workspaces whose stale list is new or changed since last time.
    /// Workspaces with nothing stale are forgotten.
    pub fn changed(&self, found: &[StaleLanes]) -> Vec<StaleLanes> {
        let Ok(mut seen) = self.0.lock() else {
            return found.to_vec();
        };
        let mut fresh = Vec::new();
        let mut next = BTreeMap::new();
        for item in found {
            let mut branches: Vec<String> = item.lanes.iter().map(|l| l.branch.clone()).collect();
            branches.sort();
            if seen.get(item.workspace_id.as_str()) != Some(&branches) {
                fresh.push(item.clone());
            }
            next.insert(item.workspace_id.as_str().to_string(), branches);
        }
        *seen = next;
        fresh
    }
}

static MEMORY: std::sync::OnceLock<SweepMemory> = std::sync::OnceLock::new();

/// The sweep with its notification: one "Clean up" item per repository
/// whose list of finished worktrees is new or changed, and `lanes:stale`.
pub async fn sweep_and_notify(state: &crate::state::AppState) -> Result<Vec<StaleLanes>, IpcError> {
    let login = state.terminals.login_env().await;
    let git = git_for(&state.data, &login)?;
    let found = sweep(&state.store, &git, state.clock.now_utc()).await?;
    for item in MEMORY.get_or_init(SweepMemory::default).changed(&found) {
        let name = workspaces::get(&state.store, &item.workspace_id)
            .await
            .map(|w| w.name)
            .unwrap_or_default();
        state
            .notifications
            .raise_in_app(
                tandem_store::repo::notifications::NotificationKind::LaneCleanup,
                cleanup_title(item.lanes.len()),
                name,
                tandem_core::source::SourceRef::Lane {
                    workspace_id: item.workspace_id.as_str().to_string(),
                },
                format!("lane_cleanup:{}", item.workspace_id.as_str()),
            )
            .await;
        state.bus.publish(tandem_core::bus::BusEvent::new(
            tandem_core::bus::Topic::App,
            "lanes:stale",
            &item,
        ));
    }
    Ok(found)
}

/// The in-app notification text (UI_SPEC §4 notifications, "Lane cleanup").
pub fn cleanup_title(n: usize) -> String {
    if n == 1 {
        "1 finished worktree can be cleaned up".into()
    } else {
        format!("{n} finished worktrees can be cleaned up")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tandem_core::origin::{LocalReview, Origin};

    /// Plain git for fixtures, with no global or system config.
    fn git_cmd(cwd: &Path, args: &[&str]) -> String {
        let empty = std::env::temp_dir().join("tandem-desktop-tests-empty-gitconfig");
        std::fs::write(&empty, "").expect("empty gitconfig");
        let out = Command::new("git")
            .current_dir(cwd)
            .env("GIT_CONFIG_GLOBAL", &empty)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .args(args)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    struct Fx {
        _dir: tempfile::TempDir,
        store: Store,
        git: Git,
        repo: Workspace,
    }

    async fn fixture() -> Fx {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let root = dir.path().join("repo");
        std::fs::create_dir_all(&root).expect("mkdir");
        git_cmd(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("app.txt"), "one\n").expect("write");
        git_cmd(&root, &["add", "."]);
        git_cmd(&root, &["commit", "-qm", "first"]);
        let login = LoginEnv {
            vars: [(
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            )]
            .into(),
            shell: "/bin/sh".into(),
            degraded: false,
        };
        let git = git_for(&data, &login).expect("git on PATH");
        let added = crate::services::workspaces::add(&store, root.to_str().expect("utf8"), Some(3))
            .await
            .expect("add");
        Fx {
            _dir: dir,
            store,
            git,
            repo: added.workspace,
        }
    }

    #[tokio::test]
    async fn f_cd_15_1_two_worktrees_for_the_same_task_nest_under_the_repo() {
        let fx = fixture().await;
        let a = create(&fx.store, &fx.git, &fx.repo.id, "fix auth")
            .await
            .expect("a");
        let b = create(&fx.store, &fx.git, &fx.repo.id, "fix auth")
            .await
            .expect("b");
        assert_eq!(a.lane_branch.as_deref(), Some("tandem/fix-auth"));
        assert_eq!(b.lane_branch.as_deref(), Some("tandem/fix-auth-2"));
        assert_eq!(a.parent_id.as_ref(), Some(&fx.repo.id));
        assert!(Path::new(&a.root).ends_with(".tandem/worktrees/fix-auth"));
        let exclude = std::fs::read_to_string(Path::new(&fx.repo.root).join(".git/info/exclude"))
            .expect("exclude");
        assert_eq!(exclude.lines().filter(|l| *l == ".tandem/").count(), 1);
        // Each worktree has its own tab, and the rail list carries both.
        assert_eq!(tabs::list(&fx.store, &a.id).await.expect("tabs").len(), 1);
        let rows = crate::services::workspaces::list(&fx.store, &PtyManager::new())
            .await
            .expect("list");
        let children: Vec<_> = rows
            .iter()
            .filter(|r| r.workspace.parent_id.as_ref() == Some(&fx.repo.id))
            .map(|r| r.branch.clone().unwrap_or_default())
            .collect();
        assert_eq!(children, vec!["tandem/fix-auth", "tandem/fix-auth-2"]);
        // One made from a worktree sits beside it, under the repository.
        let c = create(&fx.store, &fx.git, &a.id, "")
            .await
            .expect("from a worktree");
        assert_eq!(c.parent_id.as_ref(), Some(&fx.repo.id));
        assert_eq!(c.lane_branch.as_deref(), Some("tandem/session"));
    }

    #[tokio::test]
    async fn f_cd_15_a_folder_that_isnt_a_repo_says_so() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let plain = dir.path().join("plain");
        std::fs::create_dir_all(&plain).expect("mkdir");
        let ws = crate::services::workspaces::add(&store, plain.to_str().expect("utf8"), None)
            .await
            .expect("add");
        let login = LoginEnv {
            vars: [(
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            )]
            .into(),
            shell: "/bin/sh".into(),
            degraded: false,
        };
        let git = git_for(&data, &login).expect("git");
        let err = create(&store, &git, &ws.workspace.id, "x")
            .await
            .expect_err("not a repo");
        assert_eq!(err.message, NOT_A_REPO);
        assert!(
            workspaces::lanes(&store, &ws.workspace.id)
                .await
                .expect("lanes")
                .is_empty(),
            "nothing was registered"
        );
    }

    #[tokio::test]
    async fn f_cd_15_2_dirty_removal_needs_force_and_only_a_local_person_can_force() {
        let fx = fixture().await;
        let lane = create(&fx.store, &fx.git, &fx.repo.id, "fix auth")
            .await
            .expect("lane");
        std::fs::write(Path::new(&lane.root).join("wip.txt"), "draft\n").expect("dirty");
        let err = remove(&fx.store, &PtyManager::new(), &fx.git, &lane.id, None)
            .await
            .expect_err("dirty");
        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(err.message, "Uncommitted changes");
        assert!(Path::new(&lane.root).is_dir(), "nothing was removed");
        // Voice and the phone can't produce the confirmation (TL-06 rule 5).
        for origin in [Origin::Conductor, Origin::Phone, Origin::Unattended] {
            assert!(LocalReview::check(&origin).is_none());
        }
        let local = LocalReview::check(&Origin::main_window()).expect("main window");
        remove(
            &fx.store,
            &PtyManager::new(),
            &fx.git,
            &lane.id,
            Some(ForceConfirm::new(local)),
        )
        .await
        .expect("forced");
        assert!(!Path::new(&lane.root).exists());
        assert!(workspaces::lanes(&fx.store, &fx.repo.id)
            .await
            .expect("lanes")
            .is_empty());
        let branches = git_cmd(Path::new(&fx.repo.root), &["branch", "--list", "tandem/*"]);
        assert!(branches.contains("tandem/fix-auth"), "the branch stays");
    }

    #[tokio::test]
    async fn f_cd_15_3_merge_conflict_is_reported_without_a_commit() {
        let fx = fixture().await;
        let root = Path::new(&fx.repo.root).to_path_buf();
        let lane = create(&fx.store, &fx.git, &fx.repo.id, "edit app")
            .await
            .expect("lane");
        std::fs::write(Path::new(&lane.root).join("app.txt"), "lane\n").expect("write");
        git_cmd(Path::new(&lane.root), &["commit", "-qam", "lane edit"]);
        std::fs::write(root.join("app.txt"), "main\n").expect("write");
        git_cmd(&root, &["commit", "-qam", "main edit"]);
        let head = git_cmd(&root, &["rev-parse", "HEAD"]);
        let report = merge(&fx.store, &fx.git, &lane.id)
            .await
            .expect("merge ran");
        assert!(!report.merged);
        assert_eq!(report.conflicts, vec!["app.txt".to_string()]);
        assert!(report.commit.is_none());
        assert_eq!(
            git_cmd(&root, &["rev-parse", "HEAD"]),
            head,
            "no commit was made"
        );
        // The diff shows the worktree's work against its base.
        let files = diff(&fx.store, &lane.id).await.expect("diff");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "app.txt");
    }

    #[tokio::test]
    async fn f_cd_15_merge_refuses_a_parent_with_uncommitted_changes() {
        let fx = fixture().await;
        let lane = create(&fx.store, &fx.git, &fx.repo.id, "x")
            .await
            .expect("lane");
        std::fs::write(Path::new(&fx.repo.root).join("app.txt"), "edited\n").expect("dirty");
        let err = merge(&fx.store, &fx.git, &lane.id)
            .await
            .expect_err("dirty parent");
        assert_eq!(err.code, ErrorCode::Conflict);
        assert!(err
            .message
            .ends_with("has uncommitted changes. Commit or stash them, then merge."));
    }

    #[test]
    fn f_cd_15_sweep_raises_only_new_or_changed_lists() {
        let mem = SweepMemory::default();
        let ws = WorkspaceId::new();
        let item = |branches: &[&str]| StaleLanes {
            workspace_id: ws.clone(),
            lanes: branches
                .iter()
                .map(|b| StaleLane {
                    branch: b.to_string(),
                    path: format!("/r/{b}"),
                    idle_days: 9,
                })
                .collect(),
        };
        assert_eq!(mem.changed(&[item(&["tandem/a"])]).len(), 1);
        assert!(mem.changed(&[item(&["tandem/a"])]).is_empty(), "unchanged");
        assert_eq!(mem.changed(&[item(&["tandem/a", "tandem/b"])]).len(), 1);
        assert!(mem.changed(&[]).is_empty());
        assert_eq!(
            mem.changed(&[item(&["tandem/a"])]).len(),
            1,
            "forgotten after a clean sweep"
        );
        assert_eq!(cleanup_title(1), "1 finished worktree can be cleaned up");
        assert_eq!(cleanup_title(3), "3 finished worktrees can be cleaned up");
    }

    /// `shop` holding `api` and `web`, each with one commit.
    async fn multi_fixture() -> (tempfile::TempDir, Store, Git, Workspace, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let root = std::fs::canonicalize(dir.path())
            .expect("canonical")
            .join("shop");
        for name in ["api", "web"] {
            let repo = root.join(name);
            std::fs::create_dir_all(repo.join("src")).expect("mkdir");
            git_cmd(&repo, &["init", "-q", "-b", "main"]);
            std::fs::write(repo.join("src/app.ts"), format!("// {name}\n")).expect("write");
            git_cmd(&repo, &["add", "."]);
            git_cmd(&repo, &["commit", "-qm", "first"]);
        }
        let login = LoginEnv {
            vars: [(
                "PATH".to_string(),
                std::env::var("PATH").unwrap_or_default(),
            )]
            .into(),
            shell: "/bin/sh".into(),
            degraded: false,
        };
        let git = git_for(&data, &login).expect("git on PATH");
        let added = crate::services::workspaces::add(&store, root.to_str().expect("utf8"), Some(3))
            .await
            .expect("add");
        (dir, store, git, added.workspace, root)
    }

    #[test]
    fn f_cd_22_3_patches_name_their_repository() {
        let patch = "diff --git a/src/app.ts b/src/app.ts\nindex 1..2 100644\n--- a/src/app.ts\n+++ b/src/app.ts\n@@ -1 +1 @@\n--- a/not a header\n+x\n";
        assert_eq!(
            prefix_patch(patch, "web"),
            "diff --git a/web/src/app.ts b/web/src/app.ts\nindex 1..2 100644\n--- a/web/src/app.ts\n+++ b/web/src/app.ts\n@@ -1 +1 @@\n--- a/not a header\n+x\n"
        );
        let renamed = "diff --git a/a.ts b/b.ts\nrename from a.ts\nrename to b.ts\n";
        assert_eq!(
            prefix_patch(renamed, "api"),
            "diff --git a/api/a.ts b/api/b.ts\nrename from api/a.ts\nrename to api/b.ts\n"
        );
    }

    #[tokio::test]
    async fn f_cd_22_1_2_a_worktree_across_both_repositories() {
        let (_d, store, git, ws, root) = multi_fixture().await;
        assert_eq!(
            crate::services::workspaces::multi_repos(&root),
            vec!["api".to_string(), "web".to_string()]
        );
        // D-869: free for everyone, no plan check.
        let lane = create(&store, &git, &ws.id, "tidy").await.expect("create");
        assert_eq!(lane.lane_branch.as_deref(), Some("tandem/tidy"));
        assert_eq!(Path::new(&lane.root), root.join(".tandem/worktrees/tidy"));
        for name in ["api", "web"] {
            let wt = root.join(".tandem/worktrees/tidy").join(name);
            assert_eq!(git_cmd(&wt, &["branch", "--show-current"]), "tandem/tidy");
        }
        let rows = crate::services::workspaces::list(&store, &PtyManager::new())
            .await
            .expect("list");
        let top = rows.iter().find(|r| r.workspace.id == ws.id).expect("top");
        assert_eq!(
            top.repos.as_deref(),
            Some(&["api".to_string(), "web".to_string()][..])
        );
        assert_eq!(top.branch, None);
        let child = rows
            .iter()
            .find(|r| r.workspace.id == lane.id)
            .expect("lane");
        assert_eq!(child.repos.as_ref().map(Vec::len), Some(2));
        // What needs one repository says so.
        let e = merge(&store, &git, &lane.id).await.expect_err("merge");
        assert_eq!(e.message, MULTI_NOT_YET);
    }

    #[tokio::test]
    async fn f_cd_22_3_5_the_diff_names_each_repository_and_discard_keeps_the_branches() {
        let (_d, store, git, ws, root) = multi_fixture().await;
        let lane = create(&store, &git, &ws.id, "tidy").await.expect("create");
        let parent = PathBuf::from(&lane.root);
        std::fs::write(parent.join("web/src/app.ts"), "// web\nchanged\n").expect("write");
        std::fs::write(parent.join("api/new.ts"), "x\n").expect("write");
        let files = diff(&store, &lane.id).await.expect("diff");
        let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        paths.sort_unstable();
        assert_eq!(paths, ["api/new.ts", "web/src/app.ts"]);
        // Dirty: refused; then forced.
        let e = remove(&store, &PtyManager::new(), &git, &lane.id, None)
            .await
            .expect_err("dirty");
        assert_eq!(e.code, ErrorCode::Conflict);
        let force =
            ForceConfirm::new(LocalReview::check(&Origin::main_window()).expect("main window"));
        remove(&store, &PtyManager::new(), &git, &lane.id, Some(force))
            .await
            .expect("forced");
        assert!(!parent.exists());
        for name in ["api", "web"] {
            assert_eq!(
                git_cmd(&root.join(name), &["branch", "--list", "tandem/tidy"]),
                "tandem/tidy"
            );
        }
        assert!(
            !workspaces::list(&store)
                .await
                .expect("list")
                .iter()
                .any(|w| w.id == lane.id),
            "the row is removed"
        );
    }

    #[tokio::test]
    async fn f_cd_22_review_setup_refuses_up_front_and_a_handoff_starts_from_the_worktree() {
        let (d, store, git, ws, root) = multi_fixture().await;
        let data = DataDir::at(d.path().join("data")).expect("data");
        let lane = create(&store, &git, &ws.id, "tidy").await.expect("create");
        // Setup doesn't apply: lane_setup says nothing, and the folder's
        // setup is refused before anything reads git.
        assert!(
            crate::services::worktree_setup::lane_setup(&store, &data, &lane.id)
                .await
                .expect("lane_setup")
                .is_none()
        );
        let e = crate::services::worktree_setup::get(&store, &data, &ws.id)
            .await
            .expect_err("setup");
        assert_eq!(e.message, MULTI_NOT_YET);
        let e = crate::services::merge_queue::repo_of(&store, &lane.id)
            .await
            .expect_err("landing");
        assert_eq!(e.message, MULTI_NOT_YET);
        // A commit in the worktree's api, then a new worktree made from it.
        let api = PathBuf::from(&lane.root).join("api");
        std::fs::write(api.join("more.ts"), "x\n").expect("write");
        git_cmd(&api, &["add", "."]);
        git_cmd(
            &api,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                "more",
            ],
        );
        let head = git_cmd(&api, &["rev-parse", "HEAD"]);
        let next = create(&store, &git, &lane.id, "next")
            .await
            .expect("from the worktree");
        let next_api = PathBuf::from(&next.root).join("api");
        assert_eq!(git_cmd(&next_api, &["rev-parse", "HEAD"]), head);
        let rows = lane_repos::list(&store, &next.id).await.expect("rows");
        assert_eq!(
            Path::new(&rows[0].repo_root),
            std::fs::canonicalize(root.join("api")).expect("api")
        );
    }

    #[tokio::test]
    async fn f_cd_22_review_discard_survives_a_missing_repository_folder_and_forgets_its_rows() {
        let (_d, store, git, ws, _root) = multi_fixture().await;
        let lane = create(&store, &git, &ws.id, "tidy").await.expect("create");
        let parent = PathBuf::from(&lane.root);
        // Someone deleted web's worktree by hand.
        std::fs::remove_dir_all(parent.join("web")).expect("rm");
        remove(&store, &PtyManager::new(), &git, &lane.id, None)
            .await
            .expect("discard without force");
        assert!(!parent.exists());
        assert!(lane_repos::list(&store, &lane.id)
            .await
            .expect("rows")
            .is_empty());
        // A new worktree has exactly its own repositories.
        let again = create(&store, &git, &ws.id, "tidy").await.expect("again");
        assert_eq!(
            lane_repos::list(&store, &again.id)
                .await
                .expect("rows")
                .len(),
            2
        );
        // A stray file in the parent folder: Conflict, so Discard can force.
        std::fs::write(PathBuf::from(&again.root).join("notes.txt"), "x").expect("write");
        let e = remove(&store, &PtyManager::new(), &git, &again.id, None)
            .await
            .expect_err("stray file");
        assert_eq!(e.code, ErrorCode::Conflict);
    }
}

/// One sibling worktree that changed some of the same files (F-WT-03).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SiblingOverlap {
    pub lane_id: WorkspaceId,
    pub branch: String,
    pub files: Vec<String>,
    /// F-WT-10: symbols both touch, from their tree-sitter indexes.
    pub symbols: Vec<tandem_git::symbols::SharedSymbol>,
}

/// `lane_overlap`: shared files with sibling worktrees, and the forecast of
/// merging this worktree's branch into its repository's HEAD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OverlapReport {
    /// Files this worktree changed since its base, uncommitted included.
    pub files: u32,
    pub siblings: Vec<SiblingOverlap>,
    pub forecast: tandem_git::overlap::Forecast,
    /// Uncommitted changes in this worktree, which the forecast can't see.
    pub uncommitted: u32,
    /// F-WT-10: changed files not checked for symbols (other languages,
    /// too big, past the cap).
    pub unparsed: u32,
}

/// `lane_overlap` (F-WT-03). Reads only: the forecast merges in memory.
pub async fn overlap(
    store: &Store,
    git: &Git,
    id: &WorkspaceId,
) -> Result<OverlapReport, IpcError> {
    use tandem_git::overlap::{changed_files, forecast, shared};
    let (lane, parent) = lane_row(store, id).await?;
    single_repo(store, &lane).await?;
    let lane_repo = discover(&lane.root, FOLDER_MISSING)?;
    let base = lane.lane_base.clone().unwrap_or_default();
    let mine = changed_files(git, lane_repo.root(), &base).await?;
    // F-WT-10: this worktree's symbols, parsed now.
    let my_symbols = crate::services::radar::refresh(git, &lane).await?;
    let mut siblings = Vec::new();
    for other in workspaces::lanes(store, &parent.id).await? {
        if other.id == lane.id || !Path::new(&other.root).is_dir() {
            continue;
        }
        let (Some(branch), Some(other_base)) = (other.lane_branch.clone(), other.lane_base.clone())
        else {
            continue;
        };
        let Ok(Some(other_repo)) = Repo::discover(Path::new(&other.root)) else {
            continue;
        };
        let theirs = changed_files(git, other_repo.root(), &other_base).await?;
        let files = shared(&mine, &theirs);
        let symbols = match crate::services::radar::refresh(git, &other).await {
            Ok(their_symbols) => crate::services::radar::shared(&my_symbols, &their_symbols),
            Err(_) => Vec::new(),
        };
        if !files.is_empty() || !symbols.is_empty() {
            siblings.push(SiblingOverlap {
                lane_id: other.id.clone(),
                branch,
                files,
                symbols,
            });
        }
    }
    let parent_repo = discover(&parent.root, "Folder not found")?;
    let branch = lane.lane_branch.clone().unwrap_or_default();
    let forecast = forecast(git, &parent_repo.main_root(), "HEAD", &branch).await?;
    let status = lane_repo.status().await?;
    let uncommitted: BTreeSet<String> = status
        .staged
        .iter()
        .chain(status.unstaged.iter())
        .map(|f| f.path.clone())
        .collect();
    tracing::info!(
        code = "lane.overlap",
        siblings = siblings.len(),
        "worktree overlap checked"
    );
    Ok(OverlapReport {
        files: u32::try_from(mine.len()).unwrap_or(u32::MAX),
        siblings,
        forecast,
        uncommitted: u32::try_from(uncommitted.len()).unwrap_or(u32::MAX),
        unparsed: my_symbols.unparsed,
    })
}

#[cfg(test)]
mod forge_tests {
    use super::*;

    #[test]
    fn f_wt_04_1_gitlab_remotes_get_merge_requests() {
        for url in [
            "git@gitlab.com:team/app.git",
            "https://gitlab.com/team/app.git",
            "ssh://git@gitlab.example.org:2222/team/app.git",
            "https://user@GitLab.internal/team/app",
        ] {
            assert_eq!(forge_for(Some(url)), Forge::Gitlab, "{url}");
        }
        assert_eq!(MR_COMMAND, "glab mr create");
    }

    #[test]
    fn f_wt_04_2_other_remotes_keep_pull_requests() {
        for url in [
            "https://github.com/team/app.git",
            "git@github.com:team/app.git",
            "/srv/git/app.git",
            "git@example.com:gitlab-mirror/app.git",
        ] {
            assert_eq!(forge_for(Some(url)), Forge::Github, "{url}");
        }
        assert_eq!(forge_for(None), Forge::Github);
        assert_eq!(
            remote_host("ssh://git@host.example:22/x").as_deref(),
            Some("host.example")
        );
    }
}
