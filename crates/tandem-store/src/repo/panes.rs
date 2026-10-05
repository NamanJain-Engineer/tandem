//! `panes` rows (F-CD-02, F-CD-03). In M2 a pane is a plain terminal; agent,
//! browser and simulator panes arrive with their milestones.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::{PaneId, TabId, WorkspaceId};
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaneRecord {
    pub id: PaneId,
    pub tab_id: TabId,
    pub workspace_id: WorkspaceId,
    /// `terminal` | `agent` | `browser` | `simulator`
    pub kind: String,
    pub cwd: Option<String>,
    pub engine_id: Option<String>,
    pub human_name: Option<String>,
    pub title: Option<String>,
    pub created_at: i64,
}

const COLUMNS: &str =
    "id, tab_id, workspace_id, kind, cwd, engine_id, human_name, title, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<PaneRecord> {
    Ok(PaneRecord {
        id: PaneId::from_db(r.get(0)?),
        tab_id: TabId::from_db(r.get(1)?),
        workspace_id: WorkspaceId::from_db(r.get(2)?),
        kind: r.get(3)?,
        cwd: r.get(4)?,
        engine_id: r.get(5)?,
        human_name: r.get(6)?,
        title: r.get(7)?,
        created_at: r.get(8)?,
    })
}

pub async fn create(
    store: &Store,
    tab: &TabId,
    kind: &str,
    cwd: Option<String>,
) -> Result<PaneRecord, StoreError> {
    if !matches!(kind, "terminal" | "agent" | "browser" | "simulator") {
        return Err(StoreError::Invalid("Unknown pane kind".into()));
    }
    let tab = tab.as_str().to_string();
    let kind = kind.to_string();
    store
        .write(move |t| {
            let ws: Option<String> =
                t.query_row("SELECT workspace_id FROM tabs WHERE id = ?1", [&tab], |r| r.get(0)).optional()?;
            let ws = ws.ok_or_else(|| StoreError::NotFound("No tab with that id".into()))?;
            let pane = PaneRecord {
                id: PaneId::new(),
                tab_id: TabId::from_db(tab.clone()),
                workspace_id: WorkspaceId::from_db(ws),
                kind,
                cwd,
                engine_id: None,
                human_name: None,
                title: None,
                created_at: now_ms(),
            };
            t.execute(
                "INSERT INTO panes (id, tab_id, workspace_id, kind, cwd, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![pane.id.as_str(), tab, pane.workspace_id.as_str(), pane.kind, pane.cwd, pane.created_at],
            )?;
            Ok(pane)
        })
        .await
}

pub async fn get(store: &Store, id: &PaneId) -> Result<PaneRecord, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            c.query_row(
                &format!("SELECT {COLUMNS} FROM panes WHERE id = ?1"),
                [&id],
                from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("No pane with that id".into()))
        })
        .await
}

/// Open panes in a tab, oldest first.
pub async fn list_open(store: &Store, tab: &TabId) -> Result<Vec<PaneRecord>, StoreError> {
    let tab = tab.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM panes WHERE tab_id = ?1 AND closed_at IS NULL ORDER BY created_at, id"
            ))?;
            let rows = stmt.query_map([tab], from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

pub async fn close(store: &Store, id: &PaneId) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE panes SET closed_at = ?2 WHERE id = ?1 AND closed_at IS NULL",
                params![id, now_ms()],
            )?;
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{tabs, workspaces};

    #[tokio::test]
    async fn panes_belong_to_a_tab_and_close() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let (ws, _) = workspaces::add(
            &store,
            workspaces::NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: "/p".into(),
                name: "p".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("ws");
        let tab = tabs::create(&store, &ws.id, None).await.expect("tab");
        let pane = create(&store, &tab.id, "terminal", Some("/p".into()))
            .await
            .expect("pane");
        assert_eq!(pane.workspace_id, ws.id);
        assert_eq!(list_open(&store, &tab.id).await.expect("list").len(), 1);
        assert_eq!(
            get(&store, &pane.id).await.expect("get").cwd.as_deref(),
            Some("/p")
        );
        close(&store, &pane.id).await.expect("close");
        assert!(list_open(&store, &tab.id).await.expect("list").is_empty());
        assert!(matches!(
            create(&store, &tab.id, "toaster", None).await,
            Err(StoreError::Invalid(_))
        ));
    }
}
