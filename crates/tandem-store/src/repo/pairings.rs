//! `pairings` and `phone_queue` (F-RM-03, ARCHITECTURE §4.2, §13.3). Keys
//! live in the keychain; this table holds the device name, grants, frame
//! counters and the revocation time. The counters must only increase.

use rusqlite::{params, OptionalExtension, Row};

use crate::{Store, StoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingRow {
    pub id: String,
    pub device_name: String,
    pub grants_json: String,
    pub epoch: u32,
    pub tx_counter: u64,
    pub rx_counter: u64,
    pub created_at: i64,
    pub last_seen: Option<i64>,
    pub revoked_at: Option<i64>,
}

fn row(r: &Row<'_>) -> rusqlite::Result<PairingRow> {
    Ok(PairingRow {
        id: r.get(0)?,
        device_name: r.get(1)?,
        grants_json: r.get(2)?,
        epoch: r.get(3)?,
        tx_counter: r.get::<_, i64>(4)? as u64,
        rx_counter: r.get::<_, i64>(5)? as u64,
        created_at: r.get(6)?,
        last_seen: r.get(7)?,
        revoked_at: r.get(8)?,
    })
}

const COLUMNS: &str =
    "id, device_name, grants_json, epoch, tx_counter, rx_counter, created_at, last_seen, revoked_at";

pub async fn insert(
    store: &Store,
    id: &str,
    device_name: &str,
    grants_json: &str,
    now: i64,
) -> Result<(), StoreError> {
    let (id, name, grants) = (
        id.to_string(),
        device_name.to_string(),
        grants_json.to_string(),
    );
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO pairings (id, device_name, grants_json, created_at, last_seen) VALUES (?1, ?2, ?3, ?4, ?4)",
                params![id, name, grants, now],
            )?;
            Ok(())
        })
        .await
}

pub async fn get(store: &Store, id: &str) -> Result<Option<PairingRow>, StoreError> {
    let id = id.to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM pairings WHERE id = ?1"),
                [&id],
                row,
            )
            .optional()?)
        })
        .await
}

/// Pairings that aren't revoked, newest first.
pub async fn list_active(store: &Store) -> Result<Vec<PairingRow>, StoreError> {
    store
        .read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM pairings WHERE revoked_at IS NULL ORDER BY created_at DESC, id"
            ))?;
            let rows = stmt.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

pub async fn set_device_name(store: &Store, id: &str, name: &str) -> Result<(), StoreError> {
    let (id, name) = (id.to_string(), name.to_string());
    store
        .write(move |t| {
            t.execute(
                "UPDATE pairings SET device_name = ?2 WHERE id = ?1",
                params![id, name],
            )?;
            Ok(())
        })
        .await
}

pub async fn set_grants(store: &Store, id: &str, grants_json: &str) -> Result<bool, StoreError> {
    let (id, grants) = (id.to_string(), grants_json.to_string());
    store
        .write(move |t| {
            Ok(t.execute(
                "UPDATE pairings SET grants_json = ?2 WHERE id = ?1 AND revoked_at IS NULL",
                params![id, grants],
            )? == 1)
        })
        .await
}

/// Records the counters and epoch after a frame. A counter never goes
/// back: the larger of the stored and the given value is kept.
pub async fn set_counters(
    store: &Store,
    id: &str,
    epoch: u32,
    tx: u64,
    rx: u64,
    seen: Option<i64>,
) -> Result<(), StoreError> {
    let id = id.to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE pairings SET epoch = MAX(epoch, ?2), tx_counter = MAX(tx_counter, ?3),
                   rx_counter = MAX(rx_counter, ?4), last_seen = COALESCE(?5, last_seen) WHERE id = ?1",
                params![id, epoch, tx as i64, rx as i64, seen],
            )?;
            Ok(())
        })
        .await
}

pub async fn revoke(store: &Store, id: &str, now: i64) -> Result<(), StoreError> {
    let id = id.to_string();
    store
        .write(move |t| {
            t.execute(
                "UPDATE pairings SET revoked_at = COALESCE(revoked_at, ?2) WHERE id = ?1",
                params![id, now],
            )?;
            t.execute(
                "UPDATE phone_queue SET state = 'rejected' WHERE pairing_id = ?1 AND state = 'queued'",
                [&id],
            )?;
            Ok(())
        })
        .await
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueRow {
    pub id: String,
    pub pairing_id: String,
    pub action_json: String,
    pub state: String,
    pub created_at: i64,
}

fn queue_row(r: &Row<'_>) -> rusqlite::Result<QueueRow> {
    Ok(QueueRow {
        id: r.get(0)?,
        pairing_id: r.get(1)?,
        action_json: r.get(2)?,
        state: r.get(3)?,
        created_at: r.get(4)?,
    })
}

pub async fn queue_insert(
    store: &Store,
    id: &str,
    pairing_id: &str,
    action_json: &str,
    now: i64,
) -> Result<(), StoreError> {
    let (id, pid, action) = (
        id.to_string(),
        pairing_id.to_string(),
        action_json.to_string(),
    );
    store
        .write(move |t| {
            t.execute(
                "INSERT INTO phone_queue (id, pairing_id, action_json, state, created_at) VALUES (?1, ?2, ?3, 'queued', ?4)",
                params![id, pid, action, now],
            )?;
            Ok(())
        })
        .await
}

/// Queued items, oldest first.
pub async fn queue_list(store: &Store) -> Result<Vec<QueueRow>, StoreError> {
    store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT id, pairing_id, action_json, state, created_at FROM phone_queue
                 WHERE state = 'queued' ORDER BY created_at, id",
            )?;
            let rows = stmt
                .query_map([], queue_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

pub async fn queue_get(store: &Store, id: &str) -> Result<Option<QueueRow>, StoreError> {
    let id = id.to_string();
    store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT id, pairing_id, action_json, state, created_at FROM phone_queue WHERE id = ?1",
                [&id],
                queue_row,
            )
            .optional()?)
        })
        .await
}

/// Settles a queued item; false when it was already settled.
pub async fn queue_settle(store: &Store, id: &str, applied: bool) -> Result<bool, StoreError> {
    let id = id.to_string();
    let state = if applied { "applied" } else { "rejected" };
    store
        .write(move |t| {
            Ok(t.execute(
                "UPDATE phone_queue SET state = ?2 WHERE id = ?1 AND state = 'queued'",
                params![id, state],
            )? == 1)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn f_rm_03_pairings_round_trip_and_counters_never_go_back() {
        let (_d, store) = crate::tests::temp_store().await;
        insert(&store, "pr_one", "Phone", "{}", 10)
            .await
            .expect("insert");
        insert(&store, "pr_two", "Phone", "{}", 20)
            .await
            .expect("insert");
        let active = list_active(&store).await.expect("list");
        assert_eq!(
            active.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["pr_two", "pr_one"]
        );
        set_counters(&store, "pr_one", 1, 5, 9, Some(30))
            .await
            .expect("counters");
        set_counters(&store, "pr_one", 0, 3, 2, None)
            .await
            .expect("older");
        let one = get(&store, "pr_one").await.expect("get").expect("row");
        assert_eq!((one.epoch, one.tx_counter, one.rx_counter), (1, 5, 9));
        assert_eq!(one.last_seen, Some(30));
        set_device_name(&store, "pr_one", "iPhone · Safari")
            .await
            .expect("name");
        assert!(set_grants(&store, "pr_one", r#"{"chat":true}"#)
            .await
            .expect("grants"));
        revoke(&store, "pr_one", 40).await.expect("revoke");
        assert!(!set_grants(&store, "pr_one", "{}").await.expect("grants"));
        let one = get(&store, "pr_one").await.expect("get").expect("row");
        assert_eq!(one.revoked_at, Some(40));
        assert_eq!(one.device_name, "iPhone · Safari");
        assert_eq!(list_active(&store).await.expect("list").len(), 1);
    }

    #[tokio::test]
    async fn f_rm_03_3_queued_items_settle_once_and_revoke_rejects_them() {
        let (_d, store) = crate::tests::temp_store().await;
        insert(&store, "pr_q", "Phone", "{}", 1)
            .await
            .expect("insert");
        queue_insert(&store, "pq_1", "pr_q", "{}", 2)
            .await
            .expect("queue");
        queue_insert(&store, "pq_2", "pr_q", "{}", 3)
            .await
            .expect("queue");
        assert_eq!(queue_list(&store).await.expect("list").len(), 2);
        assert!(queue_settle(&store, "pq_1", true).await.expect("settle"));
        assert!(!queue_settle(&store, "pq_1", false).await.expect("again"));
        assert_eq!(
            queue_get(&store, "pq_1")
                .await
                .expect("get")
                .expect("row")
                .state,
            "applied"
        );
        revoke(&store, "pr_q", 4).await.expect("revoke");
        assert!(queue_list(&store).await.expect("list").is_empty());
        assert_eq!(
            queue_get(&store, "pq_2")
                .await
                .expect("get")
                .expect("row")
                .state,
            "rejected"
        );
    }
}
