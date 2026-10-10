//! Forwards bus events to the webview as `<domain>:<event>` (ARCHITECTURE §3.3).
//! This is the status bus of PRODUCT_SPEC §5.2: each topic's forwarder runs
//! under the watchdog and beats while it waits.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use tandem_core::bus::{EventBus, Topic};
use tandem_core::clock::Clock;
use tandem_core::supervisor::{self, Supervision};

/// A forwarder waits at most this long before beating again.
const BEAT: Duration = Duration::from_secs(1);

pub fn forward(app: AppHandle, bus: &EventBus, clock: Arc<dyn Clock>) {
    for topic in Topic::ALL {
        let name = format!("status bus/{}", topic_name(topic));
        let sup = Supervision {
            clock: clock.clone(),
            bus: bus.clone(),
            watchdog: supervisor::watchdog(),
        };
        let (app, bus) = (app.clone(), bus.clone());
        tauri::async_runtime::spawn(async move {
            let beat_name = name.clone();
            supervisor::supervise_task(&name, sup, Some(BEAT * 10), move || {
                // A restart subscribes again; what was in flight is lost, and
                // the UI's stores re-read on the next event they get.
                let mut rx = bus.subscribe(topic);
                let (app, name) = (app.clone(), beat_name.clone());
                async move {
                    let dog = supervisor::watchdog();
                    loop {
                        dog.beat(&name);
                        let next = match tokio::time::timeout(BEAT, rx.recv()).await {
                            Err(_) => continue,
                            Ok(next) => next,
                        };
                        match next {
                            Ok(ev) => {
                                if let Err(err) = app.emit_to("main", &ev.name, ev.payload) {
                                    tracing::debug!(error = %err, "couldn't emit an event");
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                tracing::warn!(skipped = n, "event forwarder lagged");
                            }
                            Err(_) => return Ok(()),
                        }
                    }
                }
            })
            .await;
        });
    }
}

fn topic_name(topic: Topic) -> String {
    serde_json::to_value(topic)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{topic:?}"))
}
