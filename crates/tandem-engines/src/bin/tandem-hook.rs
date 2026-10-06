//! `tandem-hook`: run by engine hooks inside Tandem panes (ARCHITECTURE §6.6).
//!
//! `tandem-hook claude <Event>` (hook JSON on stdin) or
//! `tandem-hook codex '<json>'`. Prints nothing, always exits 0, and does
//! nothing at all outside a Tandem pane.

use std::time::{SystemTime, UNIX_EPOCH};

use tandem_engines::hook::{self, PANE_VAR, POST_TIMEOUT};
use tandem_engines::launch::{STATUS_SECRET_VAR, STATUS_URL_VAR};

fn main() {
    let (Ok(pane), Ok(secret), Ok(url)) = (
        std::env::var(PANE_VAR),
        std::env::var(STATUS_SECRET_VAR),
        std::env::var(STATUS_URL_VAR),
    ) else {
        return;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default();
    let mut args = std::env::args().skip(1);
    let report = match (args.next().as_deref(), args.next()) {
        (Some("claude"), Some(event)) => {
            let stdin = hook::read_limited(std::io::stdin().lock());
            hook::claude_report(&event, &stdin, &pane, now)
        }
        (Some("codex"), Some(json)) => hook::codex_report(&json, &pane, now),
        _ => None,
    };
    let Some(report) = report else { return };
    // Debug builds can record what's posted, so tests can check it.
    if cfg!(debug_assertions) {
        if let (Some(path), Ok(body)) = (
            std::env::var_os("TANDEM_HOOK_LOG"),
            serde_json::to_string(&report),
        ) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{body}");
            }
        }
    }
    if let Some(req) = hook::request(&report, &secret) {
        let _ = hook::post(&url, &req, POST_TIMEOUT);
    }
}
