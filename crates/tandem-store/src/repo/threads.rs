//! Threads, their turns and their event log (F-TH-01, F-TH-03): `threads`,
//! `thread_turns` and `thread_messages`. Events are stored as the JSON the
//! threads service serialized (`payload_json`) under their tag (`type`).

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::access::{Access, ThreadMode};
use tandem_core::ids::{AccountId, ThreadId, TurnId, WorkspaceId};
use ts_rs::TS;

use crate::{now_ms, sort_key, Store, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ThreadKind {
    Thread,
    TeammateChat,
}

/// The rail's status for a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ThreadState {
    Idle,
    Working,
    Waiting,
    Done,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "Thread")]
pub struct ThreadRecord {
    pub id: ThreadId,
    pub kind: ThreadKind,
    /// None: "No folder" (F-TH-02).
    pub workspace_id: Option<WorkspaceId>,
    pub cwd: String,
    pub engine_id: String,
    pub account_id: Option<AccountId>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub access: Access,
    pub mode: ThreadMode,
    pub title: String,
    pub state: ThreadState,
    pub attended: bool,
    pub engine_session_id: Option<String>,
    pub pinned: bool,
    pub archived: bool,
    pub unread: bool,
    pub sort_key: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewThread {
    /// Chosen by the caller: a folderless thread's cwd is named after it.
    pub id: ThreadId,
    pub kind: ThreadKind,
    pub workspace_id: Option<WorkspaceId>,
    pub cwd: String,
    pub engine_id: String,
    pub account_id: Option<AccountId>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub access: Access,
    pub mode: ThreadMode,
    pub attended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TurnState {
    Running,
    Completed,
    Cancelled,
    Limit,
    Error,
    /// Running when the app went away.
    Interrupted,
}

impl TurnState {
    fn as_str(self) -> &'static str {
        match self {
            TurnState::Running => "running",
            TurnState::Completed => "completed",
            TurnState::Cancelled => "cancelled",
            TurnState::Limit => "limit",
            TurnState::Error => "error",
            TurnState::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "Turn")]
pub struct TurnRecord {
    pub id: TurnId,
    pub thread_id: ThreadId,
    pub n: u32,
    pub engine_id: String,
    pub account_id: Option<AccountId>,
    pub state: TurnState,
    pub blocked_actions: u32,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

/// One stored event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRow {
    pub id: i64,
    pub thread_id: ThreadId,
    pub turn_id: TurnId,
    pub seq: u64,
    pub kind: String,
    pub payload_json: String,
    pub created_at: i64,
}

fn snake<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|j| j.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn parse<T: for<'de> Deserialize<'de>>(s: &str) -> Result<T, rusqlite::Error> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

const COLUMNS: &str = "id, kind, workspace_id, cwd, engine_id, account_id, model, effort, access, mode, title, state, attended, engine_session_id, pinned, archived, unread, sort_key, created_at, updated_at";

fn from_row(r: &Row) -> Result<ThreadRecord, rusqlite::Error> {
    Ok(ThreadRecord {
        id: ThreadId::from_db(r.get(0)?),
        kind: parse(&r.get::<_, String>(1)?)?,
        workspace_id: r.get::<_, Option<String>>(2)?.map(WorkspaceId::from_db),
        cwd: r.get(3)?,
        engine_id: r.get(4)?,
        account_id: r.get::<_, Option<String>>(5)?.map(AccountId::from_db),
        model: r.get(6)?,
        effort: r.get(7)?,
        access: parse(&r.get::<_, String>(8)?)?,
        mode: parse(&r.get::<_, String>(9)?)?,
        title: r.get(10)?,
        state: parse(&r.get::<_, String>(11)?)?,
        attended: r.get(12)?,
        engine_session_id: r.get(13)?,
        pinned: r.get(14)?,
        archived: r.get(15)?,
        unread: r.get(16)?,
        sort_key: r.get(17)?,
        created_at: r.get(18)?,
        updated_at: r.get(19)?,
    })
}

const TURN_COLUMNS: &str =
    "id, thread_id, n, engine_id, account_id, state, blocked_actions, started_at, ended_at";

fn turn_from_row(r: &Row) -> Result<TurnRecord, rusqlite::Error> {
    Ok(TurnRecord {
        id: TurnId::from_db(r.get(0)?),
        thread_id: ThreadId::from_db(r.get(1)?),
        n: r.get(2)?,
        engine_id: r.get(3)?,
        account_id: r.get::<_, Option<String>>(4)?.map(AccountId::from_db),
        state: parse(&r.get::<_, String>(5)?)?,
        blocked_actions: r.get(6)?,
        started_at: r.get(7)?,
        ended_at: r.get(8)?,
    })
}

fn message_from_row(r: &Row) -> Result<MessageRow, rusqlite::Error> {
    Ok(MessageRow {
        id: r.get(0)?,
        thread_id: ThreadId::from_db(r.get(1)?),
        turn_id: TurnId::from_db(r.get(2)?),
        seq: r.get::<_, i64>(3)?.max(0) as u64,
        kind: r.get(4)?,
        payload_json: r.get(5)?,
        created_at: r.get(6)?,
    })
}

/// A new thread, at the top of the rail.
pub async fn insert(store: &Store, new: NewThread) -> Result<ThreadRecord, StoreError> {
    let id = new.id.clone();
    store
        .write(move |t| {
            let first: Option<String> = t.query_row(
                "SELECT MIN(sort_key) FROM threads WHERE deleted_at IS NULL",
                [],
                |r| r.get(0),
            )?;
            let key = sort_key::between(None, first.as_deref());
            let now = now_ms();
            t.execute(
                "INSERT INTO threads (id, kind, workspace_id, cwd, engine_id, account_id, model, effort, access, mode, attended, sort_key, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
                params![
                    id.as_str(),
                    snake(&new.kind),
                    new.workspace_id.as_ref().map(WorkspaceId::as_str),
                    new.cwd,
                    new.engine_id,
                    new.account_id.as_ref().map(AccountId::as_str),
                    new.model,
                    new.effort,
                    new.access.as_str(),
                    new.mode.as_str(),
                    new.attended,
                    key,
                    now
                ],
            )?;
            Ok(t.query_row(
                &format!("SELECT {COLUMNS} FROM threads WHERE id = ?1"),
                [id.as_str()],
                from_row,
            )?)
        })
        .await
}

pub async fn get(store: &Store, id: &ThreadId) -> Result<Option<ThreadRecord>, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM threads WHERE id = ?1 AND deleted_at IS NULL"),
                [id],
                from_row,
            )
            .optional()?)
        })
        .await
}

/// The rail: pinned first, then by sort key.
pub async fn list(
    store: &Store,
    kind: ThreadKind,
    include_archived: bool,
) -> Result<Vec<ThreadRecord>, StoreError> {
    let kind = snake(&kind);
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM threads WHERE kind = ?1 AND deleted_at IS NULL AND (?2 OR archived = 0)
                 ORDER BY pinned DESC, sort_key, created_at"
            ))?;
            let rows = stmt.query_map(params![kind, include_archived], from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

pub async fn set_state(store: &Store, id: &ThreadId, state: ThreadState) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    let state = snake(&state);
    store
        .write(move |t| {
            t.execute(
                "UPDATE threads SET state = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, state, now_ms()],
            )?;
            Ok(())
        })
        .await
}

pub async fn set_engine_session(
    store: &Store,
    id: &ThreadId,
    session: Option<&str>,
) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    let session = session.map(str::to_string);
    store
        .write(move |t| {
            t.execute(
                "UPDATE threads SET engine_session_id = ?2 WHERE id = ?1",
                params![id, session],
            )?;
            Ok(())
        })
        .await
}

/// The composer draft (`Draft` JSON, F-TH-07).
pub async fn draft(store: &Store, id: &ThreadId) -> Result<String, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            Ok(
                c.query_row("SELECT draft_json FROM threads WHERE id = ?1", [id], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?
                .unwrap_or_else(|| "{}".to_string()),
            )
        })
        .await
}

pub async fn set_draft(store: &Store, id: &ThreadId, draft_json: String) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE threads SET draft_json = ?2 WHERE id = ?1",
                params![id, draft_json],
            )?;
            Ok(())
        })
        .await
}

/// "Allow for this thread" rules (`AllowRule` JSON array, F-TH-11).
pub async fn allow_rules(store: &Store, id: &ThreadId) -> Result<String, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT allow_rules_json FROM threads WHERE id = ?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| "[]".to_string()))
        })
        .await
}

pub async fn set_allow_rules(
    store: &Store,
    id: &ThreadId,
    rules_json: String,
) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE threads SET allow_rules_json = ?2 WHERE id = ?1",
                params![id, rules_json],
            )?;
            Ok(())
        })
        .await
}

/// A new running turn, numbered after the last.
pub async fn start_turn(
    store: &Store,
    thread: &ThreadId,
    turn: &TurnId,
    engine_id: &str,
    account_id: Option<&AccountId>,
) -> Result<TurnRecord, StoreError> {
    let (thread, turn) = (thread.as_str().to_string(), turn.as_str().to_string());
    let engine_id = engine_id.to_string();
    let account = account_id.map(|a| a.as_str().to_string());
    store
        .write(move |t| {
            let n: u32 = t.query_row(
                "SELECT COALESCE(MAX(n), 0) + 1 FROM thread_turns WHERE thread_id = ?1",
                [&thread],
                |r| r.get(0),
            )?;
            t.execute(
                "INSERT INTO thread_turns (id, thread_id, n, engine_id, account_id, state, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6)",
                params![turn, thread, n, engine_id, account, now_ms()],
            )?;
            Ok(t.query_row(
                &format!("SELECT {TURN_COLUMNS} FROM thread_turns WHERE id = ?1"),
                [&turn],
                turn_from_row,
            )?)
        })
        .await
}

/// Ends a running turn; a turn that already ended keeps its state.
pub async fn end_turn(
    store: &Store,
    turn: &TurnId,
    state: TurnState,
    blocked_actions: u32,
) -> Result<(), StoreError> {
    let turn = turn.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE thread_turns SET state = ?2, blocked_actions = ?3, ended_at = ?4 WHERE id = ?1 AND state = 'running'",
                params![turn, state.as_str(), blocked_actions, now_ms()],
            )?;
            Ok(())
        })
        .await
}

pub async fn turns(store: &Store, thread: &ThreadId) -> Result<Vec<TurnRecord>, StoreError> {
    let thread = thread.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {TURN_COLUMNS} FROM thread_turns WHERE thread_id = ?1 ORDER BY n"
            ))?;
            let rows = stmt.query_map([thread], turn_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// Appends one event; returns its row id.
pub async fn append(
    store: &Store,
    thread: &ThreadId,
    turn: &TurnId,
    seq: u64,
    kind: &str,
    payload_json: String,
) -> Result<i64, StoreError> {
    let (thread, turn, kind) = (
        thread.as_str().to_string(),
        turn.as_str().to_string(),
        kind.to_string(),
    );
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO thread_messages (thread_id, turn_id, seq, type, payload_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![thread, turn, seq as i64, kind, payload_json, now_ms()],
            )?;
            Ok(t.last_insert_rowid())
        })
        .await
}

/// Rewrites a stored event in place (a partial assistant block becoming
/// the final one).
pub async fn replace(
    store: &Store,
    row_id: i64,
    kind: &str,
    payload_json: String,
) -> Result<(), StoreError> {
    let kind = kind.to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE thread_messages SET type = ?2, payload_json = ?3 WHERE id = ?1",
                params![row_id, kind, payload_json],
            )?;
            Ok(())
        })
        .await
}

/// One turn's events, in order.
pub async fn turn_messages(store: &Store, turn: &TurnId) -> Result<Vec<MessageRow>, StoreError> {
    let turn = turn.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, thread_id, turn_id, seq, type, payload_json, created_at
                 FROM thread_messages WHERE turn_id = ?1 ORDER BY seq, id",
            )?;
            let rows = stmt.query_map([turn], message_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// A page of a thread's events before `before_id` (newest page when None),
/// returned oldest first.
pub async fn page(
    store: &Store,
    thread: &ThreadId,
    before_id: Option<i64>,
    limit: u32,
) -> Result<Vec<MessageRow>, StoreError> {
    let thread = thread.as_str().to_string();
    let limit = limit.clamp(1, 500);
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, thread_id, turn_id, seq, type, payload_json, created_at
                 FROM thread_messages WHERE thread_id = ?1 AND id < ?2 ORDER BY id DESC LIMIT ?3",
            )?;
            let rows = stmt.query_map(
                params![thread, before_id.unwrap_or(i64::MAX), limit],
                message_from_row,
            )?;
            let mut out = rows.collect::<Result<Vec<_>, _>>()?;
            out.reverse();
            Ok(out)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::DataDir;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, store)
    }

    fn new(engine: &str) -> NewThread {
        NewThread {
            id: ThreadId::new(),
            kind: ThreadKind::Thread,
            workspace_id: None,
            cwd: "/tmp/scratch".into(),
            engine_id: engine.into(),
            account_id: None,
            model: None,
            effort: None,
            access: Access::Read,
            mode: ThreadMode::Act,
            attended: true,
        }
    }

    #[tokio::test]
    async fn f_th_01_new_threads_go_to_the_top_and_keep_their_settings() {
        let (_dir, store) = store();
        let a = insert(&store, new("claude")).await.expect("a");
        let b = insert(&store, new("codex")).await.expect("b");
        assert_eq!(a.state, ThreadState::Idle);
        assert_eq!(a.access, Access::Read);
        assert_eq!(a.workspace_id, None);
        let rail = list(&store, ThreadKind::Thread, false).await.expect("list");
        assert_eq!(
            rail.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
            vec![b.id.clone(), a.id.clone()]
        );
        set_state(&store, &a.id, ThreadState::Working)
            .await
            .expect("state");
        set_engine_session(&store, &a.id, Some("sess-1"))
            .await
            .expect("session");
        let a = get(&store, &a.id).await.expect("get").expect("a");
        assert_eq!(a.state, ThreadState::Working);
        assert_eq!(a.engine_session_id.as_deref(), Some("sess-1"));
        assert!(list(&store, ThreadKind::TeammateChat, false)
            .await
            .expect("chats")
            .is_empty());
    }

    #[tokio::test]
    async fn f_th_03_turns_are_numbered_and_their_events_kept_in_order() {
        let (_dir, store) = store();
        let th = insert(&store, new("claude")).await.expect("thread");
        let t1 = TurnId::new();
        let r1 = start_turn(&store, &th.id, &t1, "claude", None)
            .await
            .expect("t1");
        assert_eq!((r1.n, r1.state), (1, TurnState::Running));
        append(&store, &th.id, &t1, 0, "user", r#"{"type":"user"}"#.into())
            .await
            .expect("user");
        let partial = append(
            &store,
            &th.id,
            &t1,
            1,
            "assistant",
            r#"{"partial":true}"#.into(),
        )
        .await
        .expect("partial");
        replace(
            &store,
            partial,
            "assistant",
            r#"{"type":"assistant"}"#.into(),
        )
        .await
        .expect("final");
        end_turn(&store, &t1, TurnState::Completed, 0)
            .await
            .expect("end");
        // A turn that ended keeps its state.
        end_turn(&store, &t1, TurnState::Cancelled, 0)
            .await
            .expect("again");
        let t2 = TurnId::new();
        let r2 = start_turn(&store, &th.id, &t2, "claude", None)
            .await
            .expect("t2");
        assert_eq!(r2.n, 2);
        append(&store, &th.id, &t2, 0, "user", "{}".into())
            .await
            .expect("user 2");

        let ts = turns(&store, &th.id).await.expect("turns");
        assert_eq!(
            ts.iter().map(|t| t.state).collect::<Vec<_>>(),
            vec![TurnState::Completed, TurnState::Running]
        );
        let rows = turn_messages(&store, &t1).await.expect("rows");
        assert_eq!(
            rows.iter()
                .map(|r| r.payload_json.as_str())
                .collect::<Vec<_>>(),
            vec![r#"{"type":"user"}"#, r#"{"type":"assistant"}"#]
        );
        let all = page(&store, &th.id, None, 2).await.expect("page");
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].turn_id, t2);
        let older = page(&store, &th.id, Some(all[0].id), 10)
            .await
            .expect("older");
        assert_eq!(older.len(), 1);

        // A crash leaves turn 2 running: the next launch settles it.
        set_state(&store, &th.id, ThreadState::Working)
            .await
            .expect("working");
        crate::repo::restore::settle(&store).await.expect("settle");
        assert_eq!(
            get(&store, &th.id).await.expect("get").expect("th").state,
            ThreadState::Interrupted
        );
        assert_eq!(
            turns(&store, &th.id).await.expect("turns")[1].state,
            TurnState::Interrupted
        );
    }
}
