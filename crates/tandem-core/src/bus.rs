//! In-process pub/sub. The Tauri layer forwards selected topics to the webview
//! as `<domain>:<event>` events (ARCHITECTURE §3.3).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use ts_rs::TS;

pub const CAPACITY: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Topic {
    Status,
    Notifications,
    Threads,
    Approvals,
    Entitlement,
    Engines,
    Routines,
    Tasks,
    Plugins,
    Voice,
    Conductor,
    Updates,
    Remote,
    /// Per-pane terminal state (`pty:state`).
    Pty,
    App,
}

impl Topic {
    pub const ALL: [Topic; 15] = [
        Topic::Status,
        Topic::Notifications,
        Topic::Threads,
        Topic::Approvals,
        Topic::Entitlement,
        Topic::Engines,
        Topic::Routines,
        Topic::Tasks,
        Topic::Plugins,
        Topic::Voice,
        Topic::Conductor,
        Topic::Updates,
        Topic::Remote,
        Topic::Pty,
        Topic::App,
    ];
}

/// One event on the bus. `name` is the webview event name (`status:changed`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BusEvent {
    pub topic: Topic,
    pub name: String,
    pub payload: Value,
}

impl BusEvent {
    pub fn new(topic: Topic, name: impl Into<String>, payload: impl Serialize) -> Self {
        Self {
            topic,
            name: name.into(),
            payload: serde_json::to_value(payload).unwrap_or(Value::Null),
        }
    }
}

#[derive(Clone)]
pub struct EventBus {
    channels: Arc<HashMap<Topic, broadcast::Sender<BusEvent>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let channels = Topic::ALL
            .iter()
            .map(|t| (*t, broadcast::channel(CAPACITY).0))
            .collect();
        Self {
            channels: Arc::new(channels),
        }
    }

    /// Events queued per topic that its slowest subscriber hasn't taken yet
    /// (Help › Diagnostics, §5.7).
    pub fn depths(&self) -> Vec<(Topic, usize)> {
        Topic::ALL
            .iter()
            .map(|t| (*t, self.channels.get(t).map(|tx| tx.len()).unwrap_or(0)))
            .collect()
    }

    /// Publishing with no subscribers is not an error.
    pub fn publish(&self, event: BusEvent) {
        if let Some(tx) = self.channels.get(&event.topic) {
            let _ = tx.send(event);
        }
    }

    pub fn subscribe(&self, topic: Topic) -> broadcast::Receiver<BusEvent> {
        match self.channels.get(&topic) {
            Some(tx) => tx.subscribe(),
            // Every topic has a channel; this arm only exists for exhaustiveness.
            None => broadcast::channel(1).1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribers_only_see_their_topic() {
        let bus = EventBus::new();
        let mut status = bus.subscribe(Topic::Status);
        let mut threads = bus.subscribe(Topic::Threads);
        bus.publish(BusEvent::new(Topic::Status, "status:changed", 1));
        let got = status.recv().await.expect("event");
        assert_eq!(got.name, "status:changed");
        assert!(threads.try_recv().is_err());
    }
}
