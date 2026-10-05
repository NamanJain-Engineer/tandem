//! Telemetry uploads (F-TL-01). Off unless the user opts in; both channels
//! talk through `Net`, so every request appears in `TANDEM_NET_LOG`. Keys
//! come from the build (`POSTHOG_KEY`, `SENTRY_DSN`; HUMAN_TODO): without
//! them nothing is ever sent.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use tandem_core::net::Net;
use tandem_core::telemetry::{scrub_crash, Allowlist, Telemetry, ANALYTICS_HOST};
use tandem_core::DataDir;

pub struct TelemetryService {
    pub core: Arc<Telemetry>,
    net: Net,
    posthog_key: Option<String>,
    sentry_dsn: Option<String>,
    crashes: PathBuf,
}

fn build_key(name: &str, baked: Option<&'static str>) -> Option<String> {
    let runtime = if cfg!(debug_assertions) {
        std::env::var(name).ok()
    } else {
        None
    };
    runtime
        .or(baked.map(str::to_string))
        .filter(|k| !k.is_empty())
}

impl TelemetryService {
    pub fn new(data: &DataDir, net: Net) -> Arc<Self> {
        Arc::new(Self {
            core: Arc::new(Telemetry::new(&data.telemetry(), Allowlist::bundled())),
            net,
            posthog_key: build_key("POSTHOG_KEY", option_env!("POSTHOG_KEY")),
            sentry_dsn: build_key("SENTRY_DSN", option_env!("SENTRY_DSN")),
            crashes: data.crashes(),
        })
    }

    /// Applies the two Privacy toggles (UI-112).
    pub fn apply(&self, usage: bool, crash: bool) {
        if let Err(err) = self.core.set_usage(usage) {
            tracing::warn!(error = %err, "couldn't update the analytics id");
        }
        self.core.set_crash(crash);
    }

    /// Sends queued events once a minute while usage statistics are on.
    pub async fn run_uploader(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            self.flush().await;
        }
    }

    pub async fn flush(&self) {
        let (Some(key), true) = (&self.posthog_key, self.core.usage_enabled()) else {
            self.core.drain();
            return;
        };
        let batch = self.core.drain();
        if batch.is_empty() {
            return;
        }
        let body = json!({ "api_key": key, "batch": batch });
        let req = self
            .net
            .client()
            .post(format!("https://{ANALYTICS_HOST}/batch/"))
            .json(&body);
        if let Err(err) = self.net.send(req).await {
            tracing::debug!(error = %err, "usage statistics upload failed");
        }
    }

    /// Uploads crash reports saved by the panic hook, if the user opted in.
    pub async fn upload_crashes(&self) {
        let (Some(dsn), true) = (&self.sentry_dsn, self.core.crash_enabled()) else {
            return;
        };
        let Some((endpoint, auth)) = sentry_endpoint(dsn) else {
            return;
        };
        let Ok(dir) = std::fs::read_dir(&self.crashes) else {
            return;
        };
        for entry in dir.flatten() {
            let path = entry.path();
            let Ok(report) = std::fs::read_to_string(&path) else {
                continue;
            };
            let event_id = uuid::Uuid::new_v4().simple().to_string();
            let event = json!({
                "event_id": event_id,
                "timestamp": chrono::Utc::now().timestamp(),
                "platform": "native",
                "level": "fatal",
                "release": format!("tandem@{}", env!("CARGO_PKG_VERSION")),
                "tags": { "os": std::env::consts::OS },
                "message": { "formatted": report },
            });
            let envelope = format!(
                "{}\n{}\n{}",
                json!({ "event_id": event_id }),
                json!({ "type": "event" }),
                event
            );
            let req = self
                .net
                .client()
                .post(&endpoint)
                .header("x-sentry-auth", &auth)
                .body(envelope);
            if matches!(self.net.send(req).await, Ok(r) if r.status().is_success()) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

/// `https://<key>@<host>/<project>` → envelope URL and auth header.
pub fn sentry_endpoint(dsn: &str) -> Option<(String, String)> {
    let url = url::Url::parse(dsn).ok()?;
    let key = url.username();
    let project = url.path().trim_matches('/');
    if key.is_empty() || project.is_empty() {
        return None;
    }
    let host = url.host_str()?;
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    Some((
        format!("{}://{host}{port}/api/{project}/envelope/", url.scheme()),
        format!(
            "Sentry sentry_version=7, sentry_key={key}, sentry_client=tandem/{}",
            env!("CARGO_PKG_VERSION")
        ),
    ))
}

/// Saves a scrubbed report for each panic; nothing is sent from the hook.
pub fn install_panic_hook(crashes: &Path) {
    let dir = crashes.to_path_buf();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let raw = format!("panicked at {location}: {message}\n{backtrace}");
        let env: Vec<(String, String)> = std::env::vars().collect();
        let report = scrub_crash(&raw, tandem_core::paths::user_home().as_deref(), &env);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join(format!("{}.txt", chrono::Utc::now().timestamp_millis())),
            report,
        );
        previous(info);
    }));
}

/// `telemetry_capture` from the UI.
pub fn capture(svc: &TelemetryService, event: &str, props: &Value) -> bool {
    let props = props.as_object().cloned().unwrap_or_default();
    svc.core.capture(event, &props)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsn_parses_to_envelope_endpoint() {
        let (url, auth) = sentry_endpoint("https://abc123@o1.ingest.sentry.io/42").expect("dsn");
        assert_eq!(url, "https://o1.ingest.sentry.io/api/42/envelope/");
        assert!(auth.contains("sentry_key=abc123"));
        assert!(sentry_endpoint("https://o1.ingest.sentry.io/42").is_none());
    }

    #[tokio::test]
    async fn f_tl_01_1_nothing_is_sent_when_off() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let log = dir.path().join("net.log");
        let svc = TelemetryService::new(&data, Net::with_log(Some(log.clone())));
        svc.apply(false, false);
        assert!(!capture(&svc, "app_started", &json!({"platform": "linux"})));
        svc.flush().await;
        svc.upload_crashes().await;
        let sent = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(
            !sent.contains("posthog") && !sent.contains("sentry"),
            "{sent}"
        );
    }
}
