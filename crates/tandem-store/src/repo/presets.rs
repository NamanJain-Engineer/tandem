//! Launch presets (F-CD-14): the four built-ins and the lineups the user
//! saved. Built-ins live in code (fixed ids, never in the table), so a
//! release can tune them and they can't be deleted.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::{AccountId, PresetId};
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

/// The most seats one row can stand for ("How many", UI-12).
pub const MAX_COUNT: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SeatKind {
    Agent,
    Shell,
    Browser,
}

/// Agents share the repository checkout, or each gets its own worktree
/// (F-CD-15). Shell and browser seats always stay in the checkout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Isolation {
    Shared,
    Worktree,
}

/// One row of a lineup. `count` stands for that many identical seats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Seat {
    pub kind: SeatKind,
    /// The engine for an agent seat; None means "the default engine",
    /// which the launch sheet resolves to an installed one.
    pub engine_id: Option<String>,
    pub account_id: Option<AccountId>,
    /// A free-text role (Builder, Reviewer, …); known roles carry a brief.
    pub role: Option<String>,
    /// The seat's own task, instead of the shared one.
    pub task: Option<String>,
    /// What a shell seat types once its prompt is up.
    pub command: Option<String>,
    pub url: Option<String>,
    pub count: u8,
}

impl Seat {
    pub fn agent(role: &str) -> Self {
        Seat {
            kind: SeatKind::Agent,
            engine_id: None,
            account_id: None,
            role: Some(role.into()),
            task: None,
            command: None,
            url: None,
            count: 1,
        }
    }

    pub fn shell(role: &str) -> Self {
        Seat {
            kind: SeatKind::Shell,
            role: Some(role.into()),
            ..Seat::agent("")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Preset {
    pub id: PresetId,
    pub name: String,
    pub seats: Vec<Seat>,
    pub isolation: Isolation,
    pub builtin: bool,
    pub created_at: i64,
}

fn builtin(slug: &str, name: &str, seats: Vec<Seat>, isolation: Isolation) -> Preset {
    Preset {
        id: PresetId::from_db(format!("pst_builtin_{slug}")),
        name: name.into(),
        seats,
        isolation,
        builtin: true,
        created_at: 0,
    }
}

/// Solo, Pair, Bench and Fleet (PRODUCT_SPEC F-CD-14, UI_SPEC UI-12).
pub fn builtins() -> Vec<Preset> {
    vec![
        builtin(
            "solo",
            "Solo",
            vec![Seat::agent("Builder")],
            Isolation::Shared,
        ),
        // A builds, B reviews, in the same tree.
        builtin(
            "pair",
            "Pair",
            vec![Seat::agent("Builder"), Seat::agent("Reviewer")],
            Isolation::Shared,
        ),
        // One agent and a shell for tests and git.
        builtin(
            "bench",
            "Bench",
            vec![Seat::agent("Builder"), Seat::shell("Tests")],
            Isolation::Shared,
        ),
        // Four agents on parallel tasks, each in its own worktree.
        builtin(
            "fleet",
            "Fleet",
            vec![
                Seat::agent("Builder"),
                Seat::agent("Builder"),
                Seat::agent("Tester"),
                Seat::agent("Reviewer"),
            ],
            Isolation::Worktree,
        ),
    ]
}

pub fn is_builtin(id: &PresetId) -> bool {
    id.as_str().starts_with("pst_builtin_")
}

/// A lineup worth saving or launching: 1–8 per row, every row the right
/// shape for its kind.
pub fn validate(seats: &[Seat]) -> Result<(), StoreError> {
    if seats.is_empty() {
        return Err(StoreError::Invalid("Add at least one seat".into()));
    }
    for s in seats {
        if s.count == 0 || s.count > MAX_COUNT {
            return Err(StoreError::Invalid(format!("How many is 1 to {MAX_COUNT}")));
        }
        if s.kind == SeatKind::Browser && s.url.as_deref().map(str::trim).unwrap_or("").is_empty() {
            return Err(StoreError::Invalid("A browser seat needs a URL".into()));
        }
    }
    Ok(())
}

fn isolation_str(i: Isolation) -> &'static str {
    match i {
        Isolation::Shared => "shared",
        Isolation::Worktree => "worktree",
    }
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<Preset> {
    let seats: String = r.get(2)?;
    let isolation: String = r.get(3)?;
    Ok(Preset {
        id: PresetId::from_db(r.get(0)?),
        name: r.get(1)?,
        // A row this version can't read shows as empty rather than failing
        // the whole list.
        seats: serde_json::from_str(&seats).unwrap_or_default(),
        isolation: if isolation == "worktree" {
            Isolation::Worktree
        } else {
            Isolation::Shared
        },
        builtin: false,
        created_at: r.get(4)?,
    })
}

/// Built-ins first, then saved presets oldest first.
pub async fn list(store: &Store) -> Result<Vec<Preset>, StoreError> {
    let saved = store
        .read(|c| {
            let mut st = c.prepare(
                "SELECT id, name, seats_json, isolation, created_at FROM presets WHERE builtin = 0 ORDER BY created_at, id",
            )?;
            let rows = st.query_map([], from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(builtins().into_iter().chain(saved).collect())
}

/// Saves a lineup under `name`: a new preset, or the saved one with that id.
/// Built-ins can't be overwritten.
pub async fn save(
    store: &Store,
    id: Option<PresetId>,
    name: &str,
    seats: Vec<Seat>,
    isolation: Isolation,
) -> Result<Preset, StoreError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(StoreError::Invalid("Name the preset".into()));
    }
    if id.as_ref().is_some_and(is_builtin) {
        return Err(StoreError::Invalid(
            "Built-in presets can't be changed".into(),
        ));
    }
    validate(&seats)?;
    let json = serde_json::to_string(&seats).map_err(|e| StoreError::Invalid(e.to_string()))?;
    store
        .write(move |t| {
            let taken: Option<String> = t
                .query_row(
                    "SELECT id FROM presets WHERE name = ?1 COLLATE NOCASE",
                    [&name],
                    |r| r.get(0),
                )
                .optional()?;
            let builtin_name = builtins().iter().any(|b| b.name.eq_ignore_ascii_case(&name));
            if builtin_name || taken.is_some_and(|t| Some(t.as_str()) != id.as_ref().map(PresetId::as_str)) {
                return Err(StoreError::Conflict(format!("There's already a preset called {name}")));
            }
            let preset = match id {
                Some(id) => {
                    let created: i64 = t
                        .query_row(
                            "SELECT created_at FROM presets WHERE id = ?1",
                            [id.as_str()],
                            |r| r.get(0),
                        )
                        .optional()?
                        .ok_or_else(|| StoreError::NotFound("No preset with that id".into()))?;
                    t.execute(
                        "UPDATE presets SET name = ?2, seats_json = ?3, isolation = ?4 WHERE id = ?1",
                        params![id.as_str(), name, json, isolation_str(isolation)],
                    )?;
                    Preset { id, name, seats, isolation, builtin: false, created_at: created }
                }
                None => {
                    let preset = Preset {
                        id: PresetId::new(),
                        name,
                        seats,
                        isolation,
                        builtin: false,
                        created_at: now_ms(),
                    };
                    t.execute(
                        "INSERT INTO presets (id, name, seats_json, isolation, builtin, created_at) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
                        params![
                            preset.id.as_str(),
                            preset.name,
                            json,
                            isolation_str(isolation),
                            preset.created_at
                        ],
                    )?;
                    preset
                }
            };
            Ok(preset)
        })
        .await
}

pub async fn delete(store: &Store, id: &PresetId) -> Result<(), StoreError> {
    if is_builtin(id) {
        return Err(StoreError::Invalid(
            "Built-in presets can't be deleted".into(),
        ));
    }
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            if t.execute("DELETE FROM presets WHERE id = ?1", [&id])? == 0 {
                return Err(StoreError::NotFound("No preset with that id".into()));
            }
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, store)
    }

    #[tokio::test]
    async fn f_cd_14_builtins_are_solo_pair_bench_fleet() {
        let (_d, store) = store();
        let all = list(&store).await.expect("list");
        let shape: Vec<(String, usize, Isolation)> = all
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    p.seats.iter().map(|s| s.count as usize).sum(),
                    p.isolation,
                )
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                ("Solo".into(), 1, Isolation::Shared),
                ("Pair".into(), 2, Isolation::Shared),
                ("Bench".into(), 2, Isolation::Shared),
                ("Fleet".into(), 4, Isolation::Worktree),
            ]
        );
        assert_eq!(all[2].seats[1].kind, SeatKind::Shell);
        assert!(all.iter().all(|p| p.builtin && is_builtin(&p.id)));
    }

    #[tokio::test]
    async fn f_cd_14_4_a_saved_preset_lists_again_with_the_same_seats_after_reopening() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = tandem_core::DataDir::at(dir.path().to_path_buf()).expect("data");
        let seats = vec![
            Seat {
                engine_id: Some("codex".into()),
                count: 2,
                ..Seat::agent("Tester")
            },
            Seat {
                command: Some("cargo watch -x test".into()),
                ..Seat::shell("Tests")
            },
        ];
        {
            let store = Store::open(&data).expect("store");
            save(&store, None, "Nightly", seats.clone(), Isolation::Worktree)
                .await
                .expect("save");
        }
        let store = Store::open(&data).expect("reopen");
        let all = list(&store).await.expect("list");
        let saved = all.iter().find(|p| p.name == "Nightly").expect("saved");
        assert_eq!(saved.seats, seats);
        assert_eq!(saved.isolation, Isolation::Worktree);
        assert!(!saved.builtin);
        assert_eq!(all.last().map(|p| p.name.as_str()), Some("Nightly"));
    }

    #[tokio::test]
    async fn f_cd_14_builtins_cant_be_changed_or_deleted_and_names_stay_unique() {
        let (_d, store) = store();
        let solo = builtins().remove(0);
        assert!(matches!(
            delete(&store, &solo.id).await,
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            save(
                &store,
                Some(solo.id.clone()),
                "Mine",
                solo.seats.clone(),
                Isolation::Shared
            )
            .await,
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            save(&store, None, "fleet", solo.seats.clone(), Isolation::Shared).await,
            Err(StoreError::Conflict(_))
        ));
        let a = save(&store, None, "Mine", solo.seats.clone(), Isolation::Shared)
            .await
            .expect("a");
        assert!(matches!(
            save(&store, None, "mine", solo.seats.clone(), Isolation::Shared).await,
            Err(StoreError::Conflict(_))
        ));
        // Renaming the same preset to its own name is fine.
        save(
            &store,
            Some(a.id.clone()),
            "Mine",
            solo.seats.clone(),
            Isolation::Worktree,
        )
        .await
        .expect("resave");
        delete(&store, &a.id).await.expect("delete");
        assert_eq!(list(&store).await.expect("list").len(), 4);
    }

    #[test]
    fn f_cd_14_a_lineup_needs_seats_counts_of_1_to_8_and_urls_for_browsers() {
        assert!(validate(&[]).is_err());
        assert!(validate(&[Seat {
            count: 9,
            ..Seat::agent("Builder")
        }])
        .is_err());
        assert!(validate(&[Seat {
            count: 0,
            ..Seat::agent("Builder")
        }])
        .is_err());
        assert!(validate(&[Seat {
            kind: SeatKind::Browser,
            ..Seat::agent("")
        }])
        .is_err());
        assert!(validate(&[Seat {
            count: 8,
            ..Seat::agent("Builder")
        }])
        .is_ok());
    }
}
