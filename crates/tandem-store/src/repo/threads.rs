//! Threads, their turns and their event log (F-TH-01, F-TH-03): `threads`,
//! `thread_turns` and `thread_messages`. Events are stored as the JSON the
//! threads service serialized (`payload_json`) under their tag (`type`).

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::access::{Access, ThreadMode};
use tandem_core::ids::{AccountId, TeammateId, ThreadId, TurnId, WorkspaceId};
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
    /// A teammate chat's teammate (F-AG-03).
    pub teammate_id: Option<TeammateId>,
    /// Mail threading (F-AG-12).
    pub conversation_ref: Option<String>,
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

const COLUMNS: &str = "id, kind, workspace_id, cwd, engine_id, account_id, model, effort, access, mode, title, state, attended, engine_session_id, pinned, archived, unread, sort_key, created_at, updated_at, teammate_id, conversation_ref";

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
        teammate_id: r.get::<_, Option<String>>(20)?.map(TeammateId::from_db),
        conversation_ref: r.get(21)?,
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

/// What a new teammate chat carries besides a thread's fields.
#[derive(Debug, Clone, Default)]
pub struct ChatExtra {
    pub teammate_id: Option<TeammateId>,
    /// Mail threading (F-AG-12).
    pub conversation_ref: Option<String>,
    /// A title from the start (mail's "{sender}: {first line}").
    pub title: Option<(String, &'static str)>,
}

/// A new teammate chat, after its teammate's other chats (tabs read left to
/// right).
pub async fn insert_chat(
    store: &Store,
    new: NewThread,
    extra: ChatExtra,
) -> Result<ThreadRecord, StoreError> {
    let id = new.id.clone();
    store
        .write(move |t| {
            let teammate = extra.teammate_id.as_ref().map(|t| t.as_str().to_string());
            let last: Option<String> = t.query_row(
                "SELECT MAX(sort_key) FROM threads WHERE teammate_id IS ?1 AND kind = 'teammate_chat' AND deleted_at IS NULL",
                [&teammate],
                |r| r.get(0),
            )?;
            let key = sort_key::between(last.as_deref(), None);
            let now = now_ms();
            let (title, source) = extra
                .title
                .clone()
                .unwrap_or_else(|| (String::new(), "none"));
            t.execute(
                "INSERT INTO threads (id, kind, teammate_id, workspace_id, cwd, engine_id, account_id, model, effort, access, mode, attended, sort_key, conversation_ref, title, title_source, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17)",
                params![
                    id.as_str(),
                    snake(&new.kind),
                    teammate,
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
                    extra.conversation_ref,
                    title,
                    source,
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

/// Sets the order of these threads (a teammate's chat tabs, left to right).
pub async fn reorder(store: &Store, ids: &[ThreadId]) -> Result<(), StoreError> {
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
                    "UPDATE threads SET sort_key = ?2 WHERE id = ?1",
                    params![id, key],
                )?;
            }
            Ok(())
        })
        .await
}

/// A teammate's chats, in tab order.
pub async fn teammate_chats(
    store: &Store,
    teammate: &TeammateId,
    include_archived: bool,
) -> Result<Vec<ThreadRecord>, StoreError> {
    let teammate = teammate.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM threads WHERE kind = 'teammate_chat' AND teammate_id = ?1
                   AND deleted_at IS NULL AND (?2 OR archived = 0)
                 ORDER BY sort_key, created_at"
            ))?;
            let rows = stmt.query_map(params![teammate, include_archived], from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// Every teammate's chats that aren't archived (the views' list).
pub async fn all_teammate_chats(store: &Store) -> Result<Vec<ThreadRecord>, StoreError> {
    store
        .read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM threads WHERE kind = 'teammate_chat' AND teammate_id IS NOT NULL
                   AND deleted_at IS NULL AND archived = 0
                 ORDER BY teammate_id, sort_key, created_at"
            ))?;
            let rows = stmt.query_map([], from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// A teammate's chats at a glance (the Agent rail, F-AG-02).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeammateChatStats {
    pub teammate_id: TeammateId,
    pub chats: u32,
    pub working: u32,
    pub waiting: bool,
    /// The most recently updated chat failed.
    pub failed: bool,
    pub last_line: Option<String>,
    pub last_at: Option<i64>,
}

/// The first non-empty line of Markdown, without its leading marks.
fn first_line(markdown: &str) -> Option<String> {
    markdown
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches(['#', '>', '-', '*', ' '])
                .trim()
        })
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(160).collect())
}

pub async fn teammate_stats(store: &Store) -> Result<Vec<TeammateChatStats>, StoreError> {
    store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT teammate_id, COUNT(*),
                        SUM(state = 'working'),
                        MAX(state = 'waiting')
                   FROM threads
                  WHERE kind = 'teammate_chat' AND teammate_id IS NOT NULL AND deleted_at IS NULL AND archived = 0
                  GROUP BY teammate_id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(TeammateChatStats {
                    teammate_id: TeammateId::from_db(r.get(0)?),
                    chats: r.get::<_, i64>(1)?.max(0) as u32,
                    working: r.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u32,
                    waiting: r.get::<_, Option<bool>>(3)?.unwrap_or(false),
                    ..Default::default()
                })
            })?;
            let mut out = rows.collect::<Result<Vec<_>, _>>()?;
            for s in &mut out {
                let id = s.teammate_id.as_str().to_string();
                s.failed = c
                    .query_row(
                        "SELECT state = 'failed' FROM threads
                          WHERE kind = 'teammate_chat' AND teammate_id = ?1 AND deleted_at IS NULL AND archived = 0
                          ORDER BY updated_at DESC LIMIT 1",
                        [&id],
                        |r| r.get(0),
                    )
                    .optional()?
                    .unwrap_or(false);
                let last: Option<(String, i64)> = c
                    .query_row(
                        "SELECT m.payload_json, m.created_at FROM thread_messages m
                           JOIN threads t ON t.id = m.thread_id
                          WHERE t.teammate_id = ?1 AND t.kind = 'teammate_chat' AND t.deleted_at IS NULL
                            AND m.type = 'assistant'
                          ORDER BY m.id DESC LIMIT 1",
                        [&id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if let Some((payload, at)) = last {
                    s.last_line = serde_json::from_str::<serde_json::Value>(&payload)
                        .ok()
                        .and_then(|v| v.get("markdown").and_then(|m| m.as_str()).and_then(first_line));
                    s.last_at = Some(at);
                }
            }
            Ok(out)
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

/// `thread_update`'s fields (F-TH-01, F-TH-09); `None` leaves a field as is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThreadPatch {
    /// A user rename: never overwritten by auto-titles (F-TH-15 AC2).
    pub title: Option<String>,
    pub access: Option<Access>,
    pub mode: Option<ThreadMode>,
    /// `Some(None)` goes back to the engine's default.
    pub model: Option<Option<String>>,
    pub effort: Option<Option<String>>,
    pub pinned: Option<bool>,
}

pub async fn update(
    store: &Store,
    id: &ThreadId,
    patch: ThreadPatch,
) -> Result<Option<ThreadRecord>, StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE threads SET
                   title = COALESCE(?2, title),
                   title_source = CASE WHEN ?2 IS NULL THEN title_source ELSE 'user' END,
                   access = COALESCE(?3, access),
                   mode = COALESCE(?4, mode),
                   model = CASE WHEN ?5 THEN ?6 ELSE model END,
                   effort = CASE WHEN ?7 THEN ?8 ELSE effort END,
                   pinned = COALESCE(?9, pinned)
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![
                    id,
                    patch.title,
                    patch.access.map(Access::as_str),
                    patch.mode.map(ThreadMode::as_str),
                    patch.model.is_some(),
                    patch.model.flatten(),
                    patch.effort.is_some(),
                    patch.effort.flatten(),
                    patch.pinned,
                ],
            )?;
            Ok(t.query_row(
                &format!("SELECT {COLUMNS} FROM threads WHERE id = ?1 AND deleted_at IS NULL"),
                [id],
                from_row,
            )
            .optional()?)
        })
        .await
}

/// Where a thread's title came from (`none`, `auto`, `fallback`, `user`, …).
pub async fn title_source(store: &Store, id: &ThreadId) -> Result<String, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT title_source FROM threads WHERE id = ?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| "none".to_string()))
        })
        .await
}

/// Sets an automatic title (`source` is `auto` or `fallback`) unless the
/// user named the thread, or an auto title is already there. Returns
/// whether it was set.
pub async fn set_auto_title(
    store: &Store,
    id: &ThreadId,
    title: String,
    source: &'static str,
) -> Result<bool, StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            // A fallback never replaces an AI title; an AI title may replace
            // the fallback that came first.
            let replaceable = if source == "auto" {
                "('none', 'fallback')"
            } else {
                "('none')"
            };
            let n = t.execute(
                &format!(
                    "UPDATE threads SET title = ?2, title_source = ?3
                     WHERE id = ?1 AND title_source IN {replaceable}"
                ),
                params![id, title, source],
            )?;
            Ok(n > 0)
        })
        .await
}

pub async fn set_archived(store: &Store, id: &ThreadId, archived: bool) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE threads SET archived = ?2, pinned = CASE WHEN ?2 THEN 0 ELSE pinned END WHERE id = ?1",
                params![id, archived],
            )?;
            Ok(())
        })
        .await
}

pub async fn set_unread(store: &Store, id: &ThreadId, unread: bool) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE threads SET unread = ?2 WHERE id = ?1",
                params![id, unread],
            )?;
            Ok(())
        })
        .await
}

/// A new engine or account for the next turn: the engine's session and the
/// thread's allow rules don't carry over (F-TH-11, F-TH-16).
pub async fn set_engine(
    store: &Store,
    id: &ThreadId,
    engine_id: &str,
    account_id: Option<&AccountId>,
) -> Result<(), StoreError> {
    let (id, engine, account) = (
        id.as_str().to_string(),
        engine_id.to_string(),
        account_id.map(|a| a.as_str().to_string()),
    );
    store
        .write(move |t| {
            t.execute(
                "UPDATE threads SET engine_id = ?2, account_id = ?3, engine_session_id = NULL,
                   allow_rules_json = '[]', model = NULL, effort = NULL
                 WHERE id = ?1",
                params![id, engine, account],
            )?;
            Ok(())
        })
        .await
}

/// Moves a thread into a folder (F-TH-02): its cwd changes, so the engine
/// session starts over.
pub async fn set_folder(
    store: &Store,
    id: &ThreadId,
    workspace_id: &WorkspaceId,
    cwd: &str,
) -> Result<(), StoreError> {
    let (id, ws, cwd) = (
        id.as_str().to_string(),
        workspace_id.as_str().to_string(),
        cwd.to_string(),
    );
    store
        .write(move |t| {
            // A new folder: the Changes panel starts over there (F-TH-13).
            t.execute(
                "UPDATE threads SET workspace_id = ?2, cwd = ?3, engine_session_id = NULL,
                   start_commit = NULL, snapshot_ref = NULL WHERE id = ?1",
                params![id, ws, cwd],
            )?;
            Ok(())
        })
        .await
}

/// Where the thread's Changes panel starts (F-TH-13): its starting commit
/// (empty: a repository with no commits yet) or its snapshot manifest.
/// Both `None` until the first turn.
pub async fn start_point(
    store: &Store,
    id: &ThreadId,
) -> Result<(Option<String>, Option<String>), StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT start_commit, snapshot_ref FROM threads WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or((None, None)))
        })
        .await
}

pub async fn set_start_point(
    store: &Store,
    id: &ThreadId,
    commit: Option<String>,
    snapshot: Option<String>,
) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE threads SET start_commit = ?2, snapshot_ref = ?3 WHERE id = ?1",
                params![id, commit, snapshot],
            )?;
            Ok(())
        })
        .await
}

/// Deletes a thread with its turns, events and attachment rows.
pub async fn delete(store: &Store, id: &ThreadId) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute("DELETE FROM threads WHERE id = ?1", [id])?;
            Ok(())
        })
        .await
}

/// Every stored event of a thread, oldest first (exports and recovery copies).
pub async fn all_messages(store: &Store, thread: &ThreadId) -> Result<Vec<MessageRow>, StoreError> {
    let thread = thread.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT id, thread_id, turn_id, seq, type, payload_json, created_at
                 FROM thread_messages WHERE thread_id = ?1 ORDER BY id",
            )?;
            let rows = stmt.query_map([thread], message_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
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

/// A plan the agent wrote in Plan mode (F-TH-10): v1, v2, … per thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlanRevision {
    pub version: u32,
    pub turn_id: TurnId,
    pub text: String,
    pub created_at: i64,
}

/// Saves a turn's plan as the thread's next version.
pub async fn add_plan_revision(
    store: &Store,
    thread: &ThreadId,
    turn: &TurnId,
    text: String,
) -> Result<PlanRevision, StoreError> {
    let thread = thread.as_str().to_string();
    let turn_id = turn.clone();
    store
        .write(move |t| {
            let version: u32 = t.query_row(
                "SELECT COALESCE(MAX(version), 0) + 1 FROM plan_revisions WHERE thread_id = ?1",
                [&thread],
                |r| r.get(0),
            )?;
            let at = now_ms();
            t.execute(
                "INSERT INTO plan_revisions (id, thread_id, version, turn_id, text, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    format!("{thread}:v{version}"),
                    thread,
                    version,
                    turn_id.as_str(),
                    text,
                    at
                ],
            )?;
            Ok(PlanRevision {
                version,
                turn_id,
                text,
                created_at: at,
            })
        })
        .await
}

/// A thread's plans, oldest first.
pub async fn plan_revisions(
    store: &Store,
    thread: &ThreadId,
) -> Result<Vec<PlanRevision>, StoreError> {
    let thread = thread.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT version, turn_id, text, created_at FROM plan_revisions
                 WHERE thread_id = ?1 ORDER BY version",
            )?;
            let rows = stmt.query_map([thread], |r| {
                Ok(PlanRevision {
                    version: r.get(0)?,
                    turn_id: TurnId::from_db(r.get(1)?),
                    text: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
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
    async fn f_th_01_update_pin_archive_and_delete() {
        let (_dir, store) = store();
        let a = insert(&store, new("claude")).await.expect("a");
        let patched = update(
            &store,
            &a.id,
            ThreadPatch {
                pinned: Some(true),
                access: Some(Access::Edits),
                model: Some(Some("sonnet".into())),
                ..ThreadPatch::default()
            },
        )
        .await
        .expect("update")
        .expect("found");
        assert!(patched.pinned);
        assert_eq!(patched.access, Access::Edits);
        assert_eq!(patched.model.as_deref(), Some("sonnet"));
        assert_eq!(patched.mode, ThreadMode::Act);
        // Some(None) clears back to the engine's default.
        let cleared = update(
            &store,
            &a.id,
            ThreadPatch {
                model: Some(None),
                ..ThreadPatch::default()
            },
        )
        .await
        .expect("update")
        .expect("found");
        assert_eq!(cleared.model, None);
        assert!(cleared.pinned);
        // Archiving hides it from the rail and unpins it; restoring keeps it.
        set_archived(&store, &a.id, true).await.expect("archive");
        assert!(list(&store, ThreadKind::Thread, false)
            .await
            .expect("rail")
            .is_empty());
        let all = list(&store, ThreadKind::Thread, true).await.expect("all");
        assert!(all[0].archived && !all[0].pinned);
        set_archived(&store, &a.id, false).await.expect("restore");
        assert_eq!(
            list(&store, ThreadKind::Thread, false)
                .await
                .expect("rail")
                .len(),
            1
        );
        delete(&store, &a.id).await.expect("delete");
        assert!(get(&store, &a.id).await.expect("get").is_none());
        assert!(update(&store, &a.id, ThreadPatch::default())
            .await
            .expect("update")
            .is_none());
    }

    #[tokio::test]
    async fn f_th_15_2_a_user_rename_is_never_overwritten_by_auto_titles() {
        let (_dir, store) = store();
        let a = insert(&store, new("claude")).await.expect("a");
        assert!(
            set_auto_title(&store, &a.id, "Fix the login bug".into(), "fallback")
                .await
                .expect("fallback")
        );
        // The AI title replaces the fallback; a second fallback doesn't.
        assert!(
            set_auto_title(&store, &a.id, "Login redirect fix".into(), "auto")
                .await
                .expect("auto")
        );
        assert!(!set_auto_title(&store, &a.id, "other".into(), "fallback")
            .await
            .expect("fallback again"));
        update(
            &store,
            &a.id,
            ThreadPatch {
                title: Some("Mine".into()),
                ..ThreadPatch::default()
            },
        )
        .await
        .expect("rename");
        assert_eq!(title_source(&store, &a.id).await.expect("source"), "user");
        assert!(!set_auto_title(&store, &a.id, "Later".into(), "auto")
            .await
            .expect("auto"));
        let a = get(&store, &a.id).await.expect("get").expect("a");
        assert_eq!(a.title, "Mine");
    }

    #[tokio::test]
    async fn f_th_11_a_new_engine_clears_the_session_and_allow_rules() {
        let (_dir, store) = store();
        let a = insert(&store, new("claude")).await.expect("a");
        set_engine_session(&store, &a.id, Some("s1"))
            .await
            .expect("session");
        set_allow_rules(&store, &a.id, r#"[{"kind":"tool","name":"Edit"}]"#.into())
            .await
            .expect("rules");
        set_engine(&store, &a.id, "codex", None)
            .await
            .expect("engine");
        let a = get(&store, &a.id).await.expect("get").expect("a");
        assert_eq!(a.engine_id, "codex");
        assert_eq!(a.engine_session_id, None);
        assert_eq!(allow_rules(&store, &a.id).await.expect("rules"), "[]");
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

    #[tokio::test]
    async fn f_rt_06_10_launch_leaves_a_headless_runs_thread_working_until_the_lock() {
        use crate::repo::restore::{settle_leftovers, settle_panes_blocking};
        let (_dir, store) = store();
        // A headless pass is driving `night`; `crashed` was left by a crash.
        let night = insert(&store, new("claude")).await.expect("night");
        let crashed = insert(&store, new("claude")).await.expect("crashed");
        let turn = TurnId::new();
        start_turn(&store, &night.id, &turn, "claude", None)
            .await
            .expect("turn");
        for id in [&night.id, &crashed.id] {
            set_state(&store, id, ThreadState::Working)
                .await
                .expect("working");
        }
        // Launch, with the lock held elsewhere: nothing about threads changes.
        let st = store.clone();
        let (_, left) = tokio::task::spawn_blocking(move || settle_panes_blocking(&st))
            .await
            .expect("join")
            .expect("panes");
        assert_eq!(left.threads.len(), 2);
        assert_eq!(
            get(&store, &night.id)
                .await
                .expect("get")
                .expect("th")
                .state,
            ThreadState::Working
        );
        assert_eq!(
            turns(&store, &night.id).await.expect("turns")[0].state,
            TurnState::Running
        );
        // Meanwhile the pass finishes its turn, and this app starts its own.
        end_turn(&store, &turn, TurnState::Completed, 0)
            .await
            .expect("end");
        set_state(&store, &night.id, ThreadState::Done)
            .await
            .expect("done");
        let mine = insert(&store, new("claude")).await.expect("mine");
        set_state(&store, &mine.id, ThreadState::Working)
            .await
            .expect("mine working");
        // The lock is this app's: only what's still left over settles.
        assert_eq!(settle_leftovers(&store, left).await.expect("settle"), 1);
        let state = |id: ThreadId| {
            let store = store.clone();
            async move { get(&store, &id).await.expect("get").expect("th").state }
        };
        assert_eq!(state(night.id.clone()).await, ThreadState::Done);
        assert_eq!(state(crashed.id.clone()).await, ThreadState::Interrupted);
        assert_eq!(state(mine.id.clone()).await, ThreadState::Working);
        assert_eq!(
            turns(&store, &night.id).await.expect("turns")[0].state,
            TurnState::Completed
        );
    }

    #[tokio::test]
    async fn f_th_10_plan_revisions_are_numbered_per_thread() {
        let (_dir, store) = store();
        let a = insert(&store, new("claude")).await.expect("a");
        let b = insert(&store, new("claude")).await.expect("b");
        let turn = TurnId::new();
        let v1 = add_plan_revision(&store, &a.id, &turn, "1. Read".into())
            .await
            .expect("v1");
        let v2 = add_plan_revision(&store, &a.id, &turn, "1. Write".into())
            .await
            .expect("v2");
        let other = add_plan_revision(&store, &b.id, &turn, "1. Other".into())
            .await
            .expect("other");
        assert_eq!((v1.version, v2.version, other.version), (1, 2, 1));
        let listed = plan_revisions(&store, &a.id).await.expect("list");
        assert_eq!(
            listed
                .iter()
                .map(|p| (p.version, p.text.as_str()))
                .collect::<Vec<_>>(),
            vec![(1, "1. Read"), (2, "1. Write")]
        );
    }
}
