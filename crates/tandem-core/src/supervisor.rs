//! Restarts failed subsystem tasks at most 3 times per 10 minutes, then reports
//! the failure (PRODUCT_SPEC §5.2 watchdogs).

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use crate::bus::{BusEvent, EventBus, Topic};
use crate::clock::Clock;

pub const MAX_RESTARTS: usize = 3;
pub const WINDOW: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubsystemFailed {
    pub name: String,
    pub restarts: usize,
    pub last_error: String,
}

/// Runs `make()` until it returns `Ok(())`. Each `Err` restarts it unless the
/// restart budget for the window is spent, in which case `app:subsystem-failed`
/// is published and supervision stops.
pub async fn supervise<F, Fut>(name: &str, clock: Arc<dyn Clock>, bus: EventBus, mut make: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let mut restarts: Vec<i64> = Vec::new();
    loop {
        match make().await {
            Ok(()) => return,
            Err(err) => {
                let now = clock.now_ms();
                restarts.retain(|t| now - *t < WINDOW.as_millis() as i64);
                if restarts.len() >= MAX_RESTARTS {
                    tracing::error!(subsystem = name, "subsystem failed after restarts");
                    bus.publish(BusEvent::new(
                        Topic::App,
                        "app:subsystem-failed",
                        SubsystemFailed {
                            name: name.to_string(),
                            restarts: restarts.len(),
                            last_error: err,
                        },
                    ));
                    return;
                }
                restarts.push(now);
                tracing::warn!(
                    subsystem = name,
                    attempt = restarts.len(),
                    "restarting subsystem"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::MockClock;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn gives_up_after_three_restarts_and_reports() {
        let clock = Arc::new(MockClock::new(chrono::Utc::now()));
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::App);
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = runs.clone();
        supervise("gateway", clock, bus, move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Err("boom".to_string())
            }
        })
        .await;
        assert_eq!(runs.load(Ordering::SeqCst), 4);
        let event = rx.recv().await.expect("event");
        assert_eq!(event.name, "app:subsystem-failed");
    }
}
