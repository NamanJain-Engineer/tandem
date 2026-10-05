//! Agent sessions in panes (F-CD-03): a `pane` row while the agent runs,
//! turned into a `saved` row when it hands the pane back to the shell, so the
//! workspace lists it under "Saved sessions (n)".

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::{PaneId, SessionId, WorkspaceId};
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SavedSession {
    pub id: SessionId,
    pub workspace_id: WorkspaceId,
    pub engine_id: String,
    pub title: Option<String>,
    /// For `--resume`; reported by the engine's hooks from M4 on.
    pub engine_session_id: Option<String>,
    pub started_at: i64,
    pub ended_at: i64,
}

fn saved_from_row(r: &Row<'_>) -> rusqlite::Result<SavedSession> {
    Ok(SavedSession {
        id: SessionId::from_db(r.get(0)?),
        workspace_id: WorkspaceId::from_db(r.get(1)?),
        engine_id: r.get(2)?,
        title: r.get(3)?,
        engine_session_id: r.get(4)?,
        started_at: r.get(5)?,
        ended_at: r.get(6)?,
    })
}

/// Records an agent starting in a pane.
pub async fn start_in_pane(
    store: &Store,
    pane: &PaneId,
    workspace: &WorkspaceId,
    engine_id: &str,
) -> Result<SessionId, StoreError> {
    let id = SessionId::new();
    let (sid, pane, ws, engine) = (
        id.as_str().to_string(),
        pane.as_str().to_string(),
        workspace.as_str().to_string(),
        engine_id.to_string(),
    );
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO sessions (id, kind, pane_id, workspace_id, engine_id, started_at) VALUES (?1, 'pane', ?2, ?3, ?4, ?5)",
                params![sid, pane, ws, engine, now_ms()],
            )?;
            Ok(())
        })
        .await?;
    Ok(id)
}

/// The agent in `pane` exited and the shell took over: its running session
/// becomes a Saved session. Returns it, or `None` when none was running.
pub async fn save_on_handback(
    store: &Store,
    pane: &PaneId,
    title: Option<String>,
) -> Result<Option<SavedSession>, StoreError> {
    let pane = pane.as_str().to_string();
    store
        .write(move |t| {
            let id: Option<String> = t
                .query_row(
                    "SELECT id FROM sessions WHERE pane_id = ?1 AND kind = 'pane' AND ended_at IS NULL ORDER BY started_at DESC LIMIT 1",
                    [&pane],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(id) = id else { return Ok(None) };
            t.execute(
                "UPDATE sessions SET kind = 'saved', ended_at = ?2, title = COALESCE(title, ?3) WHERE id = ?1",
                params![id, now_ms(), title],
            )?;
            Ok(t.query_row(
                "SELECT id, workspace_id, engine_id, title, engine_session_id, started_at, ended_at FROM sessions WHERE id = ?1",
                [&id],
                saved_from_row,
            )
            .optional()?)
        })
        .await
}

/// A workspace's Saved sessions, newest first.
pub async fn saved_list(
    store: &Store,
    workspace: &WorkspaceId,
) -> Result<Vec<SavedSession>, StoreError> {
    let ws = workspace.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, workspace_id, engine_id, title, engine_session_id, started_at, ended_at FROM sessions
                 WHERE workspace_id = ?1 AND kind = 'saved' AND ended_at IS NOT NULL ORDER BY ended_at DESC",
            )?;
            let rows = stmt.query_map([ws], saved_from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{panes, tabs, workspaces};

    #[tokio::test]
    async fn f_cd_03_3_a_handback_becomes_a_saved_session() {
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
        let pane = panes::create_with(&store, &tab.id, "agent", None, Some("claude".into()))
            .await
            .expect("pane");
        assert_eq!(pane.engine_id.as_deref(), Some("claude"));
        assert!(
            save_on_handback(&store, &pane.id, None)
                .await
                .expect("none")
                .is_none(),
            "nothing running yet"
        );
        start_in_pane(&store, &pane.id, &ws.id, "claude")
            .await
            .expect("start");
        assert!(
            saved_list(&store, &ws.id).await.expect("list").is_empty(),
            "running isn't saved"
        );
        let saved = save_on_handback(&store, &pane.id, Some("Claude Code · 1".into()))
            .await
            .expect("save")
            .expect("a session");
        assert_eq!(saved.engine_id, "claude");
        assert_eq!(saved.title.as_deref(), Some("Claude Code · 1"));
        assert_eq!(saved_list(&store, &ws.id).await.expect("list"), vec![saved]);
        assert!(
            save_on_handback(&store, &pane.id, None)
                .await
                .expect("again")
                .is_none(),
            "saved once"
        );
    }
}
