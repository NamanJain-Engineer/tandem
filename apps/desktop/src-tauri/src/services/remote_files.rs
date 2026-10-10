//! Files and the Changes view for contexts on SSH hosts (F-RM-01 "Files
//! (read and write), and git"): the same commands as local contexts, served
//! by `tandem-host` (every path held to the root there by `contain()`).
//! The host's file system isn't watched (D-411): the view lists again after
//! each action, on Refresh and when the window gets focus.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use tandem_core::files::{FileKind, FsKind, ReadOnlyReason};
use tandem_core::ids::HostId;
use tandem_core::{ErrorCode, IpcError};
use tandem_git::types::{FileDiff, GitStatus};
use tandem_host::protocol::{method, Entry, EntryKind, FileData, FsWritten, ReadKind};

use crate::services::files::{badges_for, FileContent, FsEntry, Saved, STATUS_TTL};
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

#[derive(Default)]
pub struct RemoteFiles {
    statuses: std::sync::Mutex<Statuses>,
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

/// What a remote Changes view can't do here (D-411): the host's own
/// network sign-in decides pushes and pulls, so they run in its terminal.
pub fn not_remote(action: &str) -> IpcError {
    IpcError::new(
        ErrorCode::Unavailable,
        format!(
            "{action} isn't available for remote folders yet. Run it in a terminal on the host."
        ),
    )
    .with_stage("remote")
}
