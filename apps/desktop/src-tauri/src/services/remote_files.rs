//! Files and the Changes view for contexts on SSH hosts (F-RM-01 "Files
//! (read and write), and git"): the same commands as local contexts, served
//! by `tandem-host` (every path held to the root there by `contain()`).
//! The host's folders are watched by the helper (`fs.watch`, D-1600): its
//! batches come back as `fs.changed` and reach the same subscribers as a
//! local folder's, and a reconnect watches again.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use tandem_core::files::watch::FsChange;
use tandem_core::files::{FileKind, FsKind, ReadOnlyReason};
use tandem_core::ids::HostId;
use tandem_core::{ErrorCode, IpcError};
use tandem_git::types::{FileDiff, GitStatus};
use tandem_host::protocol::{method, Entry, EntryKind, FileData, FsWatching, FsWritten, ReadKind};

use crate::services::files::{badges_for, ChangeSink, FileContent, FsEntry, Saved, STATUS_TTL};

fn lock_err<T>(_: T) -> IpcError {
    IpcError::new(ErrorCode::Internal, "The remote files state is unavailable")
}
use crate::services::remote::RemoteService;

/// A context on a host: the host and the folder there.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RemoteRoot {
    pub host: HostId,
    pub root: String,
}

/// `git.status`'s reply.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRepo {
    pub top: String,
    pub status: GitStatus,
}

impl RemoteRepo {
    /// The context's folder inside the repository, `/`-separated.
    pub fn folder(&self, root: &str) -> String {
        root.strip_prefix(&self.top)
            .map(|r| r.trim_start_matches('/').to_string())
            .unwrap_or_default()
    }
}

type Statuses = HashMap<RemoteRoot, (Instant, Option<Arc<RemoteRepo>>)>;

/// One watched folder on a host: its subscribers, and the helper's watch
/// id while the connection holds one.
#[derive(Default)]
struct Watched {
    sinks: HashMap<String, ChangeSink>,
    host_watch: Option<String>,
}

#[derive(Default)]
pub struct RemoteFiles {
    statuses: std::sync::Mutex<Statuses>,
    watches: std::sync::Mutex<HashMap<RemoteRoot, Watched>>,
}

fn kind_in(k: EntryKind) -> FsKind {
    match k {
        EntryKind::File => FsKind::File,
        EntryKind::Dir => FsKind::Dir,
    }
}

fn kind_out(k: FsKind) -> &'static str {
    match k {
        FsKind::File => "file",
        FsKind::Dir => "dir",
    }
}

impl RemoteFiles {
    pub async fn status(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
    ) -> Option<Arc<RemoteRepo>> {
        if let Ok(cache) = self.statuses.lock() {
            if let Some((at, s)) = cache.get(r) {
                if at.elapsed() < STATUS_TTL {
                    return s.clone();
                }
            }
        }
        let fresh: Option<Arc<RemoteRepo>> = remote
            .call::<Option<RemoteRepo>>(&r.host, method::GIT_STATUS, json!({"root": r.root}))
            .await
            .ok()
            .flatten()
            .map(Arc::new);
        if let Ok(mut cache) = self.statuses.lock() {
            cache.insert(r.clone(), (Instant::now(), fresh.clone()));
        }
        fresh
    }

    /// `fs_watch` for a folder on a host (D-1600): the first subscriber
    /// asks the helper to watch it; the rest share that watch.
    pub async fn watch(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
        sub: String,
        sink: ChangeSink,
    ) -> Result<(), IpcError> {
        let first = {
            let mut w = self.watches.lock().map_err(lock_err)?;
            let entry = w.entry(r.clone()).or_default();
            entry.sinks.insert(sub.clone(), sink);
            entry.host_watch.is_none() && entry.sinks.len() == 1
        };
        if !first {
            return Ok(());
        }
        match remote
            .call::<FsWatching>(&r.host, method::FS_WATCH, json!({"root": r.root}))
            .await
        {
            Ok(w) => {
                if let Ok(mut all) = self.watches.lock() {
                    if let Some(entry) = all.get_mut(r) {
                        entry.host_watch = Some(w.id);
                    }
                }
                Ok(())
            }
            Err(e) => {
                if let Ok(mut all) = self.watches.lock() {
                    if let Some(entry) = all.get_mut(r) {
                        entry.sinks.remove(&sub);
                        if entry.sinks.is_empty() {
                            all.remove(r);
                        }
                    }
                }
                Err(e)
            }
        }
    }

    /// `fs_unwatch`: the helper stops watching with the folder's last
    /// subscriber. False when `sub` isn't a remote subscription.
    pub fn unwatch(&self, remote: &Arc<RemoteService>, sub: &str) -> bool {
        let stop = {
            let Ok(mut all) = self.watches.lock() else {
                return false;
            };
            let Some(r) = all
                .iter()
                .find(|(_, w)| w.sinks.contains_key(sub))
                .map(|(r, _)| r.clone())
            else {
                return false;
            };
            let empty = all.get_mut(&r).is_some_and(|w| {
                w.sinks.remove(sub);
                w.sinks.is_empty()
            });
            if !empty {
                return true;
            }
            all.remove(&r)
                .and_then(|w| w.host_watch.map(|id| (r.host, id)))
        };
        if let Some((host, id)) = stop {
            let remote = remote.clone();
            // `fs_unwatch` is a sync command on the main thread, outside any
            // Tokio context: Tauri's runtime takes the call from there.
            tauri::async_runtime::spawn(async move {
                let _ = remote
                    .call::<serde_json::Value>(&host, method::FS_UNWATCH, json!({"id": id}))
                    .await;
            });
        }
        true
    }

    /// An `fs.changed` from a host: the folder's status is read again and
    /// its subscribers get the batch (a git change touches every folder in
    /// the repository, so the host's statuses all go).
    pub fn changed(&self, host: &HostId, id: &str, change: &FsChange) {
        let mut all = match self.watches.lock() {
            Ok(a) => a,
            Err(_) => return,
        };
        let Some((r, w)) = all
            .iter_mut()
            .find(|(r, w)| &r.host == host && w.host_watch.as_deref() == Some(id))
        else {
            return;
        };
        if change.git || change.overflow {
            self.invalidate(host);
        } else if let Ok(mut cache) = self.statuses.lock() {
            cache.remove(r);
        }
        w.sinks.retain(|_, sink| sink(change));
    }

    /// After a reconnect (the helper's watches ended with the old
    /// connection): watch each folder again, and tell its subscribers to
    /// read everything, since what changed meanwhile isn't known.
    pub async fn rewatch(&self, host: &HostId, client: &Arc<tandem_host::client::HostClient>) {
        let roots: Vec<RemoteRoot> = match self.watches.lock() {
            Ok(mut all) => all
                .iter_mut()
                .filter(|(r, _)| &r.host == host)
                .map(|(r, w)| {
                    w.host_watch = None;
                    r.clone()
                })
                .collect(),
            Err(_) => return,
        };
        self.invalidate(host);
        for r in roots {
            let id = client
                .call::<_, FsWatching>(method::FS_WATCH, json!({"root": r.root}))
                .await
                .ok()
                .map(|w| w.id);
            if let Ok(mut all) = self.watches.lock() {
                if let Some(w) = all.get_mut(&r) {
                    w.host_watch = id;
                    let all_again = FsChange {
                        overflow: true,
                        ..FsChange::default()
                    };
                    w.sinks.retain(|_, sink| sink(&all_again));
                }
            }
        }
    }

    /// Remote folders being watched (tests).
    pub fn watched(&self) -> Vec<(RemoteRoot, usize, bool)> {
        self.watches
            .lock()
            .map(|all| {
                all.iter()
                    .map(|(r, w)| (r.clone(), w.sinks.len(), w.host_watch.is_some()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every cached status on this host (a git action changes them all).
    pub fn invalidate(&self, host: &HostId) {
        if let Ok(mut cache) = self.statuses.lock() {
            cache.retain(|k, _| &k.host != host);
        }
    }

    pub async fn list(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
        dir: String,
        show_ignored: bool,
    ) -> Result<Vec<FsEntry>, IpcError> {
        let entries: Vec<Entry> = remote
            .call(
                &r.host,
                method::FS_LIST,
                json!({"root": r.root, "path": dir, "show_ignored": show_ignored}),
            )
            .await?;
        let badges = match self.status(remote, r).await {
            Some(repo) => badges_for(&repo.folder(&r.root), &repo.status),
            None => HashMap::new(),
        };
        Ok(entries
            .into_iter()
            .map(|e| FsEntry {
                git_status: badges.get(&e.path).copied(),
                name: e.name,
                path: e.path,
                kind: kind_in(e.kind),
                symlink: e.symlink,
                ignored: e.ignored,
            })
            .collect())
    }

    pub async fn read(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
        path: String,
    ) -> Result<FileContent, IpcError> {
        let f: FileData = remote
            .call(
                &r.host,
                method::FS_READ,
                json!({"root": r.root, "path": path}),
            )
            .await?;
        let reason = if f.large {
            Some(ReadOnlyReason::Large)
        } else if !f.writable {
            Some(ReadOnlyReason::Permission)
        } else {
            None
        };
        Ok(FileContent {
            path: f.path,
            kind: match f.kind {
                ReadKind::Text => FileKind::Text,
                ReadKind::Binary => FileKind::Binary,
                ReadKind::Image => FileKind::Image,
            },
            text: f.text,
            size: f.size,
            readonly: reason.is_some(),
            readonly_reason: reason,
            truncated: f.truncated,
            hash: f.hash,
            mtime: f.mtime,
            data_url: f.data_url,
            hex: f.hex,
        })
    }

    pub async fn write(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
        path: String,
        text: String,
        expected: Option<String>,
    ) -> Result<Saved, IpcError> {
        let w: FsWritten = remote
            .call(
                &r.host,
                method::FS_WRITE,
                json!({"root": r.root, "path": path, "text": text, "expected_hash": expected}),
            )
            .await?;
        self.invalidate(&r.host);
        Ok(Saved {
            hash: w.hash,
            mtime: w.mtime,
        })
    }

    pub async fn compare(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
        path: String,
        text: String,
    ) -> Result<FileDiff, IpcError> {
        let disk = self.read(remote, r, path).await?;
        let Some(on_host) = disk.text else {
            return Err(IpcError::new(
                ErrorCode::Invalid,
                "Only text files can be compared",
            ));
        };
        Ok(tandem_git::diff_texts(
            &disk.path,
            on_host.as_bytes(),
            text.as_bytes(),
        )?)
    }

    pub async fn create(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
        path: String,
        kind: FsKind,
    ) -> Result<(), IpcError> {
        remote
            .call::<serde_json::Value>(
                &r.host,
                method::FS_CREATE,
                json!({"root": r.root, "path": path, "kind": kind_out(kind)}),
            )
            .await?;
        self.invalidate(&r.host);
        Ok(())
    }

    pub async fn rename(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
        from: String,
        to: String,
    ) -> Result<(), IpcError> {
        remote
            .call::<serde_json::Value>(
                &r.host,
                method::FS_RENAME,
                json!({"root": r.root, "from": from, "to": to}),
            )
            .await?;
        self.invalidate(&r.host);
        Ok(())
    }

    pub async fn delete(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
        path: String,
    ) -> Result<(), IpcError> {
        remote
            .call::<serde_json::Value>(
                &r.host,
                method::FS_DELETE,
                json!({"root": r.root, "path": path}),
            )
            .await?;
        self.invalidate(&r.host);
        Ok(())
    }

    /// Uploads an attachment for a remote thread (F-RM-01 "Conversation
    /// attachments are stored under the remote workspace"): a new file
    /// under `<root>/.tandem/attachments/<thread>/`. Returns its path on
    /// the host.
    pub async fn upload(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
        thread: &str,
        name: &str,
        bytes: &[u8],
    ) -> Result<String, IpcError> {
        use base64::Engine;
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let safe = safe.trim_start_matches('.');
        let rel = format!(
            ".tandem/attachments/{thread}/{}-{}",
            ulid::Ulid::new().to_string()[..10].to_lowercase(),
            if safe.is_empty() { "file" } else { safe }
        );
        remote
            .call::<FsWritten>(
                &r.host,
                method::FS_WRITE,
                json!({"root": r.root, "path": rel, "data_b64": base64::engine::general_purpose::STANDARD.encode(bytes)}),
            )
            .await?;
        Ok(format!("{}/{rel}", r.root.trim_end_matches('/')))
    }

    // ── git ──

    pub async fn git_status(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
    ) -> Result<Option<(String, String, GitStatus)>, IpcError> {
        self.invalidate(&r.host);
        Ok(self
            .status(remote, r)
            .await
            .map(|repo| (repo.top.clone(), repo.folder(&r.root), repo.status.clone())))
    }

    pub async fn git_diff(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
        path: &str,
        staged: bool,
    ) -> Result<FileDiff, IpcError> {
        remote
            .call(
                &r.host,
                method::GIT_DIFF,
                json!({"root": r.root, "path": path, "staged": staged}),
            )
            .await
    }

    pub async fn git_op(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
        op: &str,
        paths: &[String],
        message: Option<&str>,
    ) -> Result<Option<String>, IpcError> {
        let out = remote
            .call::<Option<String>>(
                &r.host,
                method::GIT_OP,
                json!({"root": r.root, "op": op, "paths": paths, "message": message}),
            )
            .await;
        self.invalidate(&r.host);
        out
    }
}

impl RemoteFiles {
    /// Push, publish, pull, switch and new branch on the host (D-1601), by
    /// the host's own git with its own credentials: nothing of this
    /// computer's reaches it. The network ones wait out the host's 120 s.
    pub async fn git_sync(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
        op: &str,
        branch: Option<&str>,
    ) -> Result<(), IpcError> {
        let client = remote.client(&r.host).await?;
        let wait = tandem_host::gitops::NETWORK_TIMEOUT + std::time::Duration::from_secs(15);
        let out = client
            .call_within::<_, Option<String>>(
                method::GIT_OP,
                json!({"root": r.root, "op": op, "paths": [], "branch": branch}),
                wait,
            )
            .await
            .map(|_| ())
            .map_err(crate::services::remote::rpc_error);
        self.invalidate(&r.host);
        out
    }

    /// The branch switcher's rows from the host (`git.branches`).
    pub async fn git_branches(
        &self,
        remote: &Arc<RemoteService>,
        r: &RemoteRoot,
    ) -> Result<Vec<tandem_git::types::Branch>, IpcError> {
        remote
            .call(&r.host, method::GIT_BRANCHES, json!({"root": r.root}))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `fs_unwatch` runs on the main thread with no Tokio runtime: the
    /// last subscriber leaving a host folder mustn't panic there (it did,
    /// and took the app down when a remote Files tab closed).
    #[test]
    fn f_rm_01_unwatching_a_host_folder_works_outside_tokio() {
        let dir = tempfile::tempdir().expect("tmp");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = tandem_store::Store::open(&data).expect("store");
        let bus = tandem_core::bus::EventBus::new();
        let status = crate::services::status::StatusService::new(
            bus.clone(),
            Arc::new(tandem_core::clock::SystemClock),
        );
        let svc = RemoteService::new(
            store,
            bus,
            &data,
            dir.path().join("bin"),
            tandem_pty::PtyManager::new(),
            status,
        );
        let r = RemoteRoot {
            host: HostId::from_db("hst_x".into()),
            root: "/home/me/project".into(),
        };
        if let Ok(mut all) = svc.files.watches.lock() {
            let mut w = Watched {
                host_watch: Some("w1".into()),
                ..Watched::default()
            };
            w.sinks
                .insert("fsw_1".into(), Arc::new(|_: &FsChange| true));
            all.insert(r, w);
        }
        let svc2 = svc.clone();
        std::thread::spawn(move || {
            assert!(svc2.files.unwatch(&svc2, "fsw_1"));
            assert!(!svc2.files.unwatch(&svc2, "fsw_1"));
        })
        .join()
        .expect("no panic outside Tokio");
        assert!(svc.files.watched().is_empty());
    }
}
