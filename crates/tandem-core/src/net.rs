//! The one HTTP client the app uses. Every request goes through [`Net::send`],
//! which appends `METHOD scheme://host/path status ms` to `TANDEM_NET_LOG`
//! when that variable is set (tests prove what leaves the machine, F-TL-01).
//! Query strings, headers and bodies are never logged.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::{Client, RequestBuilder, Response};

#[derive(Clone)]
pub struct Net {
    client: Client,
    log: Option<Arc<Mutex<std::fs::File>>>,
}

impl Net {
    /// Reads `TANDEM_NET_LOG` (a file path) once.
    pub fn from_env() -> Self {
        Self::with_log(std::env::var_os("TANDEM_NET_LOG").map(PathBuf::from))
    }

    pub fn with_log(path: Option<PathBuf>) -> Self {
        let log = path.and_then(|p| {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&p)
                .map_err(|e| tracing::warn!(error = %e, "couldn't open the network log"))
                .ok()
                .map(|f| Arc::new(Mutex::new(f)))
        });
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("Tandem/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Self { client, log }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Sends a request built from [`Net::client`] and logs where it went.
    pub async fn send(&self, req: RequestBuilder) -> reqwest::Result<Response> {
        let req = req.build()?;
        let line = describe(req.method().as_str(), req.url());
        let started = Instant::now();
        let result = self.client.execute(req).await;
        let status = match &result {
            Ok(r) => r.status().as_u16().to_string(),
            Err(_) => "error".to_string(),
        };
        self.record(&format!(
            "{line} {status} {}ms",
            started.elapsed().as_millis()
        ));
        result
    }

    /// Logs a long-lived stream (SSE) when it opens.
    pub fn record(&self, line: &str) {
        if let Some(log) = &self.log {
            if let Ok(mut f) = log.lock() {
                let _ = writeln!(f, "{} {line}", chrono::Utc::now().to_rfc3339());
            }
        }
    }
}

/// `GET https://api.example.com/v1/me` — no query, no fragment, no userinfo.
pub fn describe(method: &str, url: &reqwest::Url) -> String {
    let host = url.host_str().unwrap_or("");
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    format!("{method} {}://{host}{port}{}", url.scheme(), url.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_drops_query_and_credentials() {
        let url = reqwest::Url::parse("https://user:pw@api.tandem.dev:8443/v1/me?token=abc#x")
            .expect("url");
        assert_eq!(
            describe("GET", &url),
            "GET https://api.tandem.dev:8443/v1/me"
        );
    }

    #[tokio::test]
    async fn every_request_is_logged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("net.log");
        let net = Net::with_log(Some(path.clone()));
        // Nothing listens on port 9; the failure is still recorded.
        let _ = net
            .send(net.client().get("http://127.0.0.1:9/v1/health?q=secret"))
            .await;
        let log = std::fs::read_to_string(&path).expect("log");
        assert!(
            log.contains("GET http://127.0.0.1:9/v1/health error"),
            "{log}"
        );
        assert!(!log.contains("secret"));
    }
}
