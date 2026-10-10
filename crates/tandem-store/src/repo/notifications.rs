//! `notifications` (F-DB-02): what the bell lists, kept 7 days. Items that
//! share a `dedupe_key` (an approval, a pane's waiting spell) resolve
//! together when their source moves on.

use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::NotificationId;
use tandem_core::source::SourceRef;
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

/// Seven days (F-DB-02).
pub const RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
pub const PAGE: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NotificationKind {
    Approval,
    Question,
    Done,
    Failed,
    RoutineReady,
    TaskReview,
    LaneCleanup,
    UpdateReady,
    CreditLow,
    PluginAttention,
    HeldActions,
    MailRefused,
    /// A teammate made or changed a routine (F-RT-05).
    RoutineChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NotificationState {
    Unread,
    Read,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Notification {
    pub id: NotificationId,
    pub kind: NotificationKind,
    pub title: String,
    /// The source line ("Claude Code · shop — Run `pnpm test`"), redacted.
    pub body: String,
    pub source: SourceRef,
    pub state: NotificationState,
    pub dedupe_key: Option<String>,
    pub created_at: i64,
    pub read_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NotificationPage {
    pub items: Vec<Notification>,
    /// Pass as `cursor` for the next page, when there's more
    /// (`"<created_at>:<seq>"`: items sharing a millisecond keep their
    /// insertion order and none is lost).
    pub next_cursor: Option<String>,
    pub unread: u32,
}

/// A new item, before it has an id.
#[derive(Debug, Clone)]
pub struct NewNotification {
    pub kind: NotificationKind,
    pub title: String,
    pub body: String,
    pub source: SourceRef,
    pub dedupe_key: Option<String>,
}

const COLS: &str = "id, kind, title, body, source_json, state, dedupe_key, created_at, read_at";

fn enum_str<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|j| j.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn enum_from<T: for<'de> Deserialize<'de>>(s: String) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(s)).ok()
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<Option<Notification>> {
    let source: String = r.get(4)?;
    let (Some(kind), Some(state), Ok(source)) = (
        enum_from::<NotificationKind>(r.get(1)?),
        enum_from::<NotificationState>(r.get(5)?),
        serde_json::from_str::<SourceRef>(&source),
    ) else {
        return Ok(None);
    };
    Ok(Some(Notification {
        id: NotificationId::from_db(r.get(0)?),
        kind,
        title: r.get(2)?,
        body: r.get(3)?,
        source,
        state,
        dedupe_key: r.get(6)?,
        created_at: r.get(7)?,
        read_at: r.get(8)?,
    }))
}

pub async fn insert(store: &Store, n: NewNotification) -> Result<Notification, StoreError> {
    let item = Notification {
        id: NotificationId::new(),
        kind: n.kind,
        title: n.title,
        body: n.body,
        source: n.source,
        state: NotificationState::Unread,
        dedupe_key: n.dedupe_key,
        created_at: now_ms(),
        read_at: None,
    };
    let row = item.clone();
    let source =
        serde_json::to_string(&row.source).map_err(|e| StoreError::Invalid(e.to_string()))?;
    store
        .write(move |t| {
            t.execute(
                &format!("INSERT INTO notifications ({COLS}) VALUES (?1, ?2, ?3, ?4, ?5, 'unread', ?6, ?7, NULL)"),
                params![
                    row.id.as_str(),
                    enum_str(&row.kind),
                    row.title,
                    row.body,
                    source,
                    row.dedupe_key,
                    row.created_at
                ],
            )?;
            Ok(())
        })
        .await?;
    Ok(item)
}

/// Newest first within the last 7 days; `cursor` continues after the last
/// item of the previous page.
pub async fn list(store: &Store, cursor: Option<String>) -> Result<NotificationPage, StoreError> {
    let since = now_ms() - RETENTION_MS;
    let (at, seq) = match cursor.as_deref().and_then(|c| c.split_once(':')) {
        Some((ms, seq)) => (
            ms.parse::<i64>()
                .map_err(|_| StoreError::Invalid("Bad cursor".into()))?,
            seq.parse::<i64>()
                .map_err(|_| StoreError::Invalid("Bad cursor".into()))?,
        ),
        None => (i64::MAX, i64::MAX),
    };
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLS}, rowid FROM notifications
                 WHERE created_at >= ?1 AND (created_at < ?2 OR (created_at = ?2 AND rowid < ?3))
                 ORDER BY created_at DESC, rowid DESC LIMIT ?4"
            ))?;
            let rows = stmt.query_map(params![since, at, seq, (PAGE + 1) as i64], |r| {
                let seq: i64 = r.get(9)?;
                Ok(from_row(r)?.map(|n| (n, seq)))
            })?;
            let mut items: Vec<(Notification, i64)> =
                rows.filter_map(|r| r.ok().flatten()).collect();
            let next_cursor = if items.len() > PAGE {
                items.truncate(PAGE);
                items
                    .last()
                    .map(|(n, seq)| format!("{}:{seq}", n.created_at))
            } else {
                None
            };
            let items: Vec<Notification> = items.into_iter().map(|(n, _)| n).collect();
            let unread: u32 = c.query_row(
                "SELECT COUNT(*) FROM notifications WHERE state = 'unread' AND created_at >= ?1",
                [since],
                |r| r.get(0),
            )?;
            Ok(NotificationPage {
                items,
                next_cursor,
                unread,
            })
        })
        .await
}

/// Marks items (or all unread ones) read; returns the ids that changed.
pub async fn mark_read(
    store: &Store,
    ids: Vec<NotificationId>,
    all: bool,
) -> Result<Vec<NotificationId>, StoreError> {
    store
        .write(move |t| {
            let now = now_ms();
            let targets: Vec<String> = if all {
                let mut stmt = t.prepare("SELECT id FROM notifications WHERE state = 'unread'")?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                rows.collect::<Result<_, _>>()?
            } else {
                ids.iter().map(|i| i.as_str().to_string()).collect()
            };
            let mut changed = Vec::new();
            for id in targets {
                let n = t.execute(
                    "UPDATE notifications SET state = 'read', read_at = ?2 WHERE id = ?1 AND state = 'unread'",
                    params![id, now],
                )?;
                if n > 0 {
                    changed.push(NotificationId::from_db(id));
                }
            }
            Ok(changed)
        })
        .await
}

/// The source moved on (the approval was answered, the agent stopped
/// waiting): its items leave "Waiting on you". Returns the changed ids.
pub async fn resolve(store: &Store, dedupe_key: &str) -> Result<Vec<NotificationId>, StoreError> {
    let key = dedupe_key.to_string();
    store
        .write(move |t| {
            let mut stmt = t.prepare(
                "SELECT id FROM notifications WHERE dedupe_key = ?1 AND state <> 'resolved'",
            )?;
            let ids: Vec<String> = stmt
                .query_map([&key], |r| r.get::<_, String>(0))?
                .collect::<Result<_, _>>()?;
            for id in &ids {
                t.execute(
                    "UPDATE notifications SET state = 'resolved' WHERE id = ?1",
                    [id],
                )?;
            }
            Ok(ids.into_iter().map(NotificationId::from_db).collect())
        })
        .await
}

/// Whether an item with this dedupe key is still open (not resolved).
pub async fn has_open(store: &Store, dedupe_key: &str) -> Result<bool, StoreError> {
    let key = dedupe_key.to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT EXISTS(SELECT 1 FROM notifications WHERE dedupe_key = ?1 AND state <> 'resolved')",
                [&key],
                |r| r.get::<_, bool>(0),
            )?)
        })
        .await
}

/// Drops items older than the retention window.
pub async fn prune(store: &Store) -> Result<usize, StoreError> {
    let before = now_ms() - RETENTION_MS;
    store
        .write(move |t| Ok(t.execute("DELETE FROM notifications WHERE created_at < ?1", [before])?))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(kind: NotificationKind, dedupe: Option<&str>) -> NewNotification {
        NewNotification {
            kind,
            title: "Ada finished".into(),
            body: "Claude Code · shop".into(),
            source: SourceRef::Pane {
                pane_id: "pn_1".into(),
            },
            dedupe_key: dedupe.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn f_db_02_items_list_newest_first_with_an_unread_count() {
        let (_d, store) = crate::tests::temp_store().await;
        let a = insert(&store, new(NotificationKind::Done, None))
            .await
            .expect("a");
        let b = insert(
            &store,
            new(NotificationKind::Approval, Some("approval:ap_1")),
        )
        .await
        .expect("b");
        let page = list(&store, None).await.expect("list");
        assert_eq!(page.unread, 2);
        assert_eq!(page.items.first().map(|n| n.id.clone()), Some(b.id.clone()));
        assert_eq!(page.items.len(), 2);
        assert_eq!(
            mark_read(&store, vec![a.id.clone()], false)
                .await
                .expect("read"),
            vec![a.id.clone()]
        );
        assert_eq!(list(&store, None).await.expect("list").unread, 1);
        assert!(mark_read(&store, vec![a.id], false)
            .await
            .expect("again")
            .is_empty());
        assert_eq!(
            mark_read(&store, vec![], true).await.expect("all"),
            vec![b.id]
        );
    }

    #[tokio::test]
    async fn f_db_02_an_open_item_is_found_by_its_dedupe_key_until_resolved() {
        let (_d, store) = crate::tests::temp_store().await;
        assert!(!has_open(&store, "credit_low").await.expect("none"));
        insert(&store, new(NotificationKind::CreditLow, Some("credit_low")))
            .await
            .expect("insert");
        assert!(has_open(&store, "credit_low").await.expect("open"));
        resolve(&store, "credit_low").await.expect("resolve");
        assert!(!has_open(&store, "credit_low").await.expect("resolved"));
    }

    #[tokio::test]
    async fn f_db_02_1_an_answered_approval_resolves_its_items() {
        let (_d, store) = crate::tests::temp_store().await;
        let b = insert(
            &store,
            new(NotificationKind::Approval, Some("approval:ap_1")),
        )
        .await
        .expect("b");
        let other = insert(
            &store,
            new(NotificationKind::Approval, Some("approval:ap_2")),
        )
        .await
        .expect("o");
        assert_eq!(
            resolve(&store, "approval:ap_1").await.expect("resolve"),
            vec![b.id.clone()]
        );
        let page = list(&store, None).await.expect("list");
        let state = |id: &NotificationId| page.items.iter().find(|n| &n.id == id).map(|n| n.state);
        assert_eq!(state(&b.id), Some(NotificationState::Resolved));
        assert_eq!(state(&other.id), Some(NotificationState::Unread));
        assert!(resolve(&store, "approval:ap_1")
            .await
            .expect("again")
            .is_empty());
    }

    #[tokio::test]
    async fn f_db_02_items_older_than_7_days_are_pruned_and_paging_continues() {
        let (_d, store) = crate::tests::temp_store().await;
        for _ in 0..(PAGE + 3) {
            insert(&store, new(NotificationKind::Done, None))
                .await
                .expect("insert");
        }
        // Many share a millisecond: the cursor carries the id too, so the
        // second page holds exactly the rest, none lost or repeated.
        let first = list(&store, None).await.expect("first");
        assert_eq!(first.items.len(), PAGE);
        let second = list(&store, first.next_cursor.clone())
            .await
            .expect("second");
        assert_eq!(second.items.len(), 3);
        assert!(second.next_cursor.is_none());
        let mut ids: Vec<_> = first
            .items
            .iter()
            .chain(&second.items)
            .map(|n| n.id.clone())
            .collect();
        ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        ids.dedup();
        assert_eq!(ids.len(), PAGE + 3);
        store
            .write(|t| {
                t.execute(
                    "UPDATE notifications SET created_at = created_at - ?1",
                    [RETENTION_MS + 1],
                )?;
                Ok(())
            })
            .await
            .expect("age");
        assert_eq!(prune(&store).await.expect("prune"), PAGE + 3);
        assert!(list(&store, None).await.expect("list").items.is_empty());
    }
}
