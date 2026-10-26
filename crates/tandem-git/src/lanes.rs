//! Worktree lanes (F-CD-15, ARCHITECTURE §7 "Lanes").

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use tandem_core::origin::LocalReview;

use crate::cli::{Git, Mode};
use crate::exclude::ensure_tandem_excluded;
use crate::repo::{path_string, Repo};
use crate::slug::{dedupe, slugify};
use crate::types::{MergeReport, StaleLane};
use crate::GitError;

pub use tandem_core::lanes::{BRANCH_PREFIX, STALE_AFTER_DAYS, WORKTREES_DIR};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lane {
    pub slug: String,
    /// `tandem/<slug>`.
    pub branch: String,
    /// `<repo>/.tandem/worktrees/<slug>`.
    pub path: PathBuf,
    /// The commit the lane started from.
    pub base: String,
}

/// Permission to remove a lane with uncommitted changes. Needs a
/// [`LocalReview`], so voice, the phone and unattended runs can't force it
/// (TL-06 rule 5).
#[derive(Debug)]
pub struct ForceConfirm(#[allow(dead_code)] LocalReview);

impl ForceConfirm {
    pub fn new(review: LocalReview) -> Self {
        Self(review)
    }
}

/// The briefing's line on claims (F-TK-05).
pub const CLAIMS_BRIEFING: &str = "If you have the tandem_tasks tools, claim the files you're about to change with claim before editing them, and check list_claims for other worktrees' claims.";

/// What a worktree's agent receives before its task (F-CD-15 "Briefing").
/// `siblings` are the other worktrees' branches in the same launch or repo.
pub fn briefing(path: &Path, branch: &str, siblings: &[String]) -> String {
    let others = if siblings.is_empty() {
        "none".to_string()
    } else {
        siblings
            .iter()
            .map(|b| format!("`{b}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "You are working in the git worktree `{}` on branch `{branch}`. Sibling branches: {others}. Don't modify other worktrees. {CLAIMS_BRIEFING}",
        path_string(path)
    )
}

/// [`briefing`] for a multi-repo worktree (F-CD-22): `path` holds one git
/// worktree per repository, all on `branch`.
pub fn multi_briefing(path: &Path, branch: &str, repos: &[String], siblings: &[String]) -> String {
    let others = if siblings.is_empty() {
        "none".to_string()
    } else {
        siblings
            .iter()
            .map(|b| format!("`{b}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let names = repos
        .iter()
        .map(|r| format!("`{r}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "You are working in `{}`, a folder holding a git worktree of each of these repositories: {names}. Each is on branch `{branch}`; commit in each repository's own folder. Sibling branches: {others}. Don't modify other worktrees.",
        path_string(path)
    )
}

/// The `origin` remote's URL of the checkout at `root`, if it has one.
pub fn origin_url(root: &Path) -> Option<String> {
    let repo = git2::Repository::discover(root).ok()?;
    let remote = repo.find_remote("origin").ok()?;
    remote.url().map(str::to_string)
}

/// A multi-repo worktree (F-CD-22): the parent folder and each
/// repository's worktree in it, by name.
#[derive(Debug, Clone)]
pub struct MultiLane {
    pub slug: String,
    pub branch: String,
    pub path: PathBuf,
    pub repos: Vec<(String, Lane)>,
}

pub struct LaneManager;

impl LaneManager {
    /// Slugs in use: `tandem/*` branches and `.tandem/worktrees/*` folders.
    pub async fn taken_slugs(repo: &Repo) -> Result<HashSet<String>, GitError> {
        let mut taken: HashSet<String> = repo
            .local_branches()
            .await?
            .into_iter()
            .filter_map(|b| b.strip_prefix(BRANCH_PREFIX).map(str::to_string))
            .collect();
        if let Ok(dir) = std::fs::read_dir(repo.main_root().join(WORKTREES_DIR)) {
            for entry in dir.flatten() {
                taken.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
        Ok(taken)
    }

    /// `git worktree add .tandem/worktrees/<slug> -b tandem/<slug> <HEAD>`
    /// from the workspace's current HEAD. `also_taken` holds slugs reserved
    /// by lanes being created in the same launch.
    pub async fn create(
        git: &Git,
        repo: &Repo,
        slug_source: &str,
        also_taken: &HashSet<String>,
    ) -> Result<Lane, GitError> {
        let base = repo.head().await?.ok_or_else(|| {
            GitError::Invalid(
                "Lanes start from a commit. Make a first commit, then try again.".into(),
            )
        })?;
        let mut taken = Self::taken_slugs(repo).await?;
        taken.extend(also_taken.iter().cloned());
        let slug = dedupe(&slugify(slug_source), &taken);
        let branch = format!("{BRANCH_PREFIX}{slug}");
        ensure_tandem_excluded(repo.common_dir())?;
        let dir = repo.main_root().join(WORKTREES_DIR);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(&slug);
        Self::add_worktree(git, repo, &path, &branch, &base).await?;
        Ok(Lane {
            slug,
            branch,
            path: std::fs::canonicalize(&path).unwrap_or(path),
            base,
        })
    }

    /// `git worktree add <path> -b <branch> <base>`.
    async fn add_worktree(
        git: &Git,
        repo: &Repo,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> Result<(), GitError> {
        let out = git
            .run(
                Mode::Write,
                repo.root(),
                &[
                    OsStr::new("worktree"),
                    OsStr::new("add"),
                    path.as_os_str(),
                    OsStr::new("-b"),
                    OsStr::new(branch),
                    OsStr::new(base),
                ],
            )
            .await?;
        if out.success() {
            return Ok(());
        }
        if out.stderr.contains("already exists") {
            return Err(GitError::Conflict(format!("{branch} already exists")));
        }
        Err(GitError::Command {
            op: "worktree add".into(),
            code: out.code,
            stderr: out.stderr,
        })
    }

    /// F-CD-22: one worktree across the repositories directly inside
    /// `folder`: the parent folder `<folder>/.tandem/worktrees/<slug>`, and
    /// in it a git worktree of each repository at `<slug>/<name>` on
    /// `tandem/<slug>`, each from its repository's HEAD. The slug is free in
    /// every repository. If one fails, those already made are removed and
    /// the error names the repository.
    pub async fn create_multi(
        git: &Git,
        folder: &Path,
        repos: &[(String, Repo)],
        slug_source: &str,
        also_taken: &HashSet<String>,
    ) -> Result<MultiLane, GitError> {
        let in_repo = |name: &str, e: GitError| GitError::Invalid(format!("In {name}: {e}"));
        let mut taken: HashSet<String> = also_taken.clone();
        for (name, repo) in repos {
            taken.extend(
                Self::taken_slugs(repo)
                    .await
                    .map_err(|e| in_repo(name, e))?,
            );
        }
        let dir = folder.join(WORKTREES_DIR);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                taken.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
        std::fs::create_dir_all(&dir)?;
        // The parent folder is made exclusively: a concurrent create that
        // picked the same slug gets the next one, never this folder.
        let (slug, parent) = loop {
            let slug = dedupe(&slugify(slug_source), &taken);
            let parent = dir.join(&slug);
            match std::fs::create_dir(&parent) {
                Ok(()) => break (slug, parent),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    taken.insert(slug);
                }
                Err(e) => return Err(e.into()),
            }
        };
        let branch = format!("{BRANCH_PREFIX}{slug}");
        let mut made: Vec<(String, Lane, &Repo)> = Vec::new();
        for (name, repo) in repos {
            let path = parent.join(name);
            let step = async {
                let base = repo.head().await?.ok_or_else(|| {
                    GitError::Invalid(
                        "it has no commits yet. Make a first commit, then try again.".into(),
                    )
                })?;
                Self::add_worktree(git, repo, &path, &branch, &base).await?;
                Ok::<String, GitError>(base)
            };
            match step.await {
                Ok(base) => made.push((
                    name.clone(),
                    Lane {
                        slug: slug.clone(),
                        branch: branch.clone(),
                        path: std::fs::canonicalize(&path).unwrap_or(path),
                        base,
                    },
                    repo,
                )),
                Err(e) => {
                    for (_, lane, repo) in &made {
                        Self::discard_new(git, repo, &lane.path, &lane.branch).await;
                    }
                    // The failing repository may have its branch or a
                    // half-made worktree already.
                    Self::discard_new(git, repo, &path, &branch).await;
                    let _ = std::fs::remove_dir_all(&parent);
                    return Err(in_repo(name, e));
                }
            }
        }
        Ok(MultiLane {
            path: std::fs::canonicalize(&parent).unwrap_or(parent),
            slug,
            branch,
            repos: made.into_iter().map(|(n, l, _)| (n, l)).collect(),
        })
    }

    /// Undoes a worktree this process just made: force-removes it (a fresh
    /// checkout can read dirty, from line endings, case collisions or the
    /// user's post-checkout hooks), prunes its registration and deletes its
    /// new branch. Failures are logged, not returned: it's a rollback.
    pub async fn discard_new(git: &Git, repo: &Repo, path: &Path, branch: &str) {
        if let Err(e) = Self::remove_inner(git, repo, path, true).await {
            tracing::warn!(code = "lanes.rollback_remove_failed", error = %e, "rollback couldn't remove a new worktree");
        }
        let _ = git
            .ok(
                Mode::Write,
                &repo.main_root(),
                &[OsStr::new("worktree"), OsStr::new("prune")],
            )
            .await;
        if Self::branch_exists(git, repo, branch).await {
            if let Err(e) = Self::delete_branch(git, repo, branch).await {
                tracing::warn!(code = "lanes.rollback_branch_failed", error = %e, "rollback couldn't delete a new branch");
            }
        }
    }

    async fn branch_exists(git: &Git, repo: &Repo, branch: &str) -> bool {
        let r = format!("refs/heads/{branch}");
        git.ok(
            Mode::Read,
            repo.root(),
            &[
                OsStr::new("show-ref"),
                OsStr::new("--verify"),
                OsStr::new("--quiet"),
                OsStr::new(&r),
            ],
        )
        .await
        .is_ok()
    }

    /// Deletes a branch a failed multi-repo worktree just made (it has no
    /// commits of its own yet).
    async fn delete_branch(git: &Git, repo: &Repo, branch: &str) -> Result<(), GitError> {
        git.ok(
            Mode::Write,
            repo.root(),
            &[OsStr::new("branch"), OsStr::new("-D"), OsStr::new(branch)],
        )
        .await?;
        Ok(())
    }

    /// `git worktree remove`. A lane with uncommitted or untracked files
    /// needs `force`; the branch is always kept.
    pub async fn remove(
        git: &Git,
        repo: &Repo,
        lane_path: &Path,
        force: Option<ForceConfirm>,
    ) -> Result<(), GitError> {
        Self::remove_inner(git, repo, lane_path, force.is_some()).await
    }

    /// [`remove`](Self::remove) for each of a multi-repo worktree's
    /// repositories (F-CD-22): one confirmation covers them all.
    pub async fn remove_many(
        git: &Git,
        lanes: &[(Repo, PathBuf)],
        force: Option<ForceConfirm>,
    ) -> Result<(), GitError> {
        for (repo, path) in lanes {
            Self::remove_inner(git, repo, path, force.is_some()).await?;
        }
        Ok(())
    }

    async fn remove_inner(
        git: &Git,
        repo: &Repo,
        lane_path: &Path,
        force: bool,
    ) -> Result<(), GitError> {
        let main = repo.main_root();
        if !lane_path.exists() {
            git.ok(
                Mode::Read,
                &main,
                &[OsStr::new("worktree"), OsStr::new("prune")],
            )
            .await?;
            return Ok(());
        }
        if !force {
            if let Some(lane) = Repo::discover(lane_path)? {
                if !lane.status().await?.is_clean() {
                    return Err(GitError::Dirty);
                }
            }
        }
        let mut args = vec![OsStr::new("worktree"), OsStr::new("remove")];
        if force {
            args.push(OsStr::new("--force"));
        }
        args.push(lane_path.as_os_str());
        let out = git.run(Mode::Write, &main, &args).await?;
        if out.success() {
            Ok(())
        } else if out.stderr.contains("modified or untracked files") {
            Err(GitError::Dirty)
        } else {
            Err(GitError::Command {
                op: "worktree remove".into(),
                code: out.code,
                stderr: out.stderr,
            })
        }
    }

    /// `git merge --no-ff <branch>` in the parent checkout. Refuses when the
    /// parent has uncommitted changes; on conflict it stops, leaves the
    /// conflicted state for the user and commits nothing.
    pub async fn merge_into_base(
        git: &Git,
        parent: &Repo,
        branch: &str,
    ) -> Result<MergeReport, GitError> {
        if parent.status().await?.has_tracked_changes() {
            return Err(GitError::Dirty);
        }
        let before = parent.head().await?;
        let out = git
            .run(
                Mode::Write,
                parent.root(),
                &[
                    OsStr::new("merge"),
                    OsStr::new("--no-ff"),
                    OsStr::new("--no-edit"),
                    OsStr::new(branch),
                ],
            )
            .await?;
        if out.success() {
            let after = parent.head().await?;
            return Ok(MergeReport {
                merged: true,
                conflicts: Vec::new(),
                commit: after.filter(|a| Some(a) != before.as_ref()),
            });
        }
        let conflicts = parent.conflicts().await?;
        if conflicts.is_empty() {
            return Err(GitError::Command {
                op: "merge".into(),
                code: out.code,
                stderr: out.stderr,
            });
        }
        Ok(MergeReport {
            merged: false,
            conflicts,
            commit: None,
        })
    }

    /// `git worktree prune`, then the lanes merged into the main checkout's
    /// HEAD and idle for more than 7 days. Never deletes anything else.
    pub async fn sweep(
        git: &Git,
        repo: &Repo,
        now: DateTime<Utc>,
    ) -> Result<Vec<StaleLane>, GitError> {
        let main_root = repo.main_root();
        git.ok(
            Mode::Read,
            &main_root,
            &[OsStr::new("worktree"), OsStr::new("prune")],
        )
        .await?;
        let Some(main) = Repo::discover(&main_root)? else {
            return Ok(Vec::new());
        };
        let Some(head) = main.head().await? else {
            return Ok(Vec::new());
        };
        let lanes = {
            let main = main.clone();
            tokio::task::spawn_blocking(move || list_lanes(&main))
                .await
                .map_err(|e| GitError::Io(std::io::Error::other(e)))??
        };
        let mut stale = Vec::new();
        for lane in lanes {
            if !main.is_merged(&lane.branch, &head).await? {
                continue;
            }
            let last = lane.last_activity;
            let idle = now.signed_duration_since(last).num_days();
            if idle > STALE_AFTER_DAYS {
                stale.push(StaleLane {
                    branch: lane.branch,
                    path: path_string(&lane.path),
                    idle_days: u32::try_from(idle).unwrap_or(u32::MAX),
                });
            }
        }
        Ok(stale)
    }
}

struct ListedLane {
    branch: String,
    path: PathBuf,
    last_activity: DateTime<Utc>,
}

/// Linked worktrees under `.tandem/worktrees` on a `tandem/*` branch.
fn list_lanes(main: &Repo) -> Result<Vec<ListedLane>, GitError> {
    let repo = git2::Repository::open(main.root())?;
    let dir = std::fs::canonicalize(main.main_root().join(WORKTREES_DIR)).ok();
    let mut out = Vec::new();
    for name in repo.worktrees()?.iter().flatten() {
        let Ok(wt) = repo.find_worktree(name) else {
            continue;
        };
        let path = std::fs::canonicalize(wt.path()).unwrap_or_else(|_| wt.path().to_path_buf());
        if !dir.as_ref().is_some_and(|d| path.starts_with(d)) {
            continue;
        }
        let Ok(lane_repo) = git2::Repository::open_from_worktree(&wt) else {
            continue;
        };
        let Ok(head) = lane_repo.head() else {
            continue;
        };
        let Some(branch) = head.shorthand().filter(|b| b.starts_with(BRANCH_PREFIX)) else {
            continue;
        };
        let mut last = head
            .peel_to_commit()
            .ok()
            .and_then(|c| DateTime::from_timestamp(c.time().seconds(), 0))
            .unwrap_or(DateTime::UNIX_EPOCH);
        // Checkouts, staging and commits in the lane touch its index and HEAD.
        for file in ["index", "HEAD"] {
            if let Some(t) = modified(&lane_repo.path().join(file)) {
                last = last.max(t);
            }
        }
        out.push(ListedLane {
            branch: branch.to_string(),
            path,
            last_activity: last,
        });
    }
    Ok(out)
}

fn modified(path: &Path) -> Option<DateTime<Utc>> {
    let time: SystemTime = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(DateTime::<Utc>::from(time))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cd_15_f_tk_05_briefing_names_the_worktree_branch_siblings_and_claims() {
        let text = briefing(
            Path::new("/repo/.tandem/worktrees/fix-auth"),
            "tandem/fix-auth",
            &["tandem/fix-auth-2".into(), "tandem/fix-auth-3".into()],
        );
        assert_eq!(
            text,
            "You are working in the git worktree `/repo/.tandem/worktrees/fix-auth` on branch `tandem/fix-auth`. \
             Sibling branches: `tandem/fix-auth-2`, `tandem/fix-auth-3`. Don't modify other worktrees. \
             If you have the tandem_tasks tools, claim the files you're about to change with claim before editing them, \
             and check list_claims for other worktrees' claims."
        );
        assert!(briefing(Path::new("/r/w"), "tandem/w", &[]).contains("Sibling branches: none."));
    }
}
