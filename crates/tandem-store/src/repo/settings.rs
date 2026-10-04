//! `settings` key/value rows (F-SH-04). Values are validated against the
//! typed schema in `tandem-core` on write and on read.

use std::collections::BTreeMap;

use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use tandem_core::settings::{self, SettingKey};

use crate::{now_ms, Store, StoreError};

/// Every key with its stored value, or its default.
pub async fn get_all(store: &Store) -> Result<BTreeMap<SettingKey, Value>, StoreError> {
    let rows: Vec<(String, String)> = store
        .read(|c| {
            let mut stmt = c.prepare("SELECT key, value_json FROM settings")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    let mut out: BTreeMap<SettingKey, Value> = SettingKey::ALL
        .iter()
        .map(|k| (*k, settings::default_value(*k)))
        .collect();
    for (key, json) in rows {
        let Some(key) = SettingKey::parse(&key) else {
            tracing::warn!(key = %key, "skipping unknown setting");
            continue;
        };
        match serde_json::from_str::<Value>(&json) {
            Ok(v) if settings::validate(key, &v).is_ok() => {
                out.insert(key, v);
            }
            _ => tracing::warn!(key = %key.as_str(), "skipping invalid stored setting"),
        }
    }
    Ok(out)
}

pub async fn get(store: &Store, key: SettingKey) -> Result<Value, StoreError> {
    let name = key.as_str();
    let stored: Option<String> = store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT value_json FROM settings WHERE key = ?1",
                [name],
                |r| r.get(0),
            )
            .optional()?)
        })
        .await?;
    Ok(stored
        .and_then(|j| serde_json::from_str::<Value>(&j).ok())
        .filter(|v| settings::validate(key, v).is_ok())
        .unwrap_or_else(|| settings::default_value(key)))
}

pub async fn set(store: &Store, key: SettingKey, value: Value) -> Result<(), StoreError> {
    settings::validate(key, &value)?;
    let name = key.as_str();
    let json = serde_json::to_string(&value).map_err(|e| StoreError::Invalid(e.to_string()))?;
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
                params![name, json, now_ms()],
            )?;
            Ok(())
        })
        .await
}

/// Removes stored values so the keys fall back to defaults. `None` resets all.
pub async fn reset(store: &Store, keys: Option<Vec<SettingKey>>) -> Result<(), StoreError> {
    store
        .write(move |t| {
            match keys {
                None => {
                    t.execute("DELETE FROM settings", [])?;
                }
                Some(keys) => {
                    for k in keys {
                        t.execute("DELETE FROM settings WHERE key = ?1", [k.as_str()])?;
                    }
                }
            }
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::DataDir;

    #[tokio::test]
    async fn f_sh_04_5_every_setting_round_trips_a_relaunch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        {
            let store = Store::open(&data).expect("open");
            for key in SettingKey::ALL {
                set(&store, *key, settings::sample_non_default(*key))
                    .await
                    .expect("set");
            }
        }
        // "Relaunch": a fresh Store over the same files.
        let store = Store::open(&data).expect("reopen");
        let all = get_all(&store).await.expect("all");
        for key in SettingKey::ALL {
            assert_eq!(all[key], settings::sample_non_default(*key), "{key:?}");
            assert_eq!(
                get(&store, *key).await.expect("get"),
                settings::sample_non_default(*key)
            );
        }
    }

    #[tokio::test]
    async fn invalid_values_are_rejected_and_reset_restores_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("open");
        assert!(
            set(&store, SettingKey::TermScrollback, serde_json::json!(5))
                .await
                .is_err()
        );
        set(&store, SettingKey::Theme, serde_json::json!("light"))
            .await
            .expect("set");
        reset(&store, None).await.expect("reset");
        assert_eq!(
            get(&store, SettingKey::Theme).await.expect("get"),
            serde_json::json!("dark")
        );
    }
}
