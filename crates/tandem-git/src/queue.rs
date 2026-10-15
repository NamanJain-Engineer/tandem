//! The git steps of the local merge queue (F-WT-07): bring the base into a
//! worktree, land the worktree in the base with `--no-ff`, and move the
//! base back for Undo landing. A conflict always aborts its merge, so a
//! stopped queue leaves nothing half-merged. Nothing here pushes or fetches.

use std::ffi::OsStr;
use std::path::Path;

use crate::cli::{Git, Mode};
use crate::error::GitError;
use crate::repo::Repo;

/// How one merge went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merged {
    /// Merged; the new HEAD (unchanged when there was nothing to merge).
    Done { head: String },
    /// Conflicted in these files; the merge was aborted.
    Conflicts { files: Vec<String> },
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
                OsStr::new(&range),
            ],
        )
        .await?;
    Ok(out.text().trim().parse().unwrap_or(0))
}

/// Whether `commit` is `of` or one of its ancestors; false when `of` names
/// nothing (no `origin/<base>`).
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
                OsStr::new(commit),
                OsStr::new(of),
            ],
        )
        .await?;
    Ok(out.success())
}

/// `git merge <args> <rev>` in `root`; a conflict is aborted.
async fn merge(git: &Git, root: &Path, args: &[&str], rev: &str) -> Result<Merged, GitError> {
    let repo = Repo::discover(root)?.ok_or(GitError::NotARepo)?;
    if repo.status().await?.has_tracked_changes() {
        return Err(GitError::Dirty);
    }
    let mut argv: Vec<&OsStr> = vec![OsStr::new("merge")];
    argv.extend(args.iter().map(OsStr::new));
    argv.push(OsStr::new(rev));
    let out = git.run(Mode::Write, root, &argv).await?;
    if out.success() {
        let head = repo.head().await?.unwrap_or_default();
        return Ok(Merged::Done { head });
    }
    let files = repo.conflicts().await?;
    // Nothing may stay half-merged: abort whatever this merge started.
    git.run(
        Mode::Write,
        root,
        &[OsStr::new("merge"), OsStr::new("--abort")],
    )
    .await?;
    if files.is_empty() {
        return Err(GitError::Command {
            op: "merge".into(),
            code: out.code,
            stderr: out.stderr,
        });
    }
    Ok(Merged::Conflicts { files })
}

/// Step 1: the base merged into the worktree at `root`.
pub async fn merge_base_into(git: &Git, root: &Path, base: &str) -> Result<Merged, GitError> {
    merge(git, root, &["--no-edit"], base).await
}

/// Step 3: `branch` merged into the checkout at `base_root` with `--no-ff`.
pub async fn land(git: &Git, base_root: &Path, branch: &str) -> Result<Merged, GitError> {
    merge(git, base_root, &["--no-ff", "--no-edit"], branch).await
}

/// Undo landing: the checkout's branch moved back to `commit`, keeping
/// untracked files (`git reset --keep`).
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
