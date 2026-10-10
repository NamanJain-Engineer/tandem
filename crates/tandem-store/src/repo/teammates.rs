//! `teammates` rows (F-AG-01, F-AG-02, F-AG-05). Names are unique by their
//! fold (`Scout` = `scout`); a deleted teammate keeps its row (its chats
//! still name it) but frees its name.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::{AccountId, TeammateId};
use tandem_core::teammates::{fold_name, TeammateSettings};
use ts_rs::TS;

use crate::{now_ms, sort_key, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Teammate {
    pub id: TeammateId,
    pub name: String,
    /// The picked palette hue (0–11); `None`: from the id.
    pub avatar_hue: Option<u8>,
    /// An uploaded image (a path under the teammate's home).
    pub avatar_image: Option<String>,
    pub brief: String,
    pub engine_id: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub preferred_account_id: Option<AccountId>,
    /// `$DATA/teammates/<id>`.
    pub home: String,
    /// Folders beyond Home the teammate may work in.
    pub places: Vec<String>,
    pub settings: TeammateSettings,
    pub is_lead: bool,
    pub pinned: bool,
    pub sort_key: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewTeammate {
    pub id: TeammateId,
    /// Already validated and trimmed.
    pub name: String,
    pub avatar_hue: Option<u8>,
    pub brief: String,
    pub engine_id: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub preferred_account_id: Option<AccountId>,
    pub home: String,
    pub places: Vec<String>,
    pub settings: TeammateSettings,
    pub is_lead: bool,
    pub pinned: bool,
}

/// Fields to change; `None` leaves one alone. `Some(None)` clears an
/// optional field.
#[derive(Debug, Clone, Default)]
pub struct TeammateUpdate {
    pub name: Option<String>,
    pub avatar_hue: Option<Option<u8>>,
    pub avatar_image: Option<Option<String>>,
    pub brief: Option<String>,
    pub engine_id: Option<String>,
    pub model: Option<Option<String>>,
    pub effort: Option<Option<String>>,
    pub preferred_account_id: Option<Option<AccountId>>,
    pub places: Option<Vec<String>>,
    pub settings: Option<TeammateSettings>,
    pub pinned: Option<bool>,
    pub sort_key: Option<String>,
}

pub const NAME_USED: &str = "Name already used";

const COLUMNS: &str = "id, name, avatar_hue, avatar_image, brief, engine_id, model, effort, preferred_account_id, home, places_json, settings_json, is_lead, pinned, sort_key, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Teammate> {
    let places: String = r.get(10)?;
    let settings: String = r.get(11)?;
    Ok(Teammate {
        id: TeammateId::from_db(r.get(0)?),
        name: r.get(1)?,
        avatar_hue: r.get::<_, Option<i64>>(2)?.map(|h| h.clamp(0, 11) as u8),
        avatar_image: r.get(3)?,
        brief: r.get(4)?,
        engine_id: r.get(5)?,
        model: r.get(6)?,
        effort: r.get(7)?,
        preferred_account_id: r.get::<_, Option<String>>(8)?.map(AccountId::from_db),
        home: r.get(9)?,
        places: serde_json::from_str(&places).unwrap_or_default(),
        settings: serde_json::from_str::<TeammateSettings>(&settings)
            .unwrap_or_default()
            .normalized(),
        is_lead: r.get(12)?,
        pinned: r.get(13)?,
        sort_key: r.get(14)?,
        created_at: r.get(15)?,
    })
}

fn json<T: Serialize>(v: &T) -> Result<String, StoreError> {
    serde_json::to_string(v).map_err(|e| StoreError::Invalid(e.to_string()))
}

fn name_taken(
    t: &rusqlite::Connection,
    fold: &str,
    except: Option<&str>,
) -> Result<bool, StoreError> {
    let other: Option<String> = t
        .query_row(
            "SELECT id FROM teammates WHERE name_fold = ?1 AND deleted_at IS NULL",
            [fold],
            |r| r.get(0),
        )
        .optional()?;
    Ok(other.is_some_and(|id| Some(id.as_str()) != except))
}

/// Whether a teammate that isn't deleted already has this name (folded).
pub async fn name_used(store: &Store, name: &str) -> Result<bool, StoreError> {
    let fold = fold_name(name);
    store.read(move |c| name_taken(c, &fold, None)).await
}

/// Every teammate that isn't deleted: pinned first, then in the rail's
/// order.
pub async fn list(store: &Store) -> Result<Vec<Teammate>, StoreError> {
    store
        .read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM teammates WHERE deleted_at IS NULL
                 ORDER BY pinned DESC, sort_key, created_at"
            ))?;
            let rows = stmt.query_map([], from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// A teammate that isn't deleted.
pub async fn get(store: &Store, id: &TeammateId) -> Result<Option<Teammate>, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM teammates WHERE id = ?1 AND deleted_at IS NULL"),
                [id],
                from_row,
            )
            .optional()?)
        })
        .await
}

/// Any teammate row, deleted or not (a chat's "Deleted teammate" label).
pub async fn get_any(
    store: &Store,
    id: &TeammateId,
) -> Result<Option<(Teammate, bool)>, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS}, deleted_at IS NOT NULL FROM teammates WHERE id = ?1"),
                [id],
                |r| Ok((from_row(r)?, r.get::<_, bool>(16)?)),
            )
            .optional()?)
        })
        .await
}

/// Whether any teammate row was ever made (the Lead is made once).
pub async fn any_ever(store: &Store) -> Result<bool, StoreError> {
    store
        .read(|c| Ok(c.query_row("SELECT EXISTS (SELECT 1 FROM teammates)", [], |r| r.get(0))?))
        .await
}

/// Teammates that aren't deleted, the Lead excluded (the Free plan's count).
pub async fn count_besides_lead(store: &Store) -> Result<u32, StoreError> {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM teammates WHERE deleted_at IS NULL AND is_lead = 0",
                [],
                |r| r.get(0),
            )?)
        })
        .await
}

pub async fn insert(store: &Store, new: NewTeammate) -> Result<Teammate, StoreError> {
    let places = json(&new.places)?;
    let settings = json(&new.settings.normalized())?;
    let id = new.id.clone();
    store
        .write(move |t| {
            let fold = fold_name(&new.name);
            if name_taken(t, &fold, None)? {
                return Err(StoreError::Conflict(NAME_USED.into()));
            }
            let last: Option<String> = t
                .query_row(
                    "SELECT MAX(sort_key) FROM teammates WHERE deleted_at IS NULL",
                    [],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            let key = sort_key::between(last.as_deref(), None);
            t.execute(
                "INSERT INTO teammates (id, name, name_fold, avatar_hue, brief, engine_id, model, effort,
                    preferred_account_id, home, places_json, settings_json, is_lead, pinned, sort_key, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    new.id.as_str(),
                    new.name,
                    fold,
                    new.avatar_hue.map(i64::from),
                    new.brief,
                    new.engine_id,
                    new.model,
                    new.effort,
                    new.preferred_account_id.as_ref().map(|a| a.as_str().to_string()),
                    new.home,
                    places,
                    settings,
                    new.is_lead,
                    new.pinned,
                    key,
                    now_ms()
                ],
            )?;
            Ok(())
        })
        .await?;
    get(store, &id)
        .await?
        .ok_or_else(|| StoreError::NotFound("This teammate doesn't exist".into()))
}

pub async fn update(
    store: &Store,
    id: &TeammateId,
    u: TeammateUpdate,
) -> Result<Option<Teammate>, StoreError> {
    let key = id.as_str().to_string();
    let places = u.places.as_ref().map(json).transpose()?;
    let settings = u.settings.map(|s| json(&s.normalized())).transpose()?;
    store
        .write(move |t| {
            let exists: bool = t.query_row(
                "SELECT EXISTS (SELECT 1 FROM teammates WHERE id = ?1 AND deleted_at IS NULL)",
                [&key],
                |r| r.get(0),
            )?;
            if !exists {
                return Ok(());
            }
            if let Some(name) = &u.name {
                let fold = fold_name(name);
                if name_taken(t, &fold, Some(&key))? {
                    return Err(StoreError::Conflict(NAME_USED.into()));
                }
                t.execute(
                    "UPDATE teammates SET name = ?2, name_fold = ?3 WHERE id = ?1",
                    params![key, name, fold],
                )?;
            }
            if let Some(h) = u.avatar_hue {
                t.execute(
                    "UPDATE teammates SET avatar_hue = ?2 WHERE id = ?1",
                    params![key, h.map(i64::from)],
                )?;
            }
            if let Some(img) = &u.avatar_image {
                t.execute(
                    "UPDATE teammates SET avatar_image = ?2 WHERE id = ?1",
                    params![key, img],
                )?;
            }
            if let Some(b) = &u.brief {
                t.execute(
                    "UPDATE teammates SET brief = ?2 WHERE id = ?1",
                    params![key, b],
                )?;
            }
            if let Some(e) = &u.engine_id {
                t.execute(
                    "UPDATE teammates SET engine_id = ?2 WHERE id = ?1",
                    params![key, e],
                )?;
            }
            if let Some(m) = &u.model {
                t.execute(
                    "UPDATE teammates SET model = ?2 WHERE id = ?1",
                    params![key, m],
                )?;
            }
            if let Some(e) = &u.effort {
                t.execute(
                    "UPDATE teammates SET effort = ?2 WHERE id = ?1",
                    params![key, e],
                )?;
            }
            if let Some(a) = &u.preferred_account_id {
                t.execute(
                    "UPDATE teammates SET preferred_account_id = ?2 WHERE id = ?1",
                    params![key, a.as_ref().map(|a| a.as_str().to_string())],
                )?;
            }
            if let Some(p) = &places {
                t.execute(
                    "UPDATE teammates SET places_json = ?2 WHERE id = ?1",
                    params![key, p],
                )?;
            }
            if let Some(s) = &settings {
                t.execute(
                    "UPDATE teammates SET settings_json = ?2 WHERE id = ?1",
                    params![key, s],
                )?;
            }
            if let Some(p) = u.pinned {
                t.execute(
                    "UPDATE teammates SET pinned = ?2 WHERE id = ?1",
                    params![key, p],
                )?;
            }
            if let Some(k) = &u.sort_key {
                t.execute(
                    "UPDATE teammates SET sort_key = ?2 WHERE id = ?1",
                    params![key, k],
                )?;
            }
            Ok(())
        })
        .await?;
    get(store, id).await
}

/// Soft delete: the row stays for its chats, the name is free again.
pub async fn soft_delete(store: &Store, id: &TeammateId) -> Result<(), StoreError> {
    let key = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE teammates SET deleted_at = ?2, pinned = 0, name_fold = name_fold || '#deleted#' || id
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![key, now_ms()],
            )?;
            Ok(())
        })
        .await
}

/// The rail's order: `ids` first to last.
pub async fn reorder(store: &Store, ids: &[TeammateId]) -> Result<(), StoreError> {
    let keys = sort_key::sequence(ids.len());
    let pairs: Vec<(String, String)> = ids
        .iter()
        .map(|i| i.as_str().to_string())
        .zip(keys)
        .collect();
    store
        .write(move |t| {
            for (id, key) in &pairs {
                t.execute(
                    "UPDATE teammates SET sort_key = ?2 WHERE id = ?1",
                    params![id, key],
                )?;
            }
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::access::Access;
    use tandem_core::teammates::LearnMode;

    fn new(name: &str) -> NewTeammate {
        let id = TeammateId::new();
        NewTeammate {
            home: format!("/data/teammates/{id}"),
            id,
            name: name.into(),
            avatar_hue: None,
            brief: String::new(),
            engine_id: "claude".into(),
            model: None,
            effort: None,
            preferred_account_id: None,
            places: Vec::new(),
            settings: TeammateSettings::default(),
            is_lead: false,
            pinned: false,
        }
    }

    #[tokio::test]
    async fn f_ag_01_1_scout_then_scout_is_name_already_used() {
        let (_d, store) = crate::tests::temp_store().await;
        let scout = insert(&store, new("Scout")).await.expect("scout");
        let err = insert(&store, new("scout")).await.expect_err("taken");
        assert_eq!(err.to_string(), NAME_USED);
        // Renaming another onto it is refused too; renaming itself isn't.
        let other = insert(&store, new("Ranger")).await.expect("ranger");
        let err = update(
            &store,
            &other.id,
            TeammateUpdate {
                name: Some("SCOUT".into()),
                ..Default::default()
            },
        )
        .await
        .expect_err("taken");
        assert_eq!(err.to_string(), NAME_USED);
        let same = update(
            &store,
            &scout.id,
            TeammateUpdate {
                name: Some("SCOUT".into()),
                ..Default::default()
            },
        )
        .await
        .expect("own name")
        .expect("row");
        assert_eq!(same.name, "SCOUT");
        // A deleted teammate frees its name.
        soft_delete(&store, &scout.id).await.expect("delete");
        assert!(get(&store, &scout.id).await.expect("get").is_none());
        assert!(get_any(&store, &scout.id)
            .await
            .expect("any")
            .is_some_and(|(_, d)| d));
        insert(&store, new("scout")).await.expect("free again");
    }

    #[tokio::test]
    async fn f_ag_05_settings_places_and_order_round_trip() {
        let (_d, store) = crate::tests::temp_store().await;
        let a = insert(&store, new("A")).await.expect("a");
        let b = insert(&store, new("B")).await.expect("b");
        assert!(a.sort_key < b.sort_key);
        let changed = update(
            &store,
            &a.id,
            TeammateUpdate {
                places: Some(vec!["/work/site".into()]),
                settings: Some(TeammateSettings {
                    learn_mode: LearnMode::Off,
                    default_access: Access::Full,
                    may_message: false,
                    ..TeammateSettings::default()
                }),
                avatar_hue: Some(Some(7)),
                model: Some(Some("opus".into())),
                ..Default::default()
            },
        )
        .await
        .expect("update")
        .expect("row");
        assert_eq!(changed.places, vec!["/work/site".to_string()]);
        assert_eq!(changed.settings.learn_mode, LearnMode::Off);
        assert!(!changed.settings.may_message);
        assert_eq!(changed.avatar_hue, Some(7));
        assert_eq!(changed.model.as_deref(), Some("opus"));
        reorder(&store, &[b.id.clone(), a.id.clone()])
            .await
            .expect("order");
        let names: Vec<String> = list(&store)
            .await
            .expect("list")
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, vec!["B", "A"]);
        assert_eq!(count_besides_lead(&store).await.expect("count"), 2);
    }
}
