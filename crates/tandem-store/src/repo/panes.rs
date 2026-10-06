//! `panes` rows (F-CD-02, F-CD-03). In M2 a pane is a plain terminal; agent,
//! browser and simulator panes arrive with their milestones.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::{AccountId, PaneId, TabId, WorkspaceId};
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
    /// The engine account it launches with; `None` = the engine's default.
    pub account_id: Option<AccountId>,
    pub human_name: Option<String>,
    pub title: Option<String>,
    pub created_at: i64,
}

const COLUMNS: &str =
    "id, tab_id, workspace_id, kind, cwd, engine_id, human_name, title, created_at, account_id";

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
        account_id: r.get::<_, Option<String>>(9)?.map(AccountId::from_db),
    })
}

pub async fn create(
    store: &Store,
    tab: &TabId,
    kind: &str,
    cwd: Option<String>,
) -> Result<PaneRecord, StoreError> {
    create_with(store, tab, kind, cwd, None).await
}

/// A pane that runs `engine_id` (agent panes).
pub async fn create_with(
    store: &Store,
    tab: &TabId,
    kind: &str,
    cwd: Option<String>,
    engine_id: Option<String>,
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
                engine_id,
                account_id: None,
                human_name: None,
                title: None,
                created_at: now_ms(),
            };
            t.execute(
                "INSERT INTO panes (id, tab_id, workspace_id, kind, cwd, engine_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![pane.id.as_str(), tab, pane.workspace_id.as_str(), pane.kind, pane.cwd, pane.engine_id, pane.created_at],
            )?;
            Ok(pane)
        })
        .await
}

/// Pins an agent pane to one engine account (F-AC-01).
pub async fn set_account(
    store: &Store,
    pane: &PaneId,
    account: &AccountId,
) -> Result<(), StoreError> {
    let (pane, account) = (pane.as_str().to_string(), account.as_str().to_string());
    store
        .write(move |t| {
            t.execute(
                "UPDATE panes SET account_id = ?2 WHERE id = ?1",
                params![pane, account],
            )?;
            Ok(())
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

/// A pane that's still open (closed panes never get a process).
pub async fn get_open(store: &Store, id: &PaneId) -> Result<PaneRecord, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            c.query_row(
                &format!("SELECT {COLUMNS} FROM panes WHERE id = ?1 AND closed_at IS NULL"),
                [&id],
                from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("This pane was closed".into()))
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

/// The shell's working directory, from its OSC 7 mark (F-CD-10): restores
/// and link resolution start there.
pub async fn set_cwd(store: &Store, id: &PaneId, cwd: String) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE panes SET cwd = ?2 WHERE id = ?1 AND closed_at IS NULL AND (cwd IS NULL OR cwd <> ?2)",
                params![id, cwd],
            )?;
            Ok(())
        })
        .await
}

/// Moves an open pane into another tab (drag to the tab strip, "Move to new
/// tab"). The process keeps running; only the pane's place changes.
pub async fn move_to_tab(
    store: &Store,
    id: &PaneId,
    tab: &TabId,
) -> Result<PaneRecord, StoreError> {
    let id = id.as_str().to_string();
    let tab = tab.as_str().to_string();
    store
        .write(move |t| {
            let ws: Option<String> =
                t.query_row("SELECT workspace_id FROM tabs WHERE id = ?1", [&tab], |r| r.get(0)).optional()?;
            let ws = ws.ok_or_else(|| StoreError::NotFound("No tab with that id".into()))?;
            let moved = t.execute(
                "UPDATE panes SET tab_id = ?2, workspace_id = ?3 WHERE id = ?1 AND closed_at IS NULL",
                params![id, tab, ws],
            )?;
            if moved == 0 {
                return Err(StoreError::NotFound("No open pane with that id".into()));
            }
            Ok(t.query_row(&format!("SELECT {COLUMNS} FROM panes WHERE id = ?1"), [&id], from_row)?)
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
        assert!(get_open(&store, &pane.id).await.is_ok());
        set_cwd(&store, &pane.id, "/tmp".into()).await.expect("cwd");
        assert_eq!(
            get(&store, &pane.id).await.expect("get").cwd.as_deref(),
            Some("/tmp")
        );
        close(&store, &pane.id).await.expect("close");
        assert!(list_open(&store, &tab.id).await.expect("list").is_empty());
        assert!(matches!(
            get_open(&store, &pane.id).await,
            Err(StoreError::NotFound(_))
        ));
        assert!(matches!(
            create(&store, &tab.id, "toaster", None).await,
            Err(StoreError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn f_cd_02_move_to_new_tab_keeps_the_pane() {
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
        let from = tabs::create(&store, &ws.id, None).await.expect("tab");
        let to = tabs::create(&store, &ws.id, None).await.expect("tab");
        let pane = create(&store, &from.id, "terminal", None)
            .await
            .expect("pane");
        let moved = move_to_tab(&store, &pane.id, &to.id).await.expect("move");
        assert_eq!(moved.id, pane.id);
        assert_eq!(moved.tab_id, to.id);
        assert!(list_open(&store, &from.id).await.expect("list").is_empty());
        assert_eq!(list_open(&store, &to.id).await.expect("list").len(), 1);
        close(&store, &pane.id).await.expect("close");
        assert!(matches!(
            move_to_tab(&store, &pane.id, &from.id).await,
            Err(StoreError::NotFound(_))
        ));
    }
}
