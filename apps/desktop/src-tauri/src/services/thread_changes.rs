//! The thread's Changes panel (F-TH-13, ARCHITECTURE §7): what changed in
//! the thread's folder since the thread started. A git folder diffs the
//! commit it started from (none yet: the empty tree) against the working
//! tree, tracked and untracked, within a subfolder workspace's prefix. Any
//! other folder compares a manifest of file hashes taken at the start.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tandem_core::{ErrorCode, IpcError};
use tandem_git::{FileChange, FileDiff, Repo};
use ts_rs::TS;

/// Files over this size aren't hashed for a snapshot.
pub const SNAPSHOT_MAX_FILE: u64 = 5 * 1024 * 1024;
/// A snapshot stops after this many files.
pub const SNAPSHOT_MAX_FILES: usize = 20_000;
/// An added file's content is shown up to this size.
const ADDED_SHOWN: u64 = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ChangesSource {
    /// A git diff from the thread's starting commit.
    Git,
    /// File hashes against the thread's starting snapshot (no git).
    Snapshot,
    /// The thread hasn't started a turn yet.
    NotStarted,
}

/// `thread_changes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ThreadChanges {
    pub source: ChangesSource,
    pub files: Vec<FileDiff>,
}

/// Where a thread starts from, as stored on the thread (`start_commit`,
/// `snapshot_ref`). An empty commit is a repository with no commits yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    Commit(String),
    Snapshot(PathBuf),
}

type Manifest = BTreeMap<String, (u64, i64, String)>;

fn io(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Io, e.to_string())
}

fn hash(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

fn mtime(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as i64)
}

/// Every file under `root` that isn't ignored (`.gitignore`, hidden
/// folders like `.tandem`), with its size and mtime: `/`-separated paths.
fn walk(root: &Path) -> Vec<(String, PathBuf, std::fs::Metadata)> {
    let mut out = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .require_git(false)
        .build();
    for entry in walker.flatten() {
        if out.len() >= SNAPSHOT_MAX_FILES {
            break;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() || meta.len() > SNAPSHOT_MAX_FILE {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        let rel = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        out.push((rel, entry.path().to_path_buf(), meta));
    }
    out
}

fn manifest(root: &Path) -> Manifest {
    walk(root)
        .into_iter()
        .filter_map(|(rel, path, meta)| Some((rel, (meta.len(), mtime(&meta), hash(&path).ok()?))))
        .collect()
}

/// The thread's start: its folder's HEAD when it's in git (empty before
/// the first commit), else a manifest written to `snapshot`.
pub async fn capture(cwd: PathBuf, snapshot: Option<PathBuf>) -> Result<Start, IpcError> {
    if let Some(repo) = Repo::discover(&cwd).map_err(io)? {
        let head = repo.head().await.map_err(io)?;
        return Ok(Start::Commit(head.unwrap_or_default()));
    }
    let snapshot = snapshot
        .ok_or_else(|| IpcError::new(ErrorCode::Unavailable, "Nowhere to keep a snapshot"))?;
    tokio::task::spawn_blocking(move || {
        let m = manifest(&cwd);
        if let Some(dir) = snapshot.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let body = serde_json::to_vec(&m).map_err(io)?;
        tandem_core::atomic::atomic_write(&snapshot, &body, Some(0o600)).map_err(io)?;
        Ok(Start::Snapshot(snapshot))
    })
    .await
    .map_err(io)?
}

fn added_patch(rel: &str, path: &Path, size: u64) -> (String, u32, bool) {
    if size > ADDED_SHOWN {
        return (String::new(), 0, false);
    }
    match std::fs::read(path) {
        Ok(bytes) if !bytes.contains(&0) => {
            let text = String::from_utf8_lossy(&bytes);
            let lines: Vec<&str> = text.lines().collect();
            let mut patch = format!(
                "diff --git a/{rel} b/{rel}\nnew file\n--- /dev/null\n+++ b/{rel}\n@@ -0,0 +1,{} @@\n",
                lines.len()
            );
            for l in &lines {
                patch.push('+');
                patch.push_str(l);
                patch.push('\n');
            }
            (patch, u32::try_from(lines.len()).unwrap_or(u32::MAX), false)
        }
        Ok(_) => (String::new(), 0, true),
        Err(_) => (String::new(), 0, false),
    }
}

/// A folder without git: files added, deleted or changed since the
/// snapshot. Changed files have no diff (their old content isn't kept);
/// added text files show their lines.
pub fn snapshot_diff(root: &Path, snapshot: &Manifest) -> Vec<FileDiff> {
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (rel, path, meta) in walk(root) {
        seen.insert(rel.clone());
        let file = |change, patch: String, additions, binary| FileDiff {
            path: rel.clone(),
            old_path: None,
            change,
            additions,
            deletions: 0,
            binary,
            patch,
            truncated: false,
        };
        match snapshot.get(&rel) {
            None => {
                let (patch, additions, binary) = added_patch(&rel, &path, meta.len());
                files.push(file(FileChange::Added, patch, additions, binary));
            }
            Some((size, at, digest)) => {
                if *size == meta.len() && *at == mtime(&meta) {
                    continue;
                }
                if hash(&path).ok().as_deref() != Some(digest.as_str()) {
                    files.push(file(FileChange::Modified, String::new(), 0, false));
                }
            }
        }
    }
    for rel in snapshot.keys().filter(|k| !seen.contains(*k)) {
        files.push(FileDiff {
            path: rel.clone(),
            old_path: None,
            change: FileChange::Deleted,
            additions: 0,
            deletions: 0,
            binary: false,
            patch: String::new(),
            truncated: false,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

/// What changed in `cwd` since `start` (F-TH-13).
pub async fn changes(cwd: PathBuf, start: Option<Start>) -> Result<ThreadChanges, IpcError> {
    match start {
        None => Ok(ThreadChanges {
            source: ChangesSource::NotStarted,
            files: Vec::new(),
        }),
        Some(Start::Commit(base)) => {
            let repo = Repo::discover(&cwd).map_err(io)?.ok_or_else(|| {
                IpcError::new(
                    ErrorCode::NotFound,
                    "This folder is no longer a git repository",
                )
            })?;
            let base = (!base.is_empty()).then_some(base);
            let files = repo.diff_from(base).await.map_err(io)?;
            Ok(ThreadChanges {
                source: ChangesSource::Git,
                files,
            })
        }
        Some(Start::Snapshot(path)) => tokio::task::spawn_blocking(move || {
            let body = std::fs::read(&path).map_err(io)?;
            let snapshot: Manifest = serde_json::from_slice(&body).map_err(io)?;
            Ok(ThreadChanges {
                source: ChangesSource::Snapshot,
                files: snapshot_diff(&cwd, &snapshot),
            })
        })
        .await
        .map_err(io)?,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(cwd: &Path, args: &[&str]) {
        let empty = std::env::temp_dir().join("tandem-desktop-tests-empty-gitconfig");
        std::fs::write(&empty, "").expect("empty gitconfig");
        let out = Command::new("git")
            .current_dir(cwd)
            .env("GIT_CONFIG_GLOBAL", &empty)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .args(args)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().canonicalize().expect("canonical").join("repo");
        std::fs::create_dir_all(root.join("app")).expect("mkdir");
        git(&root, &["-c", "init.defaultBranch=main", "init", "-q"]);
        std::fs::write(root.join("app/a.txt"), "one\ntwo\n").expect("write");
        std::fs::write(root.join("top.txt"), "top\n").expect("write");
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "first"]);
        (dir, root)
    }

    fn summary(c: &ThreadChanges) -> Vec<(String, FileChange)> {
        c.files.iter().map(|f| (f.path.clone(), f.change)).collect()
    }

    #[tokio::test]
    async fn f_th_13_1_a_git_thread_lists_both_edited_files_with_diffs() {
        let (dir, root) = repo();
        let start = capture(root.clone(), Some(dir.path().join("snap.json")))
            .await
            .expect("start");
        assert!(matches!(&start, Start::Commit(c) if c.len() == 40));
        std::fs::write(root.join("app/a.txt"), "one\nTWO\n").expect("edit");
        std::fs::write(root.join("app/b.txt"), "new\n").expect("add");
        let c = changes(root.clone(), Some(start)).await.expect("changes");
        assert_eq!(c.source, ChangesSource::Git);
        assert_eq!(
            summary(&c),
            vec![
                ("app/a.txt".into(), FileChange::Modified),
                ("app/b.txt".into(), FileChange::Added)
            ]
        );
        assert!(c.files[0].patch.contains("-two\n+TWO\n"));
        // No snapshot is written for a git folder.
        assert!(!dir.path().join("snap.json").exists());
    }

    #[tokio::test]
    async fn f_th_13_2_a_subfolder_thread_lists_only_its_folder() {
        let (dir, root) = repo();
        let sub = root.join("app");
        let start = capture(sub.clone(), Some(dir.path().join("snap.json")))
            .await
            .expect("start");
        std::fs::write(sub.join("a.txt"), "changed\n").expect("edit");
        std::fs::write(root.join("top.txt"), "changed\n").expect("edit outside");
        let c = changes(sub, Some(start)).await.expect("changes");
        assert_eq!(
            summary(&c),
            vec![("app/a.txt".into(), FileChange::Modified)]
        );
    }

    #[tokio::test]
    async fn f_th_13_a_folder_without_git_compares_a_snapshot() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().join("plain");
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::write(root.join("src/keep.txt"), "same\n").expect("write");
        std::fs::write(root.join("src/edit.txt"), "before\n").expect("write");
        std::fs::write(root.join("gone.txt"), "bye\n").expect("write");
        std::fs::write(root.join(".gitignore"), "build/\n").expect("write");
        let snap = dir.path().join("snapshots/t.json");
        let start = capture(root.clone(), Some(snap.clone()))
            .await
            .expect("start");
        assert_eq!(start, Start::Snapshot(snap.clone()));
        assert!(snap.exists());
        std::fs::write(root.join("src/edit.txt"), "after, longer\n").expect("edit");
        std::fs::remove_file(root.join("gone.txt")).expect("rm");
        std::fs::write(root.join("src/new.txt"), "a\nb\n").expect("add");
        std::fs::create_dir_all(root.join("build")).expect("mkdir");
        std::fs::write(root.join("build/out.bin"), "ignored").expect("ignored");
        let c = changes(root, Some(start)).await.expect("changes");
        assert_eq!(c.source, ChangesSource::Snapshot);
        assert_eq!(
            summary(&c),
            vec![
                ("gone.txt".into(), FileChange::Deleted),
                ("src/edit.txt".into(), FileChange::Modified),
                ("src/new.txt".into(), FileChange::Added)
            ]
        );
        // An added text file shows its lines; a changed one has no old text.
        let added = &c.files[2];
        assert_eq!(added.additions, 2);
        assert!(added.patch.contains("+a\n+b\n"));
        assert!(c.files[1].patch.is_empty());
    }

    #[tokio::test]
    async fn f_th_13_a_thread_that_has_not_started_has_nothing_yet() {
        let c = changes(std::env::temp_dir(), None).await.expect("changes");
        assert_eq!(c.source, ChangesSource::NotStarted);
        assert!(c.files.is_empty());
    }
}
