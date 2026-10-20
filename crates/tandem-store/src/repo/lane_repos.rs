//! A multi-repo worktree's repositories (F-CD-22): its `workspaces` row is
//! the parent folder, and each repository's git worktree is a row here.

use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::WorkspaceId;
use ts_rs::TS;

use crate::{Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LaneRepo {
    /// The repository's folder name (`api`).
    pub name: String,
    /// The repository's own checkout, `<folder>/<name>`.
    pub repo_root: String,
    /// Its worktree, `<parent folder>/<name>`.
    pub root: String,
    /// The commit its worktree started from.
    pub base: String,
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<LaneRepo> {
    Ok(LaneRepo {
        name: r.get(0)?,
        repo_root: r.get(1)?,
        root: r.get(2)?,
        base: r.get(3)?,
    })
}

/// Records a worktree's repositories, replacing any it had.
pub async fn set(
    store: &Store,
    lane: &WorkspaceId,
    repos: Vec<LaneRepo>,
) -> Result<(), StoreError> {
    let lane = lane.as_str().to_string();
    store
        .write(move |t| {
            t.execute("DELETE FROM lane_repos WHERE lane_id = ?1", [lane.as_str()])?;
            for r in &repos {
                t.execute(
                    "INSERT INTO lane_repos (lane_id, name, repo_root, root, base) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![lane, r.name, r.repo_root, r.root, r.base],
                )?;
            }
            Ok(())
        })
        .await
}

/// A worktree's repositories by name; empty for an ordinary worktree.
pub async fn list(store: &Store, lane: &WorkspaceId) -> Result<Vec<LaneRepo>, StoreError> {
    let lane = lane.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT name, repo_root, root, base FROM lane_repos WHERE lane_id = ?1 ORDER BY name",
            )?;
            let rows = stmt.query_map([lane], from_row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::workspaces::{self, NewLane, NewWorkspace};
    use tandem_core::DataDir;

    #[tokio::test]
    async fn f_cd_22_a_worktree_keeps_its_repositories_and_loses_them_with_it() {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(tmp.path().join("data")).expect("data");
        let store = Store::open(&data).expect("open");
        let (parent, _) = workspaces::add(
            &store,
            NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: "/code/shop".into(),
                name: "shop".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("parent");
        let lane = workspaces::add_lane(
            &store,
            NewLane {
                parent_id: parent.id.clone(),
                root: "/code/shop/.tandem/worktrees/fix".into(),
                name: "fix".into(),
                branch: "tandem/fix".into(),
                base: String::new(),
            },
        )
        .await
        .expect("lane");
        let repo = |name: &str| LaneRepo {
            name: name.into(),
            repo_root: format!("/code/shop/{name}"),
            root: format!("/code/shop/.tandem/worktrees/fix/{name}"),
            base: "abc".into(),
        };
        set(&store, &lane.id, vec![repo("web"), repo("api")])
            .await
            .expect("set");
        let got = list(&store, &lane.id).await.expect("list");
        assert_eq!(
            got.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["api", "web"]
        );
        // An ordinary worktree has none.
        assert!(list(&store, &parent.id).await.expect("list").is_empty());
        // Deleting the worktree's row deletes its repositories.
        let id = lane.id.as_str().to_string();
        store
            .write(move |t| {
                t.execute("DELETE FROM workspaces WHERE id = ?1", [id])?;
                Ok(())
            })
            .await
            .expect("delete");
        assert!(list(&store, &lane.id).await.expect("list").is_empty());
    }
}
