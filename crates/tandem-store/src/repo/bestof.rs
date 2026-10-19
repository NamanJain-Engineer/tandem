//! `bestof_runs`, `bestof_lanes` (F-EX-04): a Best of N run, its worktree
//! lanes and their check results, kept until the run is merged or discarded.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Store, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RunState {
    Running,
    Merged,
    Discarded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CheckState {
    None,
    /// Waiting for its turn ("one check at a time", F-EX-06).
    Queued,
    Running,
    Passed,
    Failed,
    Stopped,
}

impl RunState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Merged => "merged",
            Self::Discarded => "discarded",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "merged" => Self::Merged,
            "discarded" => Self::Discarded,
            _ => Self::Running,
        }
    }
}

impl CheckState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "passed" => Self::Passed,
            "failed" => Self::Failed,
            "stopped" => Self::Stopped,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BestOfLane {
    pub seat: u32,
    /// The worktree's workspace; `None` when it failed to start.
    pub lane_id: Option<String>,
    pub branch: Option<String>,
    pub engine_id: String,
    pub account_id: Option<String>,
    pub pane_id: Option<String>,
    /// Why the seat didn't start.
    pub error: Option<String>,
    pub check: CheckState,
    pub check_exit: Option<i32>,
    /// The check's last 200 lines, redacted.
    pub check_output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BestOfRun {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub tab_id: Option<String>,
    /// The commit every lane started from.
    pub base: String,
    pub check_command: Option<String>,
    pub state: RunState,
    #[ts(type = "number")]
    pub created_at: i64,
    pub lanes: Vec<BestOfLane>,
}

pub async fn insert(store: &Store, run: BestOfRun) -> Result<BestOfRun, StoreError> {
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO bestof_runs (id, workspace_id, title, tab_id, base, check_command, state, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![run.id, run.workspace_id, run.title, run.tab_id, run.base, run.check_command, run.state.as_str(), run.created_at],
            )?;
            for l in &run.lanes {
                t.execute(
                    "INSERT INTO bestof_lanes (run_id, seat, lane_id, branch, engine_id, account_id, pane_id, error, check_state, check_exit, check_output) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    params![run.id, l.seat, l.lane_id, l.branch, l.engine_id, l.account_id, l.pane_id, l.error, l.check.as_str(), l.check_exit, l.check_output],
                )?;
            }
            Ok(run)
        })
        .await
}

fn lanes_of(c: &rusqlite::Connection, run: &str) -> Result<Vec<BestOfLane>, StoreError> {
    let mut st = c.prepare(
        "SELECT seat, lane_id, branch, engine_id, account_id, pane_id, error, check_state, check_exit, check_output FROM bestof_lanes WHERE run_id = ?1 ORDER BY seat",
    )?;
    let rows = st.query_map([run], |r| {
        Ok(BestOfLane {
            seat: r.get(0)?,
            lane_id: r.get(1)?,
            branch: r.get(2)?,
            engine_id: r.get(3)?,
            account_id: r.get(4)?,
            pane_id: r.get(5)?,
            error: r.get(6)?,
            check: CheckState::parse(&r.get::<_, String>(7)?),
            check_exit: r.get(8)?,
            check_output: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn run_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<BestOfRun> {
    Ok(BestOfRun {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        title: r.get(2)?,
        tab_id: r.get(3)?,
        base: r.get(4)?,
        check_command: r.get(5)?,
        state: RunState::parse(&r.get::<_, String>(6)?),
        created_at: r.get(7)?,
        lanes: Vec::new(),
    })
}

const RUN_COLS: &str = "id, workspace_id, title, tab_id, base, check_command, state, created_at";

pub async fn get(store: &Store, id: String) -> Result<Option<BestOfRun>, StoreError> {
    store
        .read(move |c| {
            let Some(mut run) = c
                .query_row(
                    &format!("SELECT {RUN_COLS} FROM bestof_runs WHERE id = ?1"),
                    [&id],
                    run_row,
                )
                .optional()?
            else {
                return Ok(None);
            };
            run.lanes = lanes_of(c, &run.id)?;
            Ok(Some(run))
        })
        .await
}

/// Newest first; `workspace_id` narrows to one workspace.
pub async fn list(
    store: &Store,
    workspace_id: Option<String>,
) -> Result<Vec<BestOfRun>, StoreError> {
    store
        .read(move |c| {
            let mut st = c.prepare(&format!(
                "SELECT {RUN_COLS} FROM bestof_runs WHERE (?1 IS NULL OR workspace_id = ?1) ORDER BY created_at DESC, id DESC"
            ))?;
            let runs = st.query_map([&workspace_id], run_row)?.collect::<Result<Vec<_>, _>>()?;
            let mut out = Vec::with_capacity(runs.len());
            for mut run in runs {
                run.lanes = lanes_of(c, &run.id)?;
                out.push(run);
            }
            Ok(out)
        })
        .await
}

pub async fn set_state(store: &Store, id: String, state: RunState) -> Result<(), StoreError> {
    store
        .write(move |t| {
            t.execute(
                "UPDATE bestof_runs SET state = ?2 WHERE id = ?1",
                params![id, state.as_str()],
            )?;
            Ok(())
        })
        .await
}

pub async fn set_check(
    store: &Store,
    run: String,
    seat: u32,
    check: CheckState,
    exit: Option<i32>,
    output: Option<String>,
) -> Result<(), StoreError> {
    store
        .write(move |t| {
            t.execute(
                "UPDATE bestof_lanes SET check_state = ?3, check_exit = ?4, check_output = ?5 WHERE run_id = ?1 AND seat = ?2",
                params![run, seat, check.as_str(), exit, output],
            )?;
            Ok(())
        })
        .await
}

/// A seat that started on Retry.
pub async fn set_started(
    store: &Store,
    run: String,
    seat: u32,
    lane_id: String,
    branch: Option<String>,
    pane_id: String,
) -> Result<(), StoreError> {
    store
        .write(move |t| {
            t.execute(
                "UPDATE bestof_lanes SET lane_id = ?3, branch = ?4, pane_id = ?5, error = NULL WHERE run_id = ?1 AND seat = ?2",
                params![run, seat, lane_id, branch, pane_id],
            )?;
            Ok(())
        })
        .await
}

/// A check left running when the app quit is stopped.
pub async fn settle_checks(store: &Store) -> Result<(), StoreError> {
    store
        .write(|t| {
            t.execute(
                "UPDATE bestof_lanes SET check_state = 'stopped' WHERE check_state = 'running'",
                [],
            )?;
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::temp_store;

    fn run(id: &str, ws: &str, at: i64) -> BestOfRun {
        BestOfRun {
            id: id.into(),
            workspace_id: ws.into(),
            title: "Fix login".into(),
            tab_id: None,
            base: "abc".into(),
            check_command: Some("pnpm test".into()),
            state: RunState::Running,
            created_at: at,
            lanes: (0..3)
                .map(|seat| BestOfLane {
                    seat,
                    lane_id: (seat < 2).then(|| format!("ws_l{seat}")),
                    branch: (seat < 2).then(|| format!("tandem/fix-login-{}", seat + 1)),
                    engine_id: "claude".into(),
                    account_id: None,
                    pane_id: None,
                    error: (seat == 2).then(|| "Couldn't create the worktree".into()),
                    check: CheckState::None,
                    check_exit: None,
                    check_output: None,
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn f_ex_04_runs_round_trip_with_their_lanes_and_checks() {
        let (_d, store) = temp_store().await;
        insert(&store, run("bo_1", "ws_a", 1))
            .await
            .expect("insert");
        insert(&store, run("bo_2", "ws_b", 2))
            .await
            .expect("insert");
        let got = get(&store, "bo_1".into()).await.expect("get").expect("run");
        assert_eq!(got.lanes.len(), 3);
        assert_eq!(
            got.lanes[2].error.as_deref(),
            Some("Couldn't create the worktree")
        );
        set_check(
            &store,
            "bo_1".into(),
            1,
            CheckState::Failed,
            Some(1),
            Some("1 failing".into()),
        )
        .await
        .expect("check");
        set_state(&store, "bo_1".into(), RunState::Merged)
            .await
            .expect("state");
        let got = get(&store, "bo_1".into()).await.expect("get").expect("run");
        assert_eq!(
            (got.state, got.lanes[1].check, got.lanes[1].check_exit),
            (RunState::Merged, CheckState::Failed, Some(1))
        );
        assert_eq!(
            list(&store, None)
                .await
                .expect("list")
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            ["bo_2", "bo_1"]
        );
        assert_eq!(
            list(&store, Some("ws_a".into())).await.expect("list").len(),
            1
        );
        set_check(&store, "bo_2".into(), 0, CheckState::Running, None, None)
            .await
            .expect("check");
        settle_checks(&store).await.expect("settle");
        assert_eq!(
            get(&store, "bo_2".into())
                .await
                .expect("get")
                .expect("run")
                .lanes[0]
                .check,
            CheckState::Stopped
        );
    }
}
