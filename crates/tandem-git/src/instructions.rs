//! One instructions file for every engine (F-EN-07, ARCHITECTURE §2.5):
//! which instructions files each engine reads at a repository's root, how
//! they differ from `AGENTS.md`, and "Use one file for all", which makes
//! `AGENTS.md` the source and each engine's own file a one-line import of it.
//!
//! The per-engine rules are data from the engine registry (D-2400); this
//! module only reads and writes the files. Nothing is written without a
//! confirmed plan, every replaced file is backed up first, and a change can
//! be undone byte for byte. Symbolic links, files over 1 MB and anything
//! that resolves outside the checkout are listed and never read or written.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ignore::gitignore::GitignoreBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::cli::{Git, Mode};
use crate::error::GitError;
use crate::types::FileDiff;

/// The source every engine is pointed at.
pub const AGENTS: &str = "AGENTS.md";
/// Instructions files larger than this are listed, never read.
pub const MAX_BYTES: u64 = 1024 * 1024;
const MANIFEST: &str = "manifest.json";

/// How one engine finds its instructions (the registry's `instructions`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineRules {
    /// Relative, `/`-separated; a path ending in `/` is a folder.
    pub files: Vec<String>,
    /// Reads only the first of `files` that exists (else every one).
    pub first: bool,
    /// Read only when none of `files` exist.
    pub fallback: Option<String>,
    /// The file that, holding `import`, makes the engine read `AGENTS.md`.
    pub pointer: Option<(String, String)>,
}

/// An installed engine; `rules: None` is Unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineInput {
    pub id: String,
    pub label: String,
    pub rules: Option<EngineRules>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "InstructionsGit")]
pub enum GitState {
    Tracked,
    /// Neither committed nor ignored.
    Untracked,
    Ignored,
    /// Missing, or the folder isn't a repository.
    None,
}

/// Why a file is listed but never read or written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "InstructionsUnsafe")]
pub enum Unsafe {
    Symlink,
    Outside,
    TooLarge,
    NotAFile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstructionsFile {
    pub path: String,
    pub exists: bool,
    #[ts(type = "number")]
    pub bytes: u64,
    pub git: GitState,
    #[serde(rename = "unsafe")]
    #[ts(rename = "unsafe")]
    pub unsafe_reason: Option<Unsafe>,
    /// For an ignored file: whether `.vibezeo/worktree.json` copies it into
    /// new worktrees.
    pub in_worktrees: Option<bool>,
    /// It holds only an import of `AGENTS.md`.
    pub points_to_agents: bool,
    /// Its difference from `AGENTS.md`, when both are readable and differ.
    pub diff: Option<FileDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineReads {
    pub engine_id: String,
    pub label: String,
    /// In the registry's instructions table (else Unknown).
    pub known: bool,
    /// The files it reads, in its order; `AGENTS.md` too when an import or
    /// the engine itself reads it.
    pub reads: Vec<String>,
    pub reads_agents: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstructionsReport {
    pub root: String,
    pub engines: Vec<EngineReads>,
    /// `AGENTS.md` first, then every file an engine looks for.
    pub files: Vec<InstructionsFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, rename = "InstructionsAction")]
pub enum Action {
    Create,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlannedChange {
    pub path: String,
    pub action: Action,
    pub before: Option<String>,
    pub after: String,
    /// The file has text that isn't in `AGENTS.md`, and it will be replaced.
    pub text_lost: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SkippedInstructions {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstructionsPlan {
    pub changes: Vec<PlannedChange>,
    pub skipped: Vec<SkippedInstructions>,
    /// What was planned, over the files as they were: apply refuses when it
    /// no longer matches.
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InstructionsChange {
    pub change_id: String,
    pub written: Vec<String>,
}

/// One file of an applied change, as the backup's manifest keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    path: String,
    /// The backup's file name, when the file existed before.
    backup: Option<String>,
    /// SHA-256 of what the change wrote.
    after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Manifest {
    root: String,
    entries: Vec<Entry>,
}

/// A path's state in the checkout, and its text when it can be read.
struct Probe {
    exists: bool,
    bytes: u64,
    unsafe_reason: Option<Unsafe>,
    text: Option<String>,
}

fn probe(root: &Path, rel: &str) -> Probe {
    let folder = rel.ends_with('/');
    let full = root.join(rel.trim_end_matches('/'));
    let Ok(meta) = std::fs::symlink_metadata(&full) else {
        return Probe {
            exists: false,
            bytes: 0,
            unsafe_reason: None,
            text: None,
        };
    };
    let mut p = Probe {
        exists: true,
        bytes: if meta.is_file() { meta.len() } else { 0 },
        unsafe_reason: None,
        text: None,
    };
    let inside = match (full.canonicalize(), root.canonicalize()) {
        (Ok(f), Ok(r)) => f.starts_with(&r),
        _ => false,
    };
    p.unsafe_reason = if meta.file_type().is_symlink() {
        Some(Unsafe::Symlink)
    } else if !inside {
        Some(Unsafe::Outside)
    } else if folder != meta.is_dir() || !(meta.is_dir() || meta.is_file()) {
        Some(Unsafe::NotAFile)
    } else if meta.is_file() && meta.len() > MAX_BYTES {
        Some(Unsafe::TooLarge)
    } else {
        None
    };
    if p.unsafe_reason.is_none() && meta.is_file() {
        p.text = std::fs::read(&full)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned());
    }
    p
}

/// Holds an import of `AGENTS.md` (`import` on a line of its own).
fn imports(text: &str, import: &str) -> bool {
    text.lines().any(|l| l.trim() == import.trim())
}

/// Holds nothing but imports of `AGENTS.md`.
fn only_imports(text: &str, imports_seen: &BTreeSet<String>) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    !lines.is_empty() && lines.iter().all(|l| imports_seen.contains(*l))
}

fn git_state(repo: Option<&git2::Repository>, rel: &str, exists: bool) -> GitState {
    let Some(repo) = repo.filter(|_| exists) else {
        return GitState::None;
    };
    let path = Path::new(rel.trim_end_matches('/'));
    if repo.is_path_ignored(path).unwrap_or(false) {
        return GitState::Ignored;
    }
    match repo.status_file(path) {
        Ok(s) if s.contains(git2::Status::IGNORED) => GitState::Ignored,
        Ok(s) if s.intersects(git2::Status::WT_NEW | git2::Status::INDEX_NEW) => {
            GitState::Untracked
        }
        Ok(_) => GitState::Tracked,
        // A folder (`.cursor/rules/`) has no status of its own.
        Err(_) if rel.ends_with('/') => GitState::Tracked,
        Err(_) => GitState::Untracked,
    }
}

/// Every file an engine looks for, `AGENTS.md` first, each once.
fn candidates(engines: &[EngineInput]) -> Vec<String> {
    let mut out = vec![AGENTS.to_string()];
    for rules in engines.iter().filter_map(|e| e.rules.as_ref()) {
        let more = rules
            .files
            .iter()
            .chain(rules.fallback.iter())
            .chain(rules.pointer.iter().map(|(f, _)| f));
        for f in more {
            if !out.contains(f) {
                out.push(f.clone());
            }
        }
    }
    out
}

fn reads_of(rules: &EngineRules, probes: &dyn Fn(&str) -> (bool, Option<String>)) -> Vec<String> {
    let existing: Vec<&String> = rules.files.iter().filter(|f| probes(f).0).collect();
    let mut reads: Vec<String> = if rules.first {
        existing.into_iter().take(1).cloned().collect()
    } else {
        existing.into_iter().cloned().collect()
    };
    if reads.is_empty() {
        if let Some(fb) = rules.fallback.as_ref().filter(|f| probes(f).0) {
            reads.push(fb.clone());
        }
    }
    if let Some((_, import)) = &rules.pointer {
        let imported = reads
            .iter()
            .any(|f| probes(f).1.is_some_and(|t| imports(&t, import)));
        if imported && probes(AGENTS).0 && !reads.iter().any(|f| f == AGENTS) {
            reads.push(AGENTS.to_string());
        }
    }
    reads
}

fn copy_matches(root: &Path, rel: &str) -> bool {
    let Ok(Some(setup)) = crate::setup::read(root) else {
        return false;
    };
    let mut builder = GitignoreBuilder::new(root);
    for p in &setup.copy {
        if builder.add_line(None, p).is_err() {
            return false;
        }
    }
    builder.build().is_ok_and(|m| {
        m.matched_path_or_any_parents(rel.trim_end_matches('/'), rel.ends_with('/'))
            .is_ignore()
    })
}

/// `instructions_get`: the engines' instructions files at `root` (a
/// repository's main checkout) and what each installed engine reads.
pub fn inspect(root: &Path, engines: &[EngineInput]) -> Result<InstructionsReport, GitError> {
    let repo = git2::Repository::open(root).ok();
    let names = candidates(engines);
    let probes: Vec<(String, Probe)> = names.iter().map(|n| (n.clone(), probe(root, n))).collect();
    let lookup = |rel: &str| {
        probes
            .iter()
            .find(|(n, _)| n == rel)
            .map(|(_, p)| (p.exists, p.text.clone()))
            .unwrap_or_else(|| {
                let p = probe(root, rel);
                (p.exists, p.text)
            })
    };
    let import_lines: BTreeSet<String> = engines
        .iter()
        .filter_map(|e| {
            e.rules
                .as_ref()?
                .pointer
                .as_ref()
                .map(|(_, i)| i.trim().to_string())
        })
        .collect();
    let agents_text = probes
        .iter()
        .find(|(n, _)| n == AGENTS)
        .and_then(|(_, p)| p.text.clone());
    let mut files = Vec::new();
    for (rel, p) in &probes {
        let git = git_state(repo.as_ref(), rel, p.exists);
        let points = p
            .text
            .as_deref()
            .is_some_and(|t| rel != AGENTS && only_imports(t, &import_lines));
        let diff = match (&agents_text, &p.text) {
            (Some(a), Some(t)) if rel != AGENTS && !points && a != t => {
                crate::repo::diff_texts(rel, a.as_bytes(), t.as_bytes()).ok()
            }
            _ => None,
        };
        files.push(InstructionsFile {
            path: rel.clone(),
            exists: p.exists,
            bytes: p.bytes,
            git,
            unsafe_reason: p.unsafe_reason,
            in_worktrees: (git == GitState::Ignored).then(|| copy_matches(root, rel)),
            points_to_agents: points,
            diff,
        });
    }
    let engines = engines
        .iter()
        .map(|e| match &e.rules {
            None => EngineReads {
                engine_id: e.id.clone(),
                label: e.label.clone(),
                known: false,
                reads: Vec::new(),
                reads_agents: false,
            },
            Some(rules) => {
                let reads = reads_of(rules, &lookup);
                EngineReads {
                    engine_id: e.id.clone(),
                    label: e.label.clone(),
                    known: true,
                    reads_agents: reads.iter().any(|f| f == AGENTS),
                    reads,
                }
            }
        })
        .collect();
    Ok(InstructionsReport {
        root: crate::repo::path_string(root),
        engines,
        files,
    })
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn digest_of(changes: &[PlannedChange]) -> String {
    let mut h = Sha256::new();
    for c in changes {
        h.update(c.path.as_bytes());
        h.update([0]);
        h.update(c.before.as_deref().unwrap_or("\u{0}missing").as_bytes());
        h.update([0]);
        h.update(c.after.as_bytes());
        h.update([0]);
    }
    hex::encode(h.finalize())
}

fn contains_text(source: &str, text: &str) -> bool {
    let t = text.trim();
    t.is_empty() || source.contains(t)
}

/// `instructions_plan`: what "Use one file for all" would write. `source`
/// names the file whose text becomes `AGENTS.md` when there's none.
pub fn plan(
    root: &Path,
    engines: &[EngineInput],
    source: Option<&str>,
) -> Result<InstructionsPlan, GitError> {
    let agents = probe(root, AGENTS);
    if agents.unsafe_reason.is_some() {
        return Err(GitError::Invalid(
            "AGENTS.md is a link, a folder or too large; Vibezeo won't write it".into(),
        ));
    }
    let mut changes = Vec::new();
    let mut skipped = Vec::new();
    let agents_text = match agents.text {
        Some(t) => t,
        None => {
            let known = candidates(engines);
            let src = source
                .or_else(|| {
                    known
                        .iter()
                        .find(|f| f.as_str() != AGENTS && probe(root, f).text.is_some())
                        .map(String::as_str)
                })
                .ok_or_else(|| {
                    GitError::Invalid("There's no instructions file to start AGENTS.md from".into())
                })?;
            if !known.iter().any(|k| k == src) || src.ends_with('/') {
                return Err(GitError::Invalid(format!(
                    "{src} isn't an instructions file Vibezeo knows"
                )));
            }
            let p = probe(root, src);
            let text = match (p.unsafe_reason, p.text) {
                (None, Some(t)) => t,
                (Some(_), _) => {
                    return Err(GitError::Invalid(format!(
                        "{src} is a link, a folder or too large; choose another file"
                    )))
                }
                (None, None) => return Err(GitError::Invalid(format!("{src} doesn't exist"))),
            };
            changes.push(PlannedChange {
                path: AGENTS.into(),
                action: Action::Create,
                before: None,
                after: text.clone(),
                text_lost: false,
            });
            text
        }
    };
    let mut seen = BTreeSet::new();
    for e in engines {
        let Some((file, import)) = e.rules.as_ref().and_then(|r| r.pointer.clone()) else {
            continue;
        };
        if !seen.insert(file.clone()) {
            continue;
        }
        let after = format!("{}\n", import.trim());
        let p = probe(root, &file);
        if let Some(why) = p.unsafe_reason {
            skipped.push(SkippedInstructions {
                path: file.clone(),
                reason: match why {
                    Unsafe::Symlink => "It's a symbolic link",
                    Unsafe::Outside => "It's outside the repository",
                    Unsafe::TooLarge => "It's larger than 1 MB",
                    Unsafe::NotAFile => "It isn't a regular file",
                }
                .into(),
            });
            continue;
        }
        if p.text.as_deref() == Some(after.as_str()) {
            continue;
        }
        let text_lost = p
            .text
            .as_deref()
            .is_some_and(|t| !imports(t, &import) && !contains_text(&agents_text, t));
        changes.push(PlannedChange {
            path: file,
            action: if p.exists {
                Action::Replace
            } else {
                Action::Create
            },
            before: p.text,
            after,
            text_lost,
        });
    }
    Ok(InstructionsPlan {
        digest: digest_of(&changes),
        changes,
        skipped,
    })
}

fn change_id() -> String {
    format!(
        "ins_{}_{:04x}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%3f"),
        rand_u16()
    )
}

fn rand_u16() -> u16 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (nanos ^ std::process::id()) as u16
}

/// `instructions_apply`: re-plans, refuses unless the plan is the one the
/// person confirmed (`digest`), backs up every file it replaces into
/// `backups/<changeId>/`, then writes each file atomically.
pub fn apply(
    root: &Path,
    engines: &[EngineInput],
    source: Option<&str>,
    digest: &str,
    backups: &Path,
) -> Result<InstructionsChange, GitError> {
    let plan = plan(root, engines, source)?;
    if plan.digest != digest {
        return Err(GitError::Conflict(
            "The files changed. Review the plan again.".into(),
        ));
    }
    let id = change_id();
    let dir = backups.join(&id);
    std::fs::create_dir_all(&dir)?;
    let mut entries = Vec::new();
    for (n, c) in plan.changes.iter().enumerate() {
        let backup = match &c.before {
            Some(_) => {
                let name = format!("{n}.bak");
                std::fs::copy(root.join(&c.path), dir.join(&name))?;
                Some(name)
            }
            None => None,
        };
        entries.push(Entry {
            path: c.path.clone(),
            backup,
            after: sha(c.after.as_bytes()),
        });
    }
    let manifest = Manifest {
        root: root.to_string_lossy().into_owned(),
        entries,
    };
    let json =
        serde_json::to_vec_pretty(&manifest).map_err(|e| GitError::Invalid(e.to_string()))?;
    tandem_core::atomic::atomic_write(&dir.join(MANIFEST), &json, Some(0o600))?;
    let mut written = Vec::new();
    for c in &plan.changes {
        let path = root.join(&c.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        tandem_core::atomic::atomic_write(&path, c.after.as_bytes(), None)?;
        written.push(c.path.clone());
    }
    Ok(InstructionsChange {
        change_id: id,
        written,
    })
}

fn valid_id(id: &str) -> bool {
    id.starts_with("ins_")
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `instructions_undo`: every file of the change back as it was; a file
/// the change created is removed. Refuses when a file changed since.
pub fn undo(backups: &Path, change_id: &str) -> Result<(), GitError> {
    if !valid_id(change_id) {
        return Err(GitError::Invalid("That isn't a change id".into()));
    }
    let dir = backups.join(change_id);
    let raw = std::fs::read(dir.join(MANIFEST))
        .map_err(|_| GitError::Invalid("That change can't be undone any more".into()))?;
    let manifest: Manifest =
        serde_json::from_slice(&raw).map_err(|e| GitError::Invalid(e.to_string()))?;
    let root = PathBuf::from(&manifest.root);
    for e in &manifest.entries {
        let now = std::fs::read(root.join(&e.path)).ok();
        if now.as_deref().map(sha) != Some(e.after.clone()) {
            return Err(GitError::Conflict(format!(
                "{} changed since; it wasn't undone",
                e.path
            )));
        }
    }
    for e in &manifest.entries {
        let path = root.join(&e.path);
        match &e.backup {
            Some(b) => {
                let bytes = std::fs::read(dir.join(b))?;
                tandem_core::atomic::atomic_write(&path, &bytes, None)?;
            }
            None => std::fs::remove_file(&path)?,
        }
    }
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// `instructions_copy_to_worktrees`: adds an ignored instructions file to
/// `.vibezeo/worktree.json`'s `copy`. Only `copy` changes, so the commands'
/// trust (a digest of the commands alone) stays as it was.
pub fn copy_to_worktrees(
    root: &Path,
    engines: &[EngineInput],
    rel: &str,
) -> Result<crate::setup::WorktreeSetup, GitError> {
    let report = inspect(root, engines)?;
    let file = report
        .files
        .iter()
        .find(|f| f.path == rel && f.exists && f.git == GitState::Ignored)
        .ok_or_else(|| GitError::Invalid(format!("{rel} isn't an ignored instructions file")))?;
    let mut setup = crate::setup::read(root)
        .map_err(|e| GitError::Invalid(e.to_string()))?
        .unwrap_or_default();
    if file.in_worktrees != Some(true) {
        setup.copy.push(rel.trim_end_matches('/').to_string());
    }
    let setup = setup
        .normalized()
        .map_err(|e| GitError::Invalid(e.to_string()))?;
    crate::setup::write(root, &setup).map_err(|e| GitError::Invalid(e.to_string()))?;
    Ok(setup)
}

/// The instructions files `names` as committed at `commit` in the checkout
/// at `root` (F-TK-08). Lead answers a plan run's question from the plan's
/// base commit in the main checkout, so nothing an agent writes in its
/// worktree, and nothing left uncommitted, reaches Lead. Git's replace refs
/// are ignored, so a ref under `refs/replace/` (which every worktree
/// shares) can't swap other content in. Folders, missing files and files
/// over `max` bytes are left out, a file's size is asked before it's read,
/// and `commit` must be a commit id.
pub async fn at_commit(
    git: &Git,
    root: &Path,
    commit: &str,
    names: &[String],
    max: usize,
) -> Result<Vec<(String, String)>, GitError> {
    let is_id = (7..=64).contains(&commit.len()) && commit.bytes().all(|b| b.is_ascii_hexdigit());
    if !is_id {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for name in names {
        let plain = !name.is_empty()
            && !name.starts_with('/')
            && !name.ends_with('/')
            && !name.split('/').any(|part| part == "..");
        if !plain {
            continue;
        }
        if let Some(text) = committed(git, root, &format!("{commit}:{name}"), max).await? {
            out.push((name.clone(), text));
        }
    }
    Ok(out)
}

/// The blob at `spec` as text, without replace refs, when it's at most
/// `max` bytes: its size first, then at most `max` bytes of it, so a large
/// file is never read whole.
async fn committed(
    git: &Git,
    root: &Path,
    spec: &str,
    max: usize,
) -> Result<Option<String>, GitError> {
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;

    let command = |args: &[&OsStr]| {
        let mut cmd = git.command(Mode::Read, root, args);
        cmd.env("GIT_NO_REPLACE_OBJECTS", "1").stderr(Stdio::null());
        cmd
    };
    let started = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GitError::NotInstalled
        } else {
            GitError::Io(e)
        }
    };
    let size = command(&[OsStr::new("cat-file"), OsStr::new("-s"), OsStr::new(spec)])
        .output()
        .await
        .map_err(started)?;
    let fits = size.status.success()
        && String::from_utf8_lossy(&size.stdout)
            .trim()
            .parse::<usize>()
            .is_ok_and(|n| n <= max);
    if !fits {
        return Ok(None);
    }
    let mut child = command(&[OsStr::new("cat-file"), OsStr::new("blob"), OsStr::new(spec)])
        .spawn()
        .map_err(started)?;
    let Some(stdout) = child.stdout.take() else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    stdout.take(max as u64 + 1).read_to_end(&mut bytes).await?;
    if bytes.len() > max {
        // The child is killed as it's dropped.
        return Ok(None);
    }
    let done = child.wait().await?;
    Ok(done
        .success()
        .then(|| String::from_utf8_lossy(&bytes).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude() -> EngineInput {
        EngineInput {
            id: "claude".into(),
            label: "Claude Code".into(),
            rules: Some(EngineRules {
                files: vec![
                    "CLAUDE.md".into(),
                    ".claude/CLAUDE.md".into(),
                    "CLAUDE.local.md".into(),
                ],
                first: false,
                fallback: Some(AGENTS.into()),
                pointer: Some(("CLAUDE.md".into(), "@AGENTS.md".into())),
            }),
        }
    }

    fn codex() -> EngineInput {
        EngineInput {
            id: "codex".into(),
            label: "Codex".into(),
            rules: Some(EngineRules {
                files: vec!["AGENTS.override.md".into(), AGENTS.into()],
                first: true,
                fallback: None,
                pointer: None,
            }),
        }
    }

    fn gemini() -> EngineInput {
        EngineInput {
            id: "gemini".into(),
            label: "Gemini CLI".into(),
            rules: Some(EngineRules {
                files: vec!["GEMINI.md".into()],
                first: false,
                fallback: None,
                pointer: Some(("GEMINI.md".into(), "@AGENTS.md".into())),
            }),
        }
    }

    fn unknown() -> EngineInput {
        EngineInput {
            id: "aider".into(),
            label: "Aider".into(),
            rules: None,
        }
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("dir");
        git2::Repository::init(dir.path()).expect("init");
        dir
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        std::fs::write(p, text).expect("write");
    }

    fn read(root: &Path, rel: &str) -> Option<String> {
        std::fs::read_to_string(root.join(rel)).ok()
    }

    fn engine<'a>(r: &'a InstructionsReport, id: &str) -> &'a EngineReads {
        r.engines.iter().find(|e| e.engine_id == id).expect(id)
    }

    fn file<'a>(r: &'a InstructionsReport, path: &str) -> &'a InstructionsFile {
        r.files.iter().find(|f| f.path == path).expect(path)
    }

    #[test]
    fn f_en_07_1_differing_claude_md_and_agents_md_are_listed_with_who_reads_each_and_the_diff() {
        let dir = repo();
        let root = dir.path();
        write(root, "CLAUDE.md", "# Rules\nUse pnpm.\n");
        write(root, AGENTS, "# Rules\nUse npm.\n");
        let r = inspect(root, &[claude(), codex()]).expect("inspect");
        let c = engine(&r, "claude");
        assert_eq!(c.reads, vec!["CLAUDE.md"]);
        assert!(c.known && !c.reads_agents);
        let x = engine(&r, "codex");
        assert_eq!(x.reads, vec![AGENTS]);
        assert!(x.reads_agents);
        let diff = file(&r, "CLAUDE.md").diff.clone().expect("diff");
        assert!(
            diff.patch.contains("-Use npm.") && diff.patch.contains("+Use pnpm."),
            "{}",
            diff.patch
        );
        assert!(file(&r, AGENTS).exists && file(&r, AGENTS).diff.is_none());
        assert_eq!(file(&r, "CLAUDE.md").git, GitState::Untracked);
        // Codex reads only the first: an override wins.
        write(root, "AGENTS.override.md", "x");
        let r = inspect(root, &[codex()]).expect("inspect");
        assert_eq!(engine(&r, "codex").reads, vec!["AGENTS.override.md"]);
        // Claude reads AGENTS.md when it has none of its own.
        std::fs::remove_file(root.join("CLAUDE.md")).expect("rm");
        let r = inspect(root, &[claude()]).expect("inspect");
        assert_eq!(engine(&r, "claude").reads, vec![AGENTS]);
    }

    #[test]
    fn f_en_07_2_one_file_for_all_proposes_pointers_writes_only_on_confirm_and_undo_restores() {
        let dir = repo();
        let root = dir.path();
        let backups = tempfile::tempdir().expect("backups");
        let old_claude = "# Rules\nUse pnpm.\nNever push to main.\n";
        write(root, "CLAUDE.md", old_claude);
        write(root, AGENTS, "# Rules\nUse pnpm.\n");
        let engines = [claude(), gemini(), codex()];
        let p = plan(root, &engines, None).expect("plan");
        let paths: Vec<(&str, Action)> = p
            .changes
            .iter()
            .map(|c| (c.path.as_str(), c.action))
            .collect();
        assert_eq!(
            paths,
            vec![
                ("CLAUDE.md", Action::Replace),
                ("GEMINI.md", Action::Create)
            ]
        );
        assert!(p.changes.iter().all(|c| c.after == "@AGENTS.md\n"));
        assert!(p.changes[0].text_lost, "'Never push' isn't in AGENTS.md");
        // Planning writes nothing.
        assert_eq!(read(root, "CLAUDE.md").as_deref(), Some(old_claude));
        assert!(read(root, "GEMINI.md").is_none());
        // A stale digest is refused.
        assert!(matches!(
            apply(root, &engines, None, "stale", backups.path()),
            Err(GitError::Conflict(_))
        ));
        let done = apply(root, &engines, None, &p.digest, backups.path()).expect("apply");
        assert_eq!(done.written, vec!["CLAUDE.md", "GEMINI.md"]);
        assert_eq!(read(root, "CLAUDE.md").as_deref(), Some("@AGENTS.md\n"));
        assert_eq!(read(root, "GEMINI.md").as_deref(), Some("@AGENTS.md\n"));
        // Both now read AGENTS.md; the pointers show no diff.
        let r = inspect(root, &engines).expect("inspect");
        assert!(engine(&r, "claude").reads_agents && engine(&r, "gemini").reads_agents);
        assert!(file(&r, "CLAUDE.md").points_to_agents && file(&r, "CLAUDE.md").diff.is_none());
        // Nothing left to do.
        assert!(plan(root, &engines, None).expect("plan").changes.is_empty());
        undo(backups.path(), &done.change_id).expect("undo");
        assert_eq!(read(root, "CLAUDE.md").as_deref(), Some(old_claude));
        assert!(read(root, "GEMINI.md").is_none());
        assert!(undo(backups.path(), &done.change_id).is_err(), "once");
    }

    #[test]
    fn f_en_07_undo_refuses_when_a_file_changed_since() {
        let dir = repo();
        let root = dir.path();
        let backups = tempfile::tempdir().expect("backups");
        write(root, "CLAUDE.md", "mine\n");
        write(root, AGENTS, "ours\n");
        let p = plan(root, &[claude()], None).expect("plan");
        let done = apply(root, &[claude()], None, &p.digest, backups.path()).expect("apply");
        write(root, "CLAUDE.md", "edited after\n");
        assert!(matches!(
            undo(backups.path(), &done.change_id),
            Err(GitError::Conflict(_))
        ));
        assert_eq!(read(root, "CLAUDE.md").as_deref(), Some("edited after\n"));
        assert!(undo(backups.path(), "../../etc").is_err());
    }

    #[test]
    fn f_en_07_3_without_agents_md_the_plan_starts_it_from_claude_md() {
        let dir = repo();
        let root = dir.path();
        let backups = tempfile::tempdir().expect("backups");
        write(root, "CLAUDE.md", "# Rules\nUse pnpm.\n");
        let p = plan(root, &[claude()], None).expect("plan");
        assert_eq!(p.changes[0].path, AGENTS);
        assert_eq!(p.changes[0].action, Action::Create);
        assert_eq!(p.changes[0].after, "# Rules\nUse pnpm.\n");
        assert_eq!(p.changes[1].path, "CLAUDE.md");
        assert!(!p.changes[1].text_lost, "its text moved into AGENTS.md");
        apply(root, &[claude()], None, &p.digest, backups.path()).expect("apply");
        assert_eq!(read(root, AGENTS).as_deref(), Some("# Rules\nUse pnpm.\n"));
        assert_eq!(read(root, "CLAUDE.md").as_deref(), Some("@AGENTS.md\n"));
        // Another source can be chosen; an unknown or missing one can't.
        let dir = repo();
        write(dir.path(), "GEMINI.md", "gemini rules\n");
        write(dir.path(), "CLAUDE.md", "claude rules\n");
        let p = plan(dir.path(), &[claude(), gemini()], Some("GEMINI.md")).expect("plan");
        assert_eq!(p.changes[0].after, "gemini rules\n");
        assert!(plan(dir.path(), &[claude()], Some("README.md")).is_err());
        assert!(
            plan(repo().path(), &[claude()], None).is_err(),
            "nothing to start from"
        );
    }

    #[test]
    fn f_en_07_4_an_ignored_claude_local_md_can_be_copied_into_worktrees_keeping_the_setup() {
        let dir = repo();
        let root = dir.path();
        write(root, ".gitignore", "CLAUDE.local.md\n");
        write(root, "CLAUDE.local.md", "my sandbox URL\n");
        write(
            root,
            ".vibezeo/worktree.json",
            r#"{"copy":[".env"],"setup":"pnpm install"}"#,
        );
        let before = crate::setup::read(root)
            .expect("read")
            .expect("some")
            .digest();
        let r = inspect(root, &[claude()]).expect("inspect");
        let local = file(&r, "CLAUDE.local.md");
        assert_eq!(local.git, GitState::Ignored);
        assert_eq!(local.in_worktrees, Some(false));
        let setup = copy_to_worktrees(root, &[claude()], "CLAUDE.local.md").expect("copy");
        assert_eq!(setup.copy, vec![".env", "CLAUDE.local.md"]);
        assert_eq!(setup.setup.as_deref(), Some("pnpm install"));
        assert_eq!(setup.digest(), before, "the commands' trust is unchanged");
        let r = inspect(root, &[claude()]).expect("inspect");
        assert_eq!(file(&r, "CLAUDE.local.md").in_worktrees, Some(true));
        // Twice adds it once; a tracked or unknown file is refused.
        let again = copy_to_worktrees(root, &[claude()], "CLAUDE.local.md").expect("again");
        assert_eq!(again.copy.len(), 2);
        write(root, "CLAUDE.md", "x");
        assert!(copy_to_worktrees(root, &[claude()], "CLAUDE.md").is_err());
        assert!(copy_to_worktrees(root, &[claude()], ".env").is_err());
    }

    #[test]
    fn f_en_07_5_an_unknown_engine_is_listed_and_nothing_is_written_for_it() {
        let dir = repo();
        let root = dir.path();
        write(root, AGENTS, "rules\n");
        let r = inspect(root, &[unknown()]).expect("inspect");
        let a = engine(&r, "aider");
        assert!(!a.known && a.reads.is_empty());
        let p = plan(root, &[unknown()], None).expect("plan");
        assert!(p.changes.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn f_en_07_6_a_symlinked_claude_md_is_listed_and_never_written() {
        let dir = repo();
        let root = dir.path();
        let outside = tempfile::tempdir().expect("outside");
        write(outside.path(), "secret.md", "outside text\n");
        std::os::unix::fs::symlink(outside.path().join("secret.md"), root.join("CLAUDE.md"))
            .expect("link");
        write(root, AGENTS, "rules\n");
        let r = inspect(root, &[claude()]).expect("inspect");
        let f = file(&r, "CLAUDE.md");
        assert_eq!(f.unsafe_reason, Some(Unsafe::Symlink));
        assert!(f.diff.is_none(), "never read");
        let p = plan(root, &[claude()], None).expect("plan");
        assert!(p.changes.is_empty());
        assert_eq!(p.skipped[0].path, "CLAUDE.md");
        assert_eq!(
            std::fs::read_to_string(outside.path().join("secret.md")).expect("r"),
            "outside text\n"
        );
        // A linked AGENTS.md can't be the source either.
        std::fs::remove_file(root.join(AGENTS)).expect("rm");
        std::os::unix::fs::symlink(outside.path().join("secret.md"), root.join(AGENTS))
            .expect("link");
        assert!(plan(root, &[claude()], None).is_err());
    }

    #[test]
    fn f_en_07_files_over_1_mb_are_listed_not_read() {
        let dir = repo();
        let root = dir.path();
        write(root, "GEMINI.md", &"x".repeat(MAX_BYTES as usize + 1));
        write(root, AGENTS, "rules\n");
        let r = inspect(root, &[gemini()]).expect("inspect");
        let f = file(&r, "GEMINI.md");
        assert_eq!(f.unsafe_reason, Some(Unsafe::TooLarge));
        assert!(f.diff.is_none());
        assert_eq!(
            plan(root, &[gemini()], None).expect("plan").skipped.len(),
            1
        );
    }
}
