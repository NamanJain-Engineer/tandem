//! `memory_changes` (F-AG-07, F-AG-08): every memory write the app made,
//! with the copy of what it replaced, so Undo puts it back.

use rusqlite::{params, OptionalExtension};
use tandem_core::ids::TeammateId;

use crate::{now_ms, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryChange {
    pub id: String,
    pub teammate_id: TeammateId,
    /// "notes" or "about".
    pub file: String,
    pub before_hash: String,
    pub after_hash: String,
    pub before_path: String,
    pub created_at: i64,
}

pub async fn insert(store: &Store, c: MemoryChange) -> Result<(), StoreError> {
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO memory_changes (id, teammate_id, file, before_hash, after_hash, before_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    c.id,
                    c.teammate_id.as_str(),
                    c.file,
                    c.before_hash,
                    c.after_hash,
                    c.before_path,
                    if c.created_at > 0 { c.created_at } else { now_ms() }
                ],
            )?;
            Ok(())
        })
        .await
}

pub async fn get(store: &Store, id: &str) -> Result<Option<MemoryChange>, StoreError> {
    let id = id.to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT id, teammate_id, file, before_hash, after_hash, before_path, created_at
                   FROM memory_changes WHERE id = ?1",
                [id],
                |r| {
                    Ok(MemoryChange {
                        id: r.get(0)?,
                        teammate_id: TeammateId::from_db(r.get(1)?),
                        file: r.get(2)?,
                        before_hash: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                        after_hash: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                        before_path: r.get(5)?,
                        created_at: r.get(6)?,
                    })
                },
            )
            .optional()?)
        })
        .await
}

pub async fn delete(store: &Store, id: &str) -> Result<(), StoreError> {
    let id = id.to_string();
    store
        .write(move |t| {
            t.execute("DELETE FROM memory_changes WHERE id = ?1", [id])?;
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn f_ag_08_a_change_is_kept_until_undone() {
        let (_d, store) = crate::tests::temp_store().await;
        let c = MemoryChange {
            id: "mc_1".into(),
            teammate_id: TeammateId::new(),
            file: "notes".into(),
            before_hash: "a".into(),
            after_hash: "b".into(),
            before_path: "/h/memory/.history/mc_1.md".into(),
            created_at: 0,
        };
        insert(&store, c.clone()).await.expect("insert");
        let got = get(&store, "mc_1").await.expect("get").expect("row");
        assert_eq!(got.before_path, c.before_path);
        assert!(got.created_at > 0);
        delete(&store, "mc_1").await.expect("delete");
        assert!(get(&store, "mc_1").await.expect("get").is_none());
    }
}
