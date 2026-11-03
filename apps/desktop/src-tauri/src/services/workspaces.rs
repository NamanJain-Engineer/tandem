//! Workspaces and tabs for the Code rail (F-CD-01, M2 slice): add with
//! dedupe by canonical path, the Free limit of 3, soft remove, and tabs.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::ids::{PaneId, WorkspaceId};
use tandem_core::redact::Redactor;
use tandem_core::{ErrorCode, IpcError};
use tandem_pty::PtyManager;
use tandem_store::repo::lane_repos;
use tandem_store::repo::panes;
use tandem_store::repo::tabs::{self, Tab};
use tandem_store::repo::workspaces::{self, Added, NewWorkspace, Workspace};
use tandem_store::{Store, StoreError};

/// `workspace_list` rows: the workspace plus what the rail shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkspaceSummary {
    #[serde(flatten)]
    #[ts(flatten)]
    pub workspace: Workspace,
    /// The folder no longer exists (warning row; panes can't launch).
    pub missing: bool,
    pub branch: Option<String>,
    pub live_count: u32,
    pub tabs: Vec<Tab>,
    /// F-CD-22: a multi-repo folder's repositories, or a multi-repo
    /// worktree's; absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub repos: Option<Vec<String>>,
    /// F-CD-22: why a folder of repositories isn't multi-repo (too many).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub repos_note: Option<String>,
    /// F-CD-24: `jj` for a jj workspace; absent for a git worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lane_vcs: Option<String>,
    /// F-CD-24: a repository with `.jj` and no `.git` (no jj workspaces).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub jj_only: Option<bool>,
}

/// `workspace_add` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AddedWorkspace {
    pub workspace: Workspace,
    /// False when the folder was already a workspace (it's focused instead).
    pub created: bool,
}

/// Canonical absolute path for `root`, or a user-facing error.
pub fn canonical_root(root: &str) -> Result<PathBuf, IpcError> {
    let path = Path::new(root);
    if !path.is_absolute() {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Choose a folder with a full path",
        ));
    }
    let canonical = tandem_core::contain::canonical(path).map_err(|_| {
        IpcError::new(
            ErrorCode::NotFound,
            format!("Folder not found: {}", Redactor::global().redact_home(root)),
        )
    })?;
    if !canonical.is_dir() {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "That's a file, not a folder",
        ));
    }
    Ok(canonical)
}

pub async fn add(
    store: &Store,
    root: &str,
    max_workspaces: Option<u32>,
) -> Result<AddedWorkspace, IpcError> {
    let canonical = canonical_root(root)?;
    let root = canonical
        .to_str()
        .ok_or_else(|| {
            IpcError::new(
                ErrorCode::Invalid,
                "Vibezeo can't open folders whose names aren't valid UTF-8",
            )
        })?
        .to_string();
    let name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.clone());
    let new = NewWorkspace {
        kind: "local".into(),
        host_id: "local".into(),
        root,
        name,
        wsl_distro: None,
    };
    let (workspace, how) =
        workspaces::add(store, new, max_workspaces)
            .await
            .map_err(|e| match e {
                StoreError::LimitReached(msg) => {
                    IpcError::new(ErrorCode::LimitReached, msg).with_stage("workspaces")
                }
                other => other.into(),
            })?;
    if tabs::list(store, &workspace.id).await?.is_empty() {
        tabs::create(store, &workspace.id, None).await?;
    }
    // A folder used before, a removed worktree's among them, gets its old
    // row and id back, which the radar fenced off for good when it went
    // (the review of c30bb8bf, N-5).
    if how != Added::Existing {
        crate::services::radar::made(workspace.id.as_str(), workspace.lane_base.as_deref());
    }
    tracing::info!(
        code = "workspace.added",
        created = how == Added::Created,
        "workspace added"
    );
    Ok(AddedWorkspace {
        workspace,
        created: how != Added::Existing,
    })
}

/// An SSH or WSL workspace (F-CD-01 kinds): the host already said `root`
/// exists and gave its real path.
pub async fn add_remote(
    store: &Store,
    kind: &str,
    host: &tandem_core::ids::HostId,
    root: &str,
    wsl_distro: Option<String>,
    max_workspaces: Option<u32>,
) -> Result<AddedWorkspace, IpcError> {
    if !root.starts_with('/') {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "Choose a folder with a full path",
        ));
    }
    let name = root
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(root)
        .to_string();
    let new = NewWorkspace {
        kind: kind.into(),
        host_id: host.as_str().to_string(),
        root: root.to_string(),
        name,
        wsl_distro,
    };
    let (workspace, how) =
        workspaces::add(store, new, max_workspaces)
            .await
            .map_err(|e| match e {
                StoreError::LimitReached(msg) => {
                    IpcError::new(ErrorCode::LimitReached, msg).with_stage("workspaces")
                }
                other => other.into(),
            })?;
    if tabs::list(store, &workspace.id).await?.is_empty() {
        tabs::create(store, &workspace.id, None).await?;
    }
    // A folder used before, a removed worktree's among them, gets its old
    // row and id back, which the radar fenced off for good when it went
    // (the review of c30bb8bf, N-5).
    if how != Added::Existing {
        crate::services::radar::made(workspace.id.as_str(), workspace.lane_base.as_deref());
    }
    tracing::info!(
        code = "workspace.added",
        kind,
        created = how == Added::Created,
        "workspace added"
    );
    Ok(AddedWorkspace {
        workspace,
        created: how != Added::Existing,
    })
}

pub async fn list(store: &Store, pty: &PtyManager) -> Result<Vec<WorkspaceSummary>, IpcError> {
    let mut out = Vec::new();
    let jj = tandem_store::repo::lane_vcs::jj_lanes(store).await?;
    for ws in workspaces::list(store).await? {
        let tabs = tabs::list(store, &ws.id).await?;
        let mut live = 0;
        for tab in &tabs {
            live += panes::list_open(store, &tab.id)
                .await?
                .iter()
                .filter(|p| pty.is_live(p.id.as_str()))
                .count() as u32;
        }
        let root = Path::new(&ws.root);
        // A remote folder is checked when its host is reached.
        let local = ws.kind == "local";
        let is_jj = jj.contains(&ws.id);
        // A jj workspace has no `.git`: git would read the repository's.
        let branch = if local && !is_jj {
            git_branch(root)
        } else {
            None
        };
        let jj_only = local && ws.parent_id.is_none() && tandem_git::jj::jj_without_git(root);
        let mut repos_note = None;
        let repos = if !local {
            Vec::new()
        } else if ws.parent_id.is_some() {
            lane_repos::list(store, &ws.id)
                .await?
                .into_iter()
                .map(|r| r.name)
                .collect()
        } else if branch.is_none() {
            match detect_multi_repo(root) {
                MultiRepo::Repos(r) => r,
                MultiRepo::TooMany(n) => {
                    repos_note = Some(too_many_repos(n));
                    Vec::new()
                }
                MultiRepo::No => Vec::new(),
            }
        } else {
            Vec::new()
        };
        out.push(WorkspaceSummary {
            missing: local && !root.is_dir(),
            branch,
            live_count: live,
            tabs,
            repos: (!repos.is_empty()).then_some(repos),
            repos_note,
            lane_vcs: is_jj.then(|| tandem_store::repo::lane_vcs::JJ.to_string()),
            jj_only: jj_only.then_some(true),
            workspace: ws,
        });
    }
    Ok(out)
}

/// What [`remove`] took away: the panes it closed, and the folders of the
/// workspace and its worktrees.
#[derive(Debug, Default)]
pub struct Removed {
    pub panes: Vec<PaneId>,
    pub folders: Vec<String>,
}

/// Soft remove. Its worktrees' rows go with it, so their panes stop too;
/// nothing on disk is touched. [`end_removed`] comes next.
pub async fn remove(
    store: &Store,
    terms: &crate::services::terminal::TerminalService,
    id: &WorkspaceId,
) -> Result<Removed, IpcError> {
    let mut removed = Removed {
        folders: vec![workspaces::get(store, id).await?.root],
        ..Removed::default()
    };
    let mut ids = vec![id.clone()];
    for lane in workspaces::lanes(store, id).await? {
        removed.folders.push(lane.root);
        ids.push(lane.id);
    }
    for ws in &ids {
        for tab in tabs::list(store, ws).await? {
            for pane in panes::list_open(store, &tab.id).await? {
                let _ = terms.end_pane(pane.id.as_str()).await;
                panes::close(store, &pane.id).await?;
                removed.panes.push(pane.id);
            }
        }
    }
    workspaces::remove(store, id).await?;
    // The radar's indexes and watchers go with them (the review of
    // 9a4e93ab, S6), and no parse of them is kept again (d06b9e3d, N-1).
    for ws in &ids {
        crate::services::radar::forget_for_good(ws.as_str());
    }
    Ok(removed)
}

/// After [`remove`] (F-TK-05): the closed panes' agent sessions end, which
/// revokes their task connections and drops the notices waiting for them,
/// and a main checkout's last session releases its claims (D-2427). The
/// worktrees' claims go too, as nothing will merge or remove them here now
/// (AC3). Blocking file IO.
pub fn end_removed(
    terminals: &crate::services::terminal::TerminalService,
    tasks: &crate::services::tasks::TasksService,
    removed: &Removed,
) {
    for pane in &removed.panes {
        terminals.end_tasks_session(pane.as_str());
    }
    for folder in &removed.folders {
        tasks.release_removed(Path::new(folder));
    }
}

/// At most this many repositories make up a multi-repo folder (F-CD-22).
pub const MAX_REPOS: usize = 12;

/// What F-CD-22's detection found in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultiRepo {
    /// Not a multi-repo folder.
    No,
    /// At least two repositories, by name.
    Repos(Vec<String>),
    /// More than [`MAX_REPOS`]: not treated as multi-repo.
    TooMany(usize),
}

/// The git common directory of the repository checked out at `dir`: its
/// `.git` folder, or for a linked worktree the main repository's.
fn common_dir(dir: &Path) -> Option<PathBuf> {
    let dot = dir.join(".git");
    let meta = std::fs::symlink_metadata(&dot).ok()?;
    let git_dir = if meta.is_dir() {
        dot
    } else if meta.is_file() {
        let text = std::fs::read_to_string(&dot).ok()?;
        let pointer = text.trim().strip_prefix("gitdir:")?.trim();
        let g = dir.join(pointer);
        match std::fs::read_to_string(g.join("commondir")) {
            Ok(c) => g.join(c.trim()),
            Err(_) => g,
        }
    } else {
        return None;
    };
    std::fs::canonicalize(git_dir).ok()
}

/// F-CD-22: the git repositories directly inside `root`, when there are at
/// least two and `root` isn't itself in a repository. Hidden folders and
/// symlinks don't count; nested deeper repositories aren't looked for.
/// Checkouts of one repository (a folder and its sibling worktree) count
/// once, under the first name.
pub fn detect_multi_repo(root: &Path) -> MultiRepo {
    if root.ancestors().any(|a| a.join(".git").exists()) {
        return MultiRepo::No;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return MultiRepo::No;
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    let mut seen = std::collections::BTreeSet::new();
    let repos: Vec<String> = names
        .into_iter()
        .filter(|n| common_dir(&root.join(n)).is_some_and(|c| seen.insert(c)))
        .collect();
    match repos.len() {
        0 | 1 => MultiRepo::No,
        n if n > MAX_REPOS => MultiRepo::TooMany(n),
        _ => MultiRepo::Repos(repos),
    }
}

/// The repositories of a multi-repo folder; empty when it isn't one.
pub fn multi_repos(root: &Path) -> Vec<String> {
    match detect_multi_repo(root) {
        MultiRepo::Repos(r) => r,
        _ => Vec::new(),
    }
}

/// Why a folder of repositories isn't multi-repo, when that's the reason.
pub fn too_many_repos(n: usize) -> String {
    format!(
        "This folder holds {n} repositories, more than {MAX_REPOS}, so its worktrees aren't made across them. Add the repositories you work in on their own."
    )
}

/// The checked-out branch from `.git/HEAD` (worktrees follow `gitdir:`),
/// or a short sha when detached. Full git support arrives with tandem-git.
pub fn git_branch(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let pointer = std::fs::read_to_string(&dot_git).ok()?;
        let dir = pointer.strip_prefix("gitdir:")?.trim();
        let dir = PathBuf::from(dir);
        if dir.is_absolute() {
            dir
        } else {
            root.join(dir)
        }
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => Some(branch.to_string()),
        None if head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit()) => {
            Some(head[..7].to_string())
        }
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::DataDir;

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=T", "-c", "user.email=t@t"])
            .args(args)
            .status()
            .expect("git")
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn f_cd_22_review_checkouts_of_one_repository_count_once_and_too_many_isnt_multi() {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = std::fs::canonicalize(tmp.path()).expect("canonical");
        for name in ["app", "web"] {
            let r = root.join(name);
            std::fs::create_dir_all(&r).expect("mkdir");
            git(&r, &["init", "-q", "-b", "main"]);
            git(&r, &["commit", "-q", "--allow-empty", "-m", "first"]);
        }
        // app's own worktree beside it is the same repository.
        git(
            &root.join("app"),
            &["worktree", "add", "-q", "-b", "hotfix", "../app-hotfix"],
        );
        assert_eq!(
            detect_multi_repo(&root),
            MultiRepo::Repos(vec!["app".into(), "web".into()])
        );
        // Just app and its worktree: one repository, not multi-repo.
        std::fs::remove_dir_all(root.join("web")).expect("rm");
        assert_eq!(detect_multi_repo(&root), MultiRepo::No);
        // More than 12: not multi-repo, with the reason.
        for i in 0..=MAX_REPOS {
            let r = root.join(format!("r{i:02}"));
            std::fs::create_dir_all(&r).expect("mkdir");
            git(&r, &["init", "-q"]);
        }
        assert_eq!(detect_multi_repo(&root), MultiRepo::TooMany(MAX_REPOS + 2));
        assert!(multi_repos(&root).is_empty());
        assert!(too_many_repos(14).contains("more than 12"));
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, store)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f_tk_05_3_removing_a_workspace_ends_its_panes_sessions_and_releases_its_claims() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        // A repository with a worktree, each with an agent's session and
        // the file it edited.
        let main = dir.path().join("app");
        std::fs::create_dir_all(&main).expect("mkdir");
        git(&main, &["init", "-q"]);
        git(
            &main,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        let lane_root = main.join(".tandem/worktrees/fix-auth");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                lane_root.to_str().expect("utf8"),
                "-b",
                "tandem/fix-auth",
            ],
        );
        let lane_root = std::fs::canonicalize(lane_root).expect("canonical");
        let added = add(&store, main.to_str().expect("utf8"), Some(3))
            .await
            .expect("ws");
        let lane = workspaces::add_lane(
            &store,
            workspaces::NewLane {
                parent_id: added.workspace.id.clone(),
                root: lane_root.to_string_lossy().into_owned(),
                name: "fix-auth".into(),
                branch: "tandem/fix-auth".into(),
                base: "main".into(),
            },
        )
        .await
        .expect("lane");
        let tasks = crate::services::tasks::TasksService::new(&data, store.clone(), None);
        let terminals =
            crate::services::terminal::TerminalService::new(PtyManager::new(), store.clone());
        terminals.bind_tasks(&tasks);
        let mut opened = Vec::new();
        for (ws, root) in [
            (&added.workspace.id, Path::new(&added.workspace.root)),
            (&lane.id, lane_root.as_path()),
        ] {
            let tab = tabs::create(&store, ws, None).await.expect("tab");
            let pane = panes::create(&store, &tab.id, "terminal", None)
                .await
                .expect("pane");
            let id = pane.id.as_str().to_string();
            tasks.track_scope(&format!("pn:{id}"), Some(root));
            tasks.claim_edits(&id, &[root.join("a.ts").to_string_lossy().into_owned()]);
            opened.push(id);
        }
        // A notice waits for the worktree's pane, whose process never started.
        terminals.notice(&opened[1], "app and fix-auth both claim a.ts");
        let tracker = tasks.tracker.clone().expect("tracker");
        assert_eq!(tracker.claims().expect("claims").len(), 2);
        let removed = remove(&store, &terminals, &added.workspace.id)
            .await
            .expect("remove");
        let mut closed: Vec<String> = removed
            .panes
            .iter()
            .map(|p| p.as_str().to_string())
            .collect();
        closed.sort();
        opened.sort();
        assert_eq!(closed, opened);
        end_removed(&terminals, &tasks, &removed);
        assert!(
            tracker.claims().expect("claims").is_empty(),
            "the main checkout's went with its last session, the worktree's with it"
        );
        for root in [Path::new(&added.workspace.root), lane_root.as_path()] {
            let folder = crate::services::tasks::claim_scope(root)
                .expect("scope")
                .folder;
            assert!(tasks.panes_in(&folder).is_empty(), "{folder}");
        }
        for pane in &opened {
            assert!(terminals.held_notices_of(pane).is_empty(), "{pane}");
        }
    }

    #[tokio::test]
    async fn f_cd_01_1_same_folder_by_another_spelling_is_one_workspace() {
        let (dir, store) = store();
        let ws = dir.path().join("proj");
        std::fs::create_dir_all(ws.join("sub")).expect("mkdir");
        let a = add(&store, ws.to_str().expect("utf8"), Some(3))
            .await
            .expect("add");
        assert!(a.created);
        let other = format!("{}/sub/..", ws.display());
        let b = add(&store, &other, Some(3)).await.expect("again");
        assert!(!b.created);
        assert_eq!(a.workspace.id, b.workspace.id);
        let rows = list(&store, &PtyManager::new()).await.expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tabs.len(), 1, "a new workspace starts with one tab");
        assert!(!rows[0].missing);
    }

    #[tokio::test]
    async fn f_bl_04_fourth_workspace_hits_the_free_limit() {
        let (dir, store) = store();
        for n in 1..=4 {
            std::fs::create_dir_all(dir.path().join(format!("w{n}"))).expect("mkdir");
        }
        for n in 1..=3 {
            add(
                &store,
                dir.path().join(format!("w{n}")).to_str().expect("utf8"),
                Some(3),
            )
            .await
            .expect("ok");
        }
        let err = add(
            &store,
            dir.path().join("w4").to_str().expect("utf8"),
            Some(3),
        )
        .await
        .expect_err("limit");
        assert_eq!(err.code, ErrorCode::LimitReached);
        add(&store, dir.path().join("w4").to_str().expect("utf8"), None)
            .await
            .expect("pro");
    }

    #[tokio::test]
    async fn missing_and_relative_folders_are_refused() {
        let (dir, store) = store();
        let err = add(
            &store,
            dir.path().join("nope").to_str().expect("utf8"),
            None,
        )
        .await
        .expect_err("missing");
        assert_eq!(err.code, ErrorCode::NotFound);
        assert!(err.message.starts_with("Folder not found"));
        let err = add(&store, "relative/dir", None)
            .await
            .expect_err("relative");
        assert_eq!(err.code, ErrorCode::Invalid);
    }

    #[test]
    fn reads_the_branch_from_head() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(git_branch(dir.path()), None);
        std::fs::create_dir_all(dir.path().join(".git")).expect("mkdir");
        std::fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/feature/x\n").expect("write");
        assert_eq!(git_branch(dir.path()).as_deref(), Some("feature/x"));
        std::fs::write(
            dir.path().join(".git/HEAD"),
            "0123456789abcdef0123456789abcdef01234567\n",
        )
        .expect("write");
        assert_eq!(git_branch(dir.path()).as_deref(), Some("0123456"));
    }
}
