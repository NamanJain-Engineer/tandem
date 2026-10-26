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
            let base = match base_of(jj.as_ref(), &path) {
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

/// A pending record: the repository's canonical folder, then the slug.
fn write_pending(dir: &Path, repo: &Path, slug: &str) -> Result<PathBuf, GitError> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = dir.join(format!("{slug}-{nanos}{PENDING}"));
    std::fs::write(&path, format!("{}\n{slug}\n", repo.to_string_lossy()))?;
    Ok(path)
}

const PENDING: &str = ".pending";

/// The workspace's row exists: its pending record goes.
pub fn done_adding(lane: &JjLane) {
    if let Some(p) = &lane.pending {
        let _ = std::fs::remove_file(p);
    }
}

/// jj workspaces whose making was cut short: records in `pending` (a
/// folder of Vibezeo's own, which the repository can't write) for the
/// repository at `repo_root`, older than `age` and not in `busy` (slugs
/// being made now). Each is forgotten and its folder removed, as Discard
/// would; its commits stay. Only names Vibezeo makes are ever forgotten:
/// never `default`. Returns how many.
pub async fn forget_orphans(
    jj: Arc<dyn JjRunner>,
    pending: &Path,
    repo_root: &Path,
    age: Duration,
    busy: &HashSet<String>,
) -> Result<usize, GitError> {
    let Ok(entries) = std::fs::read_dir(pending) else {
        return Ok(0);
    };
    let repo_key = std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    let dir = repo_root.join(WORKTREES_DIR);
    let mut found = Vec::new();
    for e in entries.flatten() {
        if !e.file_name().to_string_lossy().ends_with(PENDING) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        let mut lines = text.lines();
        let (Some(repo), Some(slug)) = (lines.next(), lines.next()) else {
            continue;
        };
        if Path::new(repo) != repo_key || !is_lane_name(slug) || busy.contains(slug) {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|d| d >= age);
        if old {
            found.push((slug.to_string(), e.path()));
        }
    }
    let mut n = 0;
    for (slug, record) in found {
        forget_workspace(jj.clone(), repo_root, &dir.join(&slug), &slug).await?;
        let _ = std::fs::remove_file(record);
        n += 1;
    }
    Ok(n)
}

/// The workspace's changes since `base`, working copy included, as git
/// lanes' are: jj's git-format diff read by the same parser.
pub async fn diff(
    jj: Arc<dyn JjRunner>,
    ws_root: &Path,
    base: &str,
) -> Result<Vec<FileDiff>, GitError> {
    let root = ws_root.to_path_buf();
    let base = commit_revset(base)?;
    tokio::task::spawn_blocking(move || {
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
    let base = commit_revset(base)?;
    tokio::task::spawn_blocking(move || {
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
}
