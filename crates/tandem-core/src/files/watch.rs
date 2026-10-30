//! Disk changes for the Files tab and the git panel (F-CD-19, F-CD-20;
//! ARCHITECTURE §7 "Status refresh"): one `notify` watcher per folder, its
//! events batched for 500 ms.
//!
//! On Linux each folder that `.gitignore` keeps gets its own non-recursive
//! inotify watch (a recursive one would also watch every folder under an
//! ignored `node_modules`); elsewhere one recursive watch is cheap. Changes
//! to the repository's own data (index, HEAD, refs) don't name paths: they
//! set [`FsChange::git`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use notify::event::{CreateKind, EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{slash_path, walker, FilesError};

/// One batch of changes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FsChange {
    /// Files and folders that changed, relative to the watched folder.
    pub paths: Vec<String>,
    /// The repository's index, HEAD or refs changed (a commit, a stage, a
    /// switch or a fetch from a terminal).
    pub git: bool,
    /// Too many changes to list (or the OS dropped some): read everything
    /// that's shown again.
    pub overflow: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// A non-recursive watch per kept folder (Linux).
    PerDir,
    /// One recursive watch (macOS, Windows).
    Recursive,
}

impl Strategy {
    pub fn for_platform() -> Self {
        if cfg!(target_os = "linux") {
            Strategy::PerDir
        } else {
            Strategy::Recursive
        }
    }
}

#[derive(Debug, Clone)]
pub struct WatchOptions {
    pub debounce: Duration,
    pub strategy: Strategy,
    /// [`Strategy::PerDir`] stops adding folders here (inotify watches are
    /// a per-user budget).
    pub max_dirs: usize,
    /// More paths than this in one batch is an overflow.
    pub max_paths: usize,
}

impl Default for WatchOptions {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(500),
            strategy: Strategy::for_platform(),
            max_dirs: 20_000,
            max_paths: 500,
        }
    }
}

/// Where a repository keeps its data: `.git` for a main checkout; for a
/// linked worktree (a lane) its own folder under the main `.git/worktrees`
/// plus the shared `.git`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDirs {
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
}

/// Stops watching when dropped.
pub struct DirWatcher {
    _watcher: Arc<Mutex<RecommendedWatcher>>,
    watched_dirs: Arc<Mutex<usize>>,
}

impl DirWatcher {
    /// Folders with their own watch ([`Strategy::PerDir`]); 1 for a
    /// recursive watch.
    pub fn watched_dirs(&self) -> usize {
        self.watched_dirs.lock().map(|n| *n).unwrap_or(0)
    }
}

type Sink = Box<dyn FnMut(FsChange) + Send + 'static>;

/// Watches `root` (canonical) and `git`'s data, calling `on_change` with
/// each batch from a background thread.
pub fn watch(
    root: &Path,
    git: Option<GitDirs>,
    opts: WatchOptions,
    on_change: Sink,
) -> Result<DirWatcher, FilesError> {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let watcher = notify::recommended_watcher(tx).map_err(notify_err)?;
    let watcher = Arc::new(Mutex::new(watcher));
    let count = Arc::new(Mutex::new(0usize));
    let state = Watching {
        root: root.to_path_buf(),
        git: git.clone(),
        opts: opts.clone(),
        watcher: Arc::downgrade(&watcher),
        count: count.clone(),
    };
    match opts.strategy {
        Strategy::Recursive => {
            state.add(root, RecursiveMode::Recursive)?;
            *count.lock().map_err(|_| poisoned())? = 1;
        }
        Strategy::PerDir => state.add_tree(root),
    }
    if let Some(g) = &git {
        let mut dirs = vec![(g.git_dir.clone(), RecursiveMode::NonRecursive)];
        if g.common_dir != g.git_dir {
            dirs.push((g.common_dir.clone(), RecursiveMode::NonRecursive));
        }
        dirs.push((g.common_dir.join("refs"), RecursiveMode::Recursive));
        for (dir, mode) in dirs {
            let covered = opts.strategy == Strategy::Recursive && dir.starts_with(root);
            if !covered && dir.is_dir() {
                if let Err(err) = state.add(&dir, mode) {
                    tracing::debug!(code = "files.watch_git_failed", error = %err, "couldn't watch the repository's data");
                }
            }
        }
    }
    std::thread::Builder::new()
        .name("files-watch".into())
        .spawn(move || state.run(rx, on_change))
        .map_err(FilesError::Io)?;
    Ok(DirWatcher {
        _watcher: watcher,
        watched_dirs: count,
    })
}

fn notify_err(e: notify::Error) -> FilesError {
    match e.kind {
        notify::ErrorKind::Io(io) => FilesError::Io(io),
        notify::ErrorKind::PathNotFound => FilesError::NotFound,
        _ => FilesError::Io(std::io::Error::other(e.to_string())),
    }
}

fn poisoned() -> FilesError {
    FilesError::Io(std::io::Error::other("watcher lock poisoned"))
}

struct Watching {
    root: PathBuf,
    git: Option<GitDirs>,
    opts: WatchOptions,
    /// Weak: the watcher owns the event channel's sender, so holding it
    /// here would keep this thread alive after the [`DirWatcher`] is gone.
    watcher: Weak<Mutex<RecommendedWatcher>>,
    count: Arc<Mutex<usize>>,
}

/// Never watched or reported: whole checkouts of other branches.
fn skipped(rel: &Path) -> bool {
    rel.starts_with(".tandem/worktrees")
}

impl Watching {
    fn add(&self, dir: &Path, mode: RecursiveMode) -> Result<(), FilesError> {
        let Some(watcher) = self.watcher.upgrade() else {
            return Ok(());
        };
        let mut w = watcher.lock().map_err(|_| poisoned())?;
        w.watch(dir, mode).map_err(notify_err)
    }

    /// Per-folder watches for `dir` and every folder under it that
    /// `.gitignore` keeps, up to the budget.
    fn add_tree(&self, dir: &Path) {
        let root = self.root.clone();
        let tree = walker(dir)
            .filter_entry(move |e| {
                let rel = e.path().strip_prefix(&root).unwrap_or(e.path());
                e.file_name() != ".git" && !skipped(rel)
            })
            .build();
        for entry in tree.flatten() {
            if !entry.file_type().is_some_and(|t| t.is_dir()) {
                continue;
            }
            let Ok(mut n) = self.count.lock() else {
                return;
            };
            if *n >= self.opts.max_dirs {
                tracing::info!(
                    code = "files.watch_capped",
                    dirs = *n,
                    "too many folders to watch; changes deeper down aren't seen"
                );
                return;
            }
            match self.add(entry.path(), RecursiveMode::NonRecursive) {
                Ok(()) => *n += 1,
                Err(err) => {
                    tracing::debug!(code = "files.watch_failed", error = %err, "couldn't watch a folder");
                }
            }
        }
    }

    /// A folder that appeared (created, or moved in): watch it unless it's
    /// ignored.
    fn adopt(&self, dir: &Path) {
        let Ok(rel) = dir.strip_prefix(&self.root) else {
            return;
        };
        if skipped(rel) || rel.components().any(|c| c.as_os_str() == ".git") {
            return;
        }
        let parent = rel.parent().unwrap_or(Path::new(""));
        let Some(name) = rel.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        if super::not_ignored(&self.root, parent).contains(name) {
            self.add_tree(dir);
        }
    }

    fn in_git_data(&self, path: &Path) -> Option<bool> {
        let g = self.git.as_ref()?;
        for dir in [&g.git_dir, &g.common_dir] {
            if let Ok(rel) = path.strip_prefix(dir) {
                // Objects are written all the time and say nothing new.
                return Some(!rel.starts_with("objects"));
            }
        }
        None
    }

    fn run(self, rx: mpsc::Receiver<notify::Result<notify::Event>>, mut on_change: Sink) {
        loop {
            let Ok(first) = rx.recv() else {
                return;
            };
            let mut batch = Batch::default();
            self.take(&mut batch, first);
            let deadline = Instant::now() + self.opts.debounce;
            let mut closed = false;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(left) {
                    Ok(ev) => self.take(&mut batch, ev),
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => {
                        closed = true;
                        break;
                    }
                }
            }
            if !batch.is_empty() {
                on_change(batch.into_change());
            }
            if closed {
                return;
            }
        }
    }

    fn take(&self, batch: &mut Batch, ev: notify::Result<notify::Event>) {
        let ev = match ev {
            Ok(ev) => ev,
            Err(err) => {
                tracing::debug!(code = "files.watch_error", error = %err, "watch error");
                batch.overflow = true;
                return;
            }
        };
        if ev.need_rescan() {
            batch.overflow = true;
        }
        if matches!(ev.kind, EventKind::Access(_)) {
            return;
        }
        let maybe_new_dir = matches!(
            ev.kind,
            EventKind::Create(CreateKind::Folder | CreateKind::Any)
                | EventKind::Modify(ModifyKind::Name(_))
        );
        for path in &ev.paths {
            match self.in_git_data(path) {
                Some(true) => {
                    batch.git = true;
                    continue;
                }
                Some(false) => continue,
                None => {}
            }
            let Ok(rel) = path.strip_prefix(&self.root) else {
                continue;
            };
            if skipped(rel) || rel.as_os_str().is_empty() {
                continue;
            }
            // A link to a folder isn't followed in: the first walk doesn't
            // follow one, and it can lead out of the folder (a link renamed
            // in had every folder it led to watched; the review of 9a4e93ab).
            if self.opts.strategy == Strategy::PerDir
                && maybe_new_dir
                && std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
            {
                self.adopt(path);
            }
            batch.push(slash_path(rel), self.opts.max_paths);
        }
    }
}

#[derive(Default)]
struct Batch {
    paths: BTreeSet<String>,
    git: bool,
    overflow: bool,
}

impl Batch {
    fn push(&mut self, path: String, max: usize) {
        if self.overflow {
            return;
        }
        self.paths.insert(path);
        if self.paths.len() > max {
            self.overflow = true;
            self.paths.clear();
        }
    }

    fn is_empty(&self) -> bool {
        self.paths.is_empty() && !self.git && !self.overflow
    }

    fn into_change(self) -> FsChange {
        FsChange {
            paths: self.paths.into_iter().collect(),
            git: self.git,
            overflow: self.overflow,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn opts(strategy: Strategy) -> WatchOptions {
        WatchOptions {
            debounce: Duration::from_millis(150),
            strategy,
            ..WatchOptions::default()
        }
    }

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tmp");
        let root = crate::contain::canonical(dir.path())
            .expect("canon")
            .join("ws");
        fs::create_dir_all(root.join("src")).expect("mkdir");
        fs::create_dir_all(root.join("node_modules/pkg")).expect("mkdir");
        fs::write(root.join(".gitignore"), "node_modules/\n").expect("write");
        (dir, root)
    }

    fn start(
        root: &Path,
        git: Option<GitDirs>,
        o: WatchOptions,
    ) -> (DirWatcher, mpsc::Receiver<FsChange>) {
        let (tx, rx) = mpsc::channel();
        let w = watch(
            root,
            git,
            o,
            Box::new(move |c| {
                let _ = tx.send(c);
            }),
        )
        .expect("watch");
        (w, rx)
    }

    /// Changes until one mentions `path` (or 5 s pass).
    fn until(rx: &mpsc::Receiver<FsChange>, pred: impl Fn(&FsChange) -> bool) -> Vec<FsChange> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            if let Ok(c) = rx.recv_timeout(Duration::from_millis(100)) {
                let done = pred(&c);
                seen.push(c);
                if done {
                    return seen;
                }
            }
        }
        panic!("no matching change; saw {seen:?}");
    }

    #[test]
    fn f_cd_19_watch_reports_file_changes_in_batches() {
        for strategy in [Strategy::PerDir, Strategy::Recursive] {
            let (_d, root) = setup();
            let (_w, rx) = start(&root, None, opts(strategy));
            for i in 0..5 {
                fs::write(root.join("src/a.rs"), format!("{i}\n")).expect("write");
            }
            fs::write(root.join("b.txt"), "b").expect("write");
            let seen = until(&rx, |c| c.paths.contains(&"b.txt".to_string()));
            let all: BTreeSet<String> = seen.iter().flat_map(|c| c.paths.clone()).collect();
            assert!(all.contains("src/a.rs"), "{strategy:?}: {seen:?}");
            // Six writes, one or two batches.
            assert!(seen.len() <= 2, "{strategy:?}: {seen:?}");
        }
    }

    #[test]
    fn f_cd_19_watch_follows_new_folders() {
        let (_d, root) = setup();
        let (_w, rx) = start(&root, None, opts(Strategy::PerDir));
        fs::create_dir_all(root.join("fresh")).expect("mkdir");
        until(&rx, |c| c.paths.contains(&"fresh".to_string()));
        fs::write(root.join("fresh/x.txt"), "x").expect("write");
        until(&rx, |c| c.paths.contains(&"fresh/x.txt".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn f_cd_19_a_link_to_a_folder_moved_in_isnt_followed() {
        let (d, root) = setup();
        let outside = d.path().join("outside");
        fs::create_dir_all(outside.join("a/b")).expect("mkdir");
        let (w, rx) = start(&root, None, opts(Strategy::PerDir));
        let before = w.watched_dirs();
        // Made elsewhere and renamed in, as an agent's `mv` would.
        std::os::unix::fs::symlink(&outside, d.path().join("link")).expect("link");
        fs::rename(d.path().join("link"), root.join("link")).expect("rename");
        until(&rx, |c| c.paths.contains(&"link".to_string()));
        assert_eq!(w.watched_dirs(), before, "nothing through the link");
        // A change out there doesn't arrive (the inside one after it does).
        fs::write(outside.join("a/x.txt"), "x").expect("write");
        fs::write(root.join("src/after.rs"), "x").expect("write");
        let seen = until(&rx, |c| c.paths.contains(&"src/after.rs".to_string()));
        assert!(
            seen.iter()
                .flat_map(|c| &c.paths)
                .all(|p| !p.starts_with("link/")),
            "{seen:?}"
        );
    }

    #[test]
    fn f_cd_19_watch_leaves_ignored_folders_alone_on_linux() {
        let (_d, root) = setup();
        let (w, rx) = start(&root, None, opts(Strategy::PerDir));
        // ws, src (node_modules and its insides aren't watched).
        assert_eq!(w.watched_dirs(), 2);
        fs::write(root.join("node_modules/pkg/index.js"), "x").expect("write");
        fs::write(root.join("src/after.rs"), "x").expect("write");
        let seen = until(&rx, |c| c.paths.contains(&"src/after.rs".to_string()));
        assert!(
            seen.iter()
                .all(|c| c.paths.iter().all(|p| !p.starts_with("node_modules/pkg"))),
            "{seen:?}"
        );
    }

    #[test]
    fn f_cd_20_watch_flags_repository_changes_without_listing_them() {
        for strategy in [Strategy::PerDir, Strategy::Recursive] {
            let (_d, root) = setup();
            let git = root.join(".git");
            fs::create_dir_all(git.join("refs/heads")).expect("mkdir");
            fs::create_dir_all(git.join("objects/ab")).expect("mkdir");
            fs::write(git.join("HEAD"), "ref: refs/heads/main\n").expect("write");
            let dirs = GitDirs {
                git_dir: git.clone(),
                common_dir: git.clone(),
            };
            let (_w, rx) = start(&root, Some(dirs), opts(strategy));
            fs::write(git.join("objects/ab/cdef"), "blob").expect("write");
            fs::write(git.join("index"), "staged").expect("write");
            let seen = until(&rx, |c| c.git);
            assert!(
                seen.iter()
                    .all(|c| c.paths.iter().all(|p| !p.starts_with(".git"))),
                "{strategy:?}: {seen:?}"
            );
            // A commit moves a branch ref.
            fs::write(git.join("refs/heads/main"), "0123\n").expect("write");
            until(&rx, |c| c.git);
        }
    }

    #[test]
    fn f_cd_20_a_lanes_repository_data_outside_its_folder_is_watched_too() {
        let (d, root) = setup();
        let common = crate::contain::canonical(d.path())
            .expect("canon")
            .join("main/.git");
        let git_dir = common.join("worktrees/lane");
        fs::create_dir_all(&git_dir).expect("mkdir");
        fs::create_dir_all(common.join("refs/heads/tandem")).expect("mkdir");
        let dirs = GitDirs {
            git_dir: git_dir.clone(),
            common_dir: common.clone(),
        };
        let (_w, rx) = start(&root, Some(dirs), opts(Strategy::PerDir));
        fs::write(git_dir.join("index"), "x").expect("write");
        until(&rx, |c| c.git);
        fs::write(common.join("refs/heads/tandem/lane"), "1\n").expect("write");
        until(&rx, |c| c.git);
    }

    #[test]
    fn f_cd_19_a_flood_of_changes_is_an_overflow() {
        let (_d, root) = setup();
        let o = WatchOptions {
            max_paths: 10,
            ..opts(Strategy::PerDir)
        };
        let (_w, rx) = start(&root, None, o);
        for i in 0..40 {
            fs::write(root.join(format!("src/f{i}.txt")), "x").expect("write");
        }
        let seen = until(&rx, |c| c.overflow);
        let last = seen.last().expect("change");
        assert!(last.paths.is_empty());
    }

    #[test]
    fn f_cd_19_dropping_the_watcher_stops_it() {
        let (_d, root) = setup();
        let (w, rx) = start(&root, None, opts(Strategy::PerDir));
        drop(w);
        std::thread::sleep(Duration::from_millis(50));
        fs::write(root.join("src/late.rs"), "x").expect("write");
        assert!(rx.recv_timeout(Duration::from_millis(600)).is_err());
    }
}
