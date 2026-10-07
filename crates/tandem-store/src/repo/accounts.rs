//! Engine accounts and their provider homes (F-AC-01), and the detection
//! cache (F-EN-01).

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::AccountId;
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

pub const DEFAULT_LABEL: &str = "Default";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Account {
    pub id: AccountId,
    pub engine_id: String,
    pub label: String,
    pub home_path: String,
    pub is_default: bool,
    pub plan_label: Option<String>,
    pub created_at: i64,
}

const COLS: &str = "id, engine_id, label, home_path, is_default, plan_label, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: AccountId::from_db(r.get(0)?),
        engine_id: r.get(1)?,
        label: r.get(2)?,
        home_path: r.get(3)?,
        is_default: r.get::<_, i64>(4)? != 0,
        plan_label: r.get(5)?,
        created_at: r.get(6)?,
    })
}

pub async fn list(store: &Store) -> Result<Vec<Account>, StoreError> {
    store
        .read(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {COLS} FROM accounts WHERE removed_at IS NULL AND one_off = 0 ORDER BY engine_id, created_at"
            ))?;
            let rows = st.query_map([], from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

pub async fn get(store: &Store, id: &AccountId) -> Result<Account, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            c.query_row(
                &format!("SELECT {COLS} FROM accounts WHERE id = ?1 AND removed_at IS NULL"),
                [&id],
                from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("account".into()))
        })
        .await
}

pub async fn default_for(store: &Store, engine_id: &str) -> Result<Option<Account>, StoreError> {
    let engine = engine_id.to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLS} FROM accounts WHERE engine_id = ?1 AND is_default = 1 AND removed_at IS NULL"),
                [&engine],
                from_row,
            )
            .optional()?)
        })
        .await
}

/// Inserts an account. `home_for(id)` gives its home path from its new id.
pub async fn create(
    store: &Store,
    engine_id: &str,
    label: &str,
    is_default: bool,
    home_for: impl FnOnce(&AccountId) -> String + Send + 'static,
) -> Result<Account, StoreError> {
    let id = AccountId::new();
    let account = Account {
        home_path: home_for(&id),
        id,
        engine_id: engine_id.to_string(),
        label: label.to_string(),
        is_default,
        plan_label: None,
        created_at: now_ms(),
    };
    let a = account.clone();
    store
        .write(move |t| {
            if a.is_default {
                t.execute(
                    "UPDATE accounts SET is_default = 0 WHERE engine_id = ?1 AND removed_at IS NULL",
                    [&a.engine_id],
                )?;
            }
            t.execute(
                "INSERT INTO accounts (id, engine_id, label, home_path, is_default, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![a.id.as_str(), a.engine_id, a.label, a.home_path, a.is_default as i64, a.created_at],
            )?;
            Ok(())
        })
        .await?;
    Ok(account)
}

/// The engine's default account, created as "Default" on first use. The
/// check and the insert are one write, so panes launched together share it.
pub async fn ensure_default(
    store: &Store,
    engine_id: &str,
    home_for: impl FnOnce(&AccountId) -> String + Send + 'static,
) -> Result<Account, StoreError> {
    if let Some(a) = default_for(store, engine_id).await? {
        return Ok(a);
    }
    let engine = engine_id.to_string();
    store
        .write(move |t| {
            let existing = t
                .query_row(
                    &format!("SELECT {COLS} FROM accounts WHERE engine_id = ?1 AND is_default = 1 AND removed_at IS NULL"),
                    [&engine],
                    from_row,
                )
                .optional()?;
            if let Some(a) = existing {
                return Ok(a);
            }
            let id = AccountId::new();
            let account = Account {
                home_path: home_for(&id),
                id,
                engine_id: engine.clone(),
                label: DEFAULT_LABEL.to_string(),
                is_default: true,
                plan_label: None,
                created_at: now_ms(),
            };
            t.execute(
                "INSERT INTO accounts (id, engine_id, label, home_path, is_default, created_at) VALUES (?1, ?2, ?3, ?4, 1, ?5)",
                params![account.id.as_str(), account.engine_id, account.label, account.home_path, account.created_at],
            )?;
            Ok(account)
        })
        .await
}

/// One detected engine, cached per host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRow {
    pub engine_id: String,
    pub path: String,
    pub version: Option<String>,
}

/// Replaces a host's detection cache: installs and the report itself.
pub async fn save_detection(
    store: &Store,
    host_id: &str,
    installs: Vec<InstallRow>,
    report_json: String,
) -> Result<(), StoreError> {
    let host = host_id.to_string();
    store
        .write(move |t| {
            let now = now_ms();
            t.execute("DELETE FROM engine_installs WHERE host_id = ?1", [&host])?;
            for i in &installs {
                t.execute(
                    "INSERT INTO engine_installs (host_id, engine_id, path, version, detected_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![host, i.engine_id, i.path, i.version, now],
                )?;
            }
            t.execute(
                "INSERT INTO engine_detection_reports (host_id, report_json, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(host_id) DO UPDATE SET report_json = excluded.report_json, created_at = excluded.created_at",
                params![host, report_json, now],
            )?;
            Ok(())
        })
        .await
}

/// The last detection report for a host, as stored.
pub async fn last_detection(store: &Store, host_id: &str) -> Result<Option<String>, StoreError> {
    let host = host_id.to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT report_json FROM engine_detection_reports WHERE host_id = ?1",
                [&host],
                |r| r.get(0),
            )
            .optional()?)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::paths::DataDir;

    async fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("open");
        (dir, store)
    }

    #[tokio::test]
    async fn f_ac_01_a_default_account_is_created_once_per_engine() {
        let (_d, store) = temp_store().await;
        let a = ensure_default(&store, "claude", |id| format!("/p/claude/{}", id.as_str()))
            .await
            .expect("a");
        let b = ensure_default(&store, "claude", |_| "/other".into())
            .await
            .expect("b");
        assert_eq!(a, b);
        assert_eq!(a.label, DEFAULT_LABEL);
        assert!(a.home_path.ends_with(a.id.as_str()));
        let codex = ensure_default(&store, "codex", |id| format!("/p/codex/{}", id.as_str()))
            .await
            .expect("codex");
        // Panes launched together share one default.
        let together =
            futures::future::join_all((0..6).map(|_| {
                ensure_default(&store, "gemini", |id| format!("/p/gemini/{}", id.as_str()))
            }))
            .await;
        let ids: std::collections::HashSet<_> = together
            .into_iter()
            .map(|a| a.expect("gemini").id)
            .collect();
        assert_eq!(ids.len(), 1);
        assert_ne!(codex.id, a.id);
        // A second default for the same engine takes over.
        let work = create(&store, "claude", "Work", true, |id| {
            format!("/p/claude/{}", id.as_str())
        })
        .await
        .expect("work");
        assert_eq!(
            default_for(&store, "claude")
                .await
                .expect("q")
                .map(|x| x.id),
            Some(work.id)
        );
        assert_eq!(list(&store).await.expect("list").len(), 4);
    }

    #[tokio::test]
    async fn f_en_01_detection_is_cached_per_host() {
        let (_d, store) = temp_store().await;
        save_detection(
            &store,
            "local",
            vec![InstallRow {
                engine_id: "claude".into(),
                path: "/x/claude".into(),
                version: Some("1.0.0".into()),
            }],
            "{\"found\":1}".into(),
        )
        .await
        .expect("save");
        save_detection(&store, "local", Vec::new(), "{\"found\":0}".into())
            .await
            .expect("again");
        assert_eq!(
            last_detection(&store, "local")
                .await
                .expect("last")
                .as_deref(),
            Some("{\"found\":0}")
        );
    }
}
