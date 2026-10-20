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

/// The `origin` remote's URL of the checkout at `root`, if it has one.
pub fn origin_url(root: &Path) -> Option<String> {
    let repo = git2::Repository::discover(root).ok()?;
    let remote = repo.find_remote("origin").ok()?;
    remote.url().map(str::to_string)
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
        let out = git
            .run(
                Mode::Write,
                repo.root(),
                &[
                    OsStr::new("worktree"),
                    OsStr::new("add"),
                    path.as_os_str(),
                    OsStr::new("-b"),
                    OsStr::new(&branch),
                    OsStr::new(&base),
                ],
            )
            .await?;
        if !out.success() {
            if out.stderr.contains("already exists") {
                return Err(GitError::Conflict(format!("{branch} already exists")));
            }
            return Err(GitError::Command {
                op: "worktree add".into(),
                code: out.code,
                stderr: out.stderr,
            });
        }
        Ok(Lane {
            slug,
            branch,
            path: std::fs::canonicalize(&path).unwrap_or(path),
            base,
        })
    }

    /// `git worktree remove`. A lane with uncommitted or untracked files
    /// needs `force`; the branch is always kept.
    pub async fn remove(
        git: &Git,
        repo: &Repo,
        lane_path: &Path,
        force: Option<ForceConfirm>,
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
        if force.is_none() {
            if let Some(lane) = Repo::discover(lane_path)? {
                if !lane.status().await?.is_clean() {
                    return Err(GitError::Dirty);
                }
            }
        }
        let mut args = vec![OsStr::new("worktree"), OsStr::new("remove")];
        if force.is_some() {
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
