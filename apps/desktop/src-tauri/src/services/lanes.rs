//! Worktree lanes (F-CD-15, ARCHITECTURE §7). Each lane is a child
//! workspace on its own `tandem/<slug>` branch under
//! `<repo>/.tandem/worktrees/<slug>`. Copy the user sees says "worktree";
//! "lane" is the internal word (UI_SPEC §6.2).

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::path::Path;
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
use tandem_store::repo::panes::{self, PaneRecord};
use tandem_store::repo::tabs;
use tandem_store::repo::workspaces::{self, NewLane, Workspace};
use tandem_store::Store;

pub const NOT_A_REPO: &str = "Worktrees need a git repository";
pub const GIT_MISSING: &str = "Git isn't installed";
pub const GH_MISSING: &str = "GitHub CLI (gh) isn't installed";
pub const FOLDER_MISSING: &str = "The worktree folder is missing";
/// The sweep runs at startup and then this often.
pub const SWEEP_EVERY: Duration = Duration::from_secs(6 * 3600);
/// "Create pull request" types into its shell once the prompt is up.
pub const PR_COMMAND: &str = "gh pr create";
pub const PR_READY_TIMEOUT: Duration = Duration::from_secs(30);

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
    let repo = discover(&ws.root, "Folder not found")?;
    let lane = LaneManager::create(git, &repo, slug_source, &HashSet::new()).await?;
    let root = lane.path.to_str().map(str::to_string).ok_or_else(|| {
        IpcError::new(
            ErrorCode::Invalid,
            "Tandem can't open folders whose names aren't valid UTF-8",
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
    Ok(row)
}

/// `lane_diff`: the worktree's changes against the commit it started from,
/// including uncommitted work.
pub async fn diff(store: &Store, id: &WorkspaceId) -> Result<Vec<tandem_git::FileDiff>, IpcError> {
    let (lane, _) = lane_row(store, id).await?;
    let repo = discover(&lane.root, FOLDER_MISSING)?;
    let base = lane.lane_base.unwrap_or_default();
    Ok(repo.diff_since(&base).await?)
}

/// `lane_merge`: `git merge --no-ff` in the parent checkout. A conflict stops
/// it and is reported; nothing is committed and nothing is resolved.
pub async fn merge(store: &Store, git: &Git, id: &WorkspaceId) -> Result<MergeReport, IpcError> {
    let (lane, parent) = lane_row(store, id).await?;
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
    let path = login.vars.get("PATH").map(String::as_str).unwrap_or("");
    let pathext = login.vars.get("PATHEXT").map(String::as_str);
    resolve_on_path("gh", path, pathext).is_some()
}

/// `lane_create_pr`: a shell pane in the worktree, in `tab`. The caller
/// types [`PR_COMMAND`] once the view has started it.
pub async fn create_pr_pane(
    store: &Store,
    login: &LoginEnv,
    id: &WorkspaceId,
    tab: &TabId,
) -> Result<PaneRecord, IpcError> {
    let (lane, _) = lane_row(store, id).await?;
    if !Path::new(&lane.root).is_dir() {
        return Err(IpcError::new(ErrorCode::NotFound, FOLDER_MISSING));
    }
    if !gh_available(login) {
        return Err(IpcError::new(ErrorCode::EngineMissing, GH_MISSING).with_stage("gh"));
    }
    Ok(panes::create_with(store, tab, "terminal", Some(lane.root.clone()), None).await?)
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
    let (lane, parent) = lane_row(store, id).await?;
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
    let repo = discover(&parent.root, "Folder not found")?;
    LaneManager::remove(git, &repo, lane_path, force).await?;
    workspaces::remove(store, &lane.id).await?;
    tracing::info!(code = "lane.removed", "worktree removed");
    Ok(())
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
}
