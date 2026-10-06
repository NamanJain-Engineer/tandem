//! The approval broker's app side (F-TH-11, F-DB-01): every request and
//! decision is recorded in `approvals` and forwarded as `approvals:changed`.

use std::sync::Arc;

use tandem_core::approvals::{ApprovalBroker, ApprovalEvent};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::clock::Clock;
use tandem_store::repo::approvals as repo;
use tandem_store::Store;
use tokio::sync::broadcast::error::RecvError;

pub struct ApprovalService {
    pub broker: Arc<ApprovalBroker>,
    store: Store,
    bus: EventBus,
}

impl ApprovalService {
    pub fn new(store: Store, bus: EventBus, clock: Arc<dyn Clock>) -> Arc<Self> {
        Arc::new(Self {
            broker: ApprovalBroker::new(clock),
            store,
            bus,
        })
    }

    /// Settles what a crash left pending, then follows the broker.
    pub fn start(self: &Arc<Self>) {
        let me = self.clone();
        let mut events = self.broker.subscribe();
        tauri::async_runtime::spawn(async move {
            if let Err(err) = repo::cancel_stale(&me.store).await {
                tracing::warn!(error = %err, "couldn't settle stale approvals");
            }
            loop {
                match events.recv().await {
                    Ok(event) => me.on_event(event).await,
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(missed = n, "approval events lagged")
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }

    async fn on_event(&self, event: ApprovalEvent) {
        match event {
            ApprovalEvent::Requested(view) => {
                if let Err(err) = repo::insert_pending(&self.store, &view).await {
                    tracing::warn!(error = %err, "couldn't record an approval");
                }
                self.bus.publish(BusEvent::new(
                    Topic::Approvals,
                    "approvals:changed",
                    serde_json::json!({ "upserted": [view], "resolved": [] }),
                ));
            }
            ApprovalEvent::Resolved {
                view,
                state,
                decision,
                via,
            } => {
                if let Err(err) =
                    repo::finish(&self.store, &view, &state, decision.as_ref(), via).await
                {
                    tracing::warn!(error = %err, "couldn't record an approval decision");
                }
                self.bus.publish(BusEvent::new(
                    Topic::Approvals,
                    "approvals:changed",
                    serde_json::json!({ "upserted": [], "resolved": [view.id] }),
                ));
            }
        }
    }
}
