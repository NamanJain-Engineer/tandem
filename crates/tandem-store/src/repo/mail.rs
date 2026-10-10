//! `mail` and `mail_turn_log` (F-AG-12): teammate mail and its loop guard.
//! A mail is claimed for delivery in one statement, so it reaches at most
//! one chat once.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::{MailId, TeammateId, ThreadId};
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MailState {
    Queued,
    Delivered,
    Refused,
    Failed,
}

impl MailState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Delivered => "delivered",
            Self::Refused => "refused",
            Self::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "delivered" => Self::Delivered,
            "refused" => Self::Refused,
            "failed" => Self::Failed,
            _ => Self::Queued,
        }
    }
}

/// Why a mail is refused, queued or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MailReason {
    NotPermitted,
    TerminalOnly,
    Deleted,
    RateLimit,
    GlobalPause,
    WakeFailed,
}

impl MailReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotPermitted => "not_permitted",
            Self::TerminalOnly => "terminal_only",
            Self::Deleted => "deleted",
            Self::RateLimit => "rate_limit",
            Self::GlobalPause => "global_pause",
            Self::WakeFailed => "wake_failed",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "not_permitted" => Self::NotPermitted,
            "terminal_only" => Self::TerminalOnly,
            "deleted" => Self::Deleted,
            "rate_limit" => Self::RateLimit,
            "global_pause" => Self::GlobalPause,
            "wake_failed" => Self::WakeFailed,
            _ => return None,
        })
    }
}

/// `mail_get`: a mail with its body and both names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Mail {
    pub id: MailId,
    /// `None`: the user wrote it.
    pub from_teammate_id: Option<TeammateId>,
    pub from: String,
    pub to_teammate_id: TeammateId,
    pub to: String,
    pub from_thread_id: Option<ThreadId>,
    pub body: String,
    pub conversation_ref: String,
    pub state: MailState,
    pub reason: Option<MailReason>,
    pub delivered_thread_id: Option<ThreadId>,
    pub created_at: i64,
    pub delivered_at: Option<i64>,
}

/// `mail_list` and `mail:changed`: a mail's one-line row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MailRow {
    pub id: MailId,
    pub from_teammate_id: Option<TeammateId>,
    pub from: String,
    pub to_teammate_id: TeammateId,
    pub to: String,
    /// The first 80 characters of the first line.
    pub preview: String,
    pub state: MailState,
    pub reason: Option<MailReason>,
    pub delivered_thread_id: Option<ThreadId>,
    pub created_at: i64,
}

/// The first 80 characters of a body's first non-empty line.
pub fn preview(body: &str) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let mut out: String = line.chars().take(80).collect();
    if line.chars().count() > 80 {
        out.push('…');
    }
    out
}

impl Mail {
    pub fn row(&self) -> MailRow {
        MailRow {
            id: self.id.clone(),
            from_teammate_id: self.from_teammate_id.clone(),
            from: self.from.clone(),
            to_teammate_id: self.to_teammate_id.clone(),
            to: self.to.clone(),
            preview: preview(&self.body),
            state: self.state,
            reason: self.reason,
            delivered_thread_id: self.delivered_thread_id.clone(),
            created_at: self.created_at,
        }
    }
}

/// A mail to insert.
#[derive(Debug, Clone)]
pub struct NewMail {
    pub id: MailId,
    pub from_teammate_id: Option<TeammateId>,
    pub to_teammate_id: TeammateId,
    pub from_thread_id: Option<ThreadId>,
    pub body: String,
    pub conversation_ref: String,
    pub state: MailState,
    pub reason: Option<MailReason>,
}

/// The user's name on mail they wrote.
pub const YOU: &str = "You";

const COLUMNS: &str =
    "m.id, m.from_teammate_id, f.name, m.to_teammate_id, t.name, m.from_thread_id, m.body,
    m.conversation_ref, m.state, m.reason, m.delivered_thread_id, m.created_at, m.delivered_at";
const FROM: &str =
    "mail m LEFT JOIN teammates f ON f.id = m.from_teammate_id LEFT JOIN teammates t ON t.id = m.to_teammate_id";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Mail> {
    let from_id: Option<String> = r.get(1)?;
    let from_name: Option<String> = r.get(2)?;
    Ok(Mail {
        id: MailId::from_db(r.get(0)?),
        from: match (&from_id, from_name) {
            (None, _) => YOU.into(),
            (Some(_), Some(n)) => n,
            (Some(_), None) => String::new(),
        },
        from_teammate_id: from_id.map(TeammateId::from_db),
        to_teammate_id: TeammateId::from_db(r.get(3)?),
        to: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        from_thread_id: r.get::<_, Option<String>>(5)?.map(ThreadId::from_db),
        body: r.get(6)?,
        conversation_ref: r.get(7)?,
        state: MailState::parse(&r.get::<_, String>(8)?),
        reason: r
            .get::<_, Option<String>>(9)?
            .as_deref()
            .and_then(MailReason::parse),
        delivered_thread_id: r.get::<_, Option<String>>(10)?.map(ThreadId::from_db),
        created_at: r.get(11)?,
        delivered_at: r.get(12)?,
    })
}

pub async fn insert(store: &Store, new: NewMail) -> Result<Mail, StoreError> {
    let id = new.id.clone();
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO mail (id, from_teammate_id, to_teammate_id, from_thread_id, body, conversation_ref, state, reason, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    new.id.as_str(),
                    new.from_teammate_id.as_ref().map(|i| i.as_str().to_string()),
                    new.to_teammate_id.as_str(),
                    new.from_thread_id.as_ref().map(|i| i.as_str().to_string()),
                    new.body,
                    new.conversation_ref,
                    new.state.as_str(),
                    new.reason.map(MailReason::as_str),
                    now_ms()
                ],
            )?;
            Ok(())
        })
        .await?;
    get(store, &id)
        .await?
        .ok_or_else(|| StoreError::NotFound("mail".into()))
}

pub async fn get(store: &Store, id: &MailId) -> Result<Option<Mail>, StoreError> {
    let id = id.as_str().to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM {FROM} WHERE m.id = ?1"),
                [id],
                from_row,
            )
            .optional()?)
        })
        .await
}

/// Mail to or from a teammate, newest first.
pub async fn list_for(
    store: &Store,
    teammate: &TeammateId,
    limit: u32,
) -> Result<Vec<Mail>, StoreError> {
    let teammate = teammate.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM {FROM}
                  WHERE m.to_teammate_id = ?1 OR m.from_teammate_id = ?1
                  ORDER BY m.created_at DESC, m.rowid DESC LIMIT ?2"
            ))?;
            let rows = stmt.query_map(params![teammate, limit], from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// Queued mail, oldest first (delivery is FIFO).
pub async fn queued(store: &Store) -> Result<Vec<Mail>, StoreError> {
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM {FROM} WHERE m.state = 'queued' ORDER BY m.created_at, m.rowid"
            ))?;
            let rows = stmt.query_map([], from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// Claims a queued mail for `thread`: true exactly once per mail, so a
/// mail is delivered at most once.
pub async fn claim(store: &Store, id: &MailId, thread: &ThreadId) -> Result<bool, StoreError> {
    let (id, thread) = (id.as_str().to_string(), thread.as_str().to_string());
    store
        .write(move |t| {
            let n = t.execute(
                "UPDATE mail SET state = 'delivered', reason = NULL, delivered_thread_id = ?2, delivered_at = ?3
                  WHERE id = ?1 AND state = 'queued'",
                params![id, thread, now_ms()],
            )?;
            Ok(n == 1)
        })
        .await
}

/// Sets a mail's state and reason.
pub async fn set_state(
    store: &Store,
    id: &MailId,
    state: MailState,
    reason: Option<MailReason>,
) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE mail SET state = ?2, reason = ?3 WHERE id = ?1",
                params![id, state.as_str(), reason.map(MailReason::as_str)],
            )?;
            Ok(())
        })
        .await
}

/// A mail-triggered turn started for `teammate` (the loop guard).
pub async fn log_turn(store: &Store, teammate: &TeammateId, at: i64) -> Result<(), StoreError> {
    let teammate = teammate.as_str().to_string();
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO mail_turn_log (teammate_id, started_at) VALUES (?1, ?2)",
                params![teammate, at],
            )?;
            // Nothing older than a day matters to the guard.
            t.execute(
                "DELETE FROM mail_turn_log WHERE started_at < ?1",
                [at - 24 * 60 * 60 * 1000],
            )?;
            Ok(())
        })
        .await
}

/// Mail-triggered turns of `teammate` since `since` (ms).
pub async fn turns_since(
    store: &Store,
    teammate: &TeammateId,
    since: i64,
) -> Result<u32, StoreError> {
    let teammate = teammate.as_str().to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM mail_turn_log WHERE teammate_id = ?1 AND started_at >= ?2",
                params![teammate, since],
                |r| r.get(0),
            )?)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn teammate(store: &Store, name: &str) -> TeammateId {
        let id = TeammateId::new();
        crate::repo::teammates::insert(
            store,
            crate::repo::teammates::NewTeammate {
                id: id.clone(),
                name: name.into(),
                avatar_hue: None,
                brief: String::new(),
                engine_id: "claude".into(),
                model: None,
                effort: None,
                preferred_account_id: None,
                home: "/h".into(),
                places: Vec::new(),
                settings: Default::default(),
                is_lead: false,
                pinned: false,
            },
        )
        .await
        .expect("teammate");
        id
    }

    fn mail(from: Option<&TeammateId>, to: &TeammateId, body: &str) -> NewMail {
        NewMail {
            id: MailId::new(),
            from_teammate_id: from.cloned(),
            to_teammate_id: to.clone(),
            from_thread_id: None,
            body: body.into(),
            conversation_ref: "th_x".into(),
            state: MailState::Queued,
            reason: None,
        }
    }

    #[tokio::test]
    async fn f_ag_12_a_mail_is_claimed_once_and_lists_for_both_teammates() {
        let (_d, store) = crate::tests::temp_store().await;
        let a = teammate(&store, "Ada").await;
        let b = teammate(&store, "Bea").await;
        let m = insert(&store, mail(Some(&a), &b, "\n  Check CI on main\nthanks"))
            .await
            .expect("insert");
        assert_eq!((m.from.as_str(), m.to.as_str()), ("Ada", "Bea"));
        assert_eq!(m.row().preview, "Check CI on main");
        let from_you = insert(&store, mail(None, &b, "hi")).await.expect("insert");
        assert_eq!(from_you.from, YOU);
        assert_eq!(queued(&store).await.expect("q").len(), 2);

        let chat = ThreadId::new();
        assert!(claim(&store, &m.id, &chat).await.expect("claim"));
        assert!(!claim(&store, &m.id, &ThreadId::new()).await.expect("again"));
        let got = get(&store, &m.id).await.expect("get").expect("row");
        assert_eq!(got.state, MailState::Delivered);
        assert_eq!(got.delivered_thread_id.as_ref(), Some(&chat));
        assert_eq!(queued(&store).await.expect("q").len(), 1);

        assert_eq!(list_for(&store, &a, 50).await.expect("a").len(), 1);
        assert_eq!(list_for(&store, &b, 50).await.expect("b").len(), 2);

        set_state(
            &store,
            &from_you.id,
            MailState::Failed,
            Some(MailReason::WakeFailed),
        )
        .await
        .expect("fail");
        let failed = get(&store, &from_you.id).await.expect("get").expect("row");
        assert_eq!(failed.reason, Some(MailReason::WakeFailed));
        // A failed mail isn't claimed until it's queued again (Retry).
        assert!(!claim(&store, &failed.id, &chat).await.expect("claim"));
    }

    #[tokio::test]
    async fn f_ag_12_the_turn_log_counts_a_window() {
        let (_d, store) = crate::tests::temp_store().await;
        let b = teammate(&store, "Bea").await;
        let now = 10 * 60 * 60 * 1000;
        for i in 0..3 {
            log_turn(&store, &b, now - i * 30 * 60 * 1000)
                .await
                .expect("log");
        }
        assert_eq!(
            turns_since(&store, &b, now - 60 * 60 * 1000)
                .await
                .expect("n"),
            3
        );
        assert_eq!(
            turns_since(&store, &b, now - 45 * 60 * 1000)
                .await
                .expect("n"),
            2
        );
    }
}
