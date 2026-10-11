//! `command_history` (F-EX-02): every command a shell ran in a pane, as
//! command recall searches it. Commands are redacted before they get here.

use rusqlite::{params, params_from_iter, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Store, StoreError};

/// The most commands kept; the oldest go first.
pub const MAX_ROWS: i64 = 50_000;

/// One run, as recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRun {
    pub pane_id: String,
    pub workspace_id: Option<String>,
    pub cwd: String,
    pub command: String,
    /// The redactor replaced something in `command`.
    pub secret: bool,
    pub exit_code: Option<i32>,
    pub started_at: i64,
    pub duration_ms: Option<i64>,
}

/// `history_search`: one row per distinct command and folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryEntry {
    /// The newest run's id.
    #[ts(type = "number")]
    pub id: i64,
    pub command: String,
    pub cwd: String,
    pub exit_code: Option<i32>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    pub pane_id: String,
    /// The pane's name when it's still open (filled in by the service).
    #[serde(default)]
    pub pane_name: Option<String>,
    pub workspace_id: Option<String>,
    #[ts(type = "number")]
    pub last_run_at: i64,
    pub runs: u32,
    /// The command has `[REDACTED]` in it: it can be inserted, never run.
    pub secret: bool,
}

pub async fn insert(store: &Store, run: NewRun) -> Result<i64, StoreError> {
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO command_history (pane_id, workspace_id, cwd, command, secret, exit_code, started_at, duration_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    run.pane_id,
                    run.workspace_id,
                    run.cwd,
                    run.command,
                    run.secret,
                    run.exit_code,
                    run.started_at,
                    run.duration_ms
                ],
            )?;
            Ok(t.last_insert_rowid())
        })
        .await
}

/// Newest first, one row per command and folder. Every word of `query`
/// must appear in the command (case-insensitive).
pub async fn search(
    store: &Store,
    query: String,
    workspace_id: Option<String>,
    limit: u32,
) -> Result<Vec<HistoryEntry>, StoreError> {
    store
        .read(move |c| {
            let words: Vec<String> = query
                .split_whitespace()
                .take(8)
                .map(|w| w.to_lowercase())
                .collect();
            let mut filter = String::from("1=1");
            let mut args: Vec<rusqlite::types::Value> = Vec::new();
            for w in &words {
                args.push(w.clone().into());
                filter.push_str(&format!(" AND instr(lower(command), ?{}) > 0", args.len()));
            }
            if let Some(ws) = &workspace_id {
                args.push(ws.clone().into());
                filter.push_str(&format!(" AND workspace_id = ?{}", args.len()));
            }
            args.push(i64::from(limit.clamp(1, 200)).into());
            let sql = format!(
                "SELECT h.id, h.command, h.cwd, h.exit_code, h.duration_ms, h.pane_id, h.workspace_id, h.started_at, g.runs, h.secret
                 FROM command_history h
                 JOIN (SELECT MAX(id) AS id, COUNT(*) AS runs FROM command_history WHERE {filter} GROUP BY command, cwd) g ON h.id = g.id
                 ORDER BY h.started_at DESC, h.id DESC LIMIT ?{}",
                args.len()
            );
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(params_from_iter(args), |r| {
                Ok(HistoryEntry {
                    id: r.get(0)?,
                    command: r.get(1)?,
                    cwd: r.get(2)?,
                    exit_code: r.get(3)?,
                    duration_ms: r.get(4)?,
                    pane_id: r.get(5)?,
                    pane_name: None,
                    workspace_id: r.get(6)?,
                    last_run_at: r.get(7)?,
                    runs: r.get(8)?,
                    secret: r.get(9)?,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// The command and its `secret` flag for a run.
pub async fn command_of(store: &Store, id: i64) -> Result<Option<(String, bool)>, StoreError> {
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT command, secret FROM command_history WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
        })
        .await
}

/// Deletes every run of the command `id` names, in its folder.
pub async fn delete_like(store: &Store, id: i64) -> Result<(), StoreError> {
    store
        .write(move |t| {
            t.execute(
                "DELETE FROM command_history WHERE (command, cwd) IN (SELECT command, cwd FROM command_history WHERE id = ?1)",
                [id],
            )?;
            Ok(())
        })
        .await
}

pub async fn clear(store: &Store) -> Result<u32, StoreError> {
    store
        .write(|t| Ok(t.execute("DELETE FROM command_history", [])? as u32))
        .await
}

/// Drops runs older than `before` (ms) and all but the newest [`MAX_ROWS`].
pub async fn prune(store: &Store, before: i64) -> Result<u32, StoreError> {
    store
        .write(move |t| {
            let old = t.execute("DELETE FROM command_history WHERE started_at < ?1", [before])?;
            let extra = t.execute(
                "DELETE FROM command_history WHERE id NOT IN (SELECT id FROM command_history ORDER BY id DESC LIMIT ?1)",
                [MAX_ROWS],
            )?;
            Ok((old + extra) as u32)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::temp_store;

    fn run(cmd: &str, cwd: &str, ws: Option<&str>, at: i64, exit: i32) -> NewRun {
        NewRun {
            pane_id: "pn_1".into(),
            workspace_id: ws.map(str::to_string),
            cwd: cwd.into(),
            command: cmd.into(),
            secret: false,
            exit_code: Some(exit),
            started_at: at,
            duration_ms: Some(12),
        }
    }

    #[tokio::test]
    async fn f_ex_02_search_groups_by_command_and_folder_newest_first() {
        let (_d, store) = temp_store().await;
        insert(&store, run("pnpm test", "/a", Some("ws_a"), 1, 1))
            .await
            .expect("insert");
        insert(&store, run("pnpm test", "/a", Some("ws_a"), 5, 0))
            .await
            .expect("insert");
        insert(
            &store,
            run("docker compose up -d", "/b", Some("ws_b"), 3, 0),
        )
        .await
        .expect("insert");
        insert(&store, run("pnpm test", "/b", Some("ws_b"), 2, 0))
            .await
            .expect("insert");
        let all = search(&store, String::new(), None, 50)
            .await
            .expect("search");
        assert_eq!(
            all.iter()
                .map(|e| (e.command.as_str(), e.cwd.as_str(), e.runs))
                .collect::<Vec<_>>(),
            [
                ("pnpm test", "/a", 2),
                ("docker compose up -d", "/b", 1),
                ("pnpm test", "/b", 1)
            ]
        );
        assert_eq!(all[0].exit_code, Some(0), "the newest run's exit");
        let words = search(&store, "COMPOSE up".into(), None, 50)
            .await
            .expect("search");
        assert_eq!(words.len(), 1);
        let here = search(&store, "pnpm".into(), Some("ws_b".into()), 50)
            .await
            .expect("search");
        assert_eq!(here.len(), 1);
        assert_eq!(here[0].cwd, "/b");
    }

    #[tokio::test]
    async fn f_ex_02_5_delete_clear_and_prune() {
        let (_d, store) = temp_store().await;
        let a = insert(&store, run("ls", "/a", None, 1, 0))
            .await
            .expect("insert");
        insert(&store, run("ls", "/a", None, 2, 0))
            .await
            .expect("insert");
        insert(&store, run("pwd", "/a", None, 3, 0))
            .await
            .expect("insert");
        delete_like(&store, a).await.expect("delete");
        assert_eq!(
            search(&store, String::new(), None, 50)
                .await
                .expect("s")
                .len(),
            1
        );
        insert(&store, run("old", "/a", None, -100, 0))
            .await
            .expect("insert");
        assert_eq!(prune(&store, 0).await.expect("prune"), 1);
        assert_eq!(clear(&store).await.expect("clear"), 1);
        assert!(search(&store, String::new(), None, 50)
            .await
            .expect("s")
            .is_empty());
    }
}
