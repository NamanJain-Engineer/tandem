//! `plugin_connections` and `plugin_grants` (F-PL-01, F-PL-05): which
//! plugins are connected (never their secrets, which live in the keychain)
//! and who may use them. Connecting makes a plugin available; a grant makes
//! it usable by one teammate, thread or workspace, or by every thread.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tandem_core::ids::ConnectionId;
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ConnectionStatus {
    Connected,
    NeedsAttention,
    Disconnected,
}

impl ConnectionStatus {
    fn as_str(self) -> &'static str {
        match self {
            ConnectionStatus::Connected => "connected",
            ConnectionStatus::NeedsAttention => "needs_attention",
            ConnectionStatus::Disconnected => "disconnected",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "connected" => ConnectionStatus::Connected,
            "needs_attention" => ConnectionStatus::NeedsAttention,
            _ => ConnectionStatus::Disconnected,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginConnection {
    pub id: ConnectionId,
    pub plugin_id: String,
    pub account_label: Option<String>,
    pub auth_kind: String,
    pub status: ConnectionStatus,
    pub created_at: i64,
}

/// Who a grant is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum GrantSubject {
    Teammate {
        #[serde(rename = "teammateId")]
        teammate_id: String,
    },
    Thread {
        #[serde(rename = "threadId")]
        thread_id: String,
    },
    Workspace {
        #[serde(rename = "workspaceId")]
        workspace_id: String,
    },
    AllThreads,
}

impl GrantSubject {
    fn cols(&self) -> (&'static str, &str) {
        match self {
            GrantSubject::Teammate { teammate_id } => ("teammate", teammate_id),
            GrantSubject::Thread { thread_id } => ("thread", thread_id),
            GrantSubject::Workspace { workspace_id } => ("workspace", workspace_id),
            GrantSubject::AllThreads => ("all_threads", "*"),
        }
    }
}

pub async fn connections(store: &Store) -> Result<Vec<PluginConnection>, StoreError> {
    store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT id, plugin_id, account_label, auth_kind, status, created_at
                 FROM plugin_connections ORDER BY created_at, id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(PluginConnection {
                    id: ConnectionId::from_db(r.get(0)?),
                    plugin_id: r.get(1)?,
                    account_label: r.get(2)?,
                    auth_kind: r.get(3)?,
                    status: ConnectionStatus::parse(&r.get::<_, String>(4)?),
                    created_at: r.get(5)?,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// Records a connection (its credential, if any, is already in the keychain).
pub async fn insert_connection(
    store: &Store,
    plugin_id: &str,
    account_label: Option<&str>,
    auth_kind: &str,
) -> Result<PluginConnection, StoreError> {
    let conn = PluginConnection {
        id: ConnectionId::new(),
        plugin_id: plugin_id.to_string(),
        account_label: account_label.map(str::to_string),
        auth_kind: auth_kind.to_string(),
        status: ConnectionStatus::Connected,
        created_at: now_ms(),
    };
    let row = conn.clone();
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO plugin_connections (id, plugin_id, account_label, auth_kind, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    row.id.as_str(),
                    row.plugin_id,
                    row.account_label,
                    row.auth_kind,
                    row.status.as_str(),
                    row.created_at
                ],
            )?;
            Ok(())
        })
        .await?;
    Ok(conn)
}

/// Removes a connection and every grant of it.
pub async fn delete_connection(store: &Store, id: &ConnectionId) -> Result<(), StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            t.execute("DELETE FROM plugin_connections WHERE id = ?1", [id])?;
            Ok(())
        })
        .await
}

/// The connections granted to a subject, oldest grant first.
pub async fn grants(
    store: &Store,
    subject: &GrantSubject,
) -> Result<Vec<ConnectionId>, StoreError> {
    let (kind, id) = subject.cols();
    let (kind, id) = (kind.to_string(), id.to_string());
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT connection_id FROM plugin_grants
                 WHERE subject_kind = ?1 AND subject_id = ?2 ORDER BY created_at, connection_id",
            )?;
            let rows =
                stmt.query_map(params![kind, id], |r| Ok(ConnectionId::from_db(r.get(0)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// Grants or revokes one connection for a subject (idempotent).
pub async fn set_grant(
    store: &Store,
    subject: &GrantSubject,
    connection: &ConnectionId,
    granted: bool,
) -> Result<(), StoreError> {
    let (kind, id) = subject.cols();
    let (kind, id, conn) = (
        kind.to_string(),
        id.to_string(),
        connection.as_str().to_string(),
    );
    store
        .write(move |t| {
            if granted {
                t.execute(
                    "INSERT OR IGNORE INTO plugin_grants (subject_kind, subject_id, connection_id, created_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![kind, id, conn, now_ms()],
                )?;
            } else {
                t.execute(
                    "DELETE FROM plugin_grants WHERE subject_kind = ?1 AND subject_id = ?2 AND connection_id = ?3",
                    params![kind, id, conn],
                )?;
            }
            Ok(())
        })
        .await
}

/// Drops every grant a subject has (a deleted teammate or thread).
pub async fn clear_grants(store: &Store, subject: &GrantSubject) -> Result<(), StoreError> {
    let (kind, id) = subject.cols();
    let (kind, id) = (kind.to_string(), id.to_string());
    store
        .write(move |t| {
            t.execute(
                "DELETE FROM plugin_grants WHERE subject_kind = ?1 AND subject_id = ?2",
                params![kind, id],
            )?;
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn f_pl_05_grants_are_per_subject_and_go_with_their_connection() {
        let (_d, store) = crate::tests::temp_store().await;
        let echo = insert_connection(&store, "test_echo", None, "none")
            .await
            .expect("insert");
        let upper = insert_connection(&store, "test_upper", Some("me"), "none")
            .await
            .expect("insert");
        assert_eq!(connections(&store).await.expect("list").len(), 2);
        let a = GrantSubject::Teammate {
            teammate_id: "tm_a".into(),
        };
        let b = GrantSubject::Teammate {
            teammate_id: "tm_b".into(),
        };
        assert!(grants(&store, &a).await.expect("grants").is_empty());
        set_grant(&store, &a, &echo.id, true).await.expect("grant");
        set_grant(&store, &a, &echo.id, true).await.expect("again");
        set_grant(&store, &b, &upper.id, true).await.expect("grant");
        assert_eq!(grants(&store, &a).await.expect("a"), vec![echo.id.clone()]);
        assert_eq!(grants(&store, &b).await.expect("b"), vec![upper.id.clone()]);
        set_grant(&store, &a, &echo.id, false)
            .await
            .expect("revoke");
        assert!(grants(&store, &a).await.expect("a").is_empty());
        delete_connection(&store, &upper.id).await.expect("delete");
        assert!(grants(&store, &b).await.expect("b").is_empty());
        assert_eq!(connections(&store).await.expect("list"), vec![echo]);
    }
}
