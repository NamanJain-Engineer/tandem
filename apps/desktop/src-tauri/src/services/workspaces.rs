//! Workspaces and tabs for the Code rail (F-CD-01, M2 slice): add with
//! dedupe by canonical path, the Free limit of 3, soft remove, and tabs.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::ids::{PaneId, WorkspaceId};
use tandem_core::redact::Redactor;
use tandem_core::{ErrorCode, IpcError};
use tandem_pty::PtyManager;
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
        out.push(WorkspaceSummary {
            missing: local && !root.is_dir(),
            branch: if local { git_branch(root) } else { None },
            live_count: live,
            tabs,
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
    pty: &PtyManager,
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
                let _ = pty.kill(pane.id.as_str());
                panes::close(store, &pane.id).await?;
                removed.panes.push(pane.id);
            }
        }
    }
    workspaces::remove(store, id).await?;
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

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, store)
    }

    fn git(cwd: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .expect("git")
            .success();
        assert!(ok, "git {args:?}");
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
        let removed = remove(&store, &terminals.pty, &added.workspace.id)
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
