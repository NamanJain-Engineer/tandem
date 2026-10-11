//! `checkpoints` (F-EX-01): the rows behind each folder's timeline. The
//! files themselves live in the folder's store under `$DATA/checkpoints`
//! (`tandem_git::checkpoint`); a row names its tree. Prompt text is never
//! stored here.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Store, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CheckpointReason {
    /// Before a thread turn.
    Turn,
    /// Before a prompt submitted to an agent pane.
    Prompt,
    /// "Checkpoint now".
    Manual,
    /// Saved by a restore, so it can be undone.
    Restore,
}

impl CheckpointReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Prompt => "prompt",
            Self::Manual => "manual",
            Self::Restore => "restore",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "turn" => Self::Turn,
            "prompt" => Self::Prompt,
            "restore" => Self::Restore,
            _ => Self::Manual,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Checkpoint {
    pub id: String,
    pub folder: String,
    pub workspace_id: Option<String>,
    pub thread_id: Option<String>,
    pub pane_id: Option<String>,
    pub turn_id: Option<String>,
    pub reason: CheckpointReason,
    /// The thread's title or the pane's name, when there is one.
    pub label: String,
    #[serde(skip)]
    #[ts(skip)]
    pub tree: String,
    pub files: u32,
    pub skipped: Vec<String>,
    pub partial: bool,
    /// An agent pane's Enter waited the full 2 s before this was written.
    pub late: bool,
    pub created_at: i64,
    /// Files that differ from the folder now (filled in by the service).
    #[serde(default)]
    pub differs: u32,
}

const COLS: &str = "id, folder, workspace_id, thread_id, pane_id, turn_id, reason, label, tree, files, skipped, partial, late, created_at";

fn row(r: &Row<'_>) -> rusqlite::Result<Checkpoint> {
    let skipped: String = r.get(10)?;
    Ok(Checkpoint {
        id: r.get(0)?,
        folder: r.get(1)?,
        workspace_id: r.get(2)?,
        thread_id: r.get(3)?,
        pane_id: r.get(4)?,
        turn_id: r.get(5)?,
        reason: CheckpointReason::parse(&r.get::<_, String>(6)?),
        label: r.get(7)?,
        tree: r.get(8)?,
        files: r.get(9)?,
        skipped: serde_json::from_str(&skipped).unwrap_or_default(),
        partial: r.get(11)?,
        late: r.get(12)?,
        created_at: r.get(13)?,
        differs: 0,
    })
}

pub async fn insert(store: &Store, c: Checkpoint) -> Result<Checkpoint, StoreError> {
    store
        .write(move |t| {
            t.execute(
                &format!("INSERT INTO checkpoints ({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)"),
                params![
                    c.id,
                    c.folder,
                    c.workspace_id,
                    c.thread_id,
                    c.pane_id,
                    c.turn_id,
                    c.reason.as_str(),
                    c.label,
                    c.tree,
                    c.files,
                    serde_json::to_string(&c.skipped).unwrap_or_else(|_| "[]".into()),
                    c.partial,
                    c.late,
                    c.created_at
                ],
            )?;
            Ok(c)
        })
        .await
}

pub async fn get(store: &Store, id: String) -> Result<Option<Checkpoint>, StoreError> {
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLS} FROM checkpoints WHERE id = ?1"),
                [id],
                row,
            )
            .optional()?)
        })
        .await
}

/// The folder's newest checkpoint.
pub async fn latest(store: &Store, folder: String) -> Result<Option<Checkpoint>, StoreError> {
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLS} FROM checkpoints WHERE folder = ?1 ORDER BY created_at DESC, id DESC LIMIT 1"),
                [folder],
                row,
            )
            .optional()?)
        })
        .await
}

/// A folder's checkpoints, or a thread's, newest first.
pub async fn list(
    store: &Store,
    folder: String,
    thread_id: Option<String>,
) -> Result<Vec<Checkpoint>, StoreError> {
    store
        .read(move |c| {
            let mut out = Vec::new();
            match thread_id {
                Some(th) => {
                    let mut st = c.prepare(&format!(
                        "SELECT {COLS} FROM checkpoints WHERE folder = ?1 AND thread_id = ?2 ORDER BY created_at DESC, id DESC"
                    ))?;
                    for r in st.query_map(params![folder, th], row)? {
                        out.push(r?);
                    }
                }
                None => {
                    let mut st = c.prepare(&format!(
                        "SELECT {COLS} FROM checkpoints WHERE folder = ?1 ORDER BY created_at DESC, id DESC"
                    ))?;
                    for r in st.query_map([folder], row)? {
                        out.push(r?);
                    }
                }
            }
            Ok(out)
        })
        .await
}

/// Every folder with checkpoints.
pub async fn folders(store: &Store) -> Result<Vec<String>, StoreError> {
    store
        .read(|c| {
            let mut st = c.prepare("SELECT DISTINCT folder FROM checkpoints")?;
            let rows = st.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<Result<Vec<String>, _>>()?)
        })
        .await
}

/// Ids to drop in `folder`: beyond the newest `keep`, or older than
/// `before` (ms), except restore checkpoints newer than `protect_after`.
pub async fn prunable(
    store: &Store,
    folder: String,
    keep: u32,
    before: i64,
    protect_after: i64,
) -> Result<Vec<String>, StoreError> {
    store
        .read(move |c| {
            let mut st = c.prepare(
                "SELECT id, reason, created_at FROM checkpoints WHERE folder = ?1 ORDER BY created_at DESC, id DESC",
            )?;
            let rows = st.query_map([folder], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?;
            let mut out = Vec::new();
            for (i, r) in rows.enumerate() {
                let (id, reason, at) = r?;
                let protected = reason == "restore" && at > protect_after;
                if !protected && (i >= keep as usize || at < before) {
                    out.push(id);
                }
            }
            Ok(out)
        })
        .await
}

pub async fn delete(store: &Store, ids: Vec<String>) -> Result<(), StoreError> {
    store
        .write(move |t| {
            for id in ids {
                t.execute("DELETE FROM checkpoints WHERE id = ?1", [id])?;
            }
            Ok(())
        })
        .await
}

pub async fn delete_all(store: &Store) -> Result<(), StoreError> {
    store
        .write(|t| {
            t.execute("DELETE FROM checkpoints", [])?;
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::temp_store;

    fn cp(id: &str, folder: &str, at: i64, reason: CheckpointReason) -> Checkpoint {
        Checkpoint {
            id: id.into(),
            folder: folder.into(),
            workspace_id: None,
            thread_id: Some("th_1".into()),
            pane_id: None,
            turn_id: None,
            reason,
            label: "Fix it".into(),
            tree: "abc".into(),
            files: 2,
            skipped: vec!["big.bin".into()],
            partial: false,
            late: false,
            created_at: at,
            differs: 0,
        }
    }

    #[tokio::test]
    async fn f_ex_01_rows_round_trip_newest_first() {
        let (_d, store) = temp_store().await;
        insert(&store, cp("cp_a", "/w", 1, CheckpointReason::Turn))
            .await
            .expect("insert");
        insert(&store, cp("cp_b", "/w", 2, CheckpointReason::Manual))
            .await
            .expect("insert");
        insert(&store, cp("cp_c", "/other", 3, CheckpointReason::Turn))
            .await
            .expect("insert");
        let rows = list(&store, "/w".into(), None).await.expect("list");
        assert_eq!(
            rows.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["cp_b", "cp_a"]
        );
        assert_eq!(rows[1].skipped, vec!["big.bin"]);
        assert_eq!(
            latest(&store, "/w".into())
                .await
                .expect("latest")
                .map(|c| c.id),
            Some("cp_b".into())
        );
        let mut f = folders(&store).await.expect("folders");
        f.sort();
        assert_eq!(f, ["/other", "/w"]);
    }

    #[tokio::test]
    async fn f_ex_01_5_pruning_keeps_the_newest_and_recent_restores() {
        let (_d, store) = temp_store().await;
        for i in 0..12 {
            insert(
                &store,
                cp(&format!("cp_{i:02}"), "/w", i, CheckpointReason::Turn),
            )
            .await
            .expect("insert");
        }
        let gone = prunable(&store, "/w".into(), 10, 0, 0)
            .await
            .expect("prunable");
        assert_eq!(gone, ["cp_01", "cp_00"]);
        // Older than the age limit.
        let gone = prunable(&store, "/w".into(), 100, 5, 0)
            .await
            .expect("prunable");
        assert_eq!(gone.len(), 5);
        // A fresh restore checkpoint survives both.
        insert(&store, cp("cp_r", "/w", -1, CheckpointReason::Restore))
            .await
            .expect("insert");
        let gone = prunable(&store, "/w".into(), 1, 100, -10)
            .await
            .expect("prunable");
        assert!(!gone.contains(&"cp_r".to_string()));
        delete(&store, gone).await.expect("delete");
        assert_eq!(
            list(&store, "/w".into(), None).await.expect("list").len(),
            1
        );
    }
}
