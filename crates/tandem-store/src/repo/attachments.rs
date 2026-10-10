//! Thread attachments (F-TH-07): files, folders and pasted images a message
//! carries. Unsent ones (`message_id` NULL) belong to the composer draft.

use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tandem_core::ids::{AttachmentId, ThreadId};
use ts_rs::TS;

use crate::{now_ms, Store, StoreError};

/// One file a message may carry: 20 MB (F-TH-07).
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AttachmentKind {
    Image,
    File,
    Folder,
    ToolOutput,
}

impl AttachmentKind {
    fn as_str(self) -> &'static str {
        match self {
            AttachmentKind::Image => "image",
            AttachmentKind::File => "file",
            AttachmentKind::Folder => "folder",
            AttachmentKind::ToolOutput => "tool_output",
        }
    }

    fn parse(s: &str) -> AttachmentKind {
        match s {
            "image" => AttachmentKind::Image,
            "folder" => AttachmentKind::Folder,
            "tool_output" => AttachmentKind::ToolOutput,
            _ => AttachmentKind::File,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Attachment {
    pub id: AttachmentId,
    pub thread_id: ThreadId,
    pub kind: AttachmentKind,
    /// Absolute: the copy under `$DATA/attachments/<thread>/` or the path
    /// that was attached.
    pub path: String,
    pub name: String,
    pub mime: Option<String>,
    pub bytes: u64,
    /// Set once the message carrying it was sent.
    pub sent: bool,
    /// "Too large (max 20 MB)": such a chip can't be sent.
    pub error: Option<String>,
}

const COLUMNS: &str = "id, thread_id, kind, path, name, mime, bytes, message_id";

fn from_row(r: &Row) -> Result<Attachment, rusqlite::Error> {
    let bytes = r.get::<_, i64>(6)?.max(0) as u64;
    Ok(Attachment {
        id: AttachmentId::from_db(r.get(0)?),
        thread_id: ThreadId::from_db(r.get(1)?),
        kind: AttachmentKind::parse(&r.get::<_, String>(2)?),
        path: r.get(3)?,
        name: r.get(4)?,
        mime: r.get(5)?,
        bytes,
        sent: r.get::<_, Option<i64>>(7)?.is_some(),
        error: (bytes > MAX_FILE_BYTES).then(|| "Too large (max 20 MB)".to_string()),
    })
}

pub async fn insert(store: &Store, a: Attachment) -> Result<Attachment, StoreError> {
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO attachments (id, thread_id, kind, path, name, mime, bytes, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    a.id.as_str(),
                    a.thread_id.as_str(),
                    a.kind.as_str(),
                    a.path,
                    a.name,
                    a.mime,
                    a.bytes as i64,
                    now_ms()
                ],
            )?;
            Ok(a)
        })
        .await
}

/// The thread's attachments with these ids, in the order asked; unknown ids
/// and other threads' attachments are left out.
pub async fn get_many(
    store: &Store,
    thread: &ThreadId,
    ids: &[AttachmentId],
) -> Result<Vec<Attachment>, StoreError> {
    let thread = thread.as_str().to_string();
    let ids: Vec<String> = ids.iter().map(|i| i.as_str().to_string()).collect();
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM attachments WHERE id = ?1 AND thread_id = ?2"
            ))?;
            let mut out = Vec::new();
            for id in ids {
                if let Some(a) = stmt.query_row(params![id, thread], from_row).optional()? {
                    out.push(a);
                }
            }
            Ok(out)
        })
        .await
}

/// The composer's chips: attachments not sent yet, oldest first.
pub async fn unsent(store: &Store, thread: &ThreadId) -> Result<Vec<Attachment>, StoreError> {
    let thread = thread.as_str().to_string();
    store
        .read(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM attachments
                 WHERE thread_id = ?1 AND message_id IS NULL AND kind != 'tool_output'
                 ORDER BY created_at, rowid"
            ))?;
            let rows = stmt.query_map([thread], from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// Removes an attachment row; returns it so the caller can delete a copy
/// it owns.
pub async fn remove(store: &Store, id: &AttachmentId) -> Result<Option<Attachment>, StoreError> {
    let id = id.as_str().to_string();
    store
        .write(move |t| {
            let found = t
                .query_row(
                    &format!("SELECT {COLUMNS} FROM attachments WHERE id = ?1"),
                    [&id],
                    from_row,
                )
                .optional()?;
            t.execute("DELETE FROM attachments WHERE id = ?1", [&id])?;
            Ok(found)
        })
        .await
}

/// Ties attachments to the message that carried them.
pub async fn mark_sent(
    store: &Store,
    ids: &[AttachmentId],
    message_id: i64,
) -> Result<(), StoreError> {
    let ids: Vec<String> = ids.iter().map(|i| i.as_str().to_string()).collect();
    store
        .write(move |t| {
            for id in ids {
                t.execute(
                    "UPDATE attachments SET message_id = ?2 WHERE id = ?1",
                    params![id, message_id],
                )?;
            }
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::threads::{self, NewThread, ThreadKind};
    use tandem_core::access::{Access, ThreadMode};
    use tandem_core::DataDir;

    async fn setup() -> (tempfile::TempDir, Store, ThreadId) {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let id = ThreadId::new();
        threads::insert(
            &store,
            NewThread {
                id: id.clone(),
                kind: ThreadKind::Thread,
                workspace_id: None,
                cwd: "/tmp".into(),
                engine_id: "claude".into(),
                account_id: None,
                model: None,
                effort: None,
                access: Access::Read,
                mode: ThreadMode::Act,
                attended: true,
            },
        )
        .await
        .expect("thread");
        (dir, store, id)
    }

    fn att(thread: &ThreadId, name: &str, bytes: u64) -> Attachment {
        Attachment {
            id: AttachmentId::new(),
            thread_id: thread.clone(),
            kind: AttachmentKind::File,
            path: format!("/tmp/{name}"),
            name: name.into(),
            mime: None,
            bytes,
            sent: false,
            error: None,
        }
    }

    #[tokio::test]
    async fn f_th_07_unsent_attachments_are_the_draft_until_sent() {
        let (_d, store, thread) = setup().await;
        let a = insert(&store, att(&thread, "a.ts", 10)).await.expect("a");
        let big = insert(&store, att(&thread, "big.mov", MAX_FILE_BYTES + 1))
            .await
            .expect("big");
        let listed = unsent(&store, &thread).await.expect("unsent");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[1].error.as_deref(), Some("Too large (max 20 MB)"));
        // Asked-for order; another thread's id is ignored.
        let other = ThreadId::new();
        let got = get_many(&store, &thread, &[big.id.clone(), a.id.clone()])
            .await
            .expect("get");
        assert_eq!(
            got.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            ["big.mov", "a.ts"]
        );
        assert!(get_many(&store, &other, std::slice::from_ref(&a.id))
            .await
            .expect("get")
            .is_empty());
        let removed = remove(&store, &big.id).await.expect("remove");
        assert_eq!(removed.map(|r| r.name), Some("big.mov".into()));
        let turn = tandem_core::ids::TurnId::new();
        threads::start_turn(&store, &thread, &turn, "claude", None)
            .await
            .expect("turn");
        let row = threads::append(&store, &thread, &turn, 0, "user", "{}".into())
            .await
            .expect("row");
        mark_sent(&store, std::slice::from_ref(&a.id), row)
            .await
            .expect("sent");
        assert!(unsent(&store, &thread).await.expect("unsent").is_empty());
        let sent = get_many(&store, &thread, &[a.id]).await.expect("get");
        assert!(sent[0].sent);
    }
}
