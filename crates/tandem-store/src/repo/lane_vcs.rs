//! Worktrees made by jj rather than git (F-CD-24): a jj workspace's
//! `workspaces` row carries its jj workspace name as `lane_branch`, and a row
//! here says so.

use std::collections::HashSet;

use rusqlite::params;
use tandem_core::ids::WorkspaceId;

use crate::repo::workspaces::{NewLane, Workspace};
use crate::{Store, StoreError};

pub const JJ: &str = "jj";

/// A jj workspace's row, marked as one, in one transaction.
pub async fn add_jj_lane(store: &Store, lane: NewLane) -> Result<Workspace, StoreError> {
    store
        .write(move |t| {
            let ws = crate::repo::workspaces::add_lane_in(t, lane)?;
            t.execute(
                "INSERT OR REPLACE INTO lane_vcs (lane_id, vcs) VALUES (?1, ?2)",
                params![ws.id.as_str(), JJ],
            )?;
            Ok(ws)
        })
        .await
}

/// Whether a worktree is a jj workspace.
pub async fn is_jj(store: &Store, lane: &WorkspaceId) -> Result<bool, StoreError> {
    let lane = lane.as_str().to_string();
    store
        .read(move |c| {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM lane_vcs WHERE lane_id = ?1 AND vcs = ?2",
                params![lane, JJ],
                |r| r.get(0),
            )?;
            Ok(n > 0)
        })
        .await
}

/// Every jj workspace's row id (removed rows included: callers filter).
pub async fn jj_lanes(store: &Store) -> Result<HashSet<WorkspaceId>, StoreError> {
    store
        .read(move |c| {
            let mut stmt = c.prepare("SELECT lane_id FROM lane_vcs WHERE vcs = ?1")?;
            let rows = stmt
                .query_map([JJ], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows.into_iter().map(WorkspaceId::from_db).collect())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::workspaces::{self, NewWorkspace};
    use tandem_core::DataDir;

    #[tokio::test]
    async fn f_cd_24_a_jj_workspace_is_marked_and_a_reused_folder_loses_the_mark() {
        let tmp = tempfile::tempdir().expect("tmp");
        let data = DataDir::at(tmp.path().join("data")).expect("data");
        let store = Store::open(&data).expect("open");
        let (repo, _) = workspaces::add(
            &store,
            NewWorkspace {
                kind: "local".into(),
                host_id: "local".into(),
                root: "/r".into(),
                name: "r".into(),
                wsl_distro: None,
            },
            None,
        )
        .await
        .expect("repo");
        let lane = |name: &str| NewLane {
            parent_id: repo.id.clone(),
            root: format!("/r/.tandem/worktrees/{name}"),
            name: name.into(),
            branch: name.into(),
            base: "abc".into(),
        };
        let jj = add_jj_lane(&store, lane("a")).await.expect("jj lane");
        let git = workspaces::add_lane(&store, lane("b"))
            .await
            .expect("git lane");
        assert!(is_jj(&store, &jj.id).await.expect("is"));
        assert!(!is_jj(&store, &git.id).await.expect("is"));
        assert_eq!(
            jj_lanes(&store).await.expect("all"),
            HashSet::from([jj.id.clone()])
        );
        // Removed, then the folder made again as a git worktree.
        workspaces::remove(&store, &jj.id).await.expect("remove");
        let again = workspaces::add_lane(&store, lane("a"))
            .await
            .expect("again");
        assert_eq!(again.id, jj.id);
        assert!(!is_jj(&store, &again.id).await.expect("is"));
    }
}
