//! `workspaces` rows (F-CD-01). Paths are stored canonical; the same host and
//! root is one workspace. Removing is a soft delete, and adding the folder
//! again restores it with its history.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::WorkspaceId;
use ts_rs::TS;

use crate::{now_ms, sort_key, Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Workspace {
    pub id: WorkspaceId,
    /// `local` | `ssh` | `wsl`
    pub kind: String,
    pub host_id: String,
    pub root: String,
    pub wsl_distro: Option<String>,
    pub name: String,
    pub parent_id: Option<WorkspaceId>,
    pub lane_branch: Option<String>,
    /// The commit a worktree started from (its "Diff vs base").
    pub lane_base: Option<String>,
    pub sort_key: String,
    pub pinned: bool,
    pub collapsed: bool,
    pub sessions_collapsed: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewWorkspace {
    pub kind: String,
    pub host_id: String,
    pub root: String,
    pub name: String,
    pub wsl_distro: Option<String>,
}

/// A worktree registered as a child of its repository's workspace (F-CD-15).
#[derive(Debug, Clone)]
pub struct NewLane {
    pub parent_id: WorkspaceId,
    pub root: String,
    pub name: String,
    pub branch: String,
    pub base: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    Created,
    Existing,
    Restored,
}

const COLUMNS: &str = "id, kind, host_id, root, wsl_distro, name, parent_id, lane_branch, sort_key, pinned, collapsed, sessions_collapsed, created_at, lane_base";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: WorkspaceId::from_db(r.get(0)?),
        kind: r.get(1)?,
        host_id: r.get(2)?,
        root: r.get(3)?,
        wsl_distro: r.get(4)?,
        name: r.get(5)?,
        parent_id: r.get::<_, Option<String>>(6)?.map(WorkspaceId::from_db),
        lane_branch: r.get(7)?,
        sort_key: r.get(8)?,
        pinned: r.get(9)?,
        collapsed: r.get(10)?,
        sessions_collapsed: r.get(11)?,
        created_at: r.get(12)?,
        lane_base: r.get(13)?,
    })
}

/// Workspaces that aren't removed, in rail order: top-level ones and their
/// worktrees (`parent_id` set), which the rail nests under their parent.
pub async fn list(store: &Store) -> Result<Vec<Workspace>, StoreError> {
    store
        .read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM workspaces WHERE removed_at IS NULL ORDER BY pinned DESC, sort_key, created_at"
            ))?;
            let rows = stmt.query_map([], from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

pub async fn get(store: &Store, id: &WorkspaceId) -> Result<Workspace, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            c.query_row(
                &format!("SELECT {COLUMNS} FROM workspaces WHERE id = ?1"),
                [&id],
                from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("No workspace with that id".into()))
        })
        .await
}

/// Adds a workspace, returning the existing one for the same host and root.
/// `max_active` is the plan limit on top-level workspaces (F-BL-04); it's
/// checked in the same transaction as the insert.
pub async fn add(
    store: &Store,
    new: NewWorkspace,
    max_active: Option<u32>,
) -> Result<(Workspace, Added), StoreError> {
    store
        .write(move |t| {
            let found: Option<(Workspace, Option<i64>)> = t
                .query_row(
                    &format!("SELECT {COLUMNS}, removed_at FROM workspaces WHERE host_id = ?1 AND root = ?2"),
                    params![new.host_id, new.root],
                    |r| Ok((from_row(r)?, r.get(14)?)),
                )
                .optional()?;
            if let Some((ws, None)) = &found {
                return Ok((ws.clone(), Added::Existing));
            }
            let active: u32 = t.query_row(
                "SELECT COUNT(*) FROM workspaces WHERE removed_at IS NULL AND parent_id IS NULL",
                [],
                |r| r.get(0),
            )?;
            if let Some(max) = max_active {
                if active >= max {
                    return Err(StoreError::LimitReached(format!("This account keeps up to {max} workspaces")));
                }
            }
            let last: Option<String> = t
                .query_row(
                    "SELECT MAX(sort_key) FROM workspaces WHERE removed_at IS NULL AND parent_id IS NULL",
                    [],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            let key = sort_key::between(last.as_deref(), None);
            if let Some((ws, Some(_))) = found {
                t.execute(
                    "UPDATE workspaces SET removed_at = NULL, sort_key = ?2 WHERE id = ?1",
                    params![ws.id.as_str(), key],
                )?;
                return Ok((Workspace { sort_key: key, ..ws }, Added::Restored));
            }
            let ws = Workspace {
                id: WorkspaceId::new(),
                kind: new.kind,
                host_id: new.host_id,
                root: new.root,
                wsl_distro: new.wsl_distro,
                name: new.name,
                parent_id: None,
                lane_branch: None,
                lane_base: None,
                sort_key: key,
                pinned: false,
                collapsed: false,
                sessions_collapsed: false,
                created_at: now_ms(),
            };
            t.execute(
                "INSERT INTO workspaces (id, kind, host_id, root, wsl_distro, name, sort_key, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![ws.id.as_str(), ws.kind, ws.host_id, ws.root, ws.wsl_distro, ws.name, ws.sort_key, ws.created_at],
            )?;
            Ok((ws, Added::Created))
        })
        .await
}

/// Registers a worktree under its parent, after the parent's other
/// worktrees. Worktrees don't count toward the plan's workspace limit. A
/// removed row for the same folder (an earlier worktree at that path) is
/// reused.
pub async fn add_lane(store: &Store, lane: NewLane) -> Result<Workspace, StoreError> {
    store.write(move |t| add_lane_in(t, lane)).await
}

/// [`add_lane`] inside a write transaction, so other rows can go with it
/// (F-CD-22's `lane_repos`). A removed row at the same folder comes back,
/// without the repositories it had.
pub(crate) fn add_lane_in(
    t: &rusqlite::Transaction<'_>,
    lane: NewLane,
) -> Result<Workspace, StoreError> {
    let parent = t
        .query_row(
            &format!("SELECT {COLUMNS} FROM workspaces WHERE id = ?1 AND removed_at IS NULL"),
            [lane.parent_id.as_str()],
            from_row,
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound("No workspace with that id".into()))?;
    if parent.parent_id.is_some() {
        return Err(StoreError::Invalid(
            "A worktree belongs to its repository's workspace".into(),
        ));
    }
    let found: Option<(String, Option<i64>)> = t
        .query_row(
            "SELECT id, removed_at FROM workspaces WHERE host_id = ?1 AND root = ?2",
            params![parent.host_id, lane.root],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let last: Option<String> = t
        .query_row(
            "SELECT MAX(sort_key) FROM workspaces WHERE removed_at IS NULL AND parent_id = ?1",
            [parent.id.as_str()],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let key = sort_key::between(last.as_deref(), None);
    let ws = Workspace {
        id: WorkspaceId::new(),
        kind: parent.kind.clone(),
        host_id: parent.host_id.clone(),
        root: lane.root,
        wsl_distro: parent.wsl_distro.clone(),
        name: lane.name,
        parent_id: Some(parent.id.clone()),
        lane_branch: Some(lane.branch),
        lane_base: Some(lane.base),
        sort_key: key,
        pinned: false,
        collapsed: false,
        sessions_collapsed: false,
        created_at: now_ms(),
    };
    match found {
        Some((_, None)) => Err(StoreError::Conflict(
            "Another workspace already uses that folder".into(),
        )),
        Some((id, Some(_))) => {
            t.execute(
                "UPDATE workspaces SET removed_at = NULL, name = ?2, parent_id = ?3, lane_branch = ?4,
                 lane_base = ?5, sort_key = ?6, kind = ?7, wsl_distro = ?8, pinned = 0, collapsed = 0
                 WHERE id = ?1",
                params![id, ws.name, parent.id.as_str(), ws.lane_branch, ws.lane_base, ws.sort_key, ws.kind, ws.wsl_distro],
            )?;
            t.execute("DELETE FROM lane_repos WHERE lane_id = ?1", [id.as_str()])?;
            Ok(Workspace {
                id: WorkspaceId::from_db(id),
                ..ws
            })
        }
        None => {
            t.execute(
                "INSERT INTO workspaces (id, kind, host_id, root, wsl_distro, name, parent_id, lane_branch, lane_base, sort_key, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    ws.id.as_str(), ws.kind, ws.host_id, ws.root, ws.wsl_distro, ws.name,
                    parent.id.as_str(), ws.lane_branch, ws.lane_base, ws.sort_key, ws.created_at
                ],
            )?;
            Ok(ws)
        }
    }
}

/// A workspace's worktrees that aren't removed, oldest first.
pub async fn lanes(store: &Store, parent: &WorkspaceId) -> Result<Vec<Workspace>, StoreError> {
    let parent = parent.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM workspaces WHERE removed_at IS NULL AND parent_id = ?1 ORDER BY sort_key, created_at"
            ))?;
            let rows = stmt.query_map([&parent], from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

pub async fn rename(store: &Store, id: &WorkspaceId, name: String) -> Result<(), StoreError> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(StoreError::Invalid("Names are 1–80 characters".into()));
    }
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            match t.execute(
                "UPDATE workspaces SET name = ?2 WHERE id = ?1",
                params![id, name],
            )? {
                0 => Err(StoreError::NotFound("No workspace with that id".into())),
                _ => Ok(()),
            }
        })
        .await
}

/// Soft remove: nothing on disk is touched, and re-adding restores it.
pub async fn remove(store: &Store, id: &WorkspaceId) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            match t.execute(
                "UPDATE workspaces SET removed_at = ?2 WHERE (id = ?1 OR parent_id = ?1) AND removed_at IS NULL",
                params![id, now_ms()],
            )? {
                0 => Err(StoreError::NotFound("No workspace with that id".into())),
                _ => Ok(()),
            }
        })
        .await
}

/// Moves a top-level workspace in the rail (F-SH-03): into the pinned band
/// or out of it, right after `after` (or first in its band). Only the moved
/// row changes.
pub async fn move_to(
    store: &Store,
    id: &WorkspaceId,
    after: Option<WorkspaceId>,
    pinned: bool,
) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    let after = after.map(|a| a.as_str().to_string());
    store
        .write(move |t| {
            let band: Vec<(String, String)> = {
                let mut stmt = t.prepare(
                    "SELECT id, sort_key FROM workspaces
                     WHERE removed_at IS NULL AND parent_id IS NULL AND pinned = ?1 AND id != ?2
                     ORDER BY sort_key, created_at",
                )?;
                let rows = stmt
                    .query_map(params![pinned, id], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            let (lo, hi) = match &after {
                None => (None, band.first().map(|(_, k)| k.clone())),
                Some(a) => {
                    let i = band
                        .iter()
                        .position(|(bid, _)| bid == a)
                        .ok_or_else(|| StoreError::NotFound("No workspace to place it after".into()))?;
                    (band.get(i).map(|(_, k)| k.clone()), band.get(i + 1).map(|(_, k)| k.clone()))
                }
            };
            // Equal neighbours (legacy rows) get fresh keys first.
            let key = if lo.is_some() && lo == hi {
                let keys = sort_key::sequence(band.len());
                for ((bid, _), k) in band.iter().zip(&keys) {
                    t.execute("UPDATE workspaces SET sort_key = ?2 WHERE id = ?1", params![bid, k])?;
                }
                let i = after.as_ref().and_then(|a| band.iter().position(|(bid, _)| bid == a)).unwrap_or(0);
                sort_key::between(keys.get(i).map(String::as_str), keys.get(i + 1).map(String::as_str))
            } else {
                sort_key::between(lo.as_deref(), hi.as_deref())
            };
            match t.execute(
                "UPDATE workspaces SET sort_key = ?2, pinned = ?3 WHERE id = ?1 AND removed_at IS NULL AND parent_id IS NULL",
                params![id, key, pinned],
            )? {
                0 => Err(StoreError::NotFound("No workspace with that id".into())),
                _ => Ok(()),
            }
        })
        .await
}

/// Points a workspace at its folder's new location ("Locate…").
pub async fn locate(store: &Store, id: &WorkspaceId, new_root: String) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            let taken: Option<String> = t
                .query_row(
                    "SELECT id FROM workspaces WHERE root = ?1 AND id != ?2 AND removed_at IS NULL
                     AND host_id = (SELECT host_id FROM workspaces WHERE id = ?2)",
                    params![new_root, id],
                    |r| r.get(0),
                )
                .optional()?;
            if taken.is_some() {
                return Err(StoreError::Conflict(
                    "Another workspace already uses that folder".into(),
                ));
            }
            match t.execute(
                "UPDATE workspaces SET root = ?2 WHERE id = ?1",
                params![id, new_root],
            )? {
                0 => Err(StoreError::NotFound("No workspace with that id".into())),
                _ => Ok(()),
            }
        })
        .await
}

pub async fn set_flags(
    store: &Store,
    id: &WorkspaceId,
    collapsed: Option<bool>,
    sessions_collapsed: Option<bool>,
) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            if let Some(c) = collapsed {
                t.execute(
                    "UPDATE workspaces SET collapsed = ?2 WHERE id = ?1",
                    params![id, c],
                )?;
            }
            if let Some(c) = sessions_collapsed {
                t.execute(
                    "UPDATE workspaces SET sessions_collapsed = ?2 WHERE id = ?1",
                    params![id, c],
                )?;
            }
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::DataDir;

    pub(crate) fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, store)
    }

    fn local(root: &str) -> NewWorkspace {
        NewWorkspace {
            kind: "local".into(),
            host_id: "local".into(),
            root: root.into(),
            name: root.rsplit('/').next().unwrap_or(root).into(),
            wsl_distro: None,
        }
    }

    #[tokio::test]
    async fn f_cd_01_1_adding_the_same_path_twice_is_one_workspace() {
        let (_d, store) = store();
        let (a, how) = add(&store, local("/a/b"), None).await.expect("add");
        assert_eq!(how, Added::Created);
        let (b, how) = add(&store, local("/a/b"), None).await.expect("add again");
        assert_eq!(how, Added::Existing);
        assert_eq!(a.id, b.id);
        assert_eq!(list(&store).await.expect("list").len(), 1);
    }

    #[tokio::test]
    async fn f_bl_04_a_workspace_limit_refuses_the_next_one() {
        let (_d, store) = store();
        for root in ["/w1", "/w2", "/w3"] {
            add(&store, local(root), Some(3))
                .await
                .expect("within limit");
        }
        let err = add(&store, local("/w4"), Some(3))
            .await
            .expect_err("4th is refused");
        assert!(matches!(err, StoreError::LimitReached(_)));
        // An existing one can still be focused at the limit.
        add(&store, local("/w2"), Some(3))
            .await
            .expect("existing is fine");
        // Pro (no limit) can add the 4th.
        add(&store, local("/w4"), None).await.expect("unlimited");
        let names: Vec<String> = list(&store)
            .await
            .expect("list")
            .into_iter()
            .map(|w| w.name)
            .collect();
        assert_eq!(
            names,
            vec!["w1", "w2", "w3", "w4"],
            "rail order is insertion order"
        );
    }

    async fn names(store: &Store) -> Vec<(String, bool)> {
        list(store)
            .await
            .expect("list")
            .into_iter()
            .map(|w| (w.name, w.pinned))
            .collect()
    }

    #[tokio::test]
    async fn f_sh_03_2_reorder_and_pin_touch_only_the_moved_row() {
        let (_d, store) = store();
        let mut ids = Vec::new();
        for root in ["/a", "/b", "/c", "/d"] {
            ids.push(add(&store, local(root), None).await.expect("add").0.id);
        }
        // d after a.
        move_to(&store, &ids[3], Some(ids[0].clone()), false)
            .await
            .expect("move");
        assert_eq!(
            names(&store).await,
            vec![
                ("a".into(), false),
                ("d".into(), false),
                ("b".into(), false),
                ("c".into(), false)
            ]
        );
        // c above the band: pinned, first.
        move_to(&store, &ids[2], None, true).await.expect("pin");
        // b pinned after c.
        move_to(&store, &ids[1], Some(ids[2].clone()), true)
            .await
            .expect("pin");
        assert_eq!(
            names(&store).await,
            vec![
                ("c".into(), true),
                ("b".into(), true),
                ("a".into(), false),
                ("d".into(), false)
            ]
        );
        // Unpin c to the top of All.
        move_to(&store, &ids[2], None, false).await.expect("unpin");
        assert_eq!(
            names(&store).await,
            vec![
                ("b".into(), true),
                ("c".into(), false),
                ("a".into(), false),
                ("d".into(), false)
            ]
        );
        assert!(matches!(
            move_to(&store, &ids[0], Some(WorkspaceId::new()), false).await,
            Err(StoreError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn f_sh_03_locate_moves_the_root_and_refuses_duplicates() {
        let (_d, store) = store();
        let (a, _) = add(&store, local("/old"), None).await.expect("add");
        add(&store, local("/taken"), None).await.expect("add");
        locate(&store, &a.id, "/new".into()).await.expect("locate");
        assert_eq!(get(&store, &a.id).await.expect("get").root, "/new");
        assert!(matches!(
            locate(&store, &a.id, "/taken".into()).await,
            Err(StoreError::Conflict(_))
        ));
        set_flags(&store, &a.id, Some(true), None)
            .await
            .expect("flags");
        assert!(get(&store, &a.id).await.expect("get").collapsed);
    }

    fn lane(parent: &WorkspaceId, slug: &str) -> NewLane {
        NewLane {
            parent_id: parent.clone(),
            root: format!("/repo/.tandem/worktrees/{slug}"),
            name: slug.into(),
            branch: format!("tandem/{slug}"),
            base: "0123abc".into(),
        }
    }

    #[tokio::test]
    async fn f_cd_15_4_worktrees_nest_under_their_parent_outside_the_limit() {
        let (_d, store) = store();
        let (repo, _) = add(&store, local("/repo"), Some(1)).await.expect("add");
        let a = add_lane(&store, lane(&repo.id, "fix-auth"))
            .await
            .expect("lane");
        let b = add_lane(&store, lane(&repo.id, "fix-auth-2"))
            .await
            .expect("lane 2");
        assert_eq!(a.parent_id.as_ref(), Some(&repo.id));
        assert_eq!(a.lane_branch.as_deref(), Some("tandem/fix-auth"));
        assert_eq!(a.lane_base.as_deref(), Some("0123abc"));
        let got = get(&store, &b.id).await.expect("get");
        assert_eq!(got.lane_base.as_deref(), Some("0123abc"));
        assert_eq!(
            lanes(&store, &repo.id)
                .await
                .expect("lanes")
                .into_iter()
                .map(|w| w.name)
                .collect::<Vec<_>>(),
            vec!["fix-auth", "fix-auth-2"],
            "worktrees list in creation order"
        );
        // A workspace limit (1 here) counts top-level workspaces only.
        assert!(matches!(
            add(&store, local("/other"), Some(1)).await,
            Err(StoreError::LimitReached(_))
        ));
        // A worktree can't hold worktrees of its own.
        assert!(matches!(
            add_lane(&store, lane(&a.id, "nested")).await,
            Err(StoreError::Invalid(_))
        ));
        // Removing the repository removes its worktrees' rows too.
        remove(&store, &repo.id).await.expect("remove");
        assert!(list(&store).await.expect("list").is_empty());
    }

    #[tokio::test]
    async fn f_cd_15_a_removed_worktree_folder_can_be_registered_again() {
        let (_d, store) = store();
        let (repo, _) = add(&store, local("/repo"), None).await.expect("add");
        let first = add_lane(&store, lane(&repo.id, "fix-auth"))
            .await
            .expect("lane");
        assert!(matches!(
            add_lane(&store, lane(&repo.id, "fix-auth")).await,
            Err(StoreError::Conflict(_))
        ));
        remove(&store, &first.id).await.expect("remove");
        assert!(lanes(&store, &repo.id).await.expect("lanes").is_empty());
        let again = add_lane(
            &store,
            NewLane {
                base: "fed9876".into(),
                ..lane(&repo.id, "fix-auth")
            },
        )
        .await
        .expect("again");
        assert_eq!(again.id, first.id, "the old row is reused");
        assert_eq!(
            get(&store, &again.id)
                .await
                .expect("get")
                .lane_base
                .as_deref(),
            Some("fed9876")
        );
    }

    #[tokio::test]
    async fn remove_is_soft_and_re_adding_restores() {
        let (_d, store) = store();
        let (ws, _) = add(&store, local("/proj"), None).await.expect("add");
        rename(&store, &ws.id, "Project".into())
            .await
            .expect("rename");
        remove(&store, &ws.id).await.expect("remove");
        assert!(list(&store).await.expect("list").is_empty());
        let (back, how) = add(&store, local("/proj"), Some(3)).await.expect("re-add");
        assert_eq!(how, Added::Restored);
        assert_eq!(back.id, ws.id);
        assert_eq!(
            back.name, "Project",
            "history (and the custom name) come back"
        );
        assert!(matches!(
            rename(&store, &ws.id, " ".into()).await,
            Err(StoreError::Invalid(_))
        ));
    }
}
