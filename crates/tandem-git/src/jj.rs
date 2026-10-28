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
use crate::lanes::{LaneManager, WORKTREES_DIR};
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
    let out = jj.run(Path::new("."), &["--version"]).ok()?;
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
        "You are working in the jj workspace `{}` (workspace `{name}`) of a repository co-located with git. Use `jj`, not `git`, in it: it has no `.git`. Sibling worktrees: {others}. Don't modify other worktrees. {}",
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
    /// The commit it started from (its working copy's parent).
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

/// A commit id as a revset that no alias can shadow: aliases expand
/// identifiers, never string literals.
fn commit_revset(id: &str) -> Result<String, GitError> {
    if is_commit_id(id) {
        Ok(format!("\"{id}\""))
    } else {
        Err(GitError::Invalid("jj gave an unexpected commit id".into()))
    }
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

/// The workspace names jj knows in the repository at `root`.
pub fn workspace_names(jj: &dyn JjRunner, root: &Path) -> Result<HashSet<String>, GitError> {
    let out = ok_or(
        jj.run(root, &["workspace", "list", "-T", "self.name() ++ \"\\n\""])?,
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

/// The working copy's parent in the workspace at `ws_root` (its first, for
/// a merge).
pub fn base_of(jj: &dyn JjRunner, ws_root: &Path) -> Result<String, GitError> {
    let out = ok_or(
        jj.run(
            ws_root,
            &[
                "log",
                "-r",
                "@-",
                "--no-graph",
                "-T",
                "self.commit_id() ++ \"\\n\"",
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
    commit_revset(&id)?;
    Ok(id)
}

/// `jj workspace add <repo>/.tandem/worktrees/<slug> --name <slug>` in the
/// co-located repository `repo`. `from`: another workspace's name, whose
/// working-copy commit it starts from (a handoff); otherwise jj starts it
/// beside the default workspace's.
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
    ensure_tandem_excluded(repo.common_dir())?;
    let dir = main.join(WORKTREES_DIR);
    std::fs::create_dir_all(&dir)?;
    let source = slug_source.to_string();
    tokio::task::spawn_blocking(move || {
        taken.extend(workspace_names(jj.as_ref(), &main)?);
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
            // The workspace's name quoted: no alias can shadow it.
            let rev = from.as_ref().map(|w| format!("\"{w}\"@"));
            let mut args = vec!["workspace", "add", path_s.as_str(), "--name", slug.as_str()];
            if let Some(r) = &rev {
                args.push("--revision");
                args.push(r.as_str());
            }
            let out = match jj.run(&main, &args) {
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
            let base = match base_of(jj.as_ref(), &path)
                .and_then(|b| verify_base(jj.as_ref(), &path, &b).map(|_| b))
            {
                Ok(b) => b,
                Err(e) => {
                    // Nothing half-made stays behind.
                    let _ = jj.run(&main, &["workspace", "forget", slug.as_str()]);
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
/// workspace is then forgotten and its folder removed, as Discard would,
/// and its commits stay. Otherwise only the record goes. Never `default`.
/// A failed forget is logged and kept for the next sweep. Returns how many
/// were forgotten. The caller holds the repository's lock throughout.
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
                match forget_workspace(jj.clone(), repo_root, &folder, &record.slug).await {
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

/// Records older than `age` for repositories not in `known` (removed from
/// the rail, or gone) go; nothing is forgotten for them.
pub fn prune_pending(pending: &Path, known: &HashSet<PathBuf>, age: Duration) -> usize {
    let mut n = 0;
    for (path, record, written) in read_pending(pending) {
        let old = written
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|d| d >= age);
        if old && !known.contains(&record.repo) {
            let _ = std::fs::remove_file(path);
            n += 1;
        }
    }
    n
}

pub const SHADOWED_BASE: &str =
    "A ref named like this worktree's base commit hides it, so Vibezeo won't diff it";

/// The base, checked before it's used as a revset: 40 hex digits, no git
/// ref of that name (a tag or branch named like the commit would be
/// resolved first, and comes with a clone, no config needed), and jj
/// resolving it to itself.
pub fn verify_base(jj: &dyn JjRunner, ws_root: &Path, base: &str) -> Result<String, GitError> {
    let revset = commit_revset(base)?;
    if let Some(main) = main_of_workspace(ws_root) {
        let git = git2::Repository::open(&main)?;
        // jj's own `refs/jj/keep/<id>` refs (they keep its commits alive)
        // are named after commits; a tag, branch or remote ref isn't.
        let shadowed = git.references()?.flatten().any(|r| {
            r.name().is_some_and(|n| {
                !n.starts_with("refs/jj/") && (n == base || n.ends_with(&format!("/{base}")))
            })
        });
        if shadowed {
            return Err(GitError::Invalid(SHADOWED_BASE.into()));
        }
    }
    let out = ok_or(
        jj.run(
            ws_root,
            &[
                "log",
                "-r",
                &revset,
                "--no-graph",
                "-T",
                "self.commit_id() ++ \"\\n\"",
            ],
        )?,
        "log",
    )?;
    if out.stdout.trim() != base {
        return Err(GitError::Invalid(SHADOWED_BASE.into()));
    }
    Ok(revset)
}

/// The workspace's changes since `base`, working copy included, as git
/// lanes' are: jj's git-format diff read by the same parser.
pub async fn diff(
    jj: Arc<dyn JjRunner>,
    ws_root: &Path,
    base: &str,
) -> Result<Vec<FileDiff>, GitError> {
    let root = ws_root.to_path_buf();
    let base = base.to_string();
    tokio::task::spawn_blocking(move || {
        let base = verify_base(jj.as_ref(), &root, &base)?;
        let out = ok_or(
            jj.run(&root, &["diff", "--git", "--from", &base, "--to", "@"])?,
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
    ws_root: &Path,
    base: &str,
) -> Result<Vec<String>, GitError> {
    let root = ws_root.to_path_buf();
    let base = base.to_string();
    tokio::task::spawn_blocking(move || {
        let base = verify_base(jj.as_ref(), &root, &base)?;
        let out = ok_or(
            jj.run(
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

/// Discard: snapshot the working copy (so nothing in it is lost: it stays
/// in the workspace's commit), `jj workspace forget <name>` in the
/// repository, then remove the folder. The workspace's commits stay.
pub async fn forget_workspace(
    jj: Arc<dyn JjRunner>,
    repo_root: &Path,
    ws_root: &Path,
    name: &str,
) -> Result<(), GitError> {
    let repo_root = repo_root.to_path_buf();
    let ws_root = ws_root.to_path_buf();
    let name = name.to_string();
    // Only a workspace Vibezeo made (never `default`), and only a folder of
    // its own: `<repo>/.tandem/worktrees/<name>`.
    if !is_lane_name(&name) {
        return Err(GitError::Invalid(
            "That isn't a jj workspace Vibezeo made".into(),
        ));
    }
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let dir = canon(&repo_root.join(WORKTREES_DIR));
    let parent = ws_root.parent().map(canon);
    let ours = parent.as_deref() == Some(dir.as_path())
        && ws_root.file_name() == Some(std::ffi::OsStr::new(&name))
        && !std::fs::symlink_metadata(&ws_root).is_ok_and(|m| m.file_type().is_symlink());
    if !ours {
        return Err(GitError::Invalid(
            "That folder isn't one of this repository's worktrees".into(),
        ));
    }
    tokio::task::spawn_blocking(move || {
        if ws_root.is_dir() {
            // `jj status` snapshots the working copy into its commit.
            let _ = jj.run(&ws_root, &["status"]);
        }
        let known = workspace_names(jj.as_ref(), &repo_root)?;
        if known.contains(&name) {
            ok_or(
                jj.run(&repo_root, &["workspace", "forget", name.as_str()])?,
                "workspace forget",
            )?;
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

    /// Records argv and answers from a table.
    #[derive(Default)]
    struct FakeJj {
        calls: Mutex<Vec<(PathBuf, Vec<String>)>>,
        answers: Vec<(&'static str, JjOutput)>,
    }

    impl FakeJj {
        fn answering(answers: Vec<(&'static str, JjOutput)>) -> Self {
            Self {
                calls: Mutex::default(),
                answers,
            }
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
            ("workspace list", ok("default\ntidy\n")),
            (
                "log -r @-",
                ok("f3d6212d78d11961dbdf3a4649b203738c1e88c0\n"),
            ),
        ]);
        let names = workspace_names(&fake, Path::new("/r")).expect("names");
        assert_eq!(names, HashSet::from(["default".into(), "tidy".into()]));
        assert_eq!(
            base_of(&fake, Path::new("/r/.tandem/worktrees/tidy")).expect("base"),
            "f3d6212d78d11961dbdf3a4649b203738c1e88c0"
        );
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
        for bad in [
            "abc",
            &HEX.to_uppercase(),
            &format!("{HEX}0"),
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
        let fake = Arc::new(FakeJj::answering(vec![(
            "workspace list",
            ok("default\ncut\nlive\nelsewhere\n"),
        )]));
        let rows = HashSet::from(["live".to_string()]);
        let n = forget_orphans(fake.clone(), &pending, &main, Duration::ZERO, &rows)
            .await
            .expect("sweep");
        assert_eq!(n, 1);
        let forgets: Vec<Vec<String>> = fake
            .calls()
            .into_iter()
            .filter(|a| {
                a.first().map(String::as_str) == Some("workspace")
                    && a.get(1).map(String::as_str) == Some("forget")
            })
            .collect();
        assert_eq!(
            forgets,
            vec![vec!["workspace".to_string(), "forget".into(), "cut".into()]]
        );
        assert!(!main.join(WORKTREES_DIR).join("cut").exists());
        assert!(
            main.join(WORKTREES_DIR).join("live").exists(),
            "a lane with a row survives"
        );
        // Only the other repository's record is left; prune drops it once
        // that repository isn't known.
        assert_eq!(read_pending(&pending).len(), 1);
        assert_eq!(
            prune_pending(&pending, &HashSet::from([key]), Duration::ZERO),
            1
        );
        assert!(read_pending(&pending).is_empty());
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
            ("workspace list", ok("default\na\nb\n")),
            (
                "workspace forget a",
                JjOutput {
                    code: Some(1),
                    stdout: String::new(),
                    stderr: "boom".into(),
                },
            ),
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

    #[tokio::test]
    async fn f_cd_24_the_exact_jj_commands_for_add_handoff_and_diff() {
        let (_t, main) = colocated_fixture();
        let repo = Repo::discover(&main).expect("discover").expect("repo");
        let fake = Arc::new(FakeJj::answering(vec![
            ("workspace list", ok("default\nfirst\n")),
            ("log -r @-", ok(&format!("{HEX}\n"))),
            ("log -r", ok(&format!("{HEX}\n"))),
        ]));
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
        assert_eq!(lane.base, HEX);
        let path = main.join(WORKTREES_DIR).join("tidy-up");
        let calls = fake.calls();
        let add = calls
            .iter()
            .find(|a| {
                a.first().map(String::as_str) == Some("workspace")
                    && a.get(1).map(String::as_str) == Some("add")
            })
            .expect("add call");
        assert_eq!(
            add,
            &vec![
                "workspace".to_string(),
                "add".into(),
                path.to_string_lossy().into_owned(),
                "--name".into(),
                "tidy-up".into(),
                "--revision".into(),
                "\"first\"@".into(),
            ]
        );
        assert!(calls.iter().any(|a| a
            == &vec![
                "log".to_string(),
                "-r".into(),
                "@-".into(),
                "--no-graph".into(),
                "-T".into(),
                "self.commit_id() ++ \"\\n\"".into()
            ]));
        // A handoff from a name Vibezeo doesn't make is refused before jj runs.
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
        // The diff checks the base resolves to itself, then diffs from it quoted.
        let before = fake.calls().len();
        diff(fake.clone(), &path, HEX).await.expect("diff");
        let quoted = format!("\"{HEX}\"");
        assert_eq!(
            fake.calls()[before..].to_vec(),
            vec![
                vec![
                    "log".to_string(),
                    "-r".into(),
                    quoted.clone(),
                    "--no-graph".into(),
                    "-T".into(),
                    "self.commit_id() ++ \"\\n\"".into()
                ],
                vec![
                    "diff".to_string(),
                    "--git".into(),
                    "--from".into(),
                    quoted.clone(),
                    "--to".into(),
                    "@".into()
                ],
            ]
        );
        // A base jj resolves to another commit (a tag of that name) is refused.
        let other = Arc::new(FakeJj::answering(vec![(
            "log -r",
            ok(&format!("{}\n", "f".repeat(40))),
        )]));
        let e = changed_files(other, &path, HEX)
            .await
            .expect_err("shadowed");
        assert!(e.to_string().contains(SHADOWED_BASE), "{e}");
    }
}
