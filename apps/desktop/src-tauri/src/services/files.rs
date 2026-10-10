//! The Files tab (F-CD-19) and the repository status its badges and the git
//! panel share (F-CD-20). A context (a Code workspace or a thread's folder)
//! resolves to a folder; paths outside every workspace are read-only
//! (F-CD-09); one watcher per folder fans its batches out to every
//! subscriber and refreshes the cached status.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tandem_core::contain::ContainPolicy;
use tandem_core::error::{ErrorCode, IpcError};
use tandem_core::files::watch::{self, DirWatcher, FsChange, GitDirs, WatchOptions};
use tandem_core::files::{self, FileKind, FilesError, FsKind, ReadOnlyReason};
use tandem_core::ids::{ThreadId, WorkspaceId};
use tandem_core::origin::Origin;
use tandem_git::{FileChange, FileDiff, GitStatus, Repo};
use tandem_store::repo::editor_tabs::{self, EditorTabs};
use tandem_store::repo::{threads as threads_repo, workspaces};
use tandem_store::Store;
use ts_rs::TS;

/// A cached status is reused this long when no watch event came in.
pub const STATUS_TTL: Duration = Duration::from_secs(2);

pub const FOLDER_MISSING: &str = "This folder is missing. It may have been moved or deleted.";
pub const NO_FOLDER: &str = "This thread has no folder";

/// Whose files the Files tab shows (ARCHITECTURE §3.2 `FileContext`; the
/// teammate Home joins with M10).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum FileContext {
    Workspace { workspace_id: WorkspaceId },
    Thread { thread_id: ThreadId },
}

impl FileContext {
    /// The `editor_tabs` key.
    pub fn key(&self) -> String {
        match self {
            FileContext::Workspace { workspace_id } => format!("ws:{}", workspace_id.as_str()),
            FileContext::Thread { thread_id } => format!("th:{}", thread_id.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FsEntry {
    pub name: String,
    /// Relative to the context's folder, `/`-separated.
    pub path: String,
    pub kind: FsKind,
    pub symlink: bool,
    /// A folder carries Untracked when everything changed in it is new,
    /// else Modified.
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_status: Option<FileChange>,
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileContent {
    /// Relative to the context's folder, or absolute outside it.
    pub path: String,
    pub kind: FileKind,
    pub text: Option<String>,
    pub size: u64,
    pub readonly: bool,
    pub readonly_reason: Option<ReadOnlyReason>,
    pub truncated: bool,
    pub hash: String,
    pub mtime: i64,
    pub data_url: Option<String>,
    pub hex: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Saved {
    pub hash: String,
    pub mtime: i64,
}

/// A repository and its status, as of a moment.
#[derive(Debug, Clone)]
pub struct RepoStatus {
    pub repo: Repo,
    pub status: GitStatus,
}

/// Delivers a batch to one subscriber; false once it's gone.
pub type ChangeSink = Arc<dyn Fn(&FsChange) -> bool + Send + Sync>;

type Subscribers = HashMap<PathBuf, HashMap<String, ChangeSink>>;
type StatusCache = HashMap<PathBuf, (Instant, Option<Arc<RepoStatus>>)>;

pub struct FilesService {
    store: Store,
    /// SSH hosts: their contexts are served by `tandem-host` (F-RM-01).
    remote: std::sync::OnceLock<Arc<crate::services::remote::RemoteService>>,
    watchers: Mutex<HashMap<PathBuf, DirWatcher>>,
    subs: Arc<Mutex<Subscribers>>,
    statuses: Arc<Mutex<StatusCache>>,
    watch_opts: WatchOptions,
    next_sub: std::sync::atomic::AtomicU64,
    /// Folders outside every workspace that are still editable: the
    /// profile's prompt library (F-EX-05).
    extra_editable: Mutex<Vec<PathBuf>>,
}

fn io_err(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Internal, e.to_string()).with_stage("files")
}

async fn blocking<T, F>(f: F) -> Result<T, IpcError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, IpcError> + Send + 'static,
{
    tokio::task::spawn_blocking(f).await.map_err(io_err)?
}

/// Where this machine reads a workspace's files: its folder, or for a WSL
/// workspace the distro's folder as Windows sees it
/// (`\\wsl.localhost\<distro>\…`, F-RM-02).
pub fn local_root(ws: &tandem_store::repo::workspaces::Workspace) -> PathBuf {
    match (ws.kind.as_str(), ws.wsl_distro.as_deref()) {
        ("wsl", Some(distro)) => tandem_core::wslpath::from_wsl_path(distro, &ws.root)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&ws.root)),
        _ => PathBuf::from(&ws.root),
    }
}

/// Paths with a `.git` component are git's own data: never created,
/// renamed into or deleted from the tree.
fn in_git_data(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git"))
}

impl FilesService {
    pub fn new(store: Store) -> Arc<Self> {
        Self::with_options(store, WatchOptions::default())
    }

    pub fn with_options(store: Store, watch_opts: WatchOptions) -> Arc<Self> {
        Arc::new(Self {
            store,
            remote: std::sync::OnceLock::new(),
            watchers: Mutex::default(),
            subs: Arc::default(),
            statuses: Arc::default(),
            watch_opts,
            next_sub: std::sync::atomic::AtomicU64::new(1),
            extra_editable: Mutex::default(),
        })
    }

    /// Files under `dir` open editable although no workspace holds them.
    pub fn allow_editing(&self, dir: PathBuf) {
        if let Ok(mut extra) = self.extra_editable.lock() {
            extra.push(dir);
        }
    }

    pub fn set_remote(&self, remote: Arc<crate::services::remote::RemoteService>) {
        let _ = self.remote.set(remote);
    }

    /// A context on an SSH host: the host and the folder there. `None`
    /// for local contexts.
    pub async fn remote_of(
        &self,
        ctx: &FileContext,
    ) -> Result<
        Option<(
            Arc<crate::services::remote::RemoteService>,
            crate::services::remote_files::RemoteRoot,
        )>,
        IpcError,
    > {
        let (ws_id, cwd) = match ctx {
            FileContext::Workspace { workspace_id } => (Some(workspace_id.clone()), None),
            FileContext::Thread { thread_id } => {
                let thread = threads_repo::get(&self.store, thread_id)
                    .await?
                    .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That thread is gone"))?;
                (thread.workspace_id.clone(), Some(thread.cwd))
            }
        };
        let Some(ws_id) = ws_id else { return Ok(None) };
        let ws = workspaces::get(&self.store, &ws_id).await?;
        if ws.kind != "ssh" {
            return Ok(None);
        }
        let remote = self.remote.get().cloned().ok_or_else(|| {
            IpcError::new(
                ErrorCode::Unavailable,
                "Remote hosts aren't available in this build",
            )
        })?;
        Ok(Some((
            remote,
            crate::services::remote_files::RemoteRoot {
                host: tandem_core::ids::HostId::from_db(ws.host_id),
                root: cwd.unwrap_or(ws.root),
            },
        )))
    }

    /// The context's folder (canonical, as workspaces are stored).
    pub async fn root(&self, ctx: &FileContext) -> Result<PathBuf, IpcError> {
        let root = match ctx {
            FileContext::Workspace { workspace_id } => {
                let ws = workspaces::get(&self.store, workspace_id).await?;
                local_root(&ws)
            }
            FileContext::Thread { thread_id } => {
                let thread = threads_repo::get(&self.store, thread_id)
                    .await?
                    .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "That thread is gone"))?;
                let Some(ws_id) = thread.workspace_id.as_ref() else {
                    return Err(IpcError::new(ErrorCode::Invalid, NO_FOLDER).with_stage("files"));
                };
                match workspaces::get(&self.store, ws_id).await {
                    Ok(ws) if ws.kind == "wsl" => {
                        local_root(&tandem_store::repo::workspaces::Workspace {
                            root: thread.cwd.clone(),
                            ..ws
                        })
                    }
                    _ => PathBuf::from(thread.cwd),
                }
            }
        };
        if !root.is_dir() {
            return Err(IpcError::new(ErrorCode::NotFound, FOLDER_MISSING).with_stage("files"));
        }
        Ok(root)
    }

    /// Every workspace's folder: files in any of them are editable (D-201).
    pub async fn workspace_roots(&self) -> Result<Vec<PathBuf>, IpcError> {
        Ok(workspaces::list(&self.store)
            .await?
            .into_iter()
            .filter(|w| w.kind == "local" || w.kind == "wsl")
            .map(|w| local_root(&w))
            .collect())
    }

    /// The workspaces' folders and the context's own.
    async fn editable_roots(&self, root: &Path) -> Result<Vec<PathBuf>, IpcError> {
        let mut roots = self.workspace_roots().await?;
        roots.push(root.to_path_buf());
        if let Ok(extra) = self.extra_editable.lock() {
            roots.extend(extra.iter().cloned());
        }
        Ok(roots)
    }

    fn resolve(root: &Path, path: &str) -> Result<PathBuf, IpcError> {
        Ok(files::resolve(
            root,
            path,
            &Origin::main_window(),
            ContainPolicy::Read,
        )?)
    }

    /// `fs_list`: one level with git badges.
    pub async fn list(
        &self,
        ctx: &FileContext,
        dir: String,
        show_ignored: bool,
    ) -> Result<Vec<FsEntry>, IpcError> {
        if let Some((remote, r)) = self.remote_of(ctx).await? {
            return remote.files.list(&remote, &r, dir, show_ignored).await;
        }
        let root = self.root(ctx).await?;
        let status = self.status(&root).await;
        blocking(move || {
            let entries = files::list_dir(&root, &dir, show_ignored)?;
            let badges = status.as_deref().map(badges).unwrap_or_default();
            Ok(entries
                .into_iter()
                .map(|e| FsEntry {
                    git_status: badges.get(&e.path).copied(),
                    name: e.name,
                    path: e.path,
                    kind: e.kind,
                    symlink: e.symlink,
                    ignored: e.ignored,
                })
                .collect())
        })
        .await
    }

    /// `fs_read`: a file for the editor, read-only outside every workspace,
    /// over 5 MB or when the file system says so.
    pub async fn read(&self, ctx: &FileContext, path: String) -> Result<FileContent, IpcError> {
        if let Some((remote, r)) = self.remote_of(ctx).await? {
            return remote.files.read(&remote, &r, path).await;
        }
        let root = self.root(ctx).await?;
        let roots = self.editable_roots(&root).await?;
        blocking(move || {
            let abs = Self::resolve(&root, &path)?;
            let read = files::read_file(&abs)?;
            let reason = if !files::inside_any(&abs, &roots) {
                Some(ReadOnlyReason::Outside)
            } else if read.large {
                Some(ReadOnlyReason::Large)
            } else if !read.writable {
                Some(ReadOnlyReason::Permission)
            } else {
                None
            };
            Ok(FileContent {
                path: files::display_path(&root, &abs),
                kind: read.kind,
                text: read.text,
                size: read.size,
                readonly: reason.is_some(),
                readonly_reason: reason,
                truncated: read.truncated,
                hash: read.hash,
                mtime: read.mtime,
                data_url: read.data_url,
                hex: read.hex,
            })
        })
        .await
    }

    async fn writable_path(&self, ctx: &FileContext, path: &str) -> Result<PathBuf, IpcError> {
        let root = self.root(ctx).await?;
        let roots = self.editable_roots(&root).await?;
        let abs = Self::resolve(&root, path)?;
        if in_git_data(abs.strip_prefix(&root).unwrap_or(&abs)) {
            return Err(IpcError::new(
                ErrorCode::PermissionDenied,
                "Vibezeo doesn't change git's own files",
            )
            .with_stage("files"));
        }
        let check = abs.clone();
        let inside = blocking(move || Ok(files::inside_any(&check, &roots))).await?;
        if !inside {
            return Err(FilesError::ReadOnly(ReadOnlyReason::Outside).into());
        }
        Ok(abs)
    }

    /// `fs_write`: saves only while the file is still what the editor
    /// opened (`expected`), else `Conflict` ("File changed on disk").
    pub async fn write(
        &self,
        ctx: &FileContext,
        path: String,
        text: String,
        expected: Option<String>,
    ) -> Result<Saved, IpcError> {
        if let Some((remote, r)) = self.remote_of(ctx).await? {
            return remote.files.write(&remote, &r, path, text, expected).await;
        }
        let abs = self.writable_path(ctx, &path).await?;
        let written =
            blocking(move || Ok(files::write_file(&abs, &text, expected.as_deref())?)).await?;
        tracing::debug!(code = "files.saved", "saved a file");
        Ok(Saved {
            hash: written.hash,
            mtime: written.mtime,
        })
    }

    /// `fs_compare`: the file on disk → `text`.
    pub async fn compare(
        &self,
        ctx: &FileContext,
        path: String,
        text: String,
    ) -> Result<FileDiff, IpcError> {
        if let Some((remote, r)) = self.remote_of(ctx).await? {
            return remote.files.compare(&remote, &r, path, text).await;
        }
        let root = self.root(ctx).await?;
        blocking(move || {
            let abs = Self::resolve(&root, &path)?;
            let disk = std::fs::read(&abs).map_err(FilesError::from)?;
            let name = files::display_path(&root, &abs);
            Ok(tandem_git::diff_texts(&name, &disk, text.as_bytes())?)
        })
        .await
    }

    /// `fs_create`.
    pub async fn create(
        &self,
        ctx: &FileContext,
        path: String,
        kind: FsKind,
    ) -> Result<(), IpcError> {
        if let Some((remote, r)) = self.remote_of(ctx).await? {
            return remote.files.create(&remote, &r, path, kind).await;
        }
        let abs = self.writable_path(ctx, &path).await?;
        blocking(move || Ok(files::create(&abs, kind)?)).await
    }

    /// `fs_rename`: both ends inside a workspace.
    pub async fn rename(
        &self,
        ctx: &FileContext,
        from: String,
        to: String,
    ) -> Result<(), IpcError> {
        if let Some((remote, r)) = self.remote_of(ctx).await? {
            return remote.files.rename(&remote, &r, from, to).await;
        }
        let from = self.writable_path(ctx, &from).await?;
        let to = self.writable_path(ctx, &to).await?;
        blocking(move || Ok(files::rename(&from, &to)?)).await
    }

    /// `fs_delete` (the UI has confirmed).
    pub async fn delete(&self, ctx: &FileContext, path: String) -> Result<(), IpcError> {
        if let Some((remote, r)) = self.remote_of(ctx).await? {
            return remote.files.delete(&remote, &r, path).await;
        }
        let root = self.root(ctx).await?;
        let abs = self.writable_path(ctx, &path).await?;
        if abs == root {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Remove the workspace instead of deleting its folder",
            ));
        }
        blocking(move || Ok(files::delete(&abs)?)).await
    }

    /// The repository status for `root` (cached for a moment; a watch
    /// event or [`FilesService::invalidate`] drops it). `None` outside a
    /// repository. Hardened by construction: libgit2 runs no repo helper.
    pub async fn status(&self, root: &Path) -> Option<Arc<RepoStatus>> {
        if let Ok(cache) = self.statuses.lock() {
            if let Some((at, status)) = cache.get(root) {
                if at.elapsed() < STATUS_TTL {
                    return status.clone();
                }
            }
        }
        let path = root.to_path_buf();
        let fresh = tokio::task::spawn_blocking(move || {
            let repo = Repo::discover(&path).ok().flatten()?;
            match repo.status_blocking() {
                Ok(status) => Some(Arc::new(RepoStatus { repo, status })),
                Err(err) => {
                    tracing::debug!(code = "files.status_failed", error = %err, "git status failed");
                    None
                }
            }
        })
        .await
        .ok()
        .flatten();
        if let Ok(mut cache) = self.statuses.lock() {
            cache.insert(root.to_path_buf(), (Instant::now(), fresh.clone()));
        }
        fresh
    }

    /// Forgets the cached status (after a git action).
    pub fn invalidate(&self, root: &Path) {
        if let Ok(mut cache) = self.statuses.lock() {
            cache.remove(root);
        }
    }

    /// Forgets the cached status of every folder in the repository at
    /// `repo_root` (a git action changes them all).
    pub fn invalidate_repo(&self, repo_root: &Path) {
        if let Ok(mut cache) = self.statuses.lock() {
            cache.retain(|folder, (_, status)| match status {
                Some(rs) => rs.repo.root() != repo_root,
                None => !folder.starts_with(repo_root),
            });
        }
    }

    /// `fs_watch`: `sink` gets every batch for the context's folder until
    /// it returns false or [`FilesService::unwatch`]. One watcher per folder.
    pub async fn watch(&self, ctx: &FileContext, sink: ChangeSink) -> Result<String, IpcError> {
        let id = format!(
            "fsw_{}",
            self.next_sub
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        // A host's folder is watched by its helper (D-1600).
        if let Some((remote, r)) = self.remote_of(ctx).await? {
            remote.files.watch(&remote, &r, id.clone(), sink).await?;
            return Ok(id);
        }
        let root = self.root(ctx).await?;
        self.subs
            .lock()
            .map_err(io_err)?
            .entry(root.clone())
            .or_default()
            .insert(id.clone(), sink);
        let running = self.watchers.lock().map_err(io_err)?.contains_key(&root);
        if !running {
            let git = blocking({
                let root = root.clone();
                move || {
                    Ok(Repo::discover(&root).ok().flatten().map(|r| GitDirs {
                        git_dir: r.git_dir().to_path_buf(),
                        common_dir: r.common_dir().to_path_buf(),
                    }))
                }
            })
            .await?;
            let (subs, statuses, key) = (self.subs.clone(), self.statuses.clone(), root.clone());
            let watcher = watch::watch(
                &root,
                git,
                self.watch_opts.clone(),
                Box::new(move |change| {
                    if let Ok(mut cache) = statuses.lock() {
                        cache.remove(&key);
                    }
                    if let Ok(mut subs) = subs.lock() {
                        if let Some(list) = subs.get_mut(&key) {
                            list.retain(|_, sink| sink(&change));
                        }
                    }
                }),
            )?;
            let mut watchers = self.watchers.lock().map_err(io_err)?;
            watchers.entry(root).or_insert(watcher);
        }
        Ok(id)
    }

    /// `fs_unwatch`. The folder's watcher stops with its last subscriber.
    pub fn unwatch(&self, sub: &str) {
        if let Some(remote) = self.remote.get() {
            if remote.files.unwatch(remote, sub) {
                return;
            }
        }
        let mut idle = Vec::new();
        if let Ok(mut subs) = self.subs.lock() {
            for (root, list) in subs.iter_mut() {
                list.remove(sub);
                if list.is_empty() {
                    idle.push(root.clone());
                }
            }
            for root in &idle {
                subs.remove(root);
            }
        }
        let stopped: Vec<DirWatcher> = match self.watchers.lock() {
            Ok(mut w) => idle.iter().filter_map(|r| w.remove(r)).collect(),
            Err(_) => Vec::new(),
        };
        drop(stopped);
    }

    /// Folders being watched (tests).
    pub fn watched_roots(&self) -> usize {
        self.watchers.lock().map(|w| w.len()).unwrap_or(0)
    }

    pub async fn tabs_get(&self, key: &str) -> Result<EditorTabs, IpcError> {
        Ok(editor_tabs::get(&self.store, key).await?)
    }

    pub async fn tabs_save(&self, key: &str, state: EditorTabs) -> Result<(), IpcError> {
        Ok(editor_tabs::save(&self.store, key, state).await?)
    }
}

/// The badge for each changed file and folder, keyed by path relative to
/// the workspace (the repository may start above it).
pub fn badges(rs: &RepoStatus) -> HashMap<String, FileChange> {
    badges_for(&files::slash_path(rs.repo.prefix()), &rs.status)
}

/// [`badges`] for a status whose repository starts `prefix` above the
/// workspace (`/`-separated, `""` at the top): a remote one too.
pub fn badges_for(
    prefix: &str,
    status: &tandem_git::types::GitStatus,
) -> HashMap<String, FileChange> {
    let prefix = prefix.to_string();
    let rel = |p: &str| -> Option<String> {
        if prefix.is_empty() {
            Some(p.to_string())
        } else {
            p.strip_prefix(&prefix)
                .and_then(|r| r.strip_prefix('/'))
                .map(str::to_string)
        }
    };
    let mut out: HashMap<String, FileChange> = HashMap::new();
    let staged_added: std::collections::HashSet<&str> = status
        .staged
        .iter()
        .filter(|f| f.change == FileChange::Added)
        .map(|f| f.path.as_str())
        .collect();
    let files = status.staged.iter().chain(status.unstaged.iter());
    for f in files {
        let Some(path) = rel(&f.path) else {
            continue;
        };
        let change = if staged_added.contains(f.path.as_str()) && f.change != FileChange::Conflicted
        {
            FileChange::Added
        } else {
            f.change
        };
        let slot = out.entry(path.clone()).or_insert(change);
        if change == FileChange::Conflicted {
            *slot = change;
        }
        // Folders above it: Untracked while everything in them is new.
        let mut dir = Path::new(&path).parent();
        while let Some(d) = dir.filter(|d| !d.as_os_str().is_empty()) {
            let key = files::slash_path(d);
            let folder = if change == FileChange::Untracked {
                FileChange::Untracked
            } else {
                FileChange::Modified
            };
            out.entry(key)
                .and_modify(|c| {
                    if *c != folder {
                        *c = FileChange::Modified;
                    }
                })
                .or_insert(folder);
            dir = d.parent();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tandem_core::DataDir;

    fn git(cwd: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(cwd)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args([
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    struct Fx {
        dir: tempfile::TempDir,
        files: Arc<FilesService>,
        store: Store,
        root: PathBuf,
        ctx: FileContext,
    }

    async fn fx() -> Fx {
        let dir = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let root = std::fs::canonicalize(dir.path())
            .expect("canon")
            .join("repo");
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("write");
        std::fs::write(root.join(".gitignore"), "node_modules/\n").expect("write");
        std::fs::create_dir_all(root.join("node_modules/x")).expect("mkdir");
        git(&root, &["init", "-q"]);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "first"]);
        let ws = crate::services::workspaces::add(&store, root.to_str().expect("utf8"), None)
            .await
            .expect("add");
        let files = FilesService::with_options(
            store.clone(),
            WatchOptions {
                debounce: Duration::from_millis(100),
                ..WatchOptions::default()
            },
        );
        Fx {
            dir,
            files,
            store,
            root,
            ctx: FileContext::Workspace {
                workspace_id: ws.workspace.id,
            },
        }
    }

    #[tokio::test]
    async fn f_cd_19_the_tree_carries_git_badges_for_files_and_folders() {
        let f = fx().await;
        std::fs::write(f.root.join("src/main.rs"), "fn main() { 1; }\n").expect("write");
        std::fs::create_dir_all(f.root.join("docs/new")).expect("mkdir");
        std::fs::write(f.root.join("docs/new/a.md"), "a\n").expect("write");
        std::fs::write(f.root.join("staged.txt"), "s\n").expect("write");
        git(&f.root, &["add", "staged.txt"]);
        let top = f
            .files
            .list(&f.ctx, String::new(), false)
            .await
            .expect("list");
        let badge = |name: &str| {
            top.iter()
                .find(|e| e.name == name)
                .and_then(|e| e.git_status)
        };
        assert_eq!(badge("src"), Some(FileChange::Modified));
        assert_eq!(badge("docs"), Some(FileChange::Untracked));
        assert_eq!(badge("staged.txt"), Some(FileChange::Added));
        assert_eq!(badge(".gitignore"), None);
        assert!(top.iter().all(|e| e.name != "node_modules"));
        let src = f
            .files
            .list(&f.ctx, "src".into(), false)
            .await
            .expect("list");
        assert_eq!(src[0].git_status, Some(FileChange::Modified));
        let shown = f
            .files
            .list(&f.ctx, String::new(), true)
            .await
            .expect("list");
        assert!(shown.iter().any(|e| e.name == "node_modules" && e.ignored));
    }

    #[tokio::test]
    async fn f_cd_19_a_subfolder_workspace_badges_relative_to_itself() {
        let f = fx().await;
        let sub = crate::services::workspaces::add(
            &f.store,
            f.root.join("src").to_str().expect("utf8"),
            None,
        )
        .await
        .expect("add");
        std::fs::write(f.root.join("src/main.rs"), "changed\n").expect("write");
        let ctx = FileContext::Workspace {
            workspace_id: sub.workspace.id,
        };
        let list = f
            .files
            .list(&ctx, String::new(), false)
            .await
            .expect("list");
        assert_eq!(list[0].path, "main.rs");
        assert_eq!(list[0].git_status, Some(FileChange::Modified));
    }

    #[tokio::test]
    async fn f_cd_09_2_a_file_outside_every_workspace_opens_read_only() {
        let f = fx().await;
        let outside = std::fs::canonicalize(f.dir.path())
            .expect("canon")
            .join("hosts");
        std::fs::write(&outside, "127.0.0.1 localhost\n").expect("write");
        let read = f
            .files
            .read(&f.ctx, outside.to_string_lossy().into_owned())
            .await
            .expect("read");
        assert!(read.readonly);
        assert_eq!(read.readonly_reason, Some(ReadOnlyReason::Outside));
        assert_eq!(read.path, outside.to_string_lossy());
        let err = f
            .files
            .write(
                &f.ctx,
                outside.to_string_lossy().into_owned(),
                "pwned".into(),
                Some(read.hash),
            )
            .await
            .expect_err("read-only");
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        assert_eq!(
            std::fs::read_to_string(&outside).expect("read"),
            "127.0.0.1 localhost\n"
        );
        // Inside: editable, and an absolute path shows relative.
        let inside = f
            .files
            .read(
                &f.ctx,
                f.root.join("src/main.rs").to_string_lossy().into_owned(),
            )
            .await
            .expect("read");
        assert!(!inside.readonly);
        assert_eq!(inside.path, "src/main.rs");
    }

    #[tokio::test]
    async fn f_cd_19_files_in_another_workspace_are_editable_too() {
        let f = fx().await;
        let other = std::fs::canonicalize(f.dir.path())
            .expect("canon")
            .join("other");
        std::fs::create_dir_all(&other).expect("mkdir");
        std::fs::write(other.join("b.txt"), "b\n").expect("write");
        crate::services::workspaces::add(&f.store, other.to_str().expect("utf8"), None)
            .await
            .expect("add");
        let path = other.join("b.txt").to_string_lossy().into_owned();
        let read = f.files.read(&f.ctx, path.clone()).await.expect("read");
        assert!(!read.readonly);
        f.files
            .write(&f.ctx, path, "B\n".into(), Some(read.hash))
            .await
            .expect("write");
        assert_eq!(
            std::fs::read_to_string(other.join("b.txt")).expect("read"),
            "B\n"
        );
    }

    #[tokio::test]
    async fn f_cd_19_1_save_after_an_agent_changed_the_file_is_a_conflict() {
        let f = fx().await;
        let read = f
            .files
            .read(&f.ctx, "src/main.rs".into())
            .await
            .expect("read");
        std::fs::write(f.root.join("src/main.rs"), "agent\n").expect("agent");
        let err = f
            .files
            .write(
                &f.ctx,
                "src/main.rs".into(),
                "mine\n".into(),
                Some(read.hash),
            )
            .await
            .expect_err("conflict");
        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(err.message, "File changed on disk");
        let diff = f
            .files
            .compare(&f.ctx, "src/main.rs".into(), "mine\n".into())
            .await
            .expect("compare");
        assert!(diff.patch.contains("-agent\n+mine\n"), "{}", diff.patch);
        assert_eq!(diff.path, "src/main.rs");
    }

    #[tokio::test]
    async fn f_cd_19_git_data_and_the_root_itself_are_off_limits() {
        let f = fx().await;
        let err = f
            .files
            .create(&f.ctx, ".git/hooks/pre-commit".into(), FsKind::File)
            .await
            .expect_err("git data");
        assert_eq!(err.code, ErrorCode::PermissionDenied);
        let err = f
            .files
            .delete(&f.ctx, String::new())
            .await
            .expect_err("root");
        assert_eq!(err.code, ErrorCode::Invalid);
        f.files
            .create(&f.ctx, "src/new.rs".into(), FsKind::File)
            .await
            .expect("create");
        f.files
            .rename(&f.ctx, "src/new.rs".into(), "src/renamed.rs".into())
            .await
            .expect("rename");
        f.files
            .delete(&f.ctx, "src/renamed.rs".into())
            .await
            .expect("delete");
        assert!(!f.root.join("src/renamed.rs").exists());
    }

    #[tokio::test]
    async fn f_cd_19_a_thread_context_is_its_folder_and_a_folderless_one_has_none() {
        use tandem_core::access::{Access, ThreadMode};
        use tandem_store::repo::threads::{NewThread, ThreadKind};
        let f = fx().await;
        let FileContext::Workspace { workspace_id } = &f.ctx else {
            panic!("workspace context");
        };
        let mk = |id: &str, ws: Option<WorkspaceId>, cwd: String| NewThread {
            id: ThreadId::from_db(id.into()),
            kind: ThreadKind::Thread,
            workspace_id: ws,
            cwd,
            engine_id: "claude".into(),
            account_id: None,
            model: None,
            effort: None,
            access: Access::Edits,
            mode: ThreadMode::Act,
            attended: true,
        };
        threads_repo::insert(
            &f.store,
            mk(
                "th_a",
                Some(workspace_id.clone()),
                f.root.to_string_lossy().into_owned(),
            ),
        )
        .await
        .expect("thread");
        threads_repo::insert(&f.store, mk("th_b", None, "/tmp/scratch".into()))
            .await
            .expect("thread");
        let with = FileContext::Thread {
            thread_id: ThreadId::from_db("th_a".into()),
        };
        assert_eq!(f.files.root(&with).await.expect("root"), f.root);
        assert_eq!(with.key(), "th:th_a");
        let without = FileContext::Thread {
            thread_id: ThreadId::from_db("th_b".into()),
        };
        let err = f.files.root(&without).await.expect_err("no folder");
        assert_eq!(err.message, NO_FOLDER);
    }

    #[tokio::test]
    async fn f_cd_20_a_watch_event_drops_the_cached_status_and_reaches_every_subscriber() {
        let f = fx().await;
        let root = f.files.root(&f.ctx).await.expect("root");
        assert!(f.files.status(&root).await.expect("repo").status.is_clean());
        let hits = Arc::new(AtomicUsize::new(0));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<FsChange>();
        let h = hits.clone();
        let a = f
            .files
            .watch(
                &f.ctx,
                Arc::new(move |c: &FsChange| {
                    h.fetch_add(1, Ordering::SeqCst);
                    tx.send(c.clone()).is_ok()
                }),
            )
            .await
            .expect("watch");
        let b = f
            .files
            .watch(&f.ctx, Arc::new(|_: &FsChange| true))
            .await
            .expect("watch");
        assert_eq!(f.files.watched_roots(), 1, "one watcher per folder");
        std::fs::write(f.root.join("src/main.rs"), "changed\n").expect("write");
        let change = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("in time")
            .expect("change");
        assert!(
            change.paths.contains(&"src/main.rs".to_string()),
            "{change:?}"
        );
        // The cached clean status is gone: the change shows at once.
        assert!(!f.files.status(&root).await.expect("repo").status.is_clean());
        f.files.unwatch(&a);
        assert_eq!(f.files.watched_roots(), 1);
        f.files.unwatch(&b);
        assert_eq!(f.files.watched_roots(), 0, "stops with the last subscriber");
        assert!(hits.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn f_cd_20_a_commit_from_a_terminal_flags_git_for_watchers() {
        let f = fx().await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<FsChange>();
        f.files
            .watch(
                &f.ctx,
                Arc::new(move |c: &FsChange| tx.send(c.clone()).is_ok()),
            )
            .await
            .expect("watch");
        std::fs::write(f.root.join("src/main.rs"), "next\n").expect("write");
        git(&f.root, &["commit", "-qam", "second"]);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let change = tokio::time::timeout(left, rx.recv())
                .await
                .expect("a git change in time")
                .expect("change");
            if change.git {
                break;
            }
        }
    }
}
