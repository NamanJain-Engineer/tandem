//! The one status store every surface reads (F-EN-05, F-DB-01).

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AgentStatus {
    Working,
    Waiting,
    Done,
    Idle,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum WaitingReason {
    Approval,
    Question,
    Input,
}

/// `pane:<id>` or `thread:<id>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(export)]
pub struct SessionKey(pub String);

impl SessionKey {
    pub fn pane(id: &str) -> Self {
        Self(format!("pane:{id}"))
    }
    pub fn thread(id: &str) -> Self {
        Self(format!("thread:{id}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StatusRecord {
    pub key: SessionKey,
    pub status: AgentStatus,
    pub reason: Option<WaitingReason>,
    /// When the session entered `status` (ms).
    pub since: i64,
    /// Timestamp of the report that produced this record (ms).
    pub ts: i64,
    pub tool: Option<String>,
    pub title: Option<String>,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StatusUpdate {
    pub status: AgentStatus,
    pub reason: Option<WaitingReason>,
    pub ts: i64,
    pub tool: Option<String>,
    pub title: Option<String>,
    pub preview: Option<String>,
}

#[derive(Default)]
pub struct StatusStore {
    records: DashMap<SessionKey, StatusRecord>,
}

impl StatusStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies an update. Returns the new record, or `None` when the update is
    /// older than the current record (stale reports are dropped).
    pub fn apply(&self, key: &SessionKey, update: StatusUpdate) -> Option<StatusRecord> {
        let mut entry = self.records.entry(key.clone());
        let next = match &mut entry {
            dashmap::Entry::Occupied(current) => {
                let cur = current.get();
                if update.ts < cur.ts {
                    return None;
                }
                let since = if cur.status == update.status {
                    cur.since
                } else {
                    update.ts
                };
                StatusRecord {
                    key: key.clone(),
                    status: update.status,
                    reason: update.reason,
                    since,
                    ts: update.ts,
                    tool: update.tool,
                    title: update.title.or_else(|| cur.title.clone()),
                    preview: update.preview.or_else(|| cur.preview.clone()),
                }
            }
            dashmap::Entry::Vacant(_) => StatusRecord {
                key: key.clone(),
                status: update.status,
                reason: update.reason,
                since: update.ts,
                ts: update.ts,
                tool: update.tool,
                title: update.title,
                preview: update.preview,
            },
        };
        entry.insert(next.clone());
        Some(next)
    }

    pub fn get(&self, key: &SessionKey) -> Option<StatusRecord> {
        self.records.get(key).map(|r| r.clone())
    }

    pub fn remove(&self, key: &SessionKey) -> Option<StatusRecord> {
        self.records.remove(key).map(|(_, r)| r)
    }

    pub fn snapshot(&self) -> Vec<StatusRecord> {
        let mut all: Vec<_> = self.records.iter().map(|r| r.clone()).collect();
        all.sort_by(|a, b| a.key.0.cmp(&b.key.0));
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(status: AgentStatus, ts: i64) -> StatusUpdate {
        StatusUpdate {
            status,
            reason: None,
            ts,
            tool: None,
            title: None,
            preview: None,
        }
    }

    #[test]
    fn stale_updates_are_dropped_and_since_tracks_transitions() {
        let store = StatusStore::new();
        let key = SessionKey::pane("pn_1");
        store
            .apply(&key, update(AgentStatus::Working, 100))
            .expect("first");
        let same = store
            .apply(&key, update(AgentStatus::Working, 150))
            .expect("same");
        assert_eq!(same.since, 100);
        assert!(store.apply(&key, update(AgentStatus::Done, 90)).is_none());
        let done = store
            .apply(&key, update(AgentStatus::Done, 200))
            .expect("done");
        assert_eq!(done.since, 200);
        assert_eq!(store.snapshot().len(), 1);
    }
}
