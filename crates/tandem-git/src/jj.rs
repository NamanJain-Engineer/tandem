//! jj (Jujutsu) workspaces in repositories co-located with git (F-CD-24).
//!
//! A co-located repository has `.jj` beside `.git` at its root. A lane can
//! be a jj workspace (`jj workspace add`) at the same
//! `<repo>/.tandem/worktrees/<slug>` path as a git worktree; it has `.jj`
//! and no `.git`. Its diff comes from `jj diff --git`, read by the same
//! parser as a git lane's, so both show the same files and hunks.
//!
//! jj runs through a [`JjRunner`]: an argument list (never a shell),
//! `--no-pager --color=never`, the login environment and a time limit, and
//! fixed `--config` overrides ([`HARDENED`]) that outrank a repository's own
//! `.jj/repo/config.toml`, so a cloned repository can't make jj run a
//! program (fsmonitor's watchman, a signing program). Vibezeo runs jj only
//! in a repository the person trusts, whose `.jj` is jj's own (not committed
//! to git, its store pointing at this repository's `.git`: [`check_repo`]).
//! It never installs jj, never writes its config, and never runs a command
//! that rewrites or publishes history (`abandon`, `rebase`, `git push`, ...).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tandem_core::lanes::exclude::ensure_tandem_excluded;
use tandem_core::lanes::slug::dedupe;

use crate::error::GitError;
use crate::lanes::{ForceConfirm, LaneManager, WORKTREES_DIR};
use crate::repo::{files_of, Repo};
use crate::slug::slugify;
use crate::types::FileDiff;

/// One jj command's limit (a snapshot of a large working copy takes a while).
pub const JJ_LIMIT: Duration = Duration::from_secs(120);

/// Flags every jj command gets: no pager, no colour.
const COMMON: [&str; 2] = ["--no-pager", "--color=never"];

/// Settings every jj command gets on its command line, which outranks the
/// repository's config: nothing a repository sets runs a program (the
/// filesystem monitor, signing, a pager or an external diff tool).
pub const HARDENED: [&str; 6] = [
    "--config=fsmonitor.backend=\"none\"",
    "--config=signing.behavior=\"drop\"",
    "--config=signing.backend=\"none\"",
    "--config=ui.diff-formatter=\":git\"",
    "--config=ui.pager=\":builtin\"",
    "--config=ui.paginate=\"never\"",
];

/// The whole argument list for `args`: the common flags, the overrides,
/// then the command.
pub fn argv(args: &[&str]) -> Vec<String> {
    COMMON
        .iter()
        .chain(HARDENED.iter())
        .chain(args.iter())
        .map(|a| a.to_string())
        .collect()
}

pub const COMMITTED_JJ: &str =
    "This repository's .jj folder is committed to git, so Vibezeo won't run jj in it";
pub const FOREIGN_STORE: &str =
    "This repository's .jj folder doesn't belong to its git repository, so Vibezeo won't run jj in it";
pub const NOT_COLOCATED: &str = "This repository isn't co-located with jj";

/// The repository at `main` is co-located with jj, and its `.jj` is jj's
/// own: not tracked by git (a clone can't ship one), a real folder, and its
/// store's `git_target` resolving to this repository's `.git`.
pub fn check_repo(main: &Path) -> Result<(), GitError> {
    if !colocated(main) {
        return Err(GitError::Invalid(NOT_COLOCATED.into()));
    }
    let git = git2::Repository::open(main)?;
    let tracked = git.index()?.iter().any(|e| {
        let p = String::from_utf8_lossy(&e.path);
        p == ".jj" || p.starts_with(".jj/")
    });
    if tracked {
        return Err(GitError::Invalid(COMMITTED_JJ.into()));
    }
    let repo_dir = main.join(".jj").join("repo");
    let real_dir = std::fs::symlink_metadata(&repo_dir).is_ok_and(|m| m.is_dir());
    let store = repo_dir.join("store");
    let target = std::fs::read_to_string(store.join("git_target"))
        .ok()
        .map(|t| store.join(t.trim()));
    let resolves = |p: &Path| std::fs::canonicalize(p).ok();
    let ours = match (real_dir, target) {
        (true, Some(t)) => resolves(&t).is_some() && resolves(&t) == resolves(git.path()),
        _ => false,
    };
    if !ours {
        return Err(GitError::Invalid(FOREIGN_STORE.into()));
    }
    Ok(())
}

/// A jj command's result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JjOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl JjOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// Runs jj (the real binary, or a fake in unit tests). `args` come after
/// the common flags. Blocking: callers run it off the async runtime.
pub trait JjRunner: Send + Sync {
    fn run(&self, cwd: &Path, args: &[&str]) -> Result<JjOutput, GitError>;
}

/// The real `jj`, with the login environment.
#[derive(Debug, Clone)]
pub struct Jj {
    program: PathBuf,
    env: Vec<(String, String)>,
    limit: Duration,
}

/// Where jj is usually installed, besides the login PATH.
pub fn usual_places(home: Option<&Path>) -> Vec<PathBuf> {
    let mut out = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
    ];
    if let Some(h) = home {
        out.insert(0, h.join(".nix-profile/bin"));
        out.insert(0, h.join(".local/bin"));
        out.insert(0, h.join(".cargo/bin"));
    }
    out
}

impl Jj {
    /// `jj` on `path` (the login PATH) or in the usual places, run with
    /// `env` (the login environment).
    pub fn locate(path: &str, home: Option<&Path>, env: Vec<(String, String)>) -> Option<Self> {
        let exe = if cfg!(windows) { "jj.exe" } else { "jj" };
        let sep = if cfg!(windows) { ';' } else { ':' };
        path.split(sep)
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .chain(usual_places(home))
            .map(|d| d.join(exe))
            .find(|p| p.is_file())
            .map(|program| Self {
                program,
                env,
                limit: JJ_LIMIT,
            })
    }

    pub fn program(&self) -> &Path {
        &self.program
    }
}

impl JjRunner for Jj {
    fn run(&self, cwd: &Path, args: &[&str]) -> Result<JjOutput, GitError> {
        let mut cmd = std::process::Command::new(&self.program);
        cmd.args(argv(args))
            .current_dir(cwd)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW.
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => GitError::Invalid("jj isn't installed".into()),
            _ => GitError::Io(e),
        })?;
        // Read both pipes while waiting, so a large diff can't fill one and
        // stall jj.
        fn drain<R: std::io::Read + Send + 'static>(
            pipe: Option<R>,
        ) -> std::thread::JoinHandle<Vec<u8>> {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut p) = pipe {
                    let _ = p.read_to_end(&mut buf);
                }
                buf
            })
        }
        let out_t = drain(child.stdout.take());
        let err_t = drain(child.stderr.take());
        let until = Instant::now() + self.limit;
        let status = loop {
            match child.try_wait()? {
                Some(status) => break Some(status),
                None if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
                None => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let stdout = String::from_utf8_lossy(&out_t.join().unwrap_or_default()).into_owned();
        let stderr = String::from_utf8_lossy(&err_t.join().unwrap_or_default()).into_owned();
        match status {
            Some(s) => Ok(JjOutput {
                code: s.code(),
                stdout,
                stderr,
            }),
            None => Err(GitError::Invalid(format!(
                "jj {} didn't finish in {} s",
                args.first().copied().unwrap_or(""),
                self.limit.as_secs()
            ))),
        }
    }
}

/// jj's version line ("0.34.0"), if it runs.
pub fn version(jj: &dyn JjRunner) -> Option<String> {
    let out = checked(jj, Path::new("."), &["--version"]).ok()?;
    if !out.success() {
        return None;
    }
    // "jj 0.34.0-<hash>".
    let v = out.stdout.split_whitespace().nth(1)?;
    Some(v.split('-').next().unwrap_or(v).to_string())
}

/// `root` holds both `.jj` and `.git`: a repository co-located with jj.
pub fn colocated(root: &Path) -> bool {
    root.join(".jj").is_dir() && root.join(".git").exists()
}

/// `root` has `.jj` and no `.git`: a jj-only repository (out of scope), or a
/// secondary jj workspace.
pub fn jj_without_git(root: &Path) -> bool {
    root.join(".jj").is_dir() && !root.join(".git").exists()
}

/// `folder` is inside a jj workspace that git discovery walked past to the
/// repository at `repo_root` (a secondary workspace has no `.git`): git
/// must not act there for it.
pub fn in_jj_workspace(folder: &Path, repo_root: &Path) -> bool {
    folder
        .ancestors()
        .take_while(|a| *a != repo_root)
        .any(jj_without_git)
}

/// The lane briefing for a jj workspace's agents (F-CD-24 AC6): as a git
/// worktree's, plus the one line that matters here: it has no `.git`, so
/// jj, not git.
pub fn briefing(path: &Path, name: &str, siblings: &[String]) -> String {
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
        "You are working in the jj workspace `{}` (workspace `{name}`) of a repository co-located with git. Use `jj`, not `git`, in it: it has no `.git`. Record your work with `jj commit` or `jj new`; never squash, absorb or edit into the commits below your working copy. Sibling worktrees: {others}. Don't modify other worktrees. {}",
        path.display(),
        crate::lanes::CLAIMS_BRIEFING
    )
}

/// A lane made as a jj workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JjLane {
    /// Also the workspace's name.
    pub slug: String,
    pub path: PathBuf,
    /// The change id of its first commit (empty, its own): the lane's
    /// changes are those since that commit's parent.
    pub base: String,
    /// Its record in the pending folder until its row exists.
    pub pending: Option<PathBuf>,
}

/// jj's default workspace: never one of Vibezeo's (a lane's slug is made
/// unique against jj's names, so it's never `default`).
pub const DEFAULT_WORKSPACE: &str = "default";

/// A name Vibezeo could have given a jj workspace: a slug, never `default`.
pub fn is_lane_name(name: &str) -> bool {
    !name.is_empty()
        && name != DEFAULT_WORKSPACE
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A full commit id as jj prints it for a git-backed repository.
fn is_commit_id(id: &str) -> bool {
    id.len() == 40
        && id
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// A full change id: 32 of jj's letters `k` to `z`. A lane's base is its
/// starting commit's change id, which survives that commit being rewritten
/// (a squash or rebase in the main checkout), where a commit id wouldn't.
fn is_change_id(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|c| ('k'..='z').contains(&c))
}

/// A base (a change id, or a commit id from before change ids) as a revset
/// no alias can shadow: aliases expand identifiers, never string literals.
fn commit_revset(id: &str) -> Result<String, GitError> {
    if is_change_id(id) || is_commit_id(id) {
        Ok(format!("\"{id}\""))
    } else {
        Err(GitError::Invalid("jj gave an unexpected commit id".into()))
    }
}

pub const FOREIGN_WORKSPACE: &str = "That folder isn't one of this repository's jj workspaces";
/// A worktree's folder git tracks files in: never removed. The remedy names
/// the folder.
pub fn tracked_folder(slug: &str) -> String {
    format!(
        "The repository's git index has files in this worktree's folder, so Vibezeo won't remove it. Run `git rm -r --cached {WORKTREES_DIR}/{slug}` in the repository, then discard it again"
    )
}

/// Whether git's index has entries at or under `.tandem/worktrees/<slug>`
/// (a folder the repository tracks is never removed). Fails closed: an
/// index that can't be read counts as tracked.
pub fn tracked_under(repo_root: &Path, slug: &str) -> bool {
    let prefix = format!("{WORKTREES_DIR}/{slug}");
    let Ok(git) = git2::Repository::open(repo_root) else {
        return true;
    };
    let Ok(index) = git.index() else {
        return true;
    };
    let tracked = index.iter().any(|e| {
        let path = String::from_utf8_lossy(&e.path);
        path == prefix || path.starts_with(&format!("{prefix}/"))
    });
    tracked
}

/// jj's own words for a working copy that a rewrite elsewhere left behind.
const STALE_WORDS: &str = "working copy is stale";

/// What Vibezeo says for a stale working copy, naming the folder it's in
/// (a lane, the handoff's source, or the main checkout).
pub fn stale(folder: &Path) -> String {
    format!(
        "The jj workspace at {} is out of date: run `jj workspace update-stale` there, then try again",
        folder.display()
    )
}

/// Runs jj in `cwd`; a stale working copy there is a Conflict that says
/// which folder and what to run.
fn checked(jj: &dyn JjRunner, cwd: &Path, args: &[&str]) -> Result<JjOutput, GitError> {
    let out = jj.run(cwd, args)?;
    if !out.success() && out.stderr.contains(STALE_WORDS) {
        return Err(GitError::Conflict(stale(cwd)));
    }
    Ok(out)
}

fn ok_or(out: JjOutput, op: &str) -> Result<JjOutput, GitError> {
    if out.success() {
        Ok(out)
    } else {
        let line: String = out
            .stderr
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .chars()
            .take(300)
            .collect();
        Err(GitError::Invalid(format!("jj {op} failed: {line}")))
    }
}

/// The workspace names jj knows in the repository at `root` (without a
/// snapshot of `root`'s working copy: the person's main checkout).
pub fn workspace_names(jj: &dyn JjRunner, root: &Path) -> Result<HashSet<String>, GitError> {
    let out = ok_or(
        checked(
            jj,
            root,
            &[
                "--ignore-working-copy",
                "workspace",
                "list",
                "-T",
                "self.name() ++ \"\\n\"",
            ],
        )?,
        "workspace list",
    )?;
    Ok(out
        .stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// The change id of the working copy's parent in the workspace at
/// `ws_root` (its first, for a merge).
pub fn base_of(jj: &dyn JjRunner, ws_root: &Path) -> Result<String, GitError> {
    let out = ok_or(
        checked(
            jj,
            ws_root,
            &[
                "log",
                "-r",
                "@-",
                "--no-graph",
                "-T",
                "self.change_id() ++ \"\\n\"",
            ],
        )?,
        "log",
    )?;
    let id = out
        .stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
        .ok_or_else(|| GitError::Invalid("The new jj workspace has no parent commit".into()))?;
    if !is_change_id(&id) {
        return Err(GitError::Invalid("jj gave an unexpected change id".into()));
    }
    Ok(id)
}

/// A handoff's starting point: a copy of the source workspace's working-copy
/// commit (`jj duplicate @`, run in the source, which snapshots it first).
/// The new lane sits on the copy, so the source's later edits (which
/// rewrite its working-copy commit) never rebase the lane or leave it stale.
/// The copy has what jj snapshotted: files jj refused or doesn't track in
/// the source aren't in it. Returns the copy's change id.
fn fork_of(jj: &dyn JjRunner, main: &Path, source: &Path) -> Result<String, GitError> {
    // jj prints the copy with `templates.commit_summary`, and says nothing
    // with `ui.quiet`, both of which config can set: fixed for this call.
    let out = ok_or(
        checked(
            jj,
            source,
            &[
                "--config=ui.quiet=false",
                "--config=templates.commit_summary=\"self.change_id()\"",
                "duplicate",
                "@",
            ],
        )?,
        "duplicate",
    )?;
    // "Duplicated <commit> as <change id>".
    let text = format!("{}\n{}", out.stdout, out.stderr);
    let id = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("Duplicated "))
        .and_then(|rest| rest.rsplit_once(" as "))
        .map(|(_, id)| id.trim().to_string())
        .filter(|id| is_change_id(id))
        .ok_or_else(|| GitError::Invalid("jj duplicate gave an unexpected answer".into()))?;
    // And jj, in the repository, knows that change as itself.
    let quoted = format!("\"{id}\"");
    let back = ok_or(
        checked(
            jj,
            main,
            &[
                "--ignore-working-copy",
                "log",
                "-r",
                &quoted,
                "--no-graph",
                "-T",
                "self.change_id() ++ \"\\n\"",
            ],
        )?,
        "log",
    )?;
    if back.stdout.trim() != id {
        return Err(GitError::Invalid(
            "jj duplicate gave an unexpected answer".into(),
        ));
    }
    Ok(id)
}

/// `jj workspace add <repo>/.tandem/worktrees/<slug> --name <slug>` in the
/// co-located repository `repo`, then `jj new` in it. `from`: another
/// workspace's name, whose working copy it starts from (a copy of it, made
/// by [`fork_of`]: a handoff); otherwise jj starts it beside the default
/// workspace's working copy. jj snapshots the main checkout while adding.
///
/// The `jj new` leaves the workspace's first commit (empty, the lane's own)
/// as its base: the lane's diff is from that commit's parent, so work an
/// agent squashes into it still shows, and nothing it squashes reaches the
/// person's commits.
///
/// `pending`: a folder of Vibezeo's own (never in the repository) where a
/// record of the workspace being made stays until its row exists
/// ([`done_adding`]), so a crash in between is found by the sweep
/// ([`forget_orphans`]).
pub async fn add_workspace(
    jj: Arc<dyn JjRunner>,
    repo: &Repo,
    slug_source: &str,
    also_taken: &HashSet<String>,
    from: Option<String>,
    pending: Option<PathBuf>,
) -> Result<JjLane, GitError> {
    let main = repo.main_root();
    check_repo(&main)?;
    if from.as_deref().is_some_and(|f| !is_lane_name(f)) {
        return Err(GitError::Invalid(
            "That isn't a jj workspace Vibezeo made".into(),
        ));
    }
    if let Some(dir) = &pending {
        std::fs::create_dir_all(dir)?;
    }
    let repo_key = std::fs::canonicalize(&main).unwrap_or_else(|_| main.clone());
    let mut taken = LaneManager::taken_slugs(repo).await?;
    taken.extend(also_taken.iter().cloned());
    // Never a lane named `default`, even when jj's default workspace was
    // renamed: Discard would refuse to forget it.
    taken.insert(DEFAULT_WORKSPACE.to_string());
    ensure_tandem_excluded(repo.common_dir())?;
    let dir = main.join(WORKTREES_DIR);
    std::fs::create_dir_all(&dir)?;
    let source = slug_source.to_string();
    tokio::task::spawn_blocking(move || {
        taken.extend(workspace_names(jj.as_ref(), &main)?);
        // A handoff starts from a copy of the source's working copy.
        let fork = match &from {
            Some(name) => {
                let source = dir.join(name);
                if main_of_workspace(&source).as_deref() != Some(repo_key.as_path()) {
                    return Err(GitError::Invalid(FOREIGN_WORKSPACE.into()));
                }
                Some(format!("\"{}\"", fork_of(jj.as_ref(), &main, &source)?))
            }
            None => None,
        };
        let start = fork.clone();
        let made = (|| {
            // A slug another create took meanwhile: the next one.
            for _ in 0..8 {
                let slug = dedupe(&slugify(&source), &taken);
                let path = dir.join(&slug);
                if path.exists() {
                    taken.insert(slug);
                    continue;
                }
                let path_s = path.to_string_lossy().into_owned();
                // Recorded before jj makes it, outside the repository.
                let marker = match &pending {
                    Some(p) => Some(write_pending(p, &repo_key, &slug)?),
                    None => None,
                };
                let unmark = || {
                    if let Some(m) = &marker {
                        let _ = std::fs::remove_file(m);
                    }
                };
                let mut args = vec!["workspace", "add", path_s.as_str(), "--name", slug.as_str()];
                if let Some(r) = &start {
                    args.push("--revision");
                    args.push(r.as_str());
                }
                let out = match checked(&*jj, &main, &args) {
                    Ok(out) => out,
                    Err(e) => {
                        unmark();
                        return Err(e);
                    }
                };
                if !out.success() && out.stderr.contains("already exists") {
                    unmark();
                    taken.insert(slug);
                    continue;
                }
                if let Err(e) = ok_or(out, "workspace add") {
                    unmark();
                    return Err(e);
                }
                let base = match ok_or(checked(&*jj, &path, &["new"])?, "new")
                    .and_then(|_| base_of(jj.as_ref(), &path))
                    .and_then(|b| verify_base(jj.as_ref(), &path, &main, &b).map(|_| b))
                {
                    Ok(b) => b,
                    Err(e) => {
                        // Nothing half-made stays behind.
                        let _ = checked(
                            &*jj,
                            &main,
                            &[
                                "--ignore-working-copy",
                                "workspace",
                                "forget",
                                slug.as_str(),
                            ],
                        );
                        let _ = std::fs::remove_dir_all(&path);
                        unmark();
                        return Err(e);
                    }
                };
                return Ok(JjLane {
                    path: std::fs::canonicalize(&path).unwrap_or(path),
                    slug,
                    base,
                    pending: marker,
                });
            }
            Err(GitError::Conflict(
                "Couldn't find a free name for the jj workspace".into(),
            ))
        })();
        // The handoff's copy goes with a create that failed.
        if let (Err(_), Some(copy)) = (&made, &fork) {
            let _ = checked(
                &*jj,
                &main,
                &["--ignore-working-copy", "abandon", copy.as_str()],
            );
        }
        made
    })
    .await
    .map_err(|e| GitError::Invalid(e.to_string()))?
}

/// A pending record, as JSON (a folder name with a newline in it can't
/// shift its fields): the repository's canonical folder and the slug.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Pending {
    repo: PathBuf,
    slug: String,
}

/// Writes a pending record (also how tests make a stale one).
pub fn write_pending(dir: &Path, repo: &Path, slug: &str) -> Result<PathBuf, GitError> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = dir.join(format!("{slug}-{nanos}{PENDING}"));
    let record = Pending {
        repo: repo.to_path_buf(),
        slug: slug.to_string(),
    };
    let bytes = serde_json::to_vec(&record).map_err(|e| GitError::Invalid(e.to_string()))?;
    std::fs::write(&path, bytes)?;
    Ok(path)
}

const PENDING: &str = ".pending";

/// The records in `pending`, with their paths and ages.
fn read_pending(pending: &Path) -> Vec<(PathBuf, Pending, Option<std::time::SystemTime>)> {
    let Ok(entries) = std::fs::read_dir(pending) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(PENDING))
        .filter_map(|e| {
            let record: Pending = serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?;
            let written = e.metadata().and_then(|m| m.modified()).ok();
            Some((e.path(), record, written))
        })
        .collect()
}

/// The workspace's row exists: its pending record goes.
pub fn done_adding(lane: &JjLane) {
    if let Some(p) = &lane.pending {
        let _ = std::fs::remove_file(p);
    }
}

/// The main repository folder a jj workspace at `ws_root` belongs to: its
/// `.jj/repo` is a file naming `<main>/.jj/repo`.
pub fn main_of_workspace(ws_root: &Path) -> Option<PathBuf> {
    let link = ws_root.join(".jj").join("repo");
    if !std::fs::symlink_metadata(&link).is_ok_and(|m| m.is_file()) {
        return None;
    }
    let text = std::fs::read_to_string(&link).ok()?;
    let named = PathBuf::from(text.trim());
    let named = if named.is_absolute() {
        named
    } else {
        ws_root.join(".jj").join(named)
    };
    let repo = std::fs::canonicalize(named).ok()?;
    // `<main>/.jj/repo` → `<main>`.
    repo.parent()?.parent().map(Path::to_path_buf)
}

/// Why the sweep left a record's workspace alone (it deleted the record).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    /// A lane row has the name: the workspace is live.
    HasRow,
    /// Not a name Vibezeo makes.
    NotOurs,
    /// No folder at `<repo>/.tandem/worktrees/<slug>`, or a symlink.
    NoFolder,
    /// The folder isn't a jj workspace of this repository.
    OtherWorkspace,
    /// The folder was made well before the record: someone else's.
    Older,
    /// The repository's git index has files in the folder.
    Tracked,
}

/// What the sweep decides for one record (pure, so it's tested everywhere).
pub fn orphan_decision(
    repo_root: &Path,
    slug: &str,
    written: Option<std::time::SystemTime>,
    has_row: &HashSet<String>,
) -> Result<PathBuf, Kept> {
    if !is_lane_name(slug) {
        return Err(Kept::NotOurs);
    }
    if has_row.contains(slug) {
        return Err(Kept::HasRow);
    }
    let folder = repo_root.join(WORKTREES_DIR).join(slug);
    let meta = std::fs::symlink_metadata(&folder);
    if !meta.as_ref().is_ok_and(|m| m.is_dir()) {
        return Err(Kept::NoFolder);
    }
    let main = std::fs::canonicalize(repo_root).ok();
    if main.is_none() || main_of_workspace(&folder) != main {
        return Err(Kept::OtherWorkspace);
    }
    if tracked_under(repo_root, slug) {
        return Err(Kept::Tracked);
    }
    // jj made the folder just after the record was written; a folder made
    // well before it is someone else's (its birth time, where the
    // filesystem keeps one).
    let born = meta.ok().and_then(|m| m.created().ok());
    if let (Some(born), Some(written)) = (born, written) {
        if born + Duration::from_secs(5) < written {
            return Err(Kept::Older);
        }
    }
    Ok(folder)
}

/// jj workspaces whose making was cut short: records in `pending` (a
/// folder of Vibezeo's own, which the repository can't write) for the
/// repository at `repo_root`, older than `age`. A record is only ever
/// acted on when nothing else could own its workspace ([`orphan_decision`]:
/// no lane row has the name, the folder at `.tandem/worktrees/<slug>` is
/// that repository's jj workspace and was made after the record); its
/// workspace is then forgotten and its folder removed, as Discard would
/// without force (a working copy that can't be snapshotted whole is kept),
/// and its commits stay. Otherwise only the record goes. Never `default`.
/// A failed forget is logged and its record kept for the next sweep (until
/// [`prune_pending`] drops it). Returns how many were forgotten. The caller
/// holds the repository's lock throughout.
pub async fn forget_orphans(
    jj: Arc<dyn JjRunner>,
    pending: &Path,
    repo_root: &Path,
    age: Duration,
    has_row: &HashSet<String>,
) -> Result<usize, GitError> {
    let repo_key = std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    let mut n = 0;
    for (path, record, written) in read_pending(pending) {
        if record.repo != repo_key {
            continue;
        }
        let old = written
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|d| d >= age);
        if !old {
            continue;
        }
        match orphan_decision(repo_root, &record.slug, written, has_row) {
            Ok(folder) => {
                match forget_workspace(jj.clone(), repo_root, &folder, &record.slug, None).await {
                    Ok(()) => {
                        let _ = std::fs::remove_file(&path);
                        n += 1;
                    }
                    Err(e) => tracing::warn!(
                        code = "jj.orphan_forget_failed",
                        error = %e,
                        "an unfinished jj workspace wasn't forgotten; the next sweep tries again"
                    ),
                }
            }
            Err(why) => {
                tracing::info!(code = "jj.orphan_kept", reason = ?why, "a pending record's workspace was left alone");
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(n)
}

/// Every record older than `age` goes, readable or not, whichever
/// repository it names: one the sweep never reaches (a repository removed
/// from the rail, untrusted, without jj, or a record from an older
/// version) mustn't stay forever. Nothing is forgotten for them.
pub fn prune_pending(pending: &Path, age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(pending) else {
        return 0;
    };
    let mut n = 0;
    for e in entries.flatten() {
        if !e.file_name().to_string_lossy().ends_with(PENDING) {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|d| d >= age);
        if old && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

pub const GONE_BASE: &str = "This jj workspace's first commit was abandoned, so Vibezeo can't tell what changed in it. Its commits are still in `jj log`; discard it when you're done with them";

pub const SHADOWED_BASE: &str =
    "A ref named like this worktree's base commit hides it, so Vibezeo won't use it";

/// The base, checked before it's used as a revset: the folder is a jj
/// workspace of the repository at `repo_root` (else nothing is run), the
/// base is a change id (or, from before them, a commit id), no git ref has
/// its name (a tag or branch named like it would be resolved first, and
/// comes with a clone, no config needed), and jj resolves it to itself.
/// Returns the revset to diff from (the base's parent, for a change id).
pub fn verify_base(
    jj: &dyn JjRunner,
    ws_root: &Path,
    repo_root: &Path,
    base: &str,
) -> Result<String, GitError> {
    let revset = commit_revset(base)?;
    let main = std::fs::canonicalize(repo_root).ok();
    if main.is_none() || main_of_workspace(ws_root) != main {
        return Err(GitError::Invalid(FOREIGN_WORKSPACE.into()));
    }
    let git = git2::Repository::open(repo_root)?;
    // jj's own `refs/jj/keep/<id>` refs (they keep its commits alive) are
    // named after commits; a tag, branch or remote ref isn't.
    let shadowed = git.references()?.flatten().any(|r| {
        r.name().is_some_and(|n| {
            !n.starts_with("refs/jj/") && (n == base || n.ends_with(&format!("/{base}")))
        })
    });
    if shadowed {
        return Err(GitError::Invalid(SHADOWED_BASE.into()));
    }
    let template = if is_change_id(base) {
        "self.change_id() ++ \"\\n\""
    } else {
        "self.commit_id() ++ \"\\n\""
    };
    let out = checked(
        jj,
        ws_root,
        &["log", "-r", &revset, "--no-graph", "-T", template],
    )?;
    if !out.success() && out.stderr.contains("doesn't exist") {
        return Err(GitError::Invalid(GONE_BASE.into()));
    }
    let out = ok_or(out, "log")?;
    if out.stdout.trim() != base {
        return Err(GitError::Invalid(SHADOWED_BASE.into()));
    }
    // A lane's base is its own first commit: its changes are from that
    // commit's parent. An older base (a commit id) is the parent itself.
    if is_change_id(base) {
        Ok(format!("{revset}-"))
    } else {
        Ok(revset)
    }
}

/// The workspace's changes since `base`, working copy included, as git
/// lanes' are: jj's git-format diff read by the same parser.
pub async fn diff(
    jj: Arc<dyn JjRunner>,
    repo_root: &Path,
    ws_root: &Path,
    base: &str,
) -> Result<Vec<FileDiff>, GitError> {
    let main = repo_root.to_path_buf();
    let root = ws_root.to_path_buf();
    let base = base.to_string();
    tokio::task::spawn_blocking(move || {
        let base = verify_base(jj.as_ref(), &root, &main, &base)?;
        let out = ok_or(
            checked(
                &*jj,
                &root,
                &["diff", "--git", "--from", &base, "--to", "@"],
            )?,
            "diff",
        )?;
        parse_git_patch(out.stdout.as_bytes())
    })
    .await
    .map_err(|e| GitError::Invalid(e.to_string()))?
}

/// A git-format patch as [`FileDiff`]s, through the same code as a git
/// lane's diff (so counts, truncation and headers match).
pub fn parse_git_patch(patch: &[u8]) -> Result<Vec<FileDiff>, GitError> {
    if patch.iter().all(u8::is_ascii_whitespace) {
        return Ok(Vec::new());
    }
    let diff = git2::Diff::from_buffer(patch)?;
    files_of(&diff)
}

/// The files the workspace changed since `base` (for the overlap check).
pub async fn changed_files(
    jj: Arc<dyn JjRunner>,
    repo_root: &Path,
    ws_root: &Path,
    base: &str,
) -> Result<Vec<String>, GitError> {
    let main = repo_root.to_path_buf();
    let root = ws_root.to_path_buf();
    let base = base.to_string();
    tokio::task::spawn_blocking(move || {
        let base = verify_base(jj.as_ref(), &root, &main, &base)?;
        let out = ok_or(
            checked(
                &*jj,
                &root,
                &["diff", "--name-only", "--from", &base, "--to", "@"],
            )?,
            "diff",
        )?;
        let mut files: Vec<String> = out
            .stdout
            .lines()
            .map(|l| l.trim().replace('\\', "/"))
            .filter(|l| !l.is_empty())
            .collect();
        files.sort();
        files.dedup();
        Ok(files)
    })
    .await
    .map_err(|e| GitError::Invalid(e.to_string()))?
}

/// The working-copy commit id of the workspace named `name`, read in the
/// repository without a snapshot of the person's checkout.
fn named_working_copy(jj: &dyn JjRunner, repo_root: &Path, name: &str) -> Option<String> {
    let rev = format!("\"{name}\"@");
    working_copy(jj, repo_root, &rev)
}

fn working_copy(jj: &dyn JjRunner, cwd: &Path, rev: &str) -> Option<String> {
    let out = jj
        .run(
            cwd,
            &[
                "--ignore-working-copy",
                "log",
                "-r",
                rev,
                "--no-graph",
                "-T",
                "self.commit_id() ++ \"\\n\"",
            ],
        )
        .ok()?;
    let id = out.stdout.trim().to_string();
    (out.success() && is_commit_id(&id)).then_some(id)
}

/// A folder in a lane that holds another repository (`.git` or `.jj`
/// below the lane's own `.jj`): jj never snapshots it, so removing the lane
/// would lose its history. Symlinks aren't followed.
fn nested_repository(ws_root: &Path) -> bool {
    let mut stack = vec![ws_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else {
                continue;
            };
            let name = e.file_name();
            let top_jj = dir == ws_root && name == ".jj";
            if (name == ".git" || name == ".jj") && !top_jj {
                return true;
            }
            if kind.is_dir() && !top_jj {
                stack.push(e.path());
            }
        }
    }
    false
}

/// Whether jj kept everything in the workspace at `ws_root`, named `name`,
/// in its commit: `jj status` there (which snapshots it) ran and printed no
/// warning at all (a refused file, or any other: one a later jj words
/// differently still counts), left nothing untracked, and found its
/// working copy; jj still lists `name`, the folder's working-copy commit
/// reads back, and no nested repository is in it. Otherwise Dirty (a stale
/// working copy: Conflict, saying what to run).
pub fn kept_whole(
    jj: &dyn JjRunner,
    repo_root: &Path,
    ws_root: &Path,
    name: &str,
) -> Result<(), GitError> {
    let out = match checked(jj, ws_root, &["status"]) {
        Ok(out) => out,
        Err(e @ GitError::Conflict(_)) => return Err(e),
        Err(_) => return Err(GitError::Dirty),
    };
    if !out.success()
        || out.stderr.contains("Warning:")
        || out.stdout.contains("Untracked paths:")
        || out.stdout.contains("No working copy")
    {
        return Err(GitError::Dirty);
    }
    // jj snapshots only a workspace still in its view: one forgotten (or
    // undone) while the agent kept editing has nothing of it kept.
    if working_copy(jj, ws_root, "@").is_none() || !workspace_names(jj, repo_root)?.contains(name) {
        return Err(GitError::Dirty);
    }
    if nested_repository(ws_root) {
        return Err(GitError::Dirty);
    }
    Ok(())
}

/// Where a Discard stands before anything runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folder {
    /// Already gone: nothing on disk to lose.
    Gone,
    /// There, this repository's jj workspace, and not tracked by git.
    Present,
}

/// Discard's checks that need no jj (so they run before anything stops):
/// a name Vibezeo makes (never `default`), the folder
/// `<repo>/.tandem/worktrees/<name>` (no symlink), and, when it's there,
/// nothing in it tracked by git's index and its `.jj/repo` naming this
/// repository's.
pub fn forget_checks(repo_root: &Path, ws_root: &Path, name: &str) -> Result<Folder, GitError> {
    if !is_lane_name(name) {
        return Err(GitError::Invalid(
            "That isn't a jj workspace Vibezeo made".into(),
        ));
    }
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let dir = canon(&repo_root.join(WORKTREES_DIR));
    let parent = ws_root.parent().map(canon);
    let ours = parent.as_deref() == Some(dir.as_path())
        && ws_root.file_name() == Some(std::ffi::OsStr::new(name))
        && !std::fs::symlink_metadata(ws_root).is_ok_and(|m| m.file_type().is_symlink());
    if !ours {
        return Err(GitError::Invalid(
            "That folder isn't one of this repository's worktrees".into(),
        ));
    }
    if !std::fs::symlink_metadata(ws_root).is_ok_and(|m| m.is_dir()) {
        return Ok(Folder::Gone);
    }
    if tracked_under(repo_root, name) {
        return Err(GitError::Invalid(tracked_folder(name)));
    }
    let main = std::fs::canonicalize(repo_root).ok();
    if main.is_none() || main_of_workspace(ws_root) != main {
        return Err(GitError::Invalid(FOREIGN_WORKSPACE.into()));
    }
    Ok(Folder::Present)
}

fn forget_name(jj: &dyn JjRunner, repo_root: &Path, name: &str) -> Result<(), GitError> {
    ok_or(
        checked(
            jj,
            repo_root,
            &["--ignore-working-copy", "workspace", "forget", name],
        )?,
        "workspace forget",
    )?;
    Ok(())
}

/// Discard: snapshot the working copy (so what's in it stays in the
/// workspace's commit), `jj workspace forget <name>` in the repository,
/// then remove the folder. The workspace's commits stay.
///
/// Without `force` (a person's confirmation, [`ForceConfirm`]), anything
/// [`kept_whole`] can't vouch for is [`GitError::Dirty`], as a git lane's
/// uncommitted changes are: removing the folder would lose what's only on
/// disk. Even forced, a folder [`forget_checks`] refuses is never removed,
/// and the name is forgotten only when it's this folder's workspace (the
/// same working-copy commit read from both sides). A folder already gone:
/// its registration is forgotten when jj lists the name (Vibezeo's slugs
/// are never names jj already had, so it's the lane's).
pub async fn forget_workspace(
    jj: Arc<dyn JjRunner>,
    repo_root: &Path,
    ws_root: &Path,
    name: &str,
    force: Option<ForceConfirm>,
) -> Result<(), GitError> {
    let folder = forget_checks(repo_root, ws_root, name)?;
    let repo_root = repo_root.to_path_buf();
    let ws_root = ws_root.to_path_buf();
    let name = name.to_string();
    tokio::task::spawn_blocking(move || {
        let known = workspace_names(jj.as_ref(), &repo_root)?;
        if folder == Folder::Gone {
            if known.contains(&name) {
                forget_name(jj.as_ref(), &repo_root, &name)?;
            }
            return Ok(());
        }
        if force.is_none() {
            kept_whole(jj.as_ref(), &repo_root, &ws_root, &name)?;
        }
        if known.contains(&name) {
            let named = named_working_copy(jj.as_ref(), &repo_root, &name);
            let here = working_copy(jj.as_ref(), &ws_root, "@");
            match (named, here) {
                (Some(n), Some(h)) if n == h => forget_name(jj.as_ref(), &repo_root, &name)?,
                // The folder's workspace is gone from jj's view (forced):
                // the name now belongs to something else; it stays.
                (_, None) => {}
                _ => return Err(GitError::Invalid(FOREIGN_WORKSPACE.into())),
            }
        }
        match std::fs::remove_dir_all(&ws_root) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(GitError::Io(e)),
        }
    })
    .await
    .map_err(|e| GitError::Invalid(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Records argv and answers from a table (the first prefix that
    /// matches). With `makes`, `workspace add <path>` makes a workspace
    /// folder of that repository there, as jj would.
    #[derive(Default)]
    struct FakeJj {
        calls: Mutex<Vec<(PathBuf, Vec<String>)>>,
        answers: Vec<(String, JjOutput)>,
        makes: Option<PathBuf>,
    }

    impl FakeJj {
        fn answering(answers: Vec<(&str, JjOutput)>) -> Self {
            Self {
                calls: Mutex::default(),
                answers: answers
                    .into_iter()
                    .map(|(p, o)| (p.to_string(), o))
                    .collect(),
                makes: None,
            }
        }
        fn making(mut self, main: &Path) -> Self {
            self.makes = Some(main.to_path_buf());
            self
        }
        fn calls(&self) -> Vec<Vec<String>> {
            self.calls
                .lock()
                .expect("calls")
                .iter()
                .map(|(_, a)| a.clone())
                .collect()
        }
    }

    impl JjRunner for FakeJj {
        fn run(&self, cwd: &Path, args: &[&str]) -> Result<JjOutput, GitError> {
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            self.calls
                .lock()
                .expect("calls")
                .push((cwd.to_path_buf(), args.clone()));
            if let (Some(main), ["workspace", "add", path, ..]) =
                (&self.makes, args_of(&args).as_slice())
            {
                let ws = PathBuf::from(path);
                std::fs::create_dir_all(ws.join(".jj/working_copy"))?;
                std::fs::write(
                    ws.join(".jj/repo"),
                    main.join(".jj/repo").to_string_lossy().as_bytes(),
                )?;
            }
            let joined = args.join(" ");
            Ok(self
                .answers
                .iter()
                .find(|(prefix, _)| joined.starts_with(prefix))
                .map(|(_, out)| out.clone())
                .unwrap_or(JjOutput {
                    code: Some(0),
                    ..Default::default()
                }))
        }
    }

    fn args_of(args: &[String]) -> Vec<&str> {
        args.iter().map(String::as_str).collect()
    }

    fn failing(stderr: &str) -> JjOutput {
        JjOutput {
            code: Some(1),
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }

    fn ok(stdout: &str) -> JjOutput {
        JjOutput {
            code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    #[test]
    fn f_cd_24_version_and_detection() {
        let fake = FakeJj::answering(vec![("--version", ok("jj 0.34.0-22900c9a\n"))]);
        assert_eq!(version(&fake).as_deref(), Some("0.34.0"));
        let tmp = tempfile::tempdir().expect("tmp");
        let root = tmp.path();
        assert!(!colocated(root));
        std::fs::create_dir(root.join(".jj")).expect("jj");
        assert!(jj_without_git(root));
        std::fs::create_dir(root.join(".git")).expect("git");
        assert!(colocated(root));
        assert!(!jj_without_git(root));
        // Found in a usual place when the login PATH lacks it.
        let home = root.join("home");
        std::fs::create_dir_all(home.join(".cargo/bin")).expect("mkdir");
        let exe = if cfg!(windows) { "jj.exe" } else { "jj" };
        std::fs::write(home.join(".cargo/bin").join(exe), "").expect("write");
        let found = Jj::locate("/nowhere", Some(&home), Vec::new()).expect("found");
        assert_eq!(found.program(), home.join(".cargo/bin").join(exe));
    }

    #[test]
    fn f_cd_24_names_bases_and_files_from_jjs_output() {
        let fake = FakeJj::answering(vec![
            (
                "--ignore-working-copy workspace list",
                ok("default\ntidy\n"),
            ),
            ("log -r @-", ok(&format!("{CID}\n"))),
        ]);
        let names = workspace_names(&fake, Path::new("/r")).expect("names");
        assert_eq!(names, HashSet::from(["default".into(), "tidy".into()]));
        assert_eq!(
            base_of(&fake, Path::new("/r/.tandem/worktrees/tidy")).expect("base"),
            CID
        );
        // A base is a change id (it survives its commit being rewritten).
        let hex = FakeJj::answering(vec![("log -r @-", ok(&format!("{HEX}\n")))]);
        assert!(base_of(&hex, Path::new("/r/.tandem/worktrees/tidy")).is_err());
        // No shell, and the common flags come from the runner, not here.
        assert!(fake
            .calls()
            .iter()
            .all(|a| !a.iter().any(|x| x == "--config")));
    }

    #[test]
    fn f_cd_24_jjs_git_patch_reads_as_a_git_lanes_does() {
        let patch = "diff --git a/a.txt b/a.txt\nindex 7898192261..422c2b7ab3 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,2 @@\n a\n+b\ndiff --git a/n.txt b/n.txt\nnew file mode 100644\nindex 0000000000..3e757656cf\n--- /dev/null\n+++ b/n.txt\n@@ -0,0 +1,1 @@\n+new\n";
        let files = parse_git_patch(patch.as_bytes()).expect("parsed");
        let summary: Vec<(String, crate::types::FileChange, u32, u32)> = files
            .iter()
            .map(|f| (f.path.clone(), f.change, f.additions, f.deletions))
            .collect();
        assert_eq!(
            summary,
            [
                ("a.txt".into(), crate::types::FileChange::Modified, 1, 0),
                ("n.txt".into(), crate::types::FileChange::Added, 1, 0),
            ]
        );
        assert!(files[0].patch.contains("+b"));
        assert!(parse_git_patch(b"").expect("empty").is_empty());
    }

    #[test]
    fn f_cd_24_3_a_folder_in_a_jj_workspace_is_seen_as_one() {
        let tmp = tempfile::tempdir().expect("tmp");
        let repo = tmp.path();
        std::fs::create_dir_all(repo.join(".git")).expect("git");
        std::fs::create_dir_all(repo.join(".jj")).expect("jj");
        let ws = repo.join(".tandem/worktrees/a");
        std::fs::create_dir_all(ws.join(".jj")).expect("ws");
        std::fs::create_dir_all(ws.join("src")).expect("src");
        std::fs::create_dir_all(repo.join("src")).expect("src");
        assert!(in_jj_workspace(&ws.join("src"), repo));
        assert!(in_jj_workspace(&ws, repo));
        assert!(!in_jj_workspace(&repo.join("src"), repo));
        assert!(!in_jj_workspace(repo, repo));
    }

    #[test]
    fn f_cd_24_6_the_briefing_says_jj_not_git() {
        let b = briefing(
            Path::new("/r/.tandem/worktrees/a"),
            "a",
            &["tandem/b".into()],
        );
        assert!(b.contains("Use `jj`, not `git`"), "{b}");
        assert!(b.contains("`tandem/b`"));
        assert!(!b.contains("git worktree"));
    }

    #[test]
    fn f_cd_24_a_failing_jj_says_what_failed_in_one_line() {
        let out = JjOutput {
            code: Some(1),
            stdout: String::new(),
            stderr: "Error: Workspace already exists\nHint: something\n".into(),
        };
        let e = ok_or(out, "workspace add").expect_err("failed");
        assert_eq!(
            e.to_string(),
            "jj workspace add failed: Error: Workspace already exists"
        );
    }

    const HEX: &str = "0123456789abcdef0123456789abcdef01234567";
    const CID: &str = "zzkkxyzwqrstuvwxyzkllmmnnoppqqrs";

    /// A git repository with a `.jj` of jj's shape beside `.git` (no jj
    /// needed): `.jj/repo/store/git_target` points at `.git`.
    fn colocated_fixture() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("tmp");
        let root = std::fs::canonicalize(tmp.path())
            .expect("canonical")
            .join("repo");
        std::fs::create_dir_all(&root).expect("mkdir");
        git2::Repository::init(&root).expect("git init");
        let store = root.join(".jj/repo/store");
        std::fs::create_dir_all(&store).expect("mkdir");
        std::fs::write(store.join("git_target"), "../../../.git").expect("write");
        (tmp, root)
    }

    /// A jj workspace folder of `main` at `<main>/.tandem/worktrees/<slug>`.
    fn workspace_folder(main: &Path, slug: &str) -> PathBuf {
        let ws = main.join(WORKTREES_DIR).join(slug);
        std::fs::create_dir_all(ws.join(".jj/working_copy")).expect("mkdir");
        std::fs::write(
            ws.join(".jj/repo"),
            main.join(".jj/repo").to_string_lossy().as_bytes(),
        )
        .expect("write");
        ws
    }

    #[test]
    fn f_cd_24_names_and_commit_ids_vibezeo_accepts() {
        assert!(is_lane_name("fix-login-2"));
        for bad in ["", "default", "Fix", "a b", "../x", "a/b", "x\n"] {
            assert!(!is_lane_name(bad), "{bad:?}");
        }
        assert_eq!(commit_revset(HEX).expect("hex"), format!("\"{HEX}\""));
        assert_eq!(commit_revset(CID).expect("change"), format!("\"{CID}\""));
        for bad in [
            "abc",
            &HEX.to_uppercase(),
            &format!("{HEX}0"),
            &format!("{CID}k"),
            &CID.replace('z', "a"),
            "root()",
            "zz",
        ] {
            assert!(commit_revset(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn f_cd_24_the_sweep_acts_only_where_nothing_else_owns_the_workspace() {
        let (_t, main) = colocated_fixture();
        let none = HashSet::new();
        let now = Some(std::time::SystemTime::now());
        // Ours: a folder of this repository's, made after the record.
        let before = Some(std::time::SystemTime::now() - Duration::from_secs(60));
        let ours = workspace_folder(&main, "cut");
        assert_eq!(orphan_decision(&main, "cut", before, &none), Ok(ours));
        // Never `default`, or a name Vibezeo doesn't make.
        assert_eq!(
            orphan_decision(&main, "default", before, &none),
            Err(Kept::NotOurs)
        );
        assert_eq!(
            orphan_decision(&main, "Odd", before, &none),
            Err(Kept::NotOurs)
        );
        // A lane row has the name: live.
        let rows = HashSet::from(["cut".to_string()]);
        assert_eq!(
            orphan_decision(&main, "cut", before, &rows),
            Err(Kept::HasRow)
        );
        // No folder (the person's own same-named workspace lives elsewhere).
        assert_eq!(
            orphan_decision(&main, "elsewhere", before, &none),
            Err(Kept::NoFolder)
        );
        // A folder that isn't this repository's jj workspace.
        std::fs::create_dir_all(main.join(WORKTREES_DIR).join("plain")).expect("mkdir");
        assert_eq!(
            orphan_decision(&main, "plain", before, &none),
            Err(Kept::OtherWorkspace)
        );
        let (_t2, other) = colocated_fixture();
        let foreign = workspace_folder(&main, "foreign");
        std::fs::write(
            foreign.join(".jj/repo"),
            other.join(".jj/repo").to_string_lossy().as_bytes(),
        )
        .expect("write");
        assert_eq!(
            orphan_decision(&main, "foreign", before, &none),
            Err(Kept::OtherWorkspace)
        );
        // A symlink to a real workspace folder isn't followed.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                main.join(WORKTREES_DIR).join("cut"),
                main.join(WORKTREES_DIR).join("link"),
            )
            .expect("symlink");
            assert_eq!(
                orphan_decision(&main, "link", before, &none),
                Err(Kept::NoFolder)
            );
        }
        // A folder git's index tracks.
        let tracked = workspace_folder(&main, "tracked");
        std::fs::write(tracked.join("f.txt"), "x").expect("write");
        let git = git2::Repository::open(&main).expect("open");
        let mut index = git.index().expect("index");
        index
            .add_path(Path::new(".tandem/worktrees/tracked/f.txt"))
            .expect("add");
        index.write().expect("write");
        assert!(tracked_under(&main, "tracked"));
        assert!(!tracked_under(&main, "track"));
        assert_eq!(
            orphan_decision(&main, "tracked", before, &none),
            Err(Kept::Tracked)
        );
        // A folder made well before the record (where birth times exist).
        let later = now.map(|t| t + Duration::from_secs(3600));
        let born =
            std::fs::metadata(main.join(WORKTREES_DIR).join("cut")).and_then(|m| m.created());
        if born.is_ok() {
            assert_eq!(
                orphan_decision(&main, "cut", later, &none),
                Err(Kept::Older)
            );
        }
    }

    #[tokio::test]
    async fn f_cd_24_the_sweep_keeps_live_workspaces_and_forgets_only_orphans() {
        let (_t, main) = colocated_fixture();
        let tmp = tempfile::tempdir().expect("tmp");
        let pending = tmp.path().join("jj-pending");
        std::fs::create_dir_all(&pending).expect("mkdir");
        let key = std::fs::canonicalize(&main).expect("canonical");
        workspace_folder(&main, "cut");
        workspace_folder(&main, "live");
        for slug in ["cut", "live", "elsewhere", "default"] {
            write_pending(&pending, &key, slug).expect("record");
        }
        let (_t2, other) = colocated_fixture();
        write_pending(&pending, &std::fs::canonicalize(&other).expect("c"), "cut").expect("record");
        let fake = Arc::new(FakeJj::answering(vec![
            (
                "--ignore-working-copy workspace list",
                ok("default\ncut\nlive\nelsewhere\n"),
            ),
            ("--ignore-working-copy log -r", ok(&format!("{HEX}\n"))),
        ]));
        let rows = HashSet::from(["live".to_string()]);
        let n = forget_orphans(fake.clone(), &pending, &main, Duration::ZERO, &rows)
            .await
            .expect("sweep");
        assert_eq!(n, 1);
        let forgets: Vec<Vec<String>> = fake
            .calls()
            .into_iter()
            .filter(|a| a.iter().any(|x| x == "forget"))
            .collect();
        assert_eq!(
            forgets,
            vec![vec![
                "--ignore-working-copy".to_string(),
                "workspace".into(),
                "forget".into(),
                "cut".into()
            ]]
        );
        assert!(!main.join(WORKTREES_DIR).join("cut").exists());
        assert!(
            main.join(WORKTREES_DIR).join("live").exists(),
            "a lane with a row survives"
        );
        // Only the other repository's record is left; prune drops every
        // old record, an unreadable one too, and nothing else.
        assert_eq!(read_pending(&pending).len(), 1);
        std::fs::write(pending.join(format!("old{PENDING}")), "cut\n/r\n").expect("write");
        std::fs::write(pending.join("notes.txt"), "x").expect("write");
        assert_eq!(prune_pending(&pending, Duration::from_secs(3600)), 0);
        assert_eq!(prune_pending(&pending, Duration::ZERO), 2);
        assert!(read_pending(&pending).is_empty());
        assert!(pending.join("notes.txt").exists());
    }

    #[tokio::test]
    async fn f_cd_24_one_failed_forget_doesnt_stop_the_sweep() {
        let (_t, main) = colocated_fixture();
        let tmp = tempfile::tempdir().expect("tmp");
        let pending = tmp.path().join("jj-pending");
        std::fs::create_dir_all(&pending).expect("mkdir");
        let key = std::fs::canonicalize(&main).expect("canonical");
        workspace_folder(&main, "a");
        workspace_folder(&main, "b");
        write_pending(&pending, &key, "a").expect("record");
        write_pending(&pending, &key, "b").expect("record");
        let fake = Arc::new(FakeJj::answering(vec![
            (
                "--ignore-working-copy workspace list",
                ok("default\na\nb\n"),
            ),
            ("--ignore-working-copy workspace forget a", failing("boom")),
            ("--ignore-working-copy log -r", ok(&format!("{HEX}\n"))),
        ]));
        let n = forget_orphans(fake, &pending, &main, Duration::ZERO, &HashSet::new())
            .await
            .expect("sweep");
        assert_eq!(n, 1, "b was still forgotten");
        assert_eq!(
            read_pending(&pending).len(),
            1,
            "a's record stays for the next sweep"
        );
    }

    #[test]
    fn f_cd_24_a_record_is_json_so_a_newline_in_a_path_cant_shift_it() {
        let tmp = tempfile::tempdir().expect("tmp");
        let repo = PathBuf::from("/r/odd\nname");
        write_pending(tmp.path(), &repo, "cut").expect("record");
        let all = read_pending(tmp.path());
        assert_eq!(all.len(), 1);
        assert_eq!(
            all[0].1,
            Pending {
                repo,
                slug: "cut".into()
            }
        );
    }

    fn log(rev: &str, template: &str) -> Vec<String> {
        vec![
            "log".to_string(),
            "-r".into(),
            rev.into(),
            "--no-graph".into(),
            "-T".into(),
            format!("self.{template}() ++ \"\\n\""),
        ]
    }

    #[tokio::test]
    async fn f_cd_24_the_exact_jj_commands_for_add_handoff_and_diff() {
        let (_t, main) = colocated_fixture();
        let repo = Repo::discover(&main).expect("discover").expect("repo");
        let source = workspace_folder(&main, "first");
        let fake = Arc::new(
            FakeJj::answering(vec![
                (
                    "--ignore-working-copy workspace list",
                    ok("default\nfirst\n"),
                ),
                (
                    "--config=ui.quiet=false",
                    JjOutput {
                        code: Some(0),
                        stdout: String::new(),
                        stderr: format!("Duplicated 0123abcd4567 as {CID}\n"),
                    },
                ),
                ("--ignore-working-copy log -r", ok(&format!("{CID}\n"))),
                ("log -r", ok(&format!("{CID}\n"))),
            ])
            .making(&main),
        );
        let lane = add_workspace(
            fake.clone(),
            &repo,
            "Tidy up",
            &HashSet::new(),
            Some("first".into()),
            None,
        )
        .await
        .expect("add");
        assert_eq!(lane.slug, "tidy-up");
        assert_eq!(lane.base, CID);
        let path = main.join(WORKTREES_DIR).join("tidy-up");
        let quoted = format!("\"{CID}\"");
        let calls = fake.calls.lock().expect("calls").clone();
        let at = |cwd: &Path, args: Vec<String>| (cwd.to_path_buf(), args);
        let mut expected = vec![
            at(
                &main,
                vec![
                    "--ignore-working-copy".to_string(),
                    "workspace".into(),
                    "list".into(),
                    "-T".into(),
                    "self.name() ++ \"\\n\"".into(),
                ],
            ),
            // The handoff starts from a copy of the source's working copy,
            // made in the source, so the source's edits can't move it.
            // With jj's summary and quietness fixed for the call.
            at(
                &source,
                vec![
                    "--config=ui.quiet=false".to_string(),
                    "--config=templates.commit_summary=\"self.change_id()\"".into(),
                    "duplicate".into(),
                    "@".into(),
                ],
            ),
        ];
        let mut full = vec!["--ignore-working-copy".to_string()];
        full.extend(log(&format!("\"{CID}\""), "change_id"));
        expected.push(at(&main, full));
        expected.push(at(
            &main,
            vec![
                "workspace".to_string(),
                "add".into(),
                path.to_string_lossy().into_owned(),
                "--name".into(),
                "tidy-up".into(),
                "--revision".into(),
                quoted.clone(),
            ],
        ));
        // The lane's own first commit, empty: its base.
        expected.push(at(&path, vec!["new".to_string()]));
        expected.push(at(&path, log("@-", "change_id")));
        expected.push(at(&path, log(&quoted, "change_id")));
        assert_eq!(calls, expected);
        // Refused before jj runs: a handoff from a name Vibezeo doesn't make,
        // or from a folder that isn't this repository's workspace.
        let before = fake.calls().len();
        assert!(add_workspace(
            fake.clone(),
            &repo,
            "x",
            &HashSet::new(),
            Some("default".into()),
            None
        )
        .await
        .is_err());
        assert_eq!(fake.calls().len(), before);
        std::fs::create_dir_all(main.join(WORKTREES_DIR).join("plain")).expect("mkdir");
        let e = add_workspace(
            fake.clone(),
            &repo,
            "y",
            &HashSet::new(),
            Some("plain".into()),
            None,
        )
        .await
        .expect_err("foreign");
        assert!(e.to_string().contains(FOREIGN_WORKSPACE), "{e}");
        assert!(
            fake.calls()[before..]
                .iter()
                .all(|a| !a.iter().any(|x| x == "duplicate")),
            "no copy made"
        );
        // The diff checks the base resolves to itself, then diffs from its
        // parent, quoted (so work squashed into the base still shows).
        let before = fake.calls().len();
        diff(fake.clone(), &main, &path, CID).await.expect("diff");
        assert_eq!(
            fake.calls()[before..].to_vec(),
            vec![
                log(&quoted, "change_id"),
                vec![
                    "diff".to_string(),
                    "--git".into(),
                    "--from".into(),
                    format!("{quoted}-"),
                    "--to".into(),
                    "@".into()
                ],
            ]
        );
        // A base from before change ids is checked by its commit id.
        let before = fake.calls().len();
        let _ = diff(fake.clone(), &main, &path, HEX).await;
        assert_eq!(
            fake.calls()[before],
            log(&format!("\"{HEX}\""), "commit_id")
        );
        // A base jj resolves to another change (a tag of that name) is refused.
        let other = Arc::new(FakeJj::answering(vec![(
            "log -r",
            ok(&format!("{}\n", "k".repeat(32))),
        )]));
        let e = changed_files(other, &main, &path, CID)
            .await
            .expect_err("shadowed");
        assert!(e.to_string().contains(SHADOWED_BASE), "{e}");
        // A stale workspace says what to do.
        let fake_stale = Arc::new(FakeJj::answering(vec![(
            "log -r",
            failing("Error: The working copy is stale (not updated since operation 1234).\nHint: Run `jj workspace update-stale` to update it."),
        )]));
        let e = diff(fake_stale, &main, &path, CID)
            .await
            .expect_err("stale");
        assert_eq!(e.to_string(), stale(&path));
        assert!(e.to_string().contains(&path.to_string_lossy().into_owned()));
        // An abandoned base says so.
        let gone = Arc::new(FakeJj::answering(vec![(
            "log -r",
            failing(&format!("Error: Revision `{CID}` doesn't exist")),
        )]));
        let e = diff(gone, &main, &path, CID).await.expect_err("gone");
        assert_eq!(e.to_string(), GONE_BASE);
    }

    #[tokio::test]
    async fn f_cd_24_a_failed_handoff_abandons_its_copy_and_fixed_output_is_read() {
        let (_t, main) = colocated_fixture();
        let repo = Repo::discover(&main).expect("discover").expect("repo");
        workspace_folder(&main, "first");
        // jj adds the workspace but fails after (no `making`: no folder, so
        // the base can't be verified).
        let fake = Arc::new(FakeJj::answering(vec![
            (
                "--ignore-working-copy workspace list",
                ok("default\nfirst\n"),
            ),
            (
                "--config=ui.quiet=false",
                JjOutput {
                    code: Some(0),
                    stdout: String::new(),
                    stderr: format!("Duplicated 0123abcd4567 as {CID}\n"),
                },
            ),
            ("--ignore-working-copy log -r", ok(&format!("{CID}\n"))),
            ("log -r", ok(&format!("{CID}\n"))),
        ]));
        add_workspace(
            fake.clone(),
            &repo,
            "x",
            &HashSet::new(),
            Some("first".into()),
            None,
        )
        .await
        .expect_err("failed");
        let quoted = format!("\"{CID}\"");
        assert!(fake.calls().contains(&vec![
            "--ignore-working-copy".to_string(),
            "abandon".into(),
            quoted
        ]));
        // A summary jj didn't print as fixed (a word, a short id, nothing):
        // refused, nothing made.
        for stderr in [
            "Duplicated 0123abcd4567 as klmnopqr 89abcdef (no description set)\n".to_string(),
            String::new(),
            format!("Duplicated 0123 as {CID} extra\n"),
        ] {
            let fake = Arc::new(FakeJj::answering(vec![
                (
                    "--ignore-working-copy workspace list",
                    ok("default\nfirst\n"),
                ),
                (
                    "--config=ui.quiet=false",
                    JjOutput {
                        code: Some(0),
                        stdout: String::new(),
                        stderr: stderr.clone(),
                    },
                ),
                ("--ignore-working-copy log -r", ok(&format!("{CID}\n"))),
            ]));
            add_workspace(
                fake.clone(),
                &repo,
                "y",
                &HashSet::new(),
                Some("first".into()),
                None,
            )
            .await
            .expect_err("unexpected");
            assert!(
                fake.calls().iter().all(|a| !a.iter().any(|x| x == "add")),
                "{stderr:?}"
            );
        }
    }

    #[test]
    fn f_cd_24_verify_base_fails_closed_and_checks_git_refs_first() {
        let (_t, main) = colocated_fixture();
        let ws = workspace_folder(&main, "tidy");
        let fake = FakeJj::answering(vec![("log -r", ok(&format!("{CID}\n")))]);
        verify_base(&fake, &ws, &main, CID).expect("verified");
        assert_eq!(fake.calls().len(), 1);
        // Not this repository's workspace (or none): refused, jj never runs.
        let (_t2, other) = colocated_fixture();
        let plain = main.join(WORKTREES_DIR).join("plain");
        std::fs::create_dir_all(&plain).expect("mkdir");
        for (folder, repo) in [(&plain, &main), (&ws, &other)] {
            let fake = FakeJj::answering(vec![("log -r", ok(&format!("{CID}\n")))]);
            let e = verify_base(&fake, folder, repo, CID).expect_err("foreign");
            assert!(e.to_string().contains(FOREIGN_WORKSPACE), "{e}");
            assert!(fake.calls().is_empty());
        }
        // A git ref named like the base (a tag, a branch, a remote's): refused
        // before jj runs; jj's own keep refs aren't counted.
        let git = git2::Repository::open(&main).expect("open");
        let sig = git2::Signature::now("t", "t@t").expect("sig");
        let tree = git
            .find_tree(git.index().expect("index").write_tree().expect("tree"))
            .expect("tree");
        let commit = git
            .commit(Some("HEAD"), &sig, &sig, "first", &tree, &[])
            .expect("commit");
        git.reference(&format!("refs/jj/keep/{HEX}"), commit, false, "keep")
            .expect("keep");
        let fake = FakeJj::answering(vec![("log -r", ok(&format!("{HEX}\n")))]);
        verify_base(&fake, &ws, &main, HEX).expect("a keep ref is jj's own");
        for name in [
            format!("refs/tags/{CID}"),
            format!("refs/heads/x/{CID}"),
            format!("refs/remotes/origin/{CID}"),
        ] {
            let mut r = git.reference(&name, commit, false, "shadow").expect("ref");
            let fake = FakeJj::answering(vec![("log -r", ok(&format!("{CID}\n")))]);
            let e = verify_base(&fake, &ws, &main, CID).expect_err("shadowed");
            assert!(e.to_string().contains(SHADOWED_BASE), "{name}: {e}");
            assert!(fake.calls().is_empty(), "{name}");
            r.delete().expect("delete");
        }
    }

    fn forced() -> Option<ForceConfirm> {
        let review =
            tandem_core::origin::LocalReview::check(&tandem_core::origin::Origin::main_window())
                .expect("main window");
        Some(ForceConfirm::new(review))
    }

    #[tokio::test]
    async fn f_cd_24_4_discard_keeps_a_working_copy_jj_couldnt_snapshot_whole() {
        let (_t, main) = colocated_fixture();
        let same = ("--ignore-working-copy log -r", ok(&format!("{HEX}\n")));
        let listed = ok("default\ntidy\n");
        let gone_from_view = ok("default\n");
        let cases: Vec<(&str, JjOutput, JjOutput)> = vec![
            (
                "failed",
                failing("Error: Concurrent checkout"),
                listed.clone(),
            ),
            (
                "refused",
                JjOutput {
                    code: Some(0),
                    stdout: "Working copy changes:\nA small.txt\n".into(),
                    stderr: "Warning: Refused to snapshot some files:\n  big.bin: 2.0MiB\n".into(),
                },
                listed.clone(),
            ),
            (
                "a warning a later jj words differently",
                JjOutput {
                    code: Some(0),
                    stdout: String::new(),
                    stderr: "Warning: Skipped some files:\n  big.bin\n".into(),
                },
                listed.clone(),
            ),
            (
                "untracked",
                ok("Untracked paths:\n? notes.txt\n"),
                listed.clone(),
            ),
            // Forgotten (or undone) while the agent kept editing: jj skips
            // the snapshot and says only this.
            (
                "no working copy",
                ok("No working copy\n"),
                gone_from_view.clone(),
            ),
            (
                "not listed",
                ok("The working copy has no changes.\n"),
                gone_from_view.clone(),
            ),
        ];
        for (why, status, list) in cases {
            let ws = workspace_folder(&main, "tidy");
            let fake = Arc::new(FakeJj::answering(vec![
                ("status", status.clone()),
                ("--ignore-working-copy workspace list", list.clone()),
                same.clone(),
            ]));
            let e = forget_workspace(fake.clone(), &main, &ws, "tidy", None)
                .await
                .expect_err(why);
            assert!(matches!(e, GitError::Dirty), "{why}: {e}");
            assert!(ws.exists(), "{why}: the folder stays");
            assert!(
                fake.calls()
                    .iter()
                    .all(|a| !a.iter().any(|x| x == "forget")),
                "{why}"
            );
            // Forced (the person confirmed): removed; the name is forgotten
            // only while jj lists it as this folder's.
            forget_workspace(fake.clone(), &main, &ws, "tidy", forced())
                .await
                .expect(why);
            assert!(!ws.exists(), "{why}");
            let forgot = fake.calls().iter().any(|a| a.iter().any(|x| x == "forget"));
            assert_eq!(forgot, list == listed, "{why}");
        }
        // A whole snapshot: forgotten and removed without force.
        let ws = workspace_folder(&main, "tidy");
        let fake = Arc::new(FakeJj::answering(vec![
            ("status", ok("The working copy has no changes.\n")),
            ("--ignore-working-copy workspace list", listed.clone()),
            same.clone(),
        ]));
        forget_workspace(fake.clone(), &main, &ws, "tidy", None)
            .await
            .expect("clean");
        assert!(!ws.exists());
        // A nested repository (jj never snapshots it): kept until forced.
        let ws = workspace_folder(&main, "tidy");
        std::fs::create_dir_all(ws.join("vendor/lib/.git")).expect("mkdir");
        let e = forget_workspace(fake.clone(), &main, &ws, "tidy", None)
            .await
            .expect_err("nested");
        assert!(matches!(e, GitError::Dirty), "{e}");
        assert!(ws.join("vendor/lib/.git").exists());
        std::fs::remove_dir_all(&ws).expect("rm");
        // A stale working copy says what to run, and where.
        let ws = workspace_folder(&main, "tidy");
        let fake = Arc::new(FakeJj::answering(vec![
            (
                "status",
                failing("Error: The working copy is stale (not updated since operation 1)"),
            ),
            ("--ignore-working-copy workspace list", listed.clone()),
            same.clone(),
        ]));
        let e = forget_workspace(fake, &main, &ws, "tidy", None)
            .await
            .expect_err("stale");
        assert!(
            matches!(e, GitError::Conflict(ref m) if *m == stale(&ws)),
            "{e}"
        );
        // A runner that can't run jj at all (timeout, missing) is Dirty.
        struct Broken;
        impl JjRunner for Broken {
            fn run(&self, _: &Path, _: &[&str]) -> Result<JjOutput, GitError> {
                Err(GitError::Invalid("jj timed out".into()))
            }
        }
        let e = kept_whole(&Broken, &main, &ws, "tidy").expect_err("broken");
        assert!(matches!(e, GitError::Dirty), "{e}");
        assert!(ws.exists());
    }

    #[tokio::test]
    async fn f_cd_24_4_discard_forgets_only_the_folders_own_workspace() {
        let (_t, main) = colocated_fixture();
        let ws = workspace_folder(&main, "tidy");
        // The workspace jj calls `tidy` has another working copy than the
        // folder: someone else's, never forgotten.
        let fake = Arc::new(FakeJj::answering(vec![
            (
                "--ignore-working-copy workspace list",
                ok("default\ntidy\n"),
            ),
            (
                "--ignore-working-copy log -r \"tidy\"@",
                ok(&format!("{HEX}\n")),
            ),
            (
                "--ignore-working-copy log -r @",
                ok(&format!("{}\n", "f".repeat(40))),
            ),
        ]));
        let e = forget_workspace(fake.clone(), &main, &ws, "tidy", forced())
            .await
            .expect_err("someone else's");
        assert!(e.to_string().contains(FOREIGN_WORKSPACE), "{e}");
        assert!(ws.exists());
        assert!(fake
            .calls()
            .iter()
            .all(|a| !a.iter().any(|x| x == "forget")));
        // A folder that isn't this repository's jj workspace, or that git's
        // index tracks: refused before jj runs, even forced, and the
        // refusal says what to do.
        let plain = main.join(WORKTREES_DIR).join("plain");
        std::fs::create_dir_all(&plain).expect("mkdir");
        let fake = Arc::new(FakeJj::default());
        let e = forget_workspace(fake.clone(), &main, &plain, "plain", forced())
            .await
            .expect_err("plain");
        assert!(e.to_string().contains(FOREIGN_WORKSPACE), "{e}");
        std::fs::write(ws.join("f.txt"), "x").expect("write");
        let git = git2::Repository::open(&main).expect("open");
        let mut index = git.index().expect("index");
        index
            .add_path(Path::new(".tandem/worktrees/tidy/f.txt"))
            .expect("add");
        index.write().expect("write");
        let e = forget_workspace(fake.clone(), &main, &ws, "tidy", forced())
            .await
            .expect_err("tracked");
        assert_eq!(e.to_string(), tracked_folder("tidy"));
        assert!(e
            .to_string()
            .contains("git rm -r --cached .tandem/worktrees/tidy"));
        assert!(fake.calls().is_empty());
        assert!(ws.join("f.txt").exists());
        assert_eq!(
            forget_checks(&main, &ws, "tidy").map_err(|e| e.to_string()),
            Err(tracked_folder("tidy"))
        );
        // A folder already gone: its registration goes when jj lists the
        // name, and nothing else runs.
        let gone = main.join(WORKTREES_DIR).join("gone");
        assert_eq!(
            forget_checks(&main, &gone, "gone").expect("gone"),
            Folder::Gone
        );
        let listed = Arc::new(FakeJj::answering(vec![(
            "--ignore-working-copy workspace list",
            ok("default\ngone\n"),
        )]));
        forget_workspace(listed.clone(), &main, &gone, "gone", None)
            .await
            .expect("gone");
        assert_eq!(
            listed.calls().last(),
            Some(&vec![
                "--ignore-working-copy".to_string(),
                "workspace".into(),
                "forget".into(),
                "gone".into()
            ])
        );
        let unlisted = Arc::new(FakeJj::answering(vec![(
            "--ignore-working-copy workspace list",
            ok("default\n"),
        )]));
        forget_workspace(unlisted.clone(), &main, &gone, "gone", None)
            .await
            .expect("gone");
        assert!(unlisted
            .calls()
            .iter()
            .all(|a| !a.iter().any(|x| x == "forget")));
    }

    #[tokio::test]
    async fn f_cd_24_a_lane_is_never_named_default() {
        let (_t, main) = colocated_fixture();
        let repo = Repo::discover(&main).expect("discover").expect("repo");
        // jj's default workspace was renamed, so jj doesn't list `default`.
        let fake = Arc::new(
            FakeJj::answering(vec![
                ("--ignore-working-copy workspace list", ok("home\n")),
                ("log -r", ok(&format!("{CID}\n"))),
            ])
            .making(&main),
        );
        let lane = add_workspace(fake, &repo, "default", &HashSet::new(), None, None)
            .await
            .expect("add");
        assert_eq!(lane.slug, "default-2");
    }
}
