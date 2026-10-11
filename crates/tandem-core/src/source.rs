//! `SourceRef`: where a dashboard card, notification or approval came from,
//! and where focusing it lands (ARCHITECTURE §3.2 "Status, dashboard,
//! notifications, focus").

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum SourceRef {
    Pane {
        pane_id: String,
    },
    Thread {
        thread_id: String,
    },
    TeammateChat {
        teammate_id: String,
        thread_id: String,
    },
    /// A teammate's page in Agent mode (Find work, F-CN-05).
    Teammate {
        teammate_id: String,
    },
    Routine {
        routine_id: String,
        run_id: Option<String>,
    },
    Task {
        issue_id: String,
    },
    Lane {
        workspace_id: String,
    },
    Update,
    /// Settings › Account & Plan (the credit-low item, F-DB-02).
    Account,
    Plugin {
        connection_id: String,
    },
    /// Test harness only: a scripted agent (E2E fixture source).
    Fixture {
        fixture_id: String,
    },
}

impl SourceRef {
    /// The status-bus key of a source that has one (panes, threads, fixtures).
    pub fn session_key(&self) -> Option<crate::status::SessionKey> {
        match self {
            SourceRef::Pane { pane_id } => Some(crate::status::SessionKey::pane(pane_id)),
            SourceRef::Thread { thread_id } | SourceRef::TeammateChat { thread_id, .. } => {
                Some(crate::status::SessionKey::thread(thread_id))
            }
            SourceRef::Fixture { fixture_id } => {
                Some(crate::status::SessionKey(format!("fixture:{fixture_id}")))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_the_ipc_shape() {
        let pane = SourceRef::Pane {
            pane_id: "pn_1".into(),
        };
        assert_eq!(
            serde_json::to_value(&pane).expect("json"),
            serde_json::json!({"type": "pane", "paneId": "pn_1"})
        );
        let chat: SourceRef = serde_json::from_value(
            serde_json::json!({"type": "teammate_chat", "teammateId": "tm_1", "threadId": "th_1"}),
        )
        .expect("parse");
        assert_eq!(
            chat.session_key().map(|k| k.0).as_deref(),
            Some("thread:th_1")
        );
        assert_eq!(
            serde_json::to_value(SourceRef::Update).expect("json"),
            serde_json::json!({"type": "update"})
        );
    }
}
