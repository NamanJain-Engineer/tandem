//! Opening URLs in the system browser. Debug builds honour
//! `TANDEM_OPEN_URL_CMD` (E2E drives a scripted browser through it,
//! MILESTONES §1); release builds always use the OS handler.

use std::path::PathBuf;
use std::process::{Command, Stdio};

pub trait UrlOpener: Send + Sync {
    fn open(&self, url: &str) -> Result<(), String>;
}

/// Runs `TANDEM_OPEN_URL_CMD <url>` without waiting for it.
pub struct CommandOpener {
    program: PathBuf,
}

impl CommandOpener {
    pub fn from_env() -> Option<Self> {
        if !cfg!(debug_assertions) {
            return None;
        }
        std::env::var_os("TANDEM_OPEN_URL_CMD")
            .filter(|v| !v.is_empty())
            .map(|p| Self {
                program: PathBuf::from(p),
            })
    }
}

impl UrlOpener for CommandOpener {
    fn open(&self, url: &str) -> Result<(), String> {
        Command::new(&self.program)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("couldn't run the URL command: {e}"))
    }
}

/// The OS default browser (tauri-plugin-opener).
pub struct SystemOpener;

impl UrlOpener for SystemOpener {
    fn open(&self, url: &str) -> Result<(), String> {
        tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
    }
}

pub fn default_opener() -> std::sync::Arc<dyn UrlOpener> {
    match CommandOpener::from_env() {
        Some(cmd) => std::sync::Arc::new(cmd),
        None => std::sync::Arc::new(SystemOpener),
    }
}

/// Only web and mail links leave the app (`app_open_external`, F-CD-09).
pub fn is_external_url(url: &str) -> bool {
    url::Url::parse(url)
        .map(|u| matches!(u.scheme(), "http" | "https" | "mailto"))
        .unwrap_or(false)
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::sync::Mutex;

    /// Records opened URLs; optionally fails.
    #[derive(Default)]
    pub struct RecordingOpener {
        pub opened: Mutex<Vec<String>>,
        pub fail: bool,
    }

    impl UrlOpener for RecordingOpener {
        fn open(&self, url: &str) -> Result<(), String> {
            if self.fail {
                return Err("no browser".into());
            }
            self.opened
                .lock()
                .map_err(|_| "poisoned".to_string())?
                .push(url.to_string());
            Ok(())
        }
    }

    #[test]
    fn only_web_and_mail_links_are_external() {
        assert!(is_external_url("https://tandem.dev"));
        assert!(is_external_url("mailto:hi@tandem.dev"));
        assert!(!is_external_url("file:///etc/passwd"));
        assert!(!is_external_url("javascript:alert(1)"));
    }
}
