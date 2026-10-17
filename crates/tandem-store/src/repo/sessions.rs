//! Agent sessions in panes (F-CD-03): a `pane` row while the agent runs,
//! turned into a `saved` row when it hands the pane back to the shell, so the
//! workspace lists it under "Saved sessions (n)".

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::{AccountId, PaneId, SessionId, WorkspaceId};
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

/// What an agent pane was launched with, kept so Resume uses the same
/// configuration (F-SH-07): never the current defaults.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchConfig {
    /// The user's extra engine args at launch time.
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// The exact provider home (one-off homes too); `None` = the engine's own.
    #[serde(default)]
    pub home: Option<String>,
    /// A session from the engine's own home outside Vibezeo (the session
    /// library, F-TH-17): it resumes with no provider home and no status
    /// hooks installed, as the user runs it in a terminal.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub own_home: bool,
}

/// The newest session that ran in a pane (restore).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSession {
    pub id: SessionId,
    pub engine_id: String,
    pub account_id: Option<AccountId>,
    pub provider_home: Option<String>,
    pub engine_session_id: Option<String>,
    /// The saved launch configuration; `Err` when the row has one that no
    /// longer parses.
    pub launch_config: Option<Result<LaunchConfig, String>>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
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
/// `account` and `home` are the provider home it runs in (resume uses the
/// same one, F-AC-01); `config` is what Resume relaunches with (F-SH-07).
pub async fn start_in_pane(
    store: &Store,
    pane: &PaneId,
    workspace: &WorkspaceId,
    engine_id: &str,
    account: Option<&AccountId>,
    home: Option<String>,
) -> Result<SessionId, StoreError> {
    let config = LaunchConfig {
        extra_args: Vec::new(),
        home: home.clone(),
        own_home: false,
    };
    start_with_config(store, pane, workspace, engine_id, account, &config).await
}

/// [`start_in_pane`] with the full launch configuration.
pub async fn start_with_config(
    store: &Store,
    pane: &PaneId,
    workspace: &WorkspaceId,
    engine_id: &str,
    account: Option<&AccountId>,
    config: &LaunchConfig,
) -> Result<SessionId, StoreError> {
    let id = SessionId::new();
    let json = serde_json::to_string(config).map_err(|e| StoreError::Invalid(e.to_string()))?;
    let (sid, pane, ws, engine, account, home) = (
        id.as_str().to_string(),
        pane.as_str().to_string(),
        workspace.as_str().to_string(),
        engine_id.to_string(),
        account.map(|a| a.as_str().to_string()),
        config.home.clone(),
    );
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO sessions (id, kind, pane_id, workspace_id, engine_id, account_id, provider_home, launch_config, started_at) VALUES (?1, 'pane', ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![sid, pane, ws, engine, account, home, json, now_ms()],
            )?;
            Ok(())
        })
        .await?;
    Ok(id)
}

/// The engine's own session id, from its hooks: what `--resume` takes.
/// Recorded on the pane's running session as soon as it's reported, so a
/// crash keeps it.
pub async fn set_engine_session(
    store: &Store,
    pane: &PaneId,
    engine_session_id: &str,
) -> Result<(), StoreError> {
    let (pane, esid) = (pane.as_str().to_string(), engine_session_id.to_string());
    store
        .write(move |t| {
            t.execute(
                "UPDATE sessions SET engine_session_id = ?2 WHERE id = (
                   SELECT id FROM sessions WHERE pane_id = ?1 AND kind = 'pane' AND ended_at IS NULL
                   ORDER BY started_at DESC, rowid DESC LIMIT 1)
                 AND (engine_session_id IS NULL OR engine_session_id <> ?2)",
                params![pane, esid],
            )?;
            Ok(())
        })
        .await
}

/// The newest session that ran in `pane`, running or saved.
pub async fn latest_for_pane(
    store: &Store,
    pane: &PaneId,
) -> Result<Option<PaneSession>, StoreError> {
    let pane = pane.as_str().to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT id, engine_id, account_id, provider_home, engine_session_id, launch_config, started_at, ended_at
                 FROM sessions WHERE pane_id = ?1 AND kind IN ('pane', 'saved')
                 ORDER BY started_at DESC, rowid DESC LIMIT 1",
                [pane],
                |r| {
                    let config: Option<String> = r.get(5)?;
                    Ok(PaneSession {
                        id: SessionId::from_db(r.get(0)?),
                        engine_id: r.get(1)?,
                        account_id: r.get::<_, Option<String>>(2)?.map(AccountId::from_db),
                        provider_home: r.get(3)?,
                        engine_session_id: r.get(4)?,
                        launch_config: config.map(|j| {
                            serde_json::from_str::<LaunchConfig>(&j).map_err(|e| e.to_string())
                        }),
                        started_at: r.get(6)?,
                        ended_at: r.get(7)?,
                    })
                },
            )
            .optional()?)
        })
        .await
}

/// A session whose process never started.
pub async fn discard(store: &Store, id: &SessionId) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
            Ok(())
        })
        .await
}

/// A resumed session runs in its pane again.
pub async fn reopen(store: &Store, id: &SessionId) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE sessions SET kind = 'pane', ended_at = NULL WHERE id = ?1",
                [id],
            )?;
            Ok(())
        })
        .await
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
        start_in_pane(&store, &pane.id, &ws.id, "claude", None, None)
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
