//! The git panel's actions (F-CD-20): stage, unstage, discard, commit, push,
//! pull, publish, switch and new branch. They're the user's own actions, so
//! they run the `git` CLI with the user's config, hooks and credential
//! helpers, but never a repo-configured fsmonitor ([`Mode::Write`]). Reads
//! (status, diffs, branches) stay in libgit2 ([`Repo`]), hardened by
//! construction (TL-06 rule 6).

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path};
use std::time::Duration;

use crate::cli::{checked_output, Git, Mode, Output};
use crate::repo::Repo;
use crate::types::FileChange;
use crate::GitError;

/// Push, pull and publish stop waiting for the network after this.
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);

/// Repository-relative paths only: no absolute paths, no `..`, no `.git`.
fn checked(paths: &[String]) -> Result<Vec<OsString>, GitError> {
    if paths.is_empty() {
        return Err(GitError::Invalid("No files chosen".into()));
    }
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let path = Path::new(p);
        let ok = !p.is_empty()
            && !p.contains('\0')
            && path.components().all(|c| match c {
                Component::Normal(part) => !part.eq_ignore_ascii_case(".git"),
                Component::CurDir => true,
                _ => false,
            });
        if !ok {
            return Err(GitError::Invalid(
                "That file isn't in this repository".into(),
            ));
        }
        out.push(OsString::from(p));
    }
    Ok(out)
}

async fn write(git: &Git, repo: &Repo, args: Vec<OsString>) -> Result<Output, GitError> {
    let argv: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    git.ok(Mode::Write, repo.root(), &argv).await
}

async fn write_with(
    git: &Git,
    repo: &Repo,
    head: &[&str],
    paths: Vec<OsString>,
) -> Result<(), GitError> {
    let mut args: Vec<OsString> = head.iter().map(OsString::from).collect();
    args.push(OsString::from("--"));
    args.extend(paths);
    write(git, repo, args).await.map(|_| ())
}

/// Stages changes, additions and deletions of `paths` (`git add -A`).
pub async fn stage(git: &Git, repo: &Repo, paths: &[String]) -> Result<(), GitError> {
    write_with(git, repo, &["add", "-A"], checked(paths)?).await
}

/// Stages every change in the working tree, untracked files included
/// (`git add -A`). Ignored files stay out.
pub async fn stage_all(git: &Git, repo: &Repo) -> Result<(), GitError> {
    write(git, repo, vec![OsString::from("add"), OsString::from("-A")])
        .await
        .map(|_| ())
}

/// Takes `paths` out of the index again; before the first commit that
/// means forgetting them (`git rm --cached`).
pub async fn unstage(git: &Git, repo: &Repo, paths: &[String]) -> Result<(), GitError> {
    let paths = checked(paths)?;
    if repo.head().await?.is_none() {
        write_with(git, repo, &["rm", "--cached", "-r", "-q"], paths).await
    } else {
        write_with(git, repo, &["restore", "--staged"], paths).await
    }
}

/// Throws away unstaged changes (the UI has confirmed): tracked files go
/// back to their staged or committed version, untracked files are deleted.
/// Conflicted files are left alone.
pub async fn discard(git: &Git, repo: &Repo, paths: &[String]) -> Result<(), GitError> {
    checked(paths)?;
    let status = repo.status().await?;
    let mut tracked = Vec::new();
    let mut untracked = Vec::new();
    for p in paths {
        match status
            .unstaged
            .iter()
            .find(|f| &f.path == p)
            .map(|f| f.change)
        {
            Some(FileChange::Untracked) => untracked.push(p.clone()),
            Some(FileChange::Conflicted) => {
                return Err(GitError::Invalid(format!(
                    "Resolve the conflict in {p} first"
                )));
            }
            Some(_) => tracked.push(OsString::from(p)),
            None => {}
        }
    }
    if !tracked.is_empty() {
        write_with(git, repo, &["restore", "--worktree"], tracked).await?;
    }
    let root = std::fs::canonicalize(repo.root())?;
    for p in untracked {
        let path = root.join(&p);
        // Never follow a link out of the repository: delete the entry itself.
        let parent = path
            .parent()
            .map(std::fs::canonicalize)
            .transpose()?
            .ok_or_else(|| GitError::Invalid("That file isn't in this repository".into()))?;
        if !parent.starts_with(&root) {
            return Err(GitError::Invalid(
                "That file isn't in this repository".into(),
            ));
        }
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&path)?,
            Ok(_) => std::fs::remove_file(&path)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Commits what's staged with `message` (hooks run). Returns the new commit.
pub async fn commit(git: &Git, repo: &Repo, message: &str) -> Result<String, GitError> {
    if message.trim().is_empty() {
        return Err(GitError::Invalid("Write a commit message first".into()));
    }
    if message.contains('\0') {
        return Err(GitError::Invalid(
            "The message has a character git can't store".into(),
        ));
    }
    if repo.status().await?.staged.is_empty() {
        return Err(GitError::Invalid(
            "Stage the changes to commit first".into(),
        ));
    }
    // The message goes on stdin: any length, any first character.
    let args = [OsStr::new("commit"), OsStr::new("--file=-")];
    git.ok_with_input(Mode::Write, repo.root(), &args, message.as_bytes())
        .await?;
    repo.head()
        .await?
        .ok_or_else(|| GitError::Invalid("The commit didn't happen".into()))
}

/// The remote a new branch is published to: `origin`, or the only one.
pub fn publish_remote(repo: &Repo) -> Result<String, GitError> {
    let r = git2::Repository::open(repo.root())?;
    let names: Vec<String> = r.remotes()?.iter().flatten().map(str::to_string).collect();
    if names.iter().any(|n| n == "origin") {
        return Ok("origin".into());
    }
    match names.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(GitError::Invalid(
            "This repository has no remote to publish to".into(),
        )),
        _ => Err(GitError::Invalid(
            "This repository has several remotes; publish from a terminal to choose one".into(),
        )),
    }
}

/// What a failed network command means, in words, from all of its stderr.
fn network_error(op: &str, stderr: &str) -> Option<GitError> {
    let s = stderr.to_ascii_lowercase();
    if s.contains("authentication failed")
        || s.contains("permission denied")
        || s.contains("could not read username")
        || s.contains("terminal prompts disabled")
        || s.contains("host key verification failed")
    {
        return Some(GitError::Auth(format!(
            "Git couldn't sign in to the remote. {op} once from a terminal so your credentials are saved, then try again."
        )));
    }
    if s.contains("non-fast-forward") || s.contains("fetch first") || s.contains("[rejected]") {
        return Some(GitError::Conflict(
            "The remote has commits you don't have. Pull first.".into(),
        ));
    }
    if s.contains("divergent branches") || s.contains("need to specify how to reconcile") {
        return Some(GitError::Conflict(
            "Your branch and the remote both have new commits. Pull from a terminal to choose merge or rebase.".into(),
        ));
    }
    if s.contains("conflict") {
        return Some(GitError::Conflict(
            "Pulling hit conflicts. Resolve them in the changed files, then commit.".into(),
        ));
    }
    None
}

async fn network(git: &Git, repo: &Repo, op: &str, args: Vec<OsString>) -> Result<(), GitError> {
    let argv: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    match tokio::time::timeout(
        NETWORK_TIMEOUT,
        git.run_verbose(Mode::Write, repo.root(), &argv),
    )
    .await
    {
        Ok(Ok((out, _))) if out.success() => Ok(()),
        Ok(Ok((out, stderr))) => {
            Err(
                network_error(op, &stderr).unwrap_or_else(|| match checked_output(out, &argv) {
                    Err(e) => e,
                    Ok(_) => GitError::Invalid(format!("{op} failed")),
                }),
            )
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(GitError::Invalid(format!(
            "{op} took longer than {} seconds and was stopped",
            NETWORK_TIMEOUT.as_secs()
        ))),
    }
}

/// Pushes the current branch; `publish` first gives it an upstream on the
/// publish remote ([`publish_remote`]).
pub async fn push(git: &Git, repo: &Repo, publish: bool) -> Result<(), GitError> {
    let status = repo.status().await?;
    if status.no_commits {
        return Err(GitError::Invalid("Commit something before pushing".into()));
    }
    let Some(branch) = status.branch else {
        return Err(GitError::Invalid(
            "Switch to a branch before pushing".into(),
        ));
    };
    if status.upstream.is_some() {
        return network(git, repo, "Push", vec![OsString::from("push")]).await;
    }
    if !publish {
        return Err(GitError::Invalid("This branch isn't published yet".into()));
    }
    let remote = publish_remote(repo)?;
    network(
        git,
        repo,
        "Publish",
        vec![
            OsString::from("push"),
            OsString::from("-u"),
            OsString::from(remote),
            OsString::from(format!("HEAD:refs/heads/{branch}")),
        ],
    )
    .await
}

/// Pulls into the current branch the way the user's config says
/// (merge, rebase or fast-forward only).
pub async fn pull(git: &Git, repo: &Repo) -> Result<(), GitError> {
    let status = repo.status().await?;
    if status.upstream.is_none() {
        return Err(GitError::Invalid(
            "This branch has no upstream to pull from".into(),
        ));
    }
    network(
        git,
        repo,
        "Pull",
        vec![OsString::from("pull"), OsString::from("--no-edit")],
    )
    .await
}

/// Switches to a local branch, or to a remote one (`origin/feature`) by
/// creating its local tracking branch.
pub async fn switch(git: &Git, repo: &Repo, name: &str) -> Result<(), GitError> {
    let branches = repo.branches().await?;
    let Some(branch) = branches.iter().find(|b| b.name == name) else {
        return Err(GitError::Invalid(format!("There's no branch named {name}")));
    };
    let args: Vec<OsString> = if branch.remote {
        vec![
            OsString::from("switch"),
            OsString::from("--track"),
            OsString::from(name),
        ]
    } else {
        vec![OsString::from("switch"), OsString::from(name)]
    };
    write(git, repo, args)
        .await
        .map(|_| ())
        .map_err(|e| match &e {
            GitError::Command { stderr, .. } if stderr.contains("would be overwritten") => {
                GitError::Conflict(
                    "Your changes would be overwritten by switching. Commit or stash them first."
                        .into(),
                )
            }
            _ => e,
        })
}

/// Creates `name` at HEAD and switches to it (before the first commit it
/// just renames the branch to be born).
pub async fn create_branch(git: &Git, repo: &Repo, name: &str) -> Result<(), GitError> {
    let name = name.trim();
    if name.is_empty() || !git2::Branch::name_is_valid(name)? || name.starts_with('-') {
        return Err(GitError::Invalid("That isn't a valid branch name".into()));
    }
    if repo.local_branches().await?.iter().any(|b| b == name) {
        return Err(GitError::Conflict(format!(
            "A branch named {name} already exists"
        )));
    }
    if repo.head().await?.is_none() {
        let target = format!("refs/heads/{name}");
        write(
            git,
            repo,
            vec![
                OsString::from("symbolic-ref"),
                OsString::from("HEAD"),
                OsString::from(target),
            ],
        )
        .await
        .map(|_| ())
    } else {
        write(
            git,
            repo,
            vec![
                OsString::from("switch"),
                OsString::from("-c"),
                OsString::from(name),
            ],
        )
        .await
        .map(|_| ())
    }
}
