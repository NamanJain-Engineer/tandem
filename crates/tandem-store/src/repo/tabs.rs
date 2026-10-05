//! `tabs` rows (F-CD-01): every workspace has at least one tab.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::{TabId, WorkspaceId};
use ts_rs::TS;

use crate::{now_ms, sort_key, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Tab {
    pub id: TabId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub sort_key: String,
    pub created_at: i64,
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<Tab> {
    Ok(Tab {
        id: TabId::from_db(r.get(0)?),
        workspace_id: WorkspaceId::from_db(r.get(1)?),
        name: r.get(2)?,
        sort_key: r.get(3)?,
        created_at: r.get(4)?,
    })
}

pub async fn list(store: &Store, workspace: &WorkspaceId) -> Result<Vec<Tab>, StoreError> {
    let ws = workspace.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, workspace_id, name, sort_key, created_at FROM tabs WHERE workspace_id = ?1 ORDER BY sort_key, created_at",
            )?;
            let rows = stmt.query_map([ws], from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

/// Adds a tab at the end; the default name is "Tab {n}".
pub async fn create(
    store: &Store,
    workspace: &WorkspaceId,
    name: Option<String>,
) -> Result<Tab, StoreError> {
    let ws = workspace.as_str().to_string();
    store
        .write(move |t| {
            let exists: Option<i64> =
                t.query_row("SELECT 1 FROM workspaces WHERE id = ?1", [&ws], |r| r.get(0)).optional()?;
            if exists.is_none() {
                return Err(StoreError::NotFound("No workspace with that id".into()));
            }
            let (count, last): (u32, Option<String>) = t.query_row(
                "SELECT COUNT(*), MAX(sort_key) FROM tabs WHERE workspace_id = ?1",
                [&ws],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let tab = Tab {
                id: TabId::new(),
                workspace_id: WorkspaceId::from_db(ws.clone()),
                name: name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("Tab {}", count + 1)),
                sort_key: sort_key::between(last.as_deref(), None),
                created_at: now_ms(),
            };
            t.execute(
                "INSERT INTO tabs (id, workspace_id, name, sort_key, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![tab.id.as_str(), ws, tab.name, tab.sort_key, tab.created_at],
            )?;
            Ok(tab)
        })
        .await
}

pub async fn get(store: &Store, id: &TabId) -> Result<Tab, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            c.query_row(
                "SELECT id, workspace_id, name, sort_key, created_at FROM tabs WHERE id = ?1",
                [&id],
                from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("No tab with that id".into()))
        })
        .await
}

pub async fn rename(store: &Store, id: &TabId, name: String) -> Result<(), StoreError> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(StoreError::Invalid("Names are 1–80 characters".into()));
    }
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            match t.execute("UPDATE tabs SET name = ?2 WHERE id = ?1", params![id, name])? {
                0 => Err(StoreError::NotFound("No tab with that id".into())),
                _ => Ok(()),
            }
        })
        .await
}

pub async fn close(store: &Store, id: &TabId) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(
            move |t| match t.execute("DELETE FROM tabs WHERE id = ?1", [id])? {
                0 => Err(StoreError::NotFound("No tab with that id".into())),
                _ => Ok(()),
            },
        )
        .await
}

/// Persists a drag reorder: `ids` is the full new order for the workspace.
pub async fn reorder(
    store: &Store,
    workspace: &WorkspaceId,
    ids: Vec<TabId>,
) -> Result<(), StoreError> {
    let ws = workspace.as_str().to_string();
    store
        .write(move |t| {
            let keys = sort_key::sequence(ids.len());
            for (id, key) in ids.iter().zip(keys) {
                let n = t.execute(
                    "UPDATE tabs SET sort_key = ?3 WHERE id = ?1 AND workspace_id = ?2",
                    params![id.as_str(), ws, key],
                )?;
                if n == 0 {
                    return Err(StoreError::NotFound(
                        "No tab with that id in this workspace".into(),
                    ));
                }
            }
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::workspaces::{self, NewWorkspace};

    #[tokio::test]
    async fn f_cd_01_2_tab_order_persists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let (ws, _) = workspaces::add(
            &store,
            NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: "/x".into(),
                name: "x".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("ws");
        let a = create(&store, &ws.id, None).await.expect("a");
        let b = create(&store, &ws.id, None).await.expect("b");
        let c = create(&store, &ws.id, Some("Build".into()))
            .await
            .expect("c");
        assert_eq!(
            (a.name.as_str(), b.name.as_str(), c.name.as_str()),
            ("Tab 1", "Tab 2", "Build")
        );
        reorder(
            &store,
            &ws.id,
            vec![c.id.clone(), a.id.clone(), b.id.clone()],
        )
        .await
        .expect("reorder");
        drop(store);
        let store = Store::open(&data).expect("reopen");
        let names: Vec<String> = list(&store, &ws.id)
            .await
            .expect("list")
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, vec!["Build", "Tab 1", "Tab 2"]);
        close(&store, &a.id).await.expect("close");
        assert_eq!(list(&store, &ws.id).await.expect("list").len(), 2);
    }
}
