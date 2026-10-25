//! `secrets` (F-TL-12): the names of the user's secrets. Their values live
//! in the keychain; this table only lists them.

use rusqlite::{params, OptionalExtension};

use crate::{Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRow {
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// By name.
pub async fn list(store: &Store) -> Result<Vec<SecretRow>, StoreError> {
    store
        .read(|c| {
            let mut stmt =
                c.prepare("SELECT name, created_at, updated_at FROM secrets ORDER BY name")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(SecretRow {
                        name: r.get(0)?,
                        created_at: r.get(1)?,
                        updated_at: r.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

pub async fn exists(store: &Store, name: &str) -> Result<bool, StoreError> {
    let name = name.to_string();
    store
        .read(move |c| {
            Ok(
                c.query_row("SELECT 1 FROM secrets WHERE name = ?1", [name], |_| Ok(()))
                    .optional()?
                    .is_some(),
            )
        })
        .await
}

/// Adds the name, or marks it changed.
pub async fn upsert(store: &Store, name: &str, now: i64) -> Result<SecretRow, StoreError> {
    let name = name.to_string();
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO secrets (name, created_at, updated_at) VALUES (?1, ?2, ?2)
                 ON CONFLICT(name) DO UPDATE SET updated_at = excluded.updated_at",
                params![name, now],
            )?;
            Ok(t.query_row(
                "SELECT name, created_at, updated_at FROM secrets WHERE name = ?1",
                [&name],
                |r| {
                    Ok(SecretRow {
                        name: r.get(0)?,
                        created_at: r.get(1)?,
                        updated_at: r.get(2)?,
                    })
                },
            )?)
        })
        .await
}

/// True when it was there.
pub async fn delete(store: &Store, name: &str) -> Result<bool, StoreError> {
    let name = name.to_string();
    store
        .write(move |t| Ok(t.execute("DELETE FROM secrets WHERE name = ?1", [name])? > 0))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::temp_store;

    #[tokio::test]
    async fn f_tl_12_1_only_names_and_times_are_stored() {
        let (_d, store) = temp_store().await;
        upsert(&store, "stripe-test", 10).await.expect("add");
        upsert(&store, "db", 11).await.expect("add");
        let again = upsert(&store, "stripe-test", 20).await.expect("replace");
        assert_eq!((again.created_at, again.updated_at), (10, 20));
        let names: Vec<String> = list(&store)
            .await
            .expect("list")
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, vec!["db".to_string(), "stripe-test".to_string()]);
        assert!(exists(&store, "db").await.expect("exists"));
        assert!(delete(&store, "db").await.expect("delete"));
        assert!(!delete(&store, "db").await.expect("delete again"));
        assert!(!exists(&store, "db").await.expect("exists"));
    }
}
