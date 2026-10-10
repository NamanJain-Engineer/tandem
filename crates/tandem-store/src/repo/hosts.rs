//! SSH (and WSL) hosts (ARCHITECTURE §4.2 `hosts`, F-RM-01): where a
//! remote workspace lives, how to reach it, and the host key pinned when
//! the user approved its fingerprint.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::HostId;
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Host {
    pub id: HostId,
    /// `ssh` | `wsl` (`local` is never listed).
    pub kind: String,
    pub label: String,
    /// An alias from `~/.ssh/config`, a host name, or `user@host`.
    pub ssh_target: Option<String>,
    pub ssh_port: Option<u16>,
    pub identity_file: Option<String>,
    pub wsl_distro: Option<String>,
    /// `SHA256:…`, pinned when the user approved it.
    pub host_key_fp: Option<String>,
    pub host_protocol: Option<u32>,
    /// `linux-x86_64`, `linux-aarch64` or `macos-universal`.
    pub arch: Option<String>,
    /// Workspaces on this host (removed ones not counted).
    pub workspaces: u32,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSshHost {
    pub label: String,
    /// An alias, a host name, or `user@host`.
    pub target: String,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
}

const COLUMNS: &str = "h.id, h.kind, h.label, h.ssh_target, h.ssh_port, h.identity_file, h.wsl_distro, h.host_key_fp, h.host_protocol, h.arch, h.created_at,
 (SELECT COUNT(*) FROM workspaces w WHERE w.host_id = h.id AND w.removed_at IS NULL)";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Host> {
    Ok(Host {
        id: HostId::from_db(r.get(0)?),
        kind: r.get(1)?,
        label: r.get(2)?,
        ssh_target: r.get(3)?,
        ssh_port: r
            .get::<_, Option<i64>>(4)?
            .and_then(|p| u16::try_from(p).ok()),
        identity_file: r.get(5)?,
        wsl_distro: r.get(6)?,
        host_key_fp: r.get(7)?,
        host_protocol: r
            .get::<_, Option<i64>>(8)?
            .and_then(|p| u32::try_from(p).ok()),
        arch: r.get(9)?,
        created_at: r.get(10)?,
        workspaces: r.get(11)?,
    })
}

/// SSH hosts, oldest first.
pub async fn list(store: &Store) -> Result<Vec<Host>, StoreError> {
    store
        .read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM hosts h WHERE h.kind = 'ssh' ORDER BY h.created_at, h.id"
            ))?;
            let rows = stmt
                .query_map([], from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

pub async fn get(store: &Store, id: &HostId) -> Result<Host, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            c.query_row(
                &format!("SELECT {COLUMNS} FROM hosts h WHERE h.id = ?1"),
                [&id],
                from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("No host with that id".into()))
        })
        .await
}

/// Adds an SSH host; the same target and port again returns the existing
/// one (with the new label and key file).
pub async fn add_ssh(store: &Store, new: NewSshHost) -> Result<Host, StoreError> {
    let id = HostId::new_remote();
    let existing = store
        .write(move |t| {
            let found: Option<String> = t
                .query_row(
                    "SELECT id FROM hosts WHERE kind = 'ssh' AND ssh_target = ?1
                     AND IFNULL(ssh_port, 0) = IFNULL(?2, 0)",
                    params![new.target, new.port],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(found) = found {
                t.execute(
                    "UPDATE hosts SET label = ?2, identity_file = ?3 WHERE id = ?1",
                    params![found, new.label, new.identity_file],
                )?;
                return Ok(found);
            }
            t.execute(
                "INSERT INTO hosts (id, kind, label, ssh_target, ssh_port, identity_file, created_at)
                 VALUES (?1, 'ssh', ?2, ?3, ?4, ?5, ?6)",
                params![id.as_str(), new.label, new.target, new.port, new.identity_file, now_ms()],
            )?;
            Ok(id.as_str().to_string())
        })
        .await?;
    get(store, &HostId::from_db(existing)).await
}

/// The WSL distro's host row (one per distro), created on first use.
pub async fn ensure_wsl(store: &Store, distro: &str) -> Result<Host, StoreError> {
    let distro = distro.to_string();
    let id = store
        .write(move |t| {
            let found: Option<String> = t
                .query_row(
                    "SELECT id FROM hosts WHERE kind = 'wsl' AND wsl_distro = ?1",
                    [&distro],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(found) = found {
                return Ok(found);
            }
            let id = HostId::new_remote();
            t.execute(
                "INSERT INTO hosts (id, kind, label, wsl_distro, created_at) VALUES (?1, 'wsl', ?2, ?2, ?3)",
                params![id.as_str(), distro, now_ms()],
            )?;
            Ok(id.as_str().to_string())
        })
        .await?;
    get(store, &HostId::from_db(id)).await
}

pub async fn set_key(
    store: &Store,
    id: &HostId,
    fingerprint: Option<String>,
) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE hosts SET host_key_fp = ?2 WHERE id = ?1",
                params![id, fingerprint],
            )?;
            Ok(())
        })
        .await
}

pub async fn set_installed(
    store: &Store,
    id: &HostId,
    protocol: u32,
    arch: &str,
) -> Result<(), StoreError> {
    let (id, arch) = (id.as_str().to_string(), arch.to_string());
    store
        .write(move |t| {
            t.execute(
                "UPDATE hosts SET host_protocol = ?2, arch = ?3 WHERE id = ?1",
                params![id, protocol, arch],
            )?;
            Ok(())
        })
        .await
}

/// Removes a host with no workspaces left (removed ones and their detection
/// rows go with it).
pub async fn remove(store: &Store, id: &HostId) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            let active: u32 = t.query_row(
                "SELECT COUNT(*) FROM workspaces WHERE host_id = ?1 AND removed_at IS NULL",
                [&id],
                |r| r.get(0),
            )?;
            if active > 0 {
                return Err(StoreError::Invalid(
                    "Remove this host's workspaces first".into(),
                ));
            }
            let removed: Vec<String> = {
                let mut stmt = t.prepare("SELECT id FROM workspaces WHERE host_id = ?1")?;
                let rows = stmt
                    .query_map([&id], |r| r.get(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            for ws in removed {
                t.execute("DELETE FROM workspaces WHERE id = ?1", [&ws])?;
            }
            t.execute("DELETE FROM hosts WHERE id = ?1 AND id != 'local'", [&id])?;
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::DataDir;

    async fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("open");
        (dir, store)
    }

    fn fixture() -> NewSshHost {
        NewSshHost {
            label: "Fixture".into(),
            target: "tandem@127.0.0.1".into(),
            port: Some(2222),
            identity_file: Some("/k".into()),
        }
    }

    #[tokio::test]
    async fn f_rm_01_hosts_add_once_pin_and_remove() {
        let (_d, store) = temp_store().await;
        let a = add_ssh(&store, fixture()).await.expect("add");
        assert!(a.id.as_str().starts_with("host_"));
        let again = add_ssh(
            &store,
            NewSshHost {
                label: "Renamed".into(),
                ..fixture()
            },
        )
        .await
        .expect("again");
        assert_eq!(again.id, a.id);
        assert_eq!(again.label, "Renamed");
        let other = add_ssh(
            &store,
            NewSshHost {
                port: None,
                ..fixture()
            },
        )
        .await
        .expect("other");
        assert_ne!(other.id, a.id);
        set_key(&store, &a.id, Some("SHA256:abc".into()))
            .await
            .expect("pin");
        set_installed(&store, &a.id, 1, "linux-x86_64")
            .await
            .expect("installed");
        let got = get(&store, &a.id).await.expect("get");
        assert_eq!(got.host_key_fp.as_deref(), Some("SHA256:abc"));
        assert_eq!(
            (got.host_protocol, got.arch.as_deref()),
            (Some(1), Some("linux-x86_64"))
        );
        assert_eq!(got.ssh_port, Some(2222));
        assert_eq!(list(&store).await.expect("list").len(), 2);
        remove(&store, &a.id).await.expect("remove");
        assert_eq!(list(&store).await.expect("list").len(), 1);
        let wsl = ensure_wsl(&store, "Ubuntu").await.expect("wsl");
        assert_eq!(
            ensure_wsl(&store, "Ubuntu").await.expect("again").id,
            wsl.id
        );
        assert_eq!(
            list(&store).await.expect("list").len(),
            1,
            "WSL hosts aren't SSH hosts"
        );
    }
}
