//! Session search (F-AG-11, ARCHITECTURE §8.6): an FTS5 index over teammate
//! chats' prose only, the user's messages and the teammate's replies, never
//! tool calls, commands, their output or reasoning. Every query is scoped to
//! one teammate.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tandem_core::ids::{TeammateId, ThreadId, TurnId};
use ts_rs::TS;

use crate::{Store, StoreError};

pub const MAX_LIMIT: u32 = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchHit {
    pub chat_id: ThreadId,
    pub title: String,
    /// Unix ms of the matching message.
    pub date: i64,
    pub snippet: String,
}

/// Indexes one finished turn of a teammate chat: its user message and the
/// assistant's text blocks.
pub async fn index_turn(
    store: &Store,
    thread: &ThreadId,
    turn: &TurnId,
    teammate: &TeammateId,
) -> Result<usize, StoreError> {
    let (thread, turn, teammate) = (
        thread.as_str().to_string(),
        turn.as_str().to_string(),
        teammate.as_str().to_string(),
    );
    store
        .write(move |t| {
            let mut stmt = t.prepare(
                "SELECT type, payload_json, created_at FROM thread_messages
                  WHERE turn_id = ?1 AND type IN ('user', 'assistant') ORDER BY id",
            )?;
            let rows: Vec<(String, String, i64)> = stmt
                .query_map([&turn], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<_, _>>()?;
            let mut n = 0;
            for (kind, payload, at) in rows {
                let v: serde_json::Value = serde_json::from_str(&payload).unwrap_or_default();
                let field = if kind == "user" { "text" } else { "markdown" };
                let Some(text) = v.get(field).and_then(|x| x.as_str()) else {
                    continue;
                };
                if text.trim().is_empty() {
                    continue;
                }
                t.execute(
                    "INSERT INTO chat_fts (text, thread_id, teammate_id, created_at) VALUES (?1, ?2, ?3, ?4)",
                    params![text, thread, teammate, at],
                )?;
                n += 1;
            }
            Ok(n)
        })
        .await
}

/// The query as FTS5 terms: every word quoted (no operators, no syntax
/// errors), all of them required.
pub fn fts_query(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '\'')
        .map(|w| w.trim_matches('\''))
        .filter(|w| !w.is_empty())
        .take(16)
        .map(|w| format!("\"{}\"", w.replace('"', "")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// A teammate's chats matching `query`, most recent first, one hit per chat.
pub async fn search(
    store: &Store,
    teammate: &TeammateId,
    query: &str,
    limit: u32,
) -> Result<Vec<SearchHit>, StoreError> {
    let Some(q) = fts_query(query) else {
        return Ok(Vec::new());
    };
    let teammate = teammate.as_str().to_string();
    let limit = limit.clamp(1, MAX_LIMIT);
    store
        .read(move |c| {
            let mut stmt = c.prepare(
                "SELECT f.thread_id, COALESCE(t.title, ''), f.created_at,
                        snippet(chat_fts, 0, '[', ']', '…', 12)
                   FROM chat_fts f JOIN threads t ON t.id = f.thread_id
                  WHERE chat_fts MATCH ?1 AND f.teammate_id = ?2 AND t.deleted_at IS NULL
                  ORDER BY f.created_at DESC",
            )?;
            let rows = stmt.query_map(params![q, teammate], |r| {
                Ok(SearchHit {
                    chat_id: ThreadId::from_db(r.get(0)?),
                    title: r.get(1)?,
                    date: r.get::<_, i64>(2)?,
                    snippet: r.get(3)?,
                })
            })?;
            let mut out: Vec<SearchHit> = Vec::new();
            for hit in rows {
                let hit = hit?;
                if out.iter().any(|h| h.chat_id == hit.chat_id) {
                    continue;
                }
                out.push(hit);
                if out.len() as u32 >= limit {
                    break;
                }
            }
            Ok(out)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::threads::{self, ChatExtra, NewThread, ThreadKind};
    use tandem_core::access::{Access, ThreadMode};

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

    async fn chat(store: &Store, teammate: &TeammateId, title: &str) -> ThreadId {
        let t = threads::insert_chat(
            store,
            NewThread {
                id: ThreadId::new(),
                kind: ThreadKind::TeammateChat,
                workspace_id: None,
                cwd: "/h".into(),
                engine_id: "claude".into(),
                account_id: None,
                model: None,
                effort: None,
                access: Access::Edits,
                mode: ThreadMode::Act,
                attended: true,
            },
            ChatExtra {
                teammate_id: Some(teammate.clone()),
                title: Some((title.into(), "user")),
                ..Default::default()
            },
        )
        .await
        .expect("chat");
        t.id
    }

    /// A turn: the user's message, a tool with output, reasoning and a reply.
    async fn turn(
        store: &Store,
        thread: &ThreadId,
        user: &str,
        tool_out: &str,
        reply: &str,
    ) -> TurnId {
        let turn = TurnId::new();
        threads::start_turn(store, thread, &turn, "claude", None)
            .await
            .expect("turn");
        let events = [
            (
                "user",
                serde_json::json!({"type": "user", "text": user, "attachments": []}),
            ),
            (
                "tool",
                serde_json::json!({"type": "tool", "id": "t1", "verb": "bash", "argSummary": "grep zebra", "status": "done", "output": tool_out}),
            ),
            (
                "reasoning",
                serde_json::json!({"type": "reasoning", "text": "thinking about quokka"}),
            ),
            (
                "assistant",
                serde_json::json!({"type": "assistant", "markdown": reply}),
            ),
        ];
        for (seq, (kind, payload)) in events.iter().enumerate() {
            threads::append(store, thread, &turn, seq as u64, kind, payload.to_string())
                .await
                .expect("message");
        }
        turn
    }

    #[tokio::test]
    async fn f_ag_11_1_only_prose_is_found_and_tool_output_is_not() {
        let (_d, store) = crate::tests::temp_store().await;
        let a = teammate(&store, "A").await;
        let c = chat(&store, &a, "Weekly deps").await;
        let tr = turn(
            &store,
            &c,
            "check the lockfile",
            "found zebra-lib 1.2",
            "The pelican package is outdated.",
        )
        .await;
        assert_eq!(index_turn(&store, &c, &tr, &a).await.expect("index"), 2);
        assert!(
            search(&store, &a, "zebra", 10).await.expect("s").is_empty(),
            "tool output"
        );
        assert!(
            search(&store, &a, "quokka", 10)
                .await
                .expect("s")
                .is_empty(),
            "reasoning"
        );
        let hits = search(&store, &a, "pelican", 10).await.expect("s");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chat_id, c);
        assert_eq!(hits[0].title, "Weekly deps");
        assert!(hits[0].snippet.contains("[pelican]"), "{}", hits[0].snippet);
        assert_eq!(
            search(&store, &a, "LOCKFILE", 10).await.expect("s").len(),
            1
        );
    }

    #[tokio::test]
    async fn f_ag_11_2_one_teammate_never_finds_anothers_chats() {
        let (_d, store) = crate::tests::temp_store().await;
        let (a, b) = (teammate(&store, "A").await, teammate(&store, "B").await);
        let ca = chat(&store, &a, "A's").await;
        let cb = chat(&store, &b, "B's").await;
        let ta = turn(&store, &ca, "hello", "", "albatross for A").await;
        let tb = turn(&store, &cb, "hello", "", "albatross for B").await;
        index_turn(&store, &ca, &ta, &a).await.expect("a");
        index_turn(&store, &cb, &tb, &b).await.expect("b");
        let hits = search(&store, &a, "albatross", 10).await.expect("s");
        assert_eq!(
            hits.iter().map(|h| h.chat_id.clone()).collect::<Vec<_>>(),
            vec![ca]
        );
    }

    #[test]
    fn f_ag_11_queries_are_quoted_terms_with_no_fts_syntax() {
        assert_eq!(
            fts_query("weekly deps").as_deref(),
            Some("\"weekly\" \"deps\"")
        );
        assert_eq!(
            fts_query("a OR b NEAR(c) \"d\" -e *").as_deref(),
            Some("\"a\" \"OR\" \"b\" \"NEAR\" \"c\" \"d\" \"e\"")
        );
        assert_eq!(fts_query("  ()* "), None);
    }
}
