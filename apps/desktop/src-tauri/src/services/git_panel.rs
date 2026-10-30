//! The Files tab's Changes view and the status bar's branch (F-CD-20). A
//! context resolves to its folder and the repository around it; the view
//! covers the whole repository (D-205), because commits and pushes do.
//! Reads go through libgit2 ([`Repo`], hardened by construction, TL-06
//! rule 6); actions run the user's git ([`tandem_git::ops`]) one at a time
//! per repository, then drop the cached status the tree's badges use.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tandem_core::error::IpcError;
use tandem_core::ErrorCode;
use tandem_git::cli::Git;
use tandem_git::{ops, Branch, FileDiff, GitError, GitStatus, Repo};
use ts_rs::TS;

use crate::services::files::{FileContext, FilesService};

/// What the Changes view shows for a context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GitPanel {
    /// The repository's folder; the status's paths are relative to it.
    pub root: String,
    /// The context's folder inside the repository, `/`-separated; `""` at
    /// the top.
    pub folder: String,
    pub status: GitStatus,
    /// The SSH host it's on, by name: pushes, pulls and branches run in a
    /// terminal there (D-411).
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Committed {
    pub sha: String,
}

pub struct GitPanelService {
    files: Arc<FilesService>,
    /// One action at a time per repository: git takes `index.lock`.
    locks: Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
}

fn slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// The repository around `folder`, opened at its top. `None` outside one.
fn whole_repo(folder: &Path) -> Result<Option<Repo>, GitError> {
    let Some(repo) = Repo::discover(folder)? else {
        return Ok(None);
    };
    if repo.prefix().as_os_str().is_empty() {
        return Ok(Some(repo));
    }
    Repo::discover(repo.root())
}

impl GitPanelService {
    pub fn new(files: Arc<FilesService>) -> Arc<Self> {
        Arc::new(GitPanelService {
            files,
            locks: Mutex::default(),
        })
    }

    /// The context's folder and its repository.
    async fn open(&self, ctx: &FileContext) -> Result<(PathBuf, Option<Repo>), IpcError> {
        let folder = self.files.root(ctx).await?;
        let path = folder.clone();
        let repo = tokio::task::spawn_blocking(move || whole_repo(&path))
            .await
            .map_err(|e| GitError::Invalid(e.to_string()))??;
        // F-CD-24: a jj workspace has no `.git`; discovery found the
        // repository around it, which git must not change for it.
        if let Some(r) = &repo {
            if tandem_git::jj::in_jj_workspace(&folder, r.root()) {
                return Err(IpcError::new(
                    ErrorCode::Invalid,
                    crate::services::jj_lanes::JJ_NOT_YET,
                )
                .with_stage("jj"));
            }
        }
        Ok((folder, repo))
    }

    async fn repo(&self, ctx: &FileContext) -> Result<Repo, IpcError> {
        match self.open(ctx).await? {
            (_, Some(repo)) => Ok(repo),
            (_, None) => Err(GitError::NotARepo.into()),
        }
    }

    fn lock_for(&self, root: &Path) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        locks.entry(root.to_path_buf()).or_default().clone()
    }

    /// Runs one action on the context's repository, then drops the cached
    /// statuses of every folder in it.
    async fn act<T, F, Fut>(&self, ctx: &FileContext, f: F) -> Result<T, IpcError>
    where
        F: FnOnce(Repo) -> Fut,
        Fut: std::future::Future<Output = Result<T, GitError>>,
    {
        let repo = self.repo(ctx).await?;
        let root = repo.root().to_path_buf();
        let lock = self.lock_for(&root);
        let _turn = lock.lock().await;
        let out = f(repo).await;
        self.files.invalidate_repo(&root);
        Ok(out?)
    }

    /// `git_status`: `None` outside a repository.
    pub async fn status(&self, ctx: &FileContext) -> Result<Option<GitPanel>, IpcError> {
        if let Some((remote, r)) = self.files.remote_of(ctx).await? {
            let label = remote.label(&r.host).await;
            return Ok(remote.files.git_status(&remote, &r).await?.map(
                |(root, folder, status)| GitPanel {
                    root,
                    folder,
                    status,
                    host: Some(label.clone()),
                },
            ));
        }
        let (folder, repo) = self.open(ctx).await?;
        let Some(repo) = repo else {
            return Ok(None);
        };
        let status = repo.status().await?;
        let inside = folder
            .strip_prefix(repo.root())
            .map(slash)
            .unwrap_or_default();
        Ok(Some(GitPanel {
            root: repo.root().to_string_lossy().into_owned(),
            folder: inside,
            status,
            host: None,
        }))
    }

    pub async fn diff(
        &self,
        ctx: &FileContext,
        path: &str,
        staged: bool,
    ) -> Result<FileDiff, IpcError> {
        if let Some((remote, r)) = self.files.remote_of(ctx).await? {
            return remote.files.git_diff(&remote, &r, path, staged).await;
        }
        Ok(self.repo(ctx).await?.diff_file(path, staged).await?)
    }

    pub async fn branches(&self, ctx: &FileContext) -> Result<Vec<Branch>, IpcError> {
        if let Some((remote, r)) = self.files.remote_of(ctx).await? {
            return remote.files.git_branches(&remote, &r).await;
        }
        Ok(self.repo(ctx).await?.branches().await?)
    }

    pub async fn stage(
        &self,
        git: &Git,
        ctx: &FileContext,
        paths: &[String],
    ) -> Result<(), IpcError> {
        if let Some((remote, r)) = self.files.remote_of(ctx).await? {
            return remote
                .files
                .git_op(&remote, &r, "stage", paths, None)
                .await
                .map(|_| ());
        }
        self.act(ctx, |r| async move { ops::stage(git, &r, paths).await })
            .await
    }

    pub async fn unstage(
        &self,
        git: &Git,
        ctx: &FileContext,
        paths: &[String],
    ) -> Result<(), IpcError> {
        if let Some((remote, r)) = self.files.remote_of(ctx).await? {
            return remote
                .files
                .git_op(&remote, &r, "unstage", paths, None)
                .await
                .map(|_| ());
        }
        self.act(ctx, |r| async move { ops::unstage(git, &r, paths).await })
            .await
    }

    /// The UI has confirmed: this can't be undone.
    pub async fn discard(
        &self,
        git: &Git,
        ctx: &FileContext,
        paths: &[String],
    ) -> Result<(), IpcError> {
        if let Some((remote, r)) = self.files.remote_of(ctx).await? {
            return remote
                .files
                .git_op(&remote, &r, "discard", paths, None)
                .await
                .map(|_| ());
        }
        self.act(ctx, |r| async move { ops::discard(git, &r, paths).await })
            .await
    }

    pub async fn commit(
        &self,
        git: &Git,
        ctx: &FileContext,
        message: &str,
    ) -> Result<Committed, IpcError> {
        if let Some((remote, r)) = self.files.remote_of(ctx).await? {
            let sha = remote
                .files
                .git_op(&remote, &r, "commit", &[], Some(message))
                .await?;
            return Ok(Committed {
                sha: sha.unwrap_or_default(),
            });
        }
        let sha = self
            .act(ctx, |r| async move { ops::commit(git, &r, message).await })
            .await?;
        Ok(Committed { sha })
    }

    pub async fn push(&self, git: &Git, ctx: &FileContext, publish: bool) -> Result<(), IpcError> {
        if let Some((remote, r)) = self.files.remote_of(ctx).await? {
            let op = if publish { "publish" } else { "push" };
            return remote.files.git_sync(&remote, &r, op, None).await;
        }
        self.act(ctx, |r| async move { ops::push(git, &r, publish).await })
            .await
    }

    pub async fn pull(&self, git: &Git, ctx: &FileContext) -> Result<(), IpcError> {
        if let Some((remote, r)) = self.files.remote_of(ctx).await? {
            return remote.files.git_sync(&remote, &r, "pull", None).await;
        }
        self.act(ctx, |r| async move { ops::pull(git, &r).await })
            .await
    }

    pub async fn switch(&self, git: &Git, ctx: &FileContext, branch: &str) -> Result<(), IpcError> {
        if let Some((remote, r)) = self.files.remote_of(ctx).await? {
            return remote
                .files
                .git_sync(&remote, &r, "switch", Some(branch))
                .await;
        }
        self.act(ctx, |r| async move { ops::switch(git, &r, branch).await })
            .await
    }

    pub async fn create_branch(
        &self,
        git: &Git,
        ctx: &FileContext,
        name: &str,
    ) -> Result<(), IpcError> {
        if let Some((remote, r)) = self.files.remote_of(ctx).await? {
            return remote
                .files
                .git_sync(&remote, &r, "create_branch", Some(name))
                .await;
        }
        self.act(
            ctx,
            |r| async move { ops::create_branch(git, &r, name).await },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tandem_core::error::ErrorCode;
    use tandem_core::DataDir;
    use tandem_git::FileChange;
    use tandem_store::Store;

    /// Plain git for setting fixtures up (it would spring a trap).
    fn git(cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(cwd)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(["-c", "init.defaultBranch=main"])
            .args(args)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A repository with the identity the panel's commits use.
    fn init(path: &Path) {
        std::fs::create_dir_all(path).expect("mkdir");
        git(path, &["init", "-q"]);
        git(path, &["config", "user.name", "T"]);
        git(path, &["config", "user.email", "t@example.invalid"]);
        git(path, &["config", "commit.gpgsign", "false"]);
    }

    struct Fx {
        dir: tempfile::TempDir,
        store: Store,
        files: Arc<FilesService>,
        panel: Arc<GitPanelService>,
        cli: Git,
    }

    fn fx() -> Fx {
        let dir = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let files = FilesService::new(store.clone());
        let panel = GitPanelService::new(files.clone());
        let cli = Git::locate(None, dir.path().join("empty-hooks")).expect("git on PATH");
        Fx {
            dir,
            store,
            files,
            panel,
            cli,
        }
    }

    impl Fx {
        fn path(&self, rel: &str) -> PathBuf {
            tandem_core::contain::canonical(self.dir.path())
                .expect("canon")
                .join(rel)
        }

        async fn workspace(&self, path: &Path) -> FileContext {
            let ws =
                crate::services::workspaces::add(&self.store, path.to_str().expect("utf8"), None)
                    .await
                    .expect("add");
            FileContext::Workspace {
                workspace_id: ws.workspace.id,
            }
        }
    }

    fn paths(list: &[tandem_git::FileStatus]) -> Vec<&str> {
        let mut out: Vec<&str> = list.iter().map(|f| f.path.as_str()).collect();
        out.sort_unstable();
        out
    }

    /// F-CD-20 AC1 through the app: the Changes view's status polling, its
    /// diffs and branches, and the tree's badges never run a repository's
    /// fsmonitor (nor its clean filter or textconv). The trap is shown
    /// live with plain git afterwards.
    #[cfg(unix)]
    #[tokio::test]
    async fn f_cd_20_1_status_polling_and_the_panels_reads_never_spring_the_repos_traps() {
        let f = fx();
        let root = f.path("trap");
        let marks = f.path("marks");
        std::fs::create_dir_all(&marks).expect("mkdir");
        init(&root);
        std::fs::write(root.join(".gitattributes"), "*.txt filter=trap diff=trap\n")
            .expect("write");
        std::fs::write(root.join("a.txt"), "one\n").expect("write");
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "first"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\n").expect("write");
        std::fs::write(root.join("b.txt"), "bee\n").expect("write");
        git(&root, &["add", "b.txt"]);
        // The traps go in last: plain git would spring them.
        let m = marks.display();
        git(
            &root,
            &["config", "core.fsmonitor", &format!("touch {m}/fsmonitor")],
        );
        git(
            &root,
            &[
                "config",
                "filter.trap.clean",
                &format!("sh -c 'touch {m}/clean; cat'"),
            ],
        );
        git(
            &root,
            &[
                "config",
                "diff.trap.textconv",
                &format!("sh -c 'touch {m}/textconv; cat \"$1\"' --"),
            ],
        );
        let ctx = f.workspace(&root).await;

        for _ in 0..3 {
            let panel = f
                .panel
                .status(&ctx)
                .await
                .expect("status")
                .expect("a repository");
            assert_eq!(paths(&panel.status.staged), vec!["b.txt"]);
            assert_eq!(paths(&panel.status.unstaged), vec!["a.txt"]);
            f.files.invalidate(&root);
            f.files
                .list(&ctx, String::new(), false)
                .await
                .expect("tree");
        }
        let diff = f.panel.diff(&ctx, "a.txt", false).await.expect("diff");
        assert!(diff.patch.contains("+two"), "{}", diff.patch);
        f.panel
            .diff(&ctx, "b.txt", true)
            .await
            .expect("staged diff");
        f.panel.branches(&ctx).await.expect("branches");
        let fired: Vec<_> = std::fs::read_dir(&marks)
            .expect("marks")
            .flatten()
            .collect();
        assert!(fired.is_empty(), "a repository helper ran: {fired:?}");

        // Plain git does spring it: the test can't pass vacuously.
        let _ = Command::new("git")
            .current_dir(&root)
            .args(["status", "--porcelain"])
            .output();
        assert!(
            marks.join("fsmonitor").exists(),
            "the trap didn't fire under plain git"
        );
    }

    /// F-CD-20 AC3: before the first commit the view lists the new files,
    /// and staging them takes the first commit.
    #[tokio::test]
    async fn f_cd_20_3_a_folder_with_no_commits_lists_its_files_and_takes_the_first_commit() {
        let f = fx();
        let root = f.path("fresh");
        init(&root);
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::write(root.join("README.md"), "# fresh\n").expect("write");
        std::fs::write(root.join("src/lib.rs"), "pub fn x() {}\n").expect("write");
        let ctx = f.workspace(&root).await;
        let panel = f
            .panel
            .status(&ctx)
            .await
            .expect("status")
            .expect("a repository");
        assert!(panel.status.no_commits);
        assert_eq!(panel.status.branch.as_deref(), Some("main"));
        assert_eq!(
            paths(&panel.status.unstaged),
            vec!["README.md", "src/lib.rs"]
        );
        assert!(panel
            .status
            .unstaged
            .iter()
            .all(|x| x.change == FileChange::Untracked));
        f.panel
            .stage(&f.cli, &ctx, &["README.md".into(), "src/lib.rs".into()])
            .await
            .expect("stage");
        let staged = f.panel.status(&ctx).await.expect("status").expect("repo");
        assert_eq!(
            paths(&staged.status.staged),
            vec!["README.md", "src/lib.rs"]
        );
        let done = f
            .panel
            .commit(&f.cli, &ctx, "First commit")
            .await
            .expect("commit");
        assert_eq!(done.sha.len(), 40);
        let after = f.panel.status(&ctx).await.expect("status").expect("repo");
        assert!(!after.status.no_commits);
        assert!(after.status.is_clean());
    }

    /// F-CD-20 "works when the workspace is a subfolder of a repo": the
    /// view covers the whole repository (D-205) and says where the
    /// workspace sits in it.
    #[tokio::test]
    async fn f_cd_20_a_subfolder_workspace_sees_its_whole_repository() {
        let f = fx();
        let root = f.path("mono");
        init(&root);
        std::fs::create_dir_all(root.join("app")).expect("mkdir");
        std::fs::write(root.join("app/keep.txt"), "k\n").expect("write");
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "first"]);
        std::fs::write(root.join("app/in.txt"), "in\n").expect("write");
        std::fs::write(root.join("out.txt"), "out\n").expect("write");
        let ctx = f.workspace(&root.join("app")).await;
        let panel = f
            .panel
            .status(&ctx)
            .await
            .expect("status")
            .expect("a repository");
        assert_eq!(panel.folder, "app");
        assert_eq!(Path::new(&panel.root), root.as_path());
        assert_eq!(paths(&panel.status.unstaged), vec!["app/in.txt", "out.txt"]);
        f.panel
            .stage(&f.cli, &ctx, &["out.txt".into()])
            .await
            .expect("stage outside the folder");
        f.panel.commit(&f.cli, &ctx, "out").await.expect("commit");
        let after = f.panel.status(&ctx).await.expect("status").expect("repo");
        assert_eq!(paths(&after.status.unstaged), vec!["app/in.txt"]);
    }

    #[tokio::test]
    async fn f_cd_20_outside_a_repository_there_is_no_status() {
        let f = fx();
        let plain = f.path("plain");
        std::fs::create_dir_all(&plain).expect("mkdir");
        let ctx = f.workspace(&plain).await;
        assert_eq!(f.panel.status(&ctx).await.expect("status"), None);
        let err = f
            .panel
            .diff(&ctx, "x.txt", false)
            .await
            .expect_err("no repo");
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    /// A git action drops the cached status, so the tree's badges follow
    /// at once instead of after the cache's 2 s.
    #[tokio::test]
    async fn f_cd_20_an_action_refreshes_the_tree_badges_at_once() {
        let f = fx();
        let root = f.path("badges");
        init(&root);
        std::fs::write(root.join("a.txt"), "one\n").expect("write");
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "first"]);
        std::fs::write(root.join("a.txt"), "two\n").expect("write");
        let ctx = f.workspace(&root).await;
        let badge = |list: &[crate::services::files::FsEntry]| {
            list.iter()
                .find(|e| e.name == "a.txt")
                .and_then(|e| e.git_status)
        };
        let before = f
            .files
            .list(&ctx, String::new(), false)
            .await
            .expect("tree");
        assert_eq!(badge(&before), Some(FileChange::Modified));
        f.panel
            .stage(&f.cli, &ctx, &["a.txt".into()])
            .await
            .expect("stage");
        f.panel.commit(&f.cli, &ctx, "two").await.expect("commit");
        let after = f
            .files
            .list(&ctx, String::new(), false)
            .await
            .expect("tree");
        assert_eq!(badge(&after), None);
    }

    /// Actions on one repository wait their turn rather than trip over
    /// git's `index.lock`.
    #[tokio::test]
    async fn f_cd_20_actions_on_one_repository_run_one_at_a_time() {
        let f = fx();
        let root = f.path("busy");
        init(&root);
        std::fs::write(root.join("seed.txt"), "s\n").expect("write");
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "first"]);
        let names: Vec<String> = (0..6).map(|i| format!("f{i}.txt")).collect();
        for n in &names {
            std::fs::write(root.join(n), "x\n").expect("write");
        }
        let ctx = f.workspace(&root).await;
        let runs = names
            .iter()
            .map(|n| f.panel.stage(&f.cli, &ctx, std::slice::from_ref(n)));
        for result in futures::future::join_all(runs).await {
            result.expect("stage");
        }
        let panel = f.panel.status(&ctx).await.expect("status").expect("repo");
        assert_eq!(panel.status.staged.len(), names.len());
    }
}
