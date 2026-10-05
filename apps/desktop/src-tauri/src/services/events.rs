//! Forwards bus events to the webview as `<domain>:<event>` (ARCHITECTURE §3.3).

use tauri::{AppHandle, Emitter};

use tandem_core::bus::{EventBus, Topic};

pub fn forward(app: AppHandle, bus: &EventBus) {
    for topic in Topic::ALL {
        let mut rx = bus.subscribe(topic);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        if let Err(err) = app.emit_to("main", &ev.name, ev.payload) {
                            tracing::debug!(error = %err, "couldn't emit an event");
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "event forwarder lagged");
                    }
                    Err(_) => return,
                }
            }
        });
    }
}
