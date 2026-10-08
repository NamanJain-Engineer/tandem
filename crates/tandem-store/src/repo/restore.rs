//! Session restore (F-SH-07): what a launch does to the last run's state
//! before the UI reads it.
//!
//! - Agent panes come back **paused**: nothing an agent runs starts again
//!   without the user's consent.
//! - Threads a crash left working or waiting are settled to `interrupted`;
//!   nothing is ever marked done that didn't finish.

use rusqlite::{params, Transaction};
use tandem_core::ids::{PaneId, WorkspaceId};

use crate::{Store, StoreError};

/// What [`settle`] changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Settled {
    pub paused_panes: usize,
    pub interrupted_threads: usize,
}

fn settle_in(t: &Transaction) -> Result<Settled, StoreError> {
    let paused_panes = t.execute(
        "UPDATE panes SET restore_state = 'paused'
         WHERE kind = 'agent' AND closed_at IS NULL AND restore_state <> 'paused'",
        [],
    )?;
    let interrupted_threads = t.execute(
        "UPDATE threads SET state = 'interrupted' WHERE state IN ('working', 'waiting')",
        [],
    )?;
    // Their turns too: a turn never stays "running" across a launch (F-TH-03).
    t.execute(
        "UPDATE thread_turns SET state = 'interrupted', ended_at = ?1 WHERE state = 'running'",
        [crate::now_ms()],
    )?;
    Ok(Settled {
        paused_panes,
        interrupted_threads,
    })
}

/// Runs once at launch, before the UI lists panes.
pub fn settle_blocking(store: &Store) -> Result<Settled, StoreError> {
    store.write_blocking(settle_in)
}

pub async fn settle(store: &Store) -> Result<Settled, StoreError> {
    store.write(settle_in).await
}

/// Sets an open pane's restore state (`none`, `live`, `paused`, `cancelled`).
pub async fn set_state(store: &Store, pane: &PaneId, state: &str) -> Result<(), StoreError> {
    if !matches!(state, "none" | "live" | "paused" | "cancelled") {
        return Err(StoreError::Invalid("Unknown restore state".into()));
    }
    let (pane, state) = (pane.as_str().to_string(), state.to_string());
    store
        .write(move |t| {
            let n = t.execute(
                "UPDATE panes SET restore_state = ?2 WHERE id = ?1 AND closed_at IS NULL",
                params![pane, state],
            )?;
            if n == 0 {
                return Err(StoreError::NotFound("No open pane with that id".into()));
            }
            Ok(())
        })
        .await
}

/// An agent pane waiting on its restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingPane {
    pub pane_id: PaneId,
    pub workspace_id: WorkspaceId,
    pub engine_id: Option<String>,
    /// `paused` or `cancelled`.
    pub state: String,
    /// When its newest session started (auto-resume goes newest first).
    pub last_started_at: Option<i64>,
}

/// Open agent panes that are paused or cancelled, newest session first.
pub async fn waiting(store: &Store) -> Result<Vec<WaitingPane>, StoreError> {
    store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT p.id, p.workspace_id, p.engine_id, p.restore_state,
                        (SELECT MAX(s.started_at) FROM sessions s WHERE s.pane_id = p.id) AS last
                 FROM panes p
                 WHERE p.kind = 'agent' AND p.closed_at IS NULL AND p.restore_state IN ('paused', 'cancelled')
                 ORDER BY COALESCE(last, p.created_at) DESC, p.created_at DESC",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(WaitingPane {
                        pane_id: PaneId::from_db(r.get(0)?),
                        workspace_id: WorkspaceId::from_db(r.get(1)?),
                        engine_id: r.get(2)?,
                        state: r.get(3)?,
                        last_started_at: r.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::now_ms;
    use crate::repo::{panes, sessions, tabs, workspaces};
    use crate::tests::temp_store;

    async fn tab(store: &Store) -> (WorkspaceId, tandem_core::ids::TabId) {
        let (ws, _) = workspaces::add(
            store,
            workspaces::NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: "/r".into(),
                name: "r".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("ws");
        let tab = tabs::create(store, &ws.id, None).await.expect("tab");
        (ws.id, tab.id)
    }

    #[tokio::test]
    async fn f_sh_07_agents_come_back_paused_and_shells_untouched() {
        let (_dir, store) = temp_store().await;
        let (_, tab) = tab(&store).await;
        let shell = panes::create(&store, &tab, "terminal", None)
            .await
            .expect("shell");
        let agent = panes::create_with(&store, &tab, "agent", None, Some("claude".into()))
            .await
            .expect("agent");
        let closed = panes::create_with(&store, &tab, "agent", None, Some("claude".into()))
            .await
            .expect("closed");
        panes::close(&store, &closed.id).await.expect("close");

        let s = settle(&store).await.expect("settle");
        assert_eq!(s.paused_panes, 1);
        assert_eq!(
            panes::get(&store, &agent.id)
                .await
                .expect("a")
                .restore_state,
            "paused"
        );
        assert_eq!(
            panes::get(&store, &shell.id)
                .await
                .expect("s")
                .restore_state,
            "none"
        );
        assert_eq!(
            panes::get(&store, &closed.id)
                .await
                .expect("c")
                .restore_state,
            "none"
        );
        // A cancelled restore is paused again at the next launch.
        set_state(&store, &agent.id, "cancelled")
            .await
            .expect("cancel");
        assert_eq!(settle(&store).await.expect("again").paused_panes, 1);
        assert!(set_state(&store, &agent.id, "bogus").await.is_err());
    }

    #[tokio::test]
    async fn f_sh_07_6_running_threads_settle_to_interrupted_never_done() {
        let (_dir, store) = temp_store().await;
        store
            .write(|t| {
                for (id, state) in [("th_1", "working"), ("th_2", "waiting"), ("th_3", "done"), ("th_4", "idle")] {
                    t.execute(
                        "INSERT INTO threads (id, kind, cwd, engine_id, access, mode, state, sort_key, created_at, updated_at)
                         VALUES (?1, 'thread', '/tmp', 'claude', 'edits', 'act', ?2, ?1, ?3, ?3)",
                        params![id, state, now_ms()],
                    )?;
                }
                Ok(())
            })
            .await
            .expect("seed");
        assert_eq!(settle(&store).await.expect("settle").interrupted_threads, 2);
        let states: Vec<(String, String)> = store
            .read(|c| {
                let mut st = c.prepare("SELECT id, state FROM threads ORDER BY id")?;
                let rows = st
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .expect("read");
        assert_eq!(
            states,
            vec![
                ("th_1".into(), "interrupted".into()),
                ("th_2".into(), "interrupted".into()),
                ("th_3".into(), "done".into()),
                ("th_4".into(), "idle".into()),
            ]
        );
    }

    #[tokio::test]
    async fn f_sh_07_2_the_session_keeps_its_engine_id_home_and_launch_config() {
        let (_dir, store) = temp_store().await;
        let (ws, tab) = tab(&store).await;
        let agent = panes::create_with(&store, &tab, "agent", None, Some("claude".into()))
            .await
            .expect("agent");
        let config = sessions::LaunchConfig {
            extra_args: vec!["--model".into(), "x".into()],
            home: Some("/h/claude/acc_1".into()),
        };
        sessions::start_with_config(&store, &agent.id, &ws, "claude", None, &config)
            .await
            .expect("start");
        sessions::set_engine_session(&store, &agent.id, "abc-1")
            .await
            .expect("sid");
        let s = sessions::latest_for_pane(&store, &agent.id)
            .await
            .expect("latest")
            .expect("one");
        assert_eq!(s.engine_session_id.as_deref(), Some("abc-1"));
        assert_eq!(s.provider_home.as_deref(), Some("/h/claude/acc_1"));
        assert_eq!(s.launch_config, Some(Ok(config)));
        assert_eq!(s.ended_at, None);
        // A broken config reads as an error, never as defaults.
        let id = s.id.as_str().to_string();
        store
            .write(move |t| {
                t.execute(
                    "UPDATE sessions SET launch_config = '{oops' WHERE id = ?1",
                    [id],
                )?;
                Ok(())
            })
            .await
            .expect("break");
        let s = sessions::latest_for_pane(&store, &agent.id)
            .await
            .expect("l")
            .expect("one");
        assert!(matches!(s.launch_config, Some(Err(_))));
    }

    #[tokio::test]
    async fn f_sh_07_3_waiting_panes_list_newest_session_first() {
        let (_dir, store) = temp_store().await;
        let (ws, tab) = tab(&store).await;
        let mut ids = Vec::new();
        for _ in 0..3 {
            let p = panes::create_with(&store, &tab, "agent", None, Some("claude".into()))
                .await
                .expect("agent");
            sessions::start_in_pane(&store, &p.id, &ws, "claude", None, None)
                .await
                .expect("start");
            tokio::time::sleep(std::time::Duration::from_millis(3)).await;
            ids.push(p.id);
        }
        settle(&store).await.expect("settle");
        let order: Vec<PaneId> = waiting(&store)
            .await
            .expect("waiting")
            .into_iter()
            .map(|w| w.pane_id)
            .collect();
        ids.reverse();
        assert_eq!(order, ids);
    }
}
