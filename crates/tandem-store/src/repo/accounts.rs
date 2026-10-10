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

/// Inserts `account` as given (its home already holds the sign-in).
pub async fn insert(store: &Store, account: &Account) -> Result<(), StoreError> {
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
        .await
}

/// Accounts the engine has (one-off homes don't count).
pub async fn count_for(store: &Store, engine_id: &str) -> Result<u32, StoreError> {
    let engine = engine_id.to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM accounts WHERE engine_id = ?1 AND removed_at IS NULL AND one_off = 0",
                [&engine],
                |r| r.get::<_, i64>(0),
            )? as u32)
        })
        .await
}

/// True when another of the engine's accounts has this nickname.
pub async fn label_taken(
    store: &Store,
    engine_id: &str,
    label: &str,
    except: Option<&AccountId>,
) -> Result<bool, StoreError> {
    let (engine, label) = (engine_id.to_string(), label.trim().to_string());
    let except = except.map(|e| e.as_str().to_string()).unwrap_or_default();
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM accounts WHERE engine_id = ?1 AND label = ?2 COLLATE NOCASE AND removed_at IS NULL AND id != ?3",
                params![engine, label, except],
                |r| r.get::<_, i64>(0),
            )? > 0)
        })
        .await
}

const NAME_TAKEN: &str = "Another account for this engine has that name";

/// Renames an account (F-AC-02).
pub async fn rename(store: &Store, id: &AccountId, label: &str) -> Result<Account, StoreError> {
    let label = label.trim().to_string();
    if label.is_empty() || label.chars().count() > 40 {
        return Err(StoreError::Invalid("Names are 1–40 characters".into()));
    }
    let account = get(store, id).await?;
    if label_taken(store, &account.engine_id, &label, Some(id)).await? {
        return Err(StoreError::Conflict(NAME_TAKEN.into()));
    }
    let id_s = id.as_str().to_string();
    let l = label.clone();
    store
        .write(move |t| {
            t.execute(
                "UPDATE accounts SET label = ?1 WHERE id = ?2",
                params![l, id_s],
            )?;
            Ok(())
        })
        .await?;
    Ok(Account { label, ..account })
}

/// Makes `id` its engine's default: new sessions use it (F-AC-02 AC2).
pub async fn set_default(store: &Store, id: &AccountId) -> Result<(), StoreError> {
    let account = get(store, id).await?;
    let id_s = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE accounts SET is_default = 0 WHERE engine_id = ?1 AND removed_at IS NULL",
                [&account.engine_id],
            )?;
            t.execute("UPDATE accounts SET is_default = 1 WHERE id = ?1", [&id_s])?;
            Ok(())
        })
        .await
}

/// Removes an account; its name is free again and, when it was the
/// default, the engine's oldest other account becomes the default.
/// Returns it, so its home can be deleted.
pub async fn remove(store: &Store, id: &AccountId) -> Result<Account, StoreError> {
    let account = get(store, id).await?;
    let a = account.clone();
    store
        .write(move |t| {
            t.execute(
                "UPDATE accounts SET removed_at = ?1, is_default = 0, label = label || ' · ' || id WHERE id = ?2",
                params![now_ms(), a.id.as_str()],
            )?;
            if a.is_default {
                t.execute(
                    "UPDATE accounts SET is_default = 1 WHERE id = (SELECT id FROM accounts WHERE engine_id = ?1 AND removed_at IS NULL AND one_off = 0 ORDER BY created_at, id LIMIT 1)",
                    [&a.engine_id],
                )?;
            }
            // Workspace overrides that named it fall back to the default.
            t.execute(
                "UPDATE workspaces SET account_overrides = json_remove(account_overrides, '$.\"' || ?1 || '\"') WHERE json_extract(account_overrides, '$.\"' || ?1 || '\"') = ?2",
                params![a.engine_id, a.id.as_str()],
            )?;
            Ok(())
        })
        .await?;
    Ok(account)
}

/// A workspace's choice of account for one engine (F-AC-03).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountOverride {
    pub workspace_id: tandem_core::ids::WorkspaceId,
    pub engine_id: String,
    pub account_id: AccountId,
}

/// The workspace's override for `engine_id`, if any.
pub async fn workspace_override(
    store: &Store,
    workspace: &tandem_core::ids::WorkspaceId,
    engine_id: &str,
) -> Result<Option<AccountId>, StoreError> {
    let (ws, engine) = (workspace.as_str().to_string(), engine_id.to_string());
    store
        .read(move |c| {
            let id: Option<String> = c
                .query_row(
                    "SELECT json_extract(account_overrides, '$.\"' || ?2 || '\"') FROM workspaces WHERE id = ?1",
                    params![ws, engine],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            Ok(id.map(AccountId::from_db))
        })
        .await
}

/// Sets (or with `None` clears) the workspace's account for an engine.
pub async fn set_workspace_override(
    store: &Store,
    workspace: &tandem_core::ids::WorkspaceId,
    engine_id: &str,
    account: Option<&AccountId>,
) -> Result<(), StoreError> {
    if !engine_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
        || engine_id.is_empty()
    {
        return Err(StoreError::Invalid("Unknown engine".into()));
    }
    if let Some(id) = account {
        if get(store, id).await?.engine_id != engine_id {
            return Err(StoreError::Invalid(
                "That account is for another engine".into(),
            ));
        }
    }
    let (ws, engine) = (workspace.as_str().to_string(), engine_id.to_string());
    let account = account.map(|a| a.as_str().to_string());
    store
        .write(move |t| {
            let n = match account {
                Some(id) => t.execute(
                    "UPDATE workspaces SET account_overrides = json_set(account_overrides, '$.\"' || ?2 || '\"', ?3) WHERE id = ?1",
                    params![ws, engine, id],
                )?,
                None => t.execute(
                    "UPDATE workspaces SET account_overrides = json_remove(account_overrides, '$.\"' || ?2 || '\"') WHERE id = ?1",
                    params![ws, engine],
                )?,
            };
            if n == 0 {
                return Err(StoreError::NotFound("workspace".into()));
            }
            Ok(())
        })
        .await
}

/// Every workspace override, for Settings › Accounts.
pub async fn overrides(store: &Store) -> Result<Vec<AccountOverride>, StoreError> {
    store
        .read(|c| {
            let mut st = c.prepare(
                "SELECT w.id, j.key, j.value FROM workspaces w, json_each(w.account_overrides) j WHERE w.removed_at IS NULL ORDER BY w.sort_key, j.key",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(AccountOverride {
                        workspace_id: tandem_core::ids::WorkspaceId::from_db(r.get(0)?),
                        engine_id: r.get(1)?,
                        account_id: AccountId::from_db(r.get(2)?),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
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

    async fn account(store: &Store, engine: &str, label: &str, default: bool) -> Account {
        create(store, engine, label, default, |id| {
            format!("/p/{}", id.as_str())
        })
        .await
        .expect("account")
    }

    #[tokio::test]
    async fn f_ac_02_2_set_default_moves_the_default_and_names_stay_unique() {
        let (_d, store) = temp_store().await;
        let work = account(&store, "claude", "Work", true).await;
        let personal = account(&store, "claude", "Personal", false).await;
        set_default(&store, &personal.id).await.expect("default");
        assert_eq!(
            default_for(&store, "claude")
                .await
                .expect("d")
                .map(|a| a.id),
            Some(personal.id.clone())
        );
        assert!(!get(&store, &work.id).await.expect("work").is_default);
        assert!(matches!(
            rename(&store, &work.id, "personal").await,
            Err(StoreError::Conflict(_))
        ));
        assert!(matches!(
            rename(&store, &work.id, "  ").await,
            Err(StoreError::Invalid(_))
        ));
        assert_eq!(
            rename(&store, &work.id, "Client A")
                .await
                .expect("renamed")
                .label,
            "Client A"
        );
        // The same name on another engine is fine.
        assert!(!label_taken(&store, "codex", "Personal", None)
            .await
            .expect("taken"));
        assert_eq!(count_for(&store, "claude").await.expect("count"), 2);
    }

    #[tokio::test]
    async fn f_ac_02_removing_the_default_promotes_the_oldest_and_frees_its_name() {
        let (_d, store) = temp_store().await;
        let work = account(&store, "claude", "Work", true).await;
        let personal = account(&store, "claude", "Personal", false).await;
        let removed = remove(&store, &work.id).await.expect("removed");
        assert_eq!(removed.home_path, work.home_path);
        assert!(get(&store, &work.id).await.is_err());
        assert_eq!(
            default_for(&store, "claude")
                .await
                .expect("d")
                .map(|a| a.id),
            Some(personal.id)
        );
        assert_eq!(count_for(&store, "claude").await.expect("count"), 1);
        assert!(!label_taken(&store, "claude", "Work", None)
            .await
            .expect("taken"));
        account(&store, "claude", "Work", false).await;
    }

    #[tokio::test]
    async fn f_ac_03_workspace_overrides_are_set_listed_cleared_and_dropped_with_the_account() {
        let (_d, store) = temp_store().await;
        let (ws, _) = crate::repo::workspaces::add(
            &store,
            crate::repo::workspaces::NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: "/code/shop".into(),
                name: "shop".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("ws");
        let a = account(&store, "cursor-agent", "A", true).await;
        let b = account(&store, "cursor-agent", "B", false).await;
        let other = account(&store, "codex", "X", true).await;
        assert_eq!(
            workspace_override(&store, &ws.id, "cursor-agent")
                .await
                .expect("none"),
            None
        );
        set_workspace_override(&store, &ws.id, "cursor-agent", Some(&b.id))
            .await
            .expect("set");
        assert_eq!(
            workspace_override(&store, &ws.id, "cursor-agent")
                .await
                .expect("b"),
            Some(b.id.clone())
        );
        assert!(matches!(
            set_workspace_override(&store, &ws.id, "cursor-agent", Some(&other.id)).await,
            Err(StoreError::Invalid(_))
        ));
        let listed = overrides(&store).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(
            (listed[0].engine_id.as_str(), &listed[0].account_id),
            ("cursor-agent", &b.id)
        );
        set_workspace_override(&store, &ws.id, "cursor-agent", None)
            .await
            .expect("clear");
        assert_eq!(
            workspace_override(&store, &ws.id, "cursor-agent")
                .await
                .expect("cleared"),
            None
        );
        set_workspace_override(&store, &ws.id, "cursor-agent", Some(&b.id))
            .await
            .expect("again");
        remove(&store, &b.id).await.expect("remove");
        assert_eq!(
            workspace_override(&store, &ws.id, "cursor-agent")
                .await
                .expect("dropped"),
            None
        );
        let _ = a;
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
