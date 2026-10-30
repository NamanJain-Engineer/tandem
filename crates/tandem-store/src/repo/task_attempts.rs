//! `task_attempts` (F-TK-04, ARCHITECTURE §9.4): each run of an issue: its
//! lane, its session, its state. The issue records themselves are files
//! (`tandem-tasks`); this is the runtime side.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::access::Access;
use tandem_core::ids::{AccountId, AttemptId, TeammateId, ThreadId, WorkspaceId};
use ts_rs::TS;

use crate::{Store, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AttemptState {
    Queued,
    Setup,
    Running,
    Reviewing,
    InReview,
    Stopped,
    Failed,
    Done,
}

impl AttemptState {
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptState::Queued => "queued",
            AttemptState::Setup => "setup",
            AttemptState::Running => "running",
            AttemptState::Reviewing => "reviewing",
            AttemptState::InReview => "in_review",
            AttemptState::Stopped => "stopped",
            AttemptState::Failed => "failed",
            AttemptState::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "queued" => AttemptState::Queued,
            "setup" => AttemptState::Setup,
            "running" => AttemptState::Running,
            "reviewing" => AttemptState::Reviewing,
            "in_review" => AttemptState::InReview,
            "stopped" => AttemptState::Stopped,
            "done" => AttemptState::Done,
            _ => AttemptState::Failed,
        }
    }

    /// Holds one of the 4 slots.
    pub fn active(self) -> bool {
        matches!(
            self,
            AttemptState::Setup | AttemptState::Running | AttemptState::Reviewing
        )
    }

    pub fn ended(self) -> bool {
        matches!(
            self,
            AttemptState::InReview
                | AttemptState::Stopped
                | AttemptState::Failed
                | AttemptState::Done
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Attempt {
    pub id: AttemptId,
    pub issue_id: String,
    pub workspace_id: WorkspaceId,
    pub lane_workspace_id: Option<WorkspaceId>,
    pub thread_id: Option<ThreadId>,
    pub teammate_id: Option<TeammateId>,
    pub engine_id: String,
    pub account_id: Option<AccountId>,
    pub access: Access,
    pub review: bool,
    pub state: AttemptState,
    pub queued_at: i64,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub review_verdict: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewAttempt {
    pub id: AttemptId,
    pub issue_id: String,
    pub workspace_id: WorkspaceId,
    pub teammate_id: Option<TeammateId>,
    pub engine_id: String,
    pub account_id: Option<AccountId>,
    pub access: Access,
    pub review: bool,
}

const COLS: &str = "id, issue_id, workspace_id, lane_workspace_id, thread_id, teammate_id, engine_id, account_id, access, review, state, queued_at, started_at, ended_at, review_verdict, error";

fn row(r: &Row<'_>) -> rusqlite::Result<Attempt> {
    let access: String = r.get(8)?;
    let state: String = r.get(10)?;
    Ok(Attempt {
        id: AttemptId::from_db(r.get(0)?),
        issue_id: r.get(1)?,
        workspace_id: WorkspaceId::from_db(r.get(2)?),
        lane_workspace_id: r.get::<_, Option<String>>(3)?.map(WorkspaceId::from_db),
        thread_id: r.get::<_, Option<String>>(4)?.map(ThreadId::from_db),
        teammate_id: r.get::<_, Option<String>>(5)?.map(TeammateId::from_db),
        engine_id: r.get(6)?,
        account_id: r.get::<_, Option<String>>(7)?.map(AccountId::from_db),
        access: Access::parse(&access).unwrap_or(Access::Edits),
        review: r.get::<_, i64>(9)? != 0,
        state: AttemptState::parse(&state),
        queued_at: r.get(11)?,
        started_at: r.get(12)?,
        ended_at: r.get(13)?,
        review_verdict: r.get(14)?,
        error: r.get(15)?,
    })
}

pub async fn insert(store: &Store, new: NewAttempt, now: i64) -> Result<Attempt, StoreError> {
    let id = new.id.clone();
    store
        .write(move |tx| {
            tx.execute(
                "INSERT INTO task_attempts (id, issue_id, workspace_id, teammate_id, engine_id, account_id, access, review, state, queued_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued', ?9)",
                params![
                    new.id.as_str(),
                    new.issue_id,
                    new.workspace_id.as_str(),
                    new.teammate_id.as_ref().map(|t| t.as_str().to_string()),
                    new.engine_id,
                    new.account_id.as_ref().map(|a| a.as_str().to_string()),
                    new.access.as_str(),
                    new.review as i64,
                    now,
                ],
            )?;
            Ok(())
        })
        .await?;
    get(store, &id).await
}

pub async fn get(store: &Store, id: &AttemptId) -> Result<Attempt, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            c.query_row(
                &format!("SELECT {COLS} FROM task_attempts WHERE id = ?1"),
                [&id],
                row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound("That run doesn't exist".into()))
        })
        .await
}

/// Every attempt, oldest queued first (the queue's order).
pub async fn list(store: &Store) -> Result<Vec<Attempt>, StoreError> {
    store
        .read(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {COLS} FROM task_attempts ORDER BY queued_at, rowid"
            ))?;
            let out = st.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(out)
        })
        .await
}

pub async fn for_issue(store: &Store, issue: &str) -> Result<Vec<Attempt>, StoreError> {
    let issue = issue.to_string();
    store
        .read(move |c| {
            let mut st = c.prepare(&format!(
                "SELECT {COLS} FROM task_attempts WHERE issue_id = ?1 ORDER BY queued_at, rowid"
            ))?;
            let out = st.query_map([issue], row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(out)
        })
        .await
}

/// The attempt a session's thread belongs to (its review session too).
pub async fn for_thread(store: &Store, thread: &ThreadId) -> Result<Option<Attempt>, StoreError> {
    let thread = thread.as_str().to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLS} FROM task_attempts WHERE thread_id = ?1"),
                [thread],
                row,
            )
            .optional()?)
        })
        .await
}

/// Fields to change; `None` leaves one alone.
#[derive(Debug, Clone, Default)]
pub struct AttemptPatch {
    pub state: Option<AttemptState>,
    pub lane_workspace_id: Option<WorkspaceId>,
    pub thread_id: Option<ThreadId>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    /// `Some(None)` clears it: a verdict left from an earlier turn read as
    /// this one's (the review of 0e20d5b7, SF-6).
    pub review_verdict: Option<Option<String>>,
    pub error: Option<Option<String>>,
}

pub async fn update(store: &Store, id: &AttemptId, p: AttemptPatch) -> Result<Attempt, StoreError> {
    let key = id.as_str().to_string();
    store
        .write(move |tx| {
            let (set_error, error) = match p.error {
                Some(e) => (1, e),
                None => (0, None),
            };
            let (set_verdict, verdict) = match p.review_verdict {
                Some(v) => (1, v),
                None => (0, None),
            };
            tx.execute(
                "UPDATE task_attempts SET
                   state = COALESCE(?2, state),
                   lane_workspace_id = COALESCE(?3, lane_workspace_id),
                   thread_id = COALESCE(?4, thread_id),
                   started_at = COALESCE(?5, started_at),
                   ended_at = COALESCE(?6, ended_at),
                   review_verdict = CASE WHEN ?7 = 1 THEN ?10 ELSE review_verdict END,
                   error = CASE WHEN ?8 = 1 THEN ?9 ELSE error END
                 WHERE id = ?1",
                params![
                    key,
                    p.state.map(AttemptState::as_str),
                    p.lane_workspace_id.as_ref().map(|w| w.as_str().to_string()),
                    p.thread_id.as_ref().map(|t| t.as_str().to_string()),
                    p.started_at,
                    p.ended_at,
                    set_verdict,
                    set_error,
                    error,
                    verdict,
                ],
            )?;
            Ok(())
        })
        .await?;
    get(store, id).await
}

/// Attempts a process left mid-way (the app quit): stopped, not resumed.
pub async fn settle_interrupted(store: &Store, now: i64) -> Result<usize, StoreError> {
    store
        .write(move |tx| {
            Ok(tx.execute(
                "UPDATE task_attempts SET state = 'stopped', ended_at = ?1,
                   error = COALESCE(error, 'Vibezeo closed during the run.')
                 WHERE state IN ('queued', 'setup', 'running', 'reviewing')",
                [now],
            )?)
        })
        .await
}
