//! The git steps of the local merge queue (F-WT-07): bring the base into a
//! worktree, land the worktree in the base with `--no-ff`, and move the
//! base back for Undo landing. Merges take commits, not branch names, so
//! what lands is what was checked. A conflict always aborts its merge, and
//! a merge that would write over a file on disk that the checkout doesn't
//! commit (an ignored `.env`) isn't started. Nothing here pushes or fetches.
//! Revisions follow `--end-of-options` (git 2.24).

use std::ffi::OsStr;
use std::path::Path;

use crate::cli::{Git, Mode};
use crate::error::GitError;
use crate::repo::Repo;

const END: &str = "--end-of-options";

/// How one merge went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merged {
    /// Merged; the new HEAD (unchanged when there was nothing to merge).
    Done { head: String },
    /// Conflicted in these files; the merge was aborted.
    Conflicts { files: Vec<String> },
    /// Not started: it would write over these files, which are on disk but
    /// not committed in this checkout.
    WouldReplace { files: Vec<String> },
}

/// The branch HEAD is on in `root`; `None` when detached or unborn.
pub async fn current_branch(git: &Git, root: &Path) -> Result<Option<String>, GitError> {
    let out = git
        .run(
            Mode::Read,
            root,
            &[
                OsStr::new("symbolic-ref"),
                OsStr::new("--quiet"),
                OsStr::new("--short"),
                OsStr::new("HEAD"),
            ],
        )
        .await?;
    let name = out.text().trim().to_string();
    Ok((out.success() && !name.is_empty()).then_some(name))
}

/// The commit `rev` names in `root`, or `None` when it names nothing.
/// `rev` is a branch name, `HEAD` or a commit: git refuses branch names
/// that start with `-`.
pub async fn resolve(git: &Git, root: &Path, rev: &str) -> Result<Option<String>, GitError> {
    let spec = format!("{rev}^{{commit}}");
    let out = git
        .run(
            Mode::Read,
            root,
            &[
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new("--quiet"),
                OsStr::new(&spec),
            ],
        )
        .await?;
    let sha = out.text().trim().to_string();
    Ok((out.success() && !sha.is_empty()).then_some(sha))
}

/// How many commits `branch` has that `base` doesn't.
pub async fn ahead(git: &Git, root: &Path, base: &str, branch: &str) -> Result<u32, GitError> {
    let range = format!("{base}..{branch}");
    let out = git
        .ok(
            Mode::Read,
            root,
            &[
                OsStr::new("rev-list"),
                OsStr::new("--count"),
                OsStr::new(END),
                OsStr::new(&range),
            ],
        )
        .await?;
    Ok(out.text().trim().parse().unwrap_or(0))
}

/// The first-parent commits after `from` up to `to`, oldest first.
pub async fn first_parents(
    git: &Git,
    root: &Path,
    from: &str,
    to: &str,
) -> Result<Vec<String>, GitError> {
    let range = format!("{from}..{to}");
    let out = git
        .ok(
            Mode::Read,
            root,
            &[
                OsStr::new("rev-list"),
                OsStr::new("--first-parent"),
                OsStr::new("--reverse"),
                OsStr::new(END),
                OsStr::new(&range),
            ],
        )
        .await?;
    Ok(out.text().lines().map(str::to_string).collect())
}

/// The remote-tracking branches that contain `commit` (it was pushed).
pub async fn remotes_containing(
    git: &Git,
    root: &Path,
    commit: &str,
) -> Result<Vec<String>, GitError> {
    let contains = format!("--contains={commit}");
    let out = git
        .ok(
            Mode::Read,
            root,
            &[
                OsStr::new("for-each-ref"),
                OsStr::new("--format=%(refname:short)"),
                OsStr::new(&contains),
                OsStr::new("refs/remotes"),
            ],
        )
        .await?;
    Ok(out
        .text()
        .lines()
        .filter(|l| !l.is_empty() && !l.ends_with("/HEAD"))
        .map(str::to_string)
        .collect())
}

/// Whether `commit` is `of` or one of its ancestors; false when `of` names
/// nothing.
pub async fn contained_in(
    git: &Git,
    root: &Path,
    commit: &str,
    of: &str,
) -> Result<bool, GitError> {
    if resolve(git, root, of).await?.is_none() {
        return Ok(false);
    }
    let out = git
        .run(
            Mode::Read,
            root,
            &[
                OsStr::new("merge-base"),
                OsStr::new("--is-ancestor"),
                OsStr::new(END),
                OsStr::new(commit),
                OsStr::new(of),
            ],
        )
        .await?;
    Ok(out.success())
}

/// The commit a merge in progress in `root` is merging (`MERGE_HEAD`).
pub async fn merge_head(git: &Git, root: &Path) -> Result<Option<String>, GitError> {
    resolve(git, root, "MERGE_HEAD").await
}

/// `git merge --abort` in `root`; an error when it fails, so a merge is
/// never reported as cleanly aborted when it's still in progress.
pub async fn abort_merge(git: &Git, root: &Path) -> Result<(), GitError> {
    git.ok(
        Mode::Write,
        root,
        &[OsStr::new("merge"), OsStr::new("--abort")],
    )
    .await?;
    Ok(())
}

/// Files merging `rev` would add to `root`'s HEAD that already exist on
/// disk there. They aren't committed in this checkout (they'd be in HEAD
/// otherwise), so they're untracked or ignored, and git overwrites ignored
/// ones in a true merge.
pub async fn would_replace(git: &Git, root: &Path, rev: &str) -> Result<Vec<String>, GitError> {
    let out = git
        .ok(
            Mode::Read,
            root,
            &[
                OsStr::new("diff"),
                OsStr::new("--name-only"),
                OsStr::new("--no-renames"),
                OsStr::new("--diff-filter=A"),
                OsStr::new("-z"),
                OsStr::new(END),
                OsStr::new("HEAD"),
                OsStr::new(rev),
            ],
        )
        .await?;
    Ok(out
        .text()
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter(|p| std::fs::symlink_metadata(root.join(p)).is_ok())
        .map(str::to_string)
        .collect())
}

/// `git merge <args> -m <message> <rev>` in `root`; a conflict is aborted.
async fn merge(
    git: &Git,
    root: &Path,
    args: &[&str],
    message: &str,
    rev: &str,
) -> Result<Merged, GitError> {
    let repo = Repo::discover(root)?.ok_or(GitError::NotARepo)?;
    if repo.status().await?.has_tracked_changes() {
        return Err(GitError::Dirty);
    }
    let files = would_replace(git, root, rev).await?;
    if !files.is_empty() {
        return Ok(Merged::WouldReplace { files });
    }
    let mut argv: Vec<&OsStr> = vec![OsStr::new("merge")];
    argv.extend(args.iter().map(OsStr::new));
    argv.extend([
        OsStr::new("--no-edit"),
        OsStr::new("-m"),
        OsStr::new(message),
        OsStr::new(END),
        OsStr::new(rev),
    ]);
    let out = git.run(Mode::Write, root, &argv).await?;
    if out.success() {
        let head = repo.head().await?.unwrap_or_default();
        return Ok(Merged::Done { head });
    }
    let files = repo.conflicts().await?;
    // Nothing may stay half-merged: abort whatever this merge started, and
    // say so when that fails rather than report a clean conflict.
    if merge_head(git, root).await?.is_some() || !files.is_empty() {
        abort_merge(git, root).await?;
    }
    if files.is_empty() {
        return Err(GitError::Command {
            op: "merge".into(),
            code: out.code,
            stderr: out.stderr,
        });
    }
    Ok(Merged::Conflicts { files })
}

/// Step 1: the base's commit `base_commit` merged into the worktree at
/// `root` (fast-forward allowed, whatever `merge.ff` says).
pub async fn merge_base_into(
    git: &Git,
    root: &Path,
    base: &str,
    base_commit: &str,
    branch: &str,
) -> Result<Merged, GitError> {
    let message = format!("Merge branch '{base}' into {branch}");
    merge(git, root, &["--ff"], &message, base_commit).await
}

/// Step 3: the worktree's checked commit merged into the checkout at
/// `base_root` with `--no-ff`, as "Merge branch '<branch>'".
pub async fn land(
    git: &Git,
    base_root: &Path,
    branch: &str,
    commit: &str,
) -> Result<Merged, GitError> {
    let message = format!("Merge branch '{branch}'");
    merge(git, base_root, &["--no-ff"], &message, commit).await
}

/// Undo landing: the checkout's branch moved back to `commit` (a recorded
/// commit id, never an option), keeping untracked files (`git reset
/// --keep`; reset doesn't take `--end-of-options`).
pub async fn reset_to(git: &Git, root: &Path, commit: &str) -> Result<(), GitError> {
    git.ok(
        Mode::Write,
        root,
        &[
            OsStr::new("reset"),
            OsStr::new("--keep"),
            OsStr::new(commit),
        ],
    )
    .await?;
    Ok(())
}
