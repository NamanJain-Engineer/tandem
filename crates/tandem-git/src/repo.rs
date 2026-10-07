//! Repository reads through libgit2 (ARCHITECTURE §7): libgit2 never runs
//! fsmonitor, hooks, credential helpers or filter / textconv commands, so
//! every read here is hardened by construction (TL-06 rule 6).

use std::path::{Path, PathBuf};

use git2::{DiffFindOptions, DiffOptions, Repository, Status, StatusOptions};

use crate::types::{FileChange, FileDiff, FileStatus, GitStatus};
use crate::GitError;

/// A file's patch is cut here; the DiffView says so.
pub const MAX_PATCH_BYTES: usize = 512 * 1024;

/// A working tree: the main checkout or a linked worktree (a lane), and
/// the workspace folder inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    root: PathBuf,
    prefix: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

impl Repo {
    /// The repository `path` is in, or `None` outside one (and for bare
    /// repositories, which have nothing to check out).
    pub fn discover(path: &Path) -> Result<Option<Repo>, GitError> {
        let path = std::fs::canonicalize(path)?;
        let repo = match Repository::discover(&path) {
            Ok(repo) => repo,
            Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let Some(workdir) = repo.workdir() else {
            return Ok(None);
        };
        let root = std::fs::canonicalize(workdir)?;
        let prefix = path
            .strip_prefix(&root)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        Ok(Some(Repo {
            root,
            prefix,
            git_dir: repo.path().to_path_buf(),
            common_dir: repo.commondir().to_path_buf(),
        }))
    }

    /// The working tree's top folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The workspace folder relative to [`Repo::root`]; empty at the top.
    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    /// The `.git` directory shared by every worktree (`info/exclude` lives here).
    pub fn common_dir(&self) -> &Path {
        &self.common_dir
    }

    /// The top folder of the main checkout, where `.tandem/worktrees` lives,
    /// also when this is a linked worktree.
    pub fn main_root(&self) -> PathBuf {
        match self.common_dir.file_name() {
            Some(name) if name == ".git" => self
                .common_dir
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.root.clone()),
            _ => self.root.clone(),
        }
    }

    /// True for a linked worktree (a lane), false for the main checkout.
    pub fn is_linked_worktree(&self) -> bool {
        self.git_dir != self.common_dir
    }

    fn open(&self) -> Result<Repository, GitError> {
        Ok(Repository::open(&self.root)?)
    }

    async fn blocking<T, F>(&self, f: F) -> Result<T, GitError>
    where
        T: Send + 'static,
        F: FnOnce(&Repo) -> Result<T, GitError> + Send + 'static,
    {
        let this = self.clone();
        tokio::task::spawn_blocking(move || f(&this))
            .await
            .map_err(|e| GitError::Io(std::io::Error::other(e)))?
    }

    /// The commit HEAD points at; `None` before the first commit.
    pub async fn head(&self) -> Result<Option<String>, GitError> {
        self.blocking(|r| r.head_blocking()).await
    }

    pub fn head_blocking(&self) -> Result<Option<String>, GitError> {
        let repo = self.open()?;
        let head = match repo.head() {
            Ok(head) => head,
            Err(e) if is_unborn(&e) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let sha = head.peel_to_commit()?.id().to_string();
        Ok(Some(sha))
    }

    /// Local branch names (`main`, `tandem/fix-auth`, …).
    pub async fn local_branches(&self) -> Result<Vec<String>, GitError> {
        self.blocking(|r| {
            let repo = r.open()?;
            let mut names = Vec::new();
            for branch in repo.branches(Some(git2::BranchType::Local))? {
                let (branch, _) = branch?;
                if let Some(name) = branch.name()? {
                    names.push(name.to_string());
                }
            }
            names.sort();
            Ok(names)
        })
        .await
    }

    pub async fn status(&self) -> Result<GitStatus, GitError> {
        self.blocking(|r| r.status_blocking()).await
    }

    pub fn status_blocking(&self) -> Result<GitStatus, GitError> {
        let repo = self.open()?;
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false)
            .exclude_submodules(true)
            .renames_head_to_index(true);
        if let Some(spec) = self.pathspec() {
            opts.pathspec(spec);
        }
        let mut out = GitStatus::default();
        for entry in repo.statuses(Some(&mut opts))?.iter() {
            let s = entry.status();
            let path = entry.path().unwrap_or_default().to_string();
            if s.is_conflicted() {
                out.unstaged.push(FileStatus {
                    path,
                    change: FileChange::Conflicted,
                    old_path: None,
                });
                continue;
            }
            let index = if s.is_index_new() {
                Some(FileChange::Added)
            } else if s.is_index_modified() {
                Some(FileChange::Modified)
            } else if s.is_index_deleted() {
                Some(FileChange::Deleted)
            } else if s.is_index_renamed() {
                Some(FileChange::Renamed)
            } else if s.is_index_typechange() {
                Some(FileChange::TypeChanged)
            } else {
                None
            };
            if let Some(change) = index {
                let old_path = (change == FileChange::Renamed)
                    .then(|| entry.head_to_index())
                    .flatten()
                    .and_then(|d| d.old_file().path().map(path_string));
                let new_path = entry
                    .head_to_index()
                    .and_then(|d| d.new_file().path().map(path_string))
                    .unwrap_or_else(|| path.clone());
                out.staged.push(FileStatus {
                    path: new_path,
                    change,
                    old_path,
                });
            }
            let worktree = if s.contains(Status::WT_NEW) {
                Some(FileChange::Untracked)
            } else if s.is_wt_modified() {
                Some(FileChange::Modified)
            } else if s.is_wt_deleted() {
                Some(FileChange::Deleted)
            } else if s.is_wt_renamed() {
                Some(FileChange::Renamed)
            } else if s.is_wt_typechange() {
                Some(FileChange::TypeChanged)
            } else {
                None
            };
            if let Some(change) = worktree {
                out.unstaged.push(FileStatus {
                    path,
                    change,
                    old_path: None,
                });
            }
        }
        match repo.head() {
            Ok(head) => {
                if head.is_branch() {
                    out.branch = head.shorthand().map(str::to_string);
                    if let (Some(name), Some(local)) = (out.branch.clone(), head.target()) {
                        if let Ok(upstream) = repo
                            .find_branch(&name, git2::BranchType::Local)
                            .and_then(|b| b.upstream())
                        {
                            out.upstream = upstream.name().ok().flatten().map(str::to_string);
                            if let Some(remote) = upstream.get().target() {
                                let (ahead, behind) = repo.graph_ahead_behind(local, remote)?;
                                out.ahead = u32::try_from(ahead).unwrap_or(u32::MAX);
                                out.behind = u32::try_from(behind).unwrap_or(u32::MAX);
                            }
                        }
                    }
                }
            }
            Err(e) if is_unborn(&e) => {
                out.no_commits = true;
                out.branch = repo
                    .find_reference("HEAD")
                    .ok()
                    .and_then(|r| r.symbolic_target().map(str::to_string))
                    .map(|t| t.trim_start_matches("refs/heads/").to_string());
            }
            Err(e) => return Err(e.into()),
        }
        Ok(out)
    }

    /// Every change in this working tree since `base` (a commit): committed,
    /// staged, unstaged and untracked files, within the workspace folder.
    pub async fn diff_since(&self, base: &str) -> Result<Vec<FileDiff>, GitError> {
        let base = base.to_string();
        self.blocking(move |r| r.diff_since_blocking(&base)).await
    }

    pub fn diff_since_blocking(&self, base: &str) -> Result<Vec<FileDiff>, GitError> {
        let repo = self.open()?;
        let tree = repo.revparse_single(base)?.peel_to_commit()?.tree()?;
        let mut opts = DiffOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .show_untracked_content(true)
            .ignore_submodules(true)
            .context_lines(3);
        if let Some(spec) = self.pathspec() {
            opts.pathspec(spec);
        }
        let mut diff = repo.diff_tree_to_workdir_with_index(Some(&tree), Some(&mut opts))?;
        diff.find_similar(Some(DiffFindOptions::new().renames(true)))?;
        let mut files = Vec::with_capacity(diff.deltas().len());
        for idx in 0..diff.deltas().len() {
            let Some(delta) = diff.get_delta(idx) else {
                continue;
            };
            let change = match delta.status() {
                git2::Delta::Added | git2::Delta::Untracked | git2::Delta::Copied => {
                    FileChange::Added
                }
                git2::Delta::Deleted => FileChange::Deleted,
                git2::Delta::Renamed => FileChange::Renamed,
                git2::Delta::Typechange => FileChange::TypeChanged,
                git2::Delta::Conflicted => FileChange::Conflicted,
                _ => FileChange::Modified,
            };
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .map(path_string)
                .unwrap_or_default();
            let old_path = (change == FileChange::Renamed)
                .then(|| delta.old_file().path().map(path_string))
                .flatten();
            let patch = git2::Patch::from_diff(&diff, idx)?;
            let binary = delta.flags().is_binary() || patch.is_none();
            let (mut additions, mut deletions, mut text, mut truncated) =
                (0, 0, String::new(), false);
            if let (false, Some(mut patch)) = (binary, patch) {
                let (_, add, del) = patch.line_stats()?;
                additions = u32::try_from(add).unwrap_or(u32::MAX);
                deletions = u32::try_from(del).unwrap_or(u32::MAX);
                let buf = patch.to_buf()?;
                let bytes: &[u8] = &buf;
                if bytes.len() > MAX_PATCH_BYTES {
                    truncated = true;
                    text = String::from_utf8_lossy(&bytes[..MAX_PATCH_BYTES]).into_owned();
                    // Don't end on half a line.
                    if let Some(cut) = text.rfind('\n') {
                        text.truncate(cut + 1);
                    }
                } else {
                    text = String::from_utf8_lossy(bytes).into_owned();
                }
            }
            files.push(FileDiff {
                path,
                old_path,
                change,
                additions,
                deletions,
                binary,
                patch: text,
                truncated,
            });
        }
        Ok(files)
    }

    /// Whether `branch`'s tip is `into` or one of its ancestors.
    pub async fn is_merged(&self, branch: &str, into: &str) -> Result<bool, GitError> {
        let (branch, into) = (branch.to_string(), into.to_string());
        self.blocking(move |r| {
            let repo = r.open()?;
            let tip = repo.revparse_single(&branch)?.peel_to_commit()?.id();
            let target = repo.revparse_single(&into)?.peel_to_commit()?.id();
            Ok(tip == target || repo.graph_descendant_of(target, tip)?)
        })
        .await
    }

    /// Paths with unresolved conflicts in the index.
    pub async fn conflicts(&self) -> Result<Vec<String>, GitError> {
        self.blocking(|r| {
            let repo = r.open()?;
            let index = repo.index()?;
            if !index.has_conflicts() {
                return Ok(Vec::new());
            }
            let mut paths = Vec::new();
            for conflict in index.conflicts()? {
                let conflict = conflict?;
                let entry = conflict.our.or(conflict.their).or(conflict.ancestor);
                if let Some(entry) = entry {
                    paths.push(String::from_utf8_lossy(&entry.path).into_owned());
                }
            }
            paths.sort();
            paths.dedup();
            Ok(paths)
        })
        .await
    }

    /// The worktree pathspec for a workspace inside the repository.
    fn pathspec(&self) -> Option<String> {
        if self.prefix.as_os_str().is_empty() {
            return None;
        }
        let mut spec = path_string(&self.prefix);
        spec.push('/');
        Some(spec)
    }
}

fn is_unborn(e: &git2::Error) -> bool {
    matches!(
        e.code(),
        git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
    )
}

/// `/`-separated, whatever the platform.
pub fn path_string(path: &Path) -> String {
    let s = path.to_string_lossy();
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s.into_owned()
    }
}
