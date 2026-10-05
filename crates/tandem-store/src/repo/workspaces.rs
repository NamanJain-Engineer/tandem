//! `workspaces` rows (F-CD-01). Paths are stored canonical; the same host and
//! root is one workspace. Removing is a soft delete, and adding the folder
//! again restores it with its history.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::WorkspaceId;
use ts_rs::TS;

use crate::{now_ms, sort_key, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Workspace {
    pub id: WorkspaceId,
    /// `local` | `ssh` | `wsl`
    pub kind: String,
    pub host_id: String,
    pub root: String,
    pub wsl_distro: Option<String>,
    pub name: String,
    pub parent_id: Option<WorkspaceId>,
    pub lane_branch: Option<String>,
    pub sort_key: String,
    pub pinned: bool,
    pub collapsed: bool,
    pub sessions_collapsed: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewWorkspace {
    pub kind: String,
    pub host_id: String,
    pub root: String,
    pub name: String,
    pub wsl_distro: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    Created,
    Existing,
    Restored,
}

const COLUMNS: &str = "id, kind, host_id, root, wsl_distro, name, parent_id, lane_branch, sort_key, pinned, collapsed, sessions_collapsed, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: WorkspaceId::from_db(r.get(0)?),
        kind: r.get(1)?,
        host_id: r.get(2)?,
        root: r.get(3)?,
        wsl_distro: r.get(4)?,
        name: r.get(5)?,
        parent_id: r.get::<_, Option<String>>(6)?.map(WorkspaceId::from_db),
        lane_branch: r.get(7)?,
        sort_key: r.get(8)?,
        pinned: r.get(9)?,
        collapsed: r.get(10)?,
        sessions_collapsed: r.get(11)?,
        created_at: r.get(12)?,
    })
}

/// Top-level workspaces that aren't removed, in rail order.
pub async fn list(store: &Store) -> Result<Vec<Workspace>, StoreError> {
    store
        .read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM workspaces WHERE removed_at IS NULL ORDER BY pinned DESC, sort_key, created_at"
            ))?;
            let rows = stmt.query_map([], from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

pub async fn get(store: &Store, id: &WorkspaceId) -> Result<Workspace, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            c.query_row(
                &format!("SELECT {COLUMNS} FROM workspaces WHERE id = ?1"),
                [&id],
                from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("No workspace with that id".into()))
        })
        .await
}

/// Adds a workspace, returning the existing one for the same host and root.
/// `max_active` is the plan limit on top-level workspaces (F-BL-04); it's
/// checked in the same transaction as the insert.
pub async fn add(
    store: &Store,
    new: NewWorkspace,
    max_active: Option<u32>,
) -> Result<(Workspace, Added), StoreError> {
    store
        .write(move |t| {
            let found: Option<(Workspace, Option<i64>)> = t
                .query_row(
                    &format!("SELECT {COLUMNS}, removed_at FROM workspaces WHERE host_id = ?1 AND root = ?2"),
                    params![new.host_id, new.root],
                    |r| Ok((from_row(r)?, r.get(13)?)),
                )
                .optional()?;
            if let Some((ws, None)) = &found {
                return Ok((ws.clone(), Added::Existing));
            }
            let active: u32 = t.query_row(
                "SELECT COUNT(*) FROM workspaces WHERE removed_at IS NULL AND parent_id IS NULL",
                [],
                |r| r.get(0),
            )?;
            if let Some(max) = max_active {
                if active >= max {
                    return Err(StoreError::LimitReached(format!("Free includes {max} workspaces")));
                }
            }
            let last: Option<String> = t
                .query_row(
                    "SELECT MAX(sort_key) FROM workspaces WHERE removed_at IS NULL AND parent_id IS NULL",
                    [],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            let key = sort_key::between(last.as_deref(), None);
            if let Some((ws, Some(_))) = found {
                t.execute(
                    "UPDATE workspaces SET removed_at = NULL, sort_key = ?2 WHERE id = ?1",
                    params![ws.id.as_str(), key],
                )?;
                return Ok((Workspace { sort_key: key, ..ws }, Added::Restored));
            }
            let ws = Workspace {
                id: WorkspaceId::new(),
                kind: new.kind,
                host_id: new.host_id,
                root: new.root,
                wsl_distro: new.wsl_distro,
                name: new.name,
                parent_id: None,
                lane_branch: None,
                sort_key: key,
                pinned: false,
                collapsed: false,
                sessions_collapsed: false,
                created_at: now_ms(),
            };
            t.execute(
                "INSERT INTO workspaces (id, kind, host_id, root, wsl_distro, name, sort_key, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![ws.id.as_str(), ws.kind, ws.host_id, ws.root, ws.wsl_distro, ws.name, ws.sort_key, ws.created_at],
            )?;
            Ok((ws, Added::Created))
        })
        .await
}

pub async fn rename(store: &Store, id: &WorkspaceId, name: String) -> Result<(), StoreError> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(StoreError::Invalid("Names are 1–80 characters".into()));
    }
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            match t.execute(
                "UPDATE workspaces SET name = ?2 WHERE id = ?1",
                params![id, name],
            )? {
                0 => Err(StoreError::NotFound("No workspace with that id".into())),
                _ => Ok(()),
            }
        })
        .await
}

/// Soft remove: nothing on disk is touched, and re-adding restores it.
pub async fn remove(store: &Store, id: &WorkspaceId) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            match t.execute(
                "UPDATE workspaces SET removed_at = ?2 WHERE (id = ?1 OR parent_id = ?1) AND removed_at IS NULL",
                params![id, now_ms()],
            )? {
                0 => Err(StoreError::NotFound("No workspace with that id".into())),
                _ => Ok(()),
            }
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::DataDir;

    pub(crate) fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, store)
    }

    fn local(root: &str) -> NewWorkspace {
        NewWorkspace {
            kind: "local".into(),
            host_id: "local".into(),
            root: root.into(),
            name: root.rsplit('/').next().unwrap_or(root).into(),
            wsl_distro: None,
        }
    }

    #[tokio::test]
    async fn f_cd_01_1_adding_the_same_path_twice_is_one_workspace() {
        let (_d, store) = store();
        let (a, how) = add(&store, local("/a/b"), None).await.expect("add");
        assert_eq!(how, Added::Created);
        let (b, how) = add(&store, local("/a/b"), None).await.expect("add again");
        assert_eq!(how, Added::Existing);
        assert_eq!(a.id, b.id);
        assert_eq!(list(&store).await.expect("list").len(), 1);
    }

    #[tokio::test]
    async fn f_bl_04_free_limit_of_three_workspaces() {
        let (_d, store) = store();
        for root in ["/w1", "/w2", "/w3"] {
            add(&store, local(root), Some(3))
                .await
                .expect("within limit");
        }
        let err = add(&store, local("/w4"), Some(3))
            .await
            .expect_err("4th is refused");
        assert!(matches!(err, StoreError::LimitReached(_)));
        // An existing one can still be focused at the limit.
        add(&store, local("/w2"), Some(3))
            .await
            .expect("existing is fine");
        // Pro (no limit) can add the 4th.
        add(&store, local("/w4"), None).await.expect("unlimited");
        let names: Vec<String> = list(&store)
            .await
            .expect("list")
            .into_iter()
            .map(|w| w.name)
            .collect();
        assert_eq!(
            names,
            vec!["w1", "w2", "w3", "w4"],
            "rail order is insertion order"
        );
    }

    #[tokio::test]
    async fn remove_is_soft_and_re_adding_restores() {
        let (_d, store) = store();
        let (ws, _) = add(&store, local("/proj"), None).await.expect("add");
        rename(&store, &ws.id, "Project".into())
            .await
            .expect("rename");
        remove(&store, &ws.id).await.expect("remove");
        assert!(list(&store).await.expect("list").is_empty());
        let (back, how) = add(&store, local("/proj"), Some(3)).await.expect("re-add");
        assert_eq!(how, Added::Restored);
        assert_eq!(back.id, ws.id);
        assert_eq!(
            back.name, "Project",
            "history (and the custom name) come back"
        );
        assert!(matches!(
            rename(&store, &ws.id, " ".into()).await,
            Err(StoreError::Invalid(_))
        ));
    }
}
