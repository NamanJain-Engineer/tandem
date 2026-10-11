//! Checkpoints (F-EX-01, ARCHITECTURE §7, D-701): a folder's files saved
//! before each agent turn, in a private bare git store under
//! `$DATA/checkpoints/<key>/store.git`, never in the folder's own repository.
//!
//! - The file list comes from the folder's repository when it has one
//!   (`ls-files --cached --others --exclude-standard`, read-only: the user's
//!   index is only read) or from an `ignore` walk outside one, capped at
//!   [`MAX_WALK_FILES`]. `.git` and `.tandem/worktrees` never count; files
//!   over the size limit are skipped and reported.
//! - The store keeps its own index, so `git add` rehashes only files whose
//!   stat data changed, and identical content is stored once.
//! - Bytes are stored as they are: no `.gitattributes` (an empty tree is the
//!   attribute source), no line-ending conversion, no filter.
//! - A checkpoint is a ref `refs/checkpoints/<id>` to the tree.
//! - Restores are written here in Rust, never through a symlink out of the
//!   folder, with the file's mode.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use git2::{ObjectType, Oid, Repository, TreeWalkMode, TreeWalkResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::cli::{checked_output, Git, Mode};
use crate::repo::{files_of, path_string, Repo};
use crate::types::FileDiff;
use crate::GitError;

/// A folder without a repository is walked up to this many files (F-EX-01).
pub const MAX_WALK_FILES: usize = 20_000;

/// git's well-known empty tree.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

const REF_PREFIX: &str = "refs/checkpoints/";

/// Where a folder's file list comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StoreKind {
    /// Inside a git repository: tracked and untracked, non-ignored files.
    Repo,
    /// Not a repository: an `ignore` walk, `.gitignore` files applied.
    Folder,
    /// git isn't installed: no checkpoints.
    NoGit,
}

/// What a snapshot saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub tree: String,
    pub files: u32,
    /// Relative paths left out because they're over the size limit.
    pub skipped: Vec<String>,
    /// The walk stopped at [`MAX_WALK_FILES`].
    pub partial: bool,
    /// Files the folder holds (for a partial snapshot, more than `files`).
    pub seen: u32,
}

/// One file a restore couldn't write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RestoreFailure {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restored {
    pub restored: Vec<String>,
    pub failed: Vec<RestoreFailure>,
}

/// The parts of a snapshot a restore needs to stay safe.
#[derive(Debug, Clone, Copy)]
pub struct Side<'a> {
    pub tree: &'a str,
    pub skipped: &'a [String],
    pub partial: bool,
}

/// The store directory's name for `folder`: the first 16 hex characters of
/// the SHA-256 of its canonical path.
pub fn key(folder: &Path) -> String {
    let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
    hex::encode(&digest[..8])
}

/// A folder's checkpoint store.
#[derive(Debug, Clone)]
pub struct Store {
    git: Git,
    /// `$DATA/checkpoints/<key>`.
    dir: PathBuf,
    folder: PathBuf,
}

impl Store {
    /// Opens (creating when needed) the store for `folder` under `root`
    /// (`$DATA/checkpoints`).
    pub fn open(git: Git, root: &Path, folder: &Path) -> Result<Self, GitError> {
        let folder = std::fs::canonicalize(folder)?;
        let dir = root.join(key(&folder));
        let store = Self {
            git,
            dir,
            folder: folder.clone(),
        };
        let git_dir = store.git_dir();
        if !git_dir.join("HEAD").is_file() {
            std::fs::create_dir_all(&store.dir)?;
            Repository::init_bare(&git_dir)?;
            // Recorded for people looking at the folder by hand.
            std::fs::write(
                store.dir.join("folder.txt"),
                folder.to_string_lossy().as_bytes(),
            )?;
        }
        std::fs::write(store.dir.join("attributes"), b"")?;
        Ok(store)
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn git_dir(&self) -> PathBuf {
        self.dir.join("store.git")
    }

    fn repo(&self) -> Result<Repository, GitError> {
        Ok(Repository::open_bare(self.git_dir())?)
    }

    /// The `--git-dir`/`--work-tree` prefix and the settings that keep
    /// bytes as they are.
    fn store_args(&self) -> Vec<std::ffi::OsString> {
        let mut gd = std::ffi::OsString::from("--git-dir=");
        gd.push(self.git_dir());
        let mut wt = std::ffi::OsString::from("--work-tree=");
        wt.push(&self.folder);
        let mut attrs = std::ffi::OsString::from("core.attributesFile=");
        attrs.push(self.dir.join("attributes"));
        let mut out = vec![gd, wt, "--literal-pathspecs".into()];
        for kv in [
            std::ffi::OsString::from("core.autocrlf=false"),
            "core.safecrlf=false".into(),
            "core.symlinks=true".into(),
            "core.ignoreCase=false".into(),
            "core.precomposeUnicode=false".into(),
            "core.bare=false".into(),
            "gc.auto=0".into(),
            attrs,
        ] {
            out.push("-c".into());
            out.push(kv);
        }
        out
    }

    /// Runs git on the store with `input` on stdin.
    async fn store_git(&self, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, GitError> {
        let mut all = self.store_args();
        all.push(format!("--attr-source={EMPTY_TREE}").into());
        all.extend(args.iter().map(std::ffi::OsString::from));
        let refs: Vec<&OsStr> = all.iter().map(|a| a.as_os_str()).collect();
        let out = match input {
            Some(input) => {
                self.git
                    .ok_with_input(Mode::Read, &self.folder, &refs, input)
                    .await?
            }
            None => self.git.ok(Mode::Read, &self.folder, &refs).await?,
        };
        Ok(out.stdout)
    }

    /// Lists the folder's files (relative, `/`-separated).
    async fn list(&self) -> Result<(Vec<String>, bool, StoreKind), GitError> {
        if let Some(repo) = Repo::discover(&self.folder)? {
            let _ = repo;
            let out = self
                .git
                .run(
                    Mode::Read,
                    &self.folder,
                    &[
                        OsStr::new("ls-files"),
                        OsStr::new("-z"),
                        OsStr::new("--cached"),
                        OsStr::new("--others"),
                        OsStr::new("--exclude-standard"),
                    ],
                )
                .await?;
            let out = checked_output(out, &[OsStr::new("ls-files")])?;
            let mut seen = BTreeSet::new();
            for raw in out.stdout.split(|b| *b == 0) {
                if raw.is_empty() {
                    continue;
                }
                let Ok(path) = std::str::from_utf8(raw) else {
                    continue;
                };
                // A nested repository or worktree shows as `dir/`.
                if path.ends_with('/') || excluded(path) {
                    continue;
                }
                seen.insert(path.to_string());
            }
            return Ok((seen.into_iter().collect(), false, StoreKind::Repo));
        }
        let folder = self.folder.clone();
        let (files, partial) = tokio::task::spawn_blocking(move || walk(&folder))
            .await
            .map_err(|e| GitError::Io(std::io::Error::other(e)))?;
        Ok((files, partial, StoreKind::Folder))
    }

    /// Saves the folder into the store's index and returns its tree.
    /// Nothing is pinned: [`Store::pin`] keeps it as a checkpoint.
    pub async fn snapshot(&self, max_file_bytes: u64) -> Result<(Snapshot, StoreKind), GitError> {
        let (listed, partial, kind) = self.list().await?;
        let mut keep = Vec::with_capacity(listed.len());
        let mut skipped = Vec::new();
        for rel in &listed {
            let Ok(meta) = std::fs::symlink_metadata(self.folder.join(rel)) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                keep.push(rel.clone());
            } else if meta.is_file() {
                if meta.len() > max_file_bytes {
                    skipped.push(rel.clone());
                } else {
                    keep.push(rel.clone());
                }
            }
        }
        let indexed = self.indexed().await?;
        let keep_set: BTreeSet<&String> = keep.iter().collect();
        let gone: Vec<&String> = indexed.iter().filter(|p| !keep_set.contains(p)).collect();
        if !gone.is_empty() {
            let mut input = Vec::new();
            for p in &gone {
                input.extend_from_slice(p.as_bytes());
                input.push(0);
            }
            self.store_git(
                &["update-index", "-z", "--force-remove", "--stdin"],
                Some(&input),
            )
            .await?;
        }
        if !keep.is_empty() {
            let mut input = Vec::new();
            for p in &keep {
                input.extend_from_slice(p.as_bytes());
                input.push(0);
            }
            // `add` reuses the index's stat data: unchanged files aren't
            // read again. `-f`: the list is already decided.
            self.store_git(
                &["add", "-f", "--pathspec-from-file=-", "--pathspec-file-nul"],
                Some(&input),
            )
            .await?;
        }
        let tree = String::from_utf8_lossy(&self.store_git(&["write-tree"], None).await?)
            .trim()
            .to_string();
        Ok((
            Snapshot {
                tree,
                files: u32::try_from(keep.len()).unwrap_or(u32::MAX),
                skipped,
                partial,
                seen: u32::try_from(listed.len()).unwrap_or(u32::MAX),
            },
            kind,
        ))
    }

    async fn indexed(&self) -> Result<Vec<String>, GitError> {
        let out = self
            .store_git(&["ls-files", "-z", "--cached"], None)
            .await?;
        Ok(out
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .filter_map(|p| std::str::from_utf8(p).ok().map(str::to_string))
            .collect())
    }

    /// Keeps `tree` as checkpoint `id`.
    pub fn pin(&self, id: &str, tree: &str) -> Result<(), GitError> {
        let repo = self.repo()?;
        repo.reference(
            &format!("{REF_PREFIX}{id}"),
            Oid::from_str(tree)?,
            true,
            "checkpoint",
        )?;
        Ok(())
    }

    /// Drops checkpoints; their data leaves the disk at the next [`Store::gc`].
    pub fn unpin(&self, ids: &[String]) -> Result<(), GitError> {
        let repo = self.repo()?;
        for id in ids {
            if let Ok(mut r) = repo.find_reference(&format!("{REF_PREFIX}{id}")) {
                r.delete()?;
            }
        }
        Ok(())
    }

    /// The checkpoint ids the store holds.
    pub fn pinned(&self) -> Result<Vec<String>, GitError> {
        let repo = self.repo()?;
        let mut out = Vec::new();
        for r in repo.references_glob(&format!("{REF_PREFIX}*"))?.flatten() {
            if let Some(name) = r.name().and_then(|n| n.strip_prefix(REF_PREFIX)) {
                out.push(name.to_string());
            }
        }
        Ok(out)
    }

    /// Removes everything no checkpoint refers to.
    pub async fn gc(&self) -> Result<(), GitError> {
        self.store_git(&["reflog", "expire", "--expire=now", "--all"], None)
            .await?;
        self.store_git(&["gc", "--prune=now", "--quiet"], None)
            .await?;
        Ok(())
    }

    /// How many files differ between two trees.
    pub fn count_changes(&self, from: &str, to: &str) -> Result<u32, GitError> {
        if from == to {
            return Ok(0);
        }
        let repo = self.repo()?;
        let a = repo.find_tree(Oid::from_str(from)?)?;
        let b = repo.find_tree(Oid::from_str(to)?)?;
        let diff = repo.diff_tree_to_tree(Some(&a), Some(&b), None)?;
        Ok(u32::try_from(diff.deltas().len()).unwrap_or(u32::MAX))
    }

    /// `from` → `to` as the DiffView shows it: `Added` = created since,
    /// `Deleted` = deleted since.
    pub fn diff(&self, from: &str, to: &str) -> Result<Vec<FileDiff>, GitError> {
        let repo = self.repo()?;
        let a = repo.find_tree(Oid::from_str(from)?)?;
        let b = repo.find_tree(Oid::from_str(to)?)?;
        let mut opts = git2::DiffOptions::new();
        opts.context_lines(3);
        let diff = repo.diff_tree_to_tree(Some(&a), Some(&b), Some(&mut opts))?;
        files_of(&diff)
    }

    /// Puts the folder's files back as they were in `then` for `paths`
    /// (every differing file when `None`), given the snapshot of `now`.
    ///
    /// Never touches a file either side skipped; never deletes when `then`
    /// is partial, and never overwrites a file `now` didn't see.
    pub fn restore(
        &self,
        then: Side<'_>,
        now: Side<'_>,
        paths: Option<&[String]>,
    ) -> Result<Restored, GitError> {
        let repo = self.repo()?;
        let old = entries(&repo, then.tree)?;
        let new = entries(&repo, now.tree)?;
        let mut differing = BTreeSet::new();
        for (path, entry) in &old {
            if new.get(path) != Some(entry) {
                differing.insert(path.clone());
            }
        }
        for path in new.keys() {
            if !old.contains_key(path) {
                differing.insert(path.clone());
            }
        }
        let skipped: BTreeSet<&String> = then.skipped.iter().chain(now.skipped).collect();
        let wanted: Vec<String> = match paths {
            Some(p) => p
                .iter()
                .filter(|p| differing.contains(*p))
                .cloned()
                .collect(),
            None => differing.into_iter().collect(),
        };
        let mut out = Restored::default();
        for path in wanted {
            if skipped.contains(&path) {
                continue;
            }
            let result = match old.get(&path) {
                Some(entry) => {
                    if now.partial && !new.contains_key(&path) && self.folder.join(&path).exists() {
                        continue;
                    }
                    self.write_entry(&repo, &path, entry)
                }
                None => {
                    if then.partial {
                        continue;
                    }
                    self.delete(&path)
                }
            };
            match result {
                Ok(()) => out.restored.push(path),
                Err(message) => out.failed.push(RestoreFailure { path, message }),
            }
        }
        Ok(out)
    }

    fn write_entry(&self, repo: &Repository, rel: &str, entry: &Entry) -> Result<(), String> {
        let target = safe_target(&self.folder, rel)?;
        let blob = repo
            .find_blob(entry.oid)
            .map_err(|_| "This checkpoint's copy is missing".to_string())?;
        if let Ok(meta) = std::fs::symlink_metadata(&target) {
            if meta.is_dir() {
                return Err("A folder has that name now".into());
            }
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if entry.mode == 0o120000 {
            let link = String::from_utf8_lossy(blob.content()).into_owned();
            let _ = std::fs::remove_file(&target);
            return make_symlink(&link, &target).map_err(|e| e.to_string());
        }
        let parent = target.parent().unwrap_or(&self.folder);
        let tmp = parent.join(format!(
            ".{}.vibezeo-restore",
            target
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ));
        std::fs::write(&tmp, blob.content()).map_err(|e| e.to_string())?;
        set_mode(&tmp, entry.mode == 0o100755).map_err(|e| e.to_string())?;
        if std::fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
            // Replace the link itself, never its target.
            let _ = std::fs::remove_file(&target);
        }
        std::fs::rename(&tmp, &target).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }

    fn delete(&self, rel: &str) -> Result<(), String> {
        let target = safe_target(&self.folder, rel)?;
        match std::fs::symlink_metadata(&target) {
            Ok(meta) if meta.is_dir() => Ok(()),
            Ok(_) => std::fs::remove_file(&target).map_err(|e| e.to_string()),
            Err(_) => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    oid: Oid,
    mode: i32,
}

fn entries(repo: &Repository, tree: &str) -> Result<BTreeMap<String, Entry>, GitError> {
    let tree = repo.find_tree(Oid::from_str(tree)?)?;
    let mut out = BTreeMap::new();
    tree.walk(TreeWalkMode::PreOrder, |dir, e| {
        if e.kind() == Some(ObjectType::Blob) {
            if let Some(name) = e.name() {
                out.insert(
                    format!("{dir}{name}"),
                    Entry {
                        oid: e.id(),
                        mode: e.filemode(),
                    },
                );
            }
        }
        TreeWalkResult::Ok
    })?;
    Ok(out)
}

/// `.tandem/worktrees` (lanes) never belong to a checkpoint.
fn excluded(rel: &str) -> bool {
    rel == ".tandem/worktrees" || rel.starts_with(".tandem/worktrees/")
}

/// Every file outside a repository, `.gitignore` files applied, hidden
/// files included, `.git` folders and lanes left out.
fn walk(folder: &Path) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let root = folder.to_path_buf();
    let walker = ignore::WalkBuilder::new(folder)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(false)
        .git_global(true)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |e| {
            if e.file_name() == ".git" {
                return false;
            }
            let rel = e
                .path()
                .strip_prefix(&root)
                .map(path_string)
                .unwrap_or_default();
            !excluded(&rel)
        })
        .build();
    let mut partial = false;
    for entry in walker.flatten() {
        let Some(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            continue;
        }
        if out.len() >= MAX_WALK_FILES {
            partial = true;
            break;
        }
        if let Ok(rel) = entry.path().strip_prefix(folder) {
            out.push(path_string(rel));
        }
    }
    out.sort();
    (out, partial)
}

/// `rel` under `folder`, refusing `..`, absolute paths and any folder on
/// the way that is a symlink (it could lead out of the folder).
fn safe_target(folder: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel_path = Path::new(rel);
    let mut current = folder.to_path_buf();
    let comps: Vec<Component<'_>> = rel_path.components().collect();
    if comps.is_empty() {
        return Err("Empty path".into());
    }
    for (i, c) in comps.iter().enumerate() {
        let Component::Normal(name) = c else {
            return Err("That path leads outside the folder".into());
        };
        if *name == OsStr::new(".git") {
            return Err("Vibezeo never writes inside .git".into());
        }
        current.push(name);
        if i + 1 < comps.len() {
            if let Ok(meta) = std::fs::symlink_metadata(&current) {
                if meta.file_type().is_symlink() {
                    return Err("A folder on the way is a link; Vibezeo doesn't follow it".into());
                }
                if !meta.is_dir() {
                    return Err("A file has the name of a folder on the way".into());
                }
            }
        }
    }
    Ok(current)
}

#[cfg(unix)]
fn set_mode(path: &Path, exec: bool) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if exec { 0o755 } else { 0o644 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _exec: bool) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn make_symlink(link: &str, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(link, at)
}

#[cfg(windows)]
fn make_symlink(link: &str, at: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(link, at)
}

/// Bytes the store takes on disk.
pub fn disk_usage(dir: &Path) -> u64 {
    let mut total = 0;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    for e in entries.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            total += disk_usage(&e.path());
        } else {
            total += meta.len();
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const BIG: u64 = 5 * 1024 * 1024;

    fn git_cmd(cwd: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        folder: PathBuf,
        data: PathBuf,
    }

    fn fixture(git: bool) -> Fixture {
        let tmp = tempfile::tempdir().expect("tmp");
        let folder = tmp.path().join("work");
        let data = tmp.path().join("data");
        std::fs::create_dir_all(&folder).expect("mkdir");
        std::fs::create_dir_all(&data).expect("mkdir");
        if git {
            git_cmd(&folder, &["init", "-q", "-b", "main"]);
        }
        Fixture {
            _tmp: tmp,
            folder,
            data,
        }
    }

    fn store(f: &Fixture) -> Store {
        let git = Git::locate(None, f.data.join("empty-hooks")).expect("git");
        Store::open(git, &f.data.join("checkpoints"), &f.folder).expect("open")
    }

    fn write(f: &Fixture, rel: &str, text: &str) {
        let p = f.folder.join(rel);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        std::fs::write(p, text).expect("write");
    }

    fn read(f: &Fixture, rel: &str) -> Option<String> {
        std::fs::read_to_string(f.folder.join(rel)).ok()
    }

    fn side(s: &Snapshot) -> Side<'_> {
        Side {
            tree: &s.tree,
            skipped: &s.skipped,
            partial: s.partial,
        }
    }

    /// Every byte of the repository's own state that a checkpoint must not touch.
    fn repo_state(folder: &Path) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        fn visit(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
            for e in std::fs::read_dir(dir).expect("read_dir").flatten() {
                let p = e.path();
                if p.is_dir() {
                    visit(&p, base, out);
                } else {
                    let rel = p
                        .strip_prefix(base)
                        .expect("rel")
                        .to_string_lossy()
                        .into_owned();
                    out.push((rel, std::fs::read(&p).expect("read")));
                }
            }
        }
        visit(&folder.join(".git"), folder, &mut out);
        out.sort();
        out
    }

    async fn files_in(s: &Store, tree: &str) -> Vec<String> {
        let repo = s.repo().expect("repo");
        entries(&repo, tree).expect("entries").into_keys().collect()
    }

    #[tokio::test]
    async fn f_ex_01_1_a_checkpoint_holds_tracked_and_untracked_files_and_leaves_the_repository_alone(
    ) {
        let f = fixture(true);
        write(&f, ".gitignore", "ignored.log\nnode_modules/\n");
        write(&f, "tracked.txt", "v1\n");
        git_cmd(&f.folder, &["add", "."]);
        git_cmd(&f.folder, &["commit", "-qm", "init"]);
        git_cmd(&f.folder, &["stash", "list"]);
        write(&f, "tracked.txt", "v2\n");
        write(&f, "new.txt", "new\n");
        write(&f, "ignored.log", "noise\n");
        write(&f, "node_modules/x/index.js", "x\n");
        git_cmd(&f.folder, &["add", "new.txt"]);
        let before = repo_state(&f.folder);
        let s = store(&f);
        let (snap, kind) = s.snapshot(BIG).await.expect("snapshot");
        assert_eq!(kind, StoreKind::Repo);
        s.pin("c1", &snap.tree).expect("pin");
        assert_eq!(
            files_in(&s, &snap.tree).await,
            vec![".gitignore", "new.txt", "tracked.txt"]
        );
        assert_eq!(repo_state(&f.folder), before, "the repository changed");
        // A restore leaves it alone too.
        write(&f, "tracked.txt", "v3\n");
        let (now, _) = s.snapshot(BIG).await.expect("now");
        let r = s.restore(side(&snap), side(&now), None).expect("restore");
        assert_eq!(r.restored, vec!["tracked.txt"]);
        assert_eq!(read(&f, "tracked.txt").as_deref(), Some("v2\n"));
        assert_eq!(repo_state(&f.folder), before, "the repository changed");
        // The user's refs show nothing new.
        let refs = git_cmd(&f.folder, &["for-each-ref"]);
        assert!(!refs.contains("checkpoint"), "{refs}");
    }

    #[tokio::test]
    async fn f_ex_01_2_restore_all_brings_back_edits_and_deletions_and_deletes_new_files() {
        let f = fixture(true);
        write(&f, ".gitignore", "*.log\n");
        write(&f, "a.txt", "a1\n");
        write(&f, "b.txt", "b1\n");
        write(&f, "keep.log", "log1\n");
        git_cmd(&f.folder, &["add", "."]);
        git_cmd(&f.folder, &["commit", "-qm", "init"]);
        let s = store(&f);
        let (then, _) = s.snapshot(BIG).await.expect("then");
        // The "agent" works.
        write(&f, "a.txt", "a2\n");
        std::fs::remove_file(f.folder.join("b.txt")).expect("rm");
        write(&f, "c.txt", "c\n");
        write(&f, "keep.log", "log2\n");
        let (now, _) = s.snapshot(BIG).await.expect("now");
        let diff = s.diff(&then.tree, &now.tree).expect("diff");
        let mut changes: Vec<_> = diff.iter().map(|d| (d.path.clone(), d.change)).collect();
        changes.sort_by(|a, b| a.0.cmp(&b.0));
        use crate::types::FileChange as C;
        assert_eq!(
            changes,
            vec![
                ("a.txt".into(), C::Modified),
                ("b.txt".into(), C::Deleted),
                ("c.txt".into(), C::Added)
            ]
        );
        let r = s.restore(side(&then), side(&now), None).expect("restore");
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert_eq!(read(&f, "a.txt").as_deref(), Some("a1\n"));
        assert_eq!(read(&f, "b.txt").as_deref(), Some("b1\n"));
        assert_eq!(read(&f, "c.txt"), None);
        assert_eq!(
            read(&f, "keep.log").as_deref(),
            Some("log2\n"),
            "ignored file touched"
        );
        // The snapshot taken before the restore brings the agent's version back.
        let (after, _) = s.snapshot(BIG).await.expect("after");
        let r = s.restore(side(&now), side(&after), None).expect("undo");
        assert_eq!(r.restored.len(), 3);
        assert_eq!(read(&f, "a.txt").as_deref(), Some("a2\n"));
        assert_eq!(read(&f, "b.txt"), None);
        assert_eq!(read(&f, "c.txt").as_deref(), Some("c\n"));
    }

    #[tokio::test]
    async fn f_ex_01_3_restoring_chosen_files_changes_only_those() {
        let f = fixture(true);
        write(&f, "a.txt", "a1\n");
        write(&f, "b.txt", "b1\n");
        let s = store(&f);
        let (then, _) = s.snapshot(BIG).await.expect("then");
        write(&f, "a.txt", "a2\n");
        write(&f, "b.txt", "b2\n");
        let (now, _) = s.snapshot(BIG).await.expect("now");
        let r = s
            .restore(side(&then), side(&now), Some(&["b.txt".to_string()]))
            .expect("restore");
        assert_eq!(r.restored, vec!["b.txt"]);
        assert_eq!(read(&f, "a.txt").as_deref(), Some("a2\n"));
        assert_eq!(read(&f, "b.txt").as_deref(), Some("b1\n"));
    }

    #[tokio::test]
    async fn f_ex_01_4_an_unchanged_folder_gives_the_same_tree() {
        let f = fixture(true);
        write(&f, "a.txt", "a\n");
        let s = store(&f);
        let (one, _) = s.snapshot(BIG).await.expect("one");
        let (two, _) = s.snapshot(BIG).await.expect("two");
        assert_eq!(one.tree, two.tree);
        write(&f, "a.txt", "b\n");
        let (three, _) = s.snapshot(BIG).await.expect("three");
        assert_ne!(one.tree, three.tree);
        assert_eq!(s.count_changes(&one.tree, &three.tree).expect("count"), 1);
    }

    #[tokio::test]
    async fn f_ex_01_5_unpinned_checkpoints_leave_the_store() {
        let f = fixture(true);
        write(&f, "a.txt", "first\n");
        let s = store(&f);
        let (one, _) = s.snapshot(BIG).await.expect("one");
        s.pin("c1", &one.tree).expect("pin");
        write(&f, "a.txt", "second\n");
        let (two, _) = s.snapshot(BIG).await.expect("two");
        s.pin("c2", &two.tree).expect("pin");
        assert_eq!(s.pinned().expect("pinned").len(), 2);
        s.unpin(&["c1".to_string()]).expect("unpin");
        assert_eq!(s.pinned().expect("pinned"), vec!["c2"]);
        // Overwrite the index's copy so nothing but c1 refers to "first".
        s.gc().await.expect("gc");
        let repo = s.repo().expect("repo");
        let first = Oid::hash_object(ObjectType::Blob, b"first\n").expect("hash");
        assert!(repo.find_blob(first).is_err(), "c1's data stayed");
        assert!(repo
            .find_tree(Oid::from_str(&two.tree).expect("oid"))
            .is_ok());
    }

    #[tokio::test]
    async fn f_ex_01_6_a_folder_without_git_is_walked_with_its_gitignore() {
        let f = fixture(false);
        write(&f, ".gitignore", "build/\n");
        write(&f, "src/main.rs", "fn main() {}\n");
        write(&f, ".env", "A=1\n");
        write(&f, "build/out.bin", "x\n");
        write(&f, ".tandem/worktrees/x/file", "lane\n");
        let s = store(&f);
        let (then, kind) = s.snapshot(BIG).await.expect("snapshot");
        assert_eq!(kind, StoreKind::Folder);
        assert!(!then.partial);
        assert_eq!(
            files_in(&s, &then.tree).await,
            vec![".env", ".gitignore", "src/main.rs"]
        );
        write(&f, "src/main.rs", "fn main() { panic!() }\n");
        write(&f, "src/new.rs", "\n");
        let (now, _) = s.snapshot(BIG).await.expect("now");
        let r = s.restore(side(&then), side(&now), None).expect("restore");
        assert_eq!(r.restored.len(), 2);
        assert_eq!(read(&f, "src/main.rs").as_deref(), Some("fn main() {}\n"));
        assert_eq!(read(&f, "src/new.rs"), None);
        assert_eq!(read(&f, "build/out.bin").as_deref(), Some("x\n"));
    }

    #[test]
    fn f_ex_01_6_the_walk_stops_at_the_limit_and_says_so() {
        let tmp = tempfile::tempdir().expect("tmp");
        for i in 0..(MAX_WALK_FILES + 5) {
            let dir = tmp.path().join(format!("d{}", i / 1000));
            std::fs::create_dir_all(&dir).expect("mkdir");
            std::fs::write(dir.join(format!("f{i}")), b"").expect("write");
        }
        let (files, partial) = walk(tmp.path());
        assert!(partial);
        assert_eq!(files.len(), MAX_WALK_FILES);
    }

    #[tokio::test]
    async fn f_ex_01_7_files_over_the_limit_are_skipped_and_never_restored_over() {
        let f = fixture(true);
        write(&f, "small.txt", "s1\n");
        write(&f, "big.bin", &"x".repeat(2048));
        let s = store(&f);
        let (then, _) = s.snapshot(1024).await.expect("then");
        assert_eq!(then.skipped, vec!["big.bin"]);
        assert_eq!(files_in(&s, &then.tree).await, vec!["small.txt"]);
        write(&f, "big.bin", &"y".repeat(4096));
        write(&f, "small.txt", "s2\n");
        // Now small enough to be saved: the checkpoint skipped it, so a
        // restore must not delete it.
        let (now, _) = s.snapshot(1 << 20).await.expect("now");
        let r = s.restore(side(&then), side(&now), None).expect("restore");
        assert_eq!(r.restored, vec!["small.txt"]);
        assert_eq!(read(&f, "big.bin").map(|t| t.len()), Some(4096));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_ex_01_8_restore_keeps_modes_and_links_and_stays_in_the_folder() {
        use std::os::unix::fs::PermissionsExt;
        let f = fixture(true);
        write(&f, "run.sh", "#!/bin/sh\n");
        std::fs::set_permissions(
            f.folder.join("run.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("chmod");
        std::os::unix::fs::symlink("run.sh", f.folder.join("link")).expect("symlink");
        write(&f, "dir/inner.txt", "inner\n");
        let s = store(&f);
        let (then, _) = s.snapshot(BIG).await.expect("then");
        std::fs::remove_file(f.folder.join("run.sh")).expect("rm");
        std::fs::remove_file(f.folder.join("link")).expect("rm");
        // `dir` becomes a link to a folder outside.
        let outside = f._tmp.path().join("outside");
        std::fs::create_dir_all(&outside).expect("mkdir");
        std::fs::remove_dir_all(f.folder.join("dir")).expect("rm");
        std::os::unix::fs::symlink(&outside, f.folder.join("dir")).expect("symlink");
        let (now, _) = s.snapshot(BIG).await.expect("now");
        // Restoring only the file under the link would write through it.
        let r = s
            .restore(
                side(&then),
                side(&now),
                Some(&["dir/inner.txt".to_string()]),
            )
            .expect("restore one");
        assert!(r.failed.iter().any(|x| x.path == "dir/inner.txt"), "{r:?}");
        assert!(!outside.join("inner.txt").exists(), "wrote through a link");
        // Restoring everything removes the link first, then writes a real folder.
        let r = s.restore(side(&then), side(&now), None).expect("restore");
        assert!(r.failed.is_empty(), "{r:?}");
        assert!(!outside.join("inner.txt").exists(), "wrote through a link");
        assert!(!std::fs::symlink_metadata(f.folder.join("dir"))
            .expect("dir")
            .file_type()
            .is_symlink());
        assert_eq!(read(&f, "dir/inner.txt").as_deref(), Some("inner\n"));
        let meta = std::fs::metadata(f.folder.join("run.sh")).expect("meta");
        assert_eq!(meta.permissions().mode() & 0o111, 0o111, "exec bit lost");
        assert_eq!(
            std::fs::read_link(f.folder.join("link")).expect("link"),
            PathBuf::from("run.sh")
        );
    }

    #[test]
    fn f_ex_01_8_targets_never_leave_the_folder() {
        let tmp = tempfile::tempdir().expect("tmp");
        for bad in ["../x", "/etc/passwd", ".git/config", "a/../../x"] {
            assert!(safe_target(tmp.path(), bad).is_err(), "{bad}");
        }
        assert!(safe_target(tmp.path(), "a/b.txt").is_ok());
    }

    #[test]
    fn f_ex_01_keys_are_stable_per_folder() {
        let tmp = tempfile::tempdir().expect("tmp");
        assert_eq!(key(tmp.path()), key(tmp.path()));
        assert_eq!(key(tmp.path()).len(), 16);
        assert_ne!(key(tmp.path()), key(&tmp.path().join("x")));
    }
}
