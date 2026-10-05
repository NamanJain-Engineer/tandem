//! Opt-in telemetry (F-TL-01, ARCHITECTURE §16.3). Both channels are off by
//! default. Events pass through the bundled allowlist: unknown events and
//! properties are dropped, and string values go through the redactor and must
//! not look like paths. Turning usage statistics off deletes the analytics id.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::redact::Redactor;

pub const ALLOWLIST_JSON: &str =
    include_str!("../../../packages/protocol/telemetry-allowlist.json");
/// Hosts the two SDK channels talk to; asserted absent from the network log
/// when telemetry is off (E2E-M2-7).
pub const ANALYTICS_HOST: &str = "us.i.posthog.com";
pub const MAX_STRING: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum PropSpec {
    Kind(String),
    OneOf(Vec<String>),
}

#[derive(Debug, Clone, Default, Deserialize)]
struct EventSpec {
    #[serde(default)]
    props: BTreeMap<String, PropSpec>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Allowlist {
    events: BTreeMap<String, EventSpec>,
}

impl Allowlist {
    pub fn bundled() -> Self {
        serde_json::from_str(ALLOWLIST_JSON).unwrap_or_default()
    }

    pub fn has_event(&self, event: &str) -> bool {
        self.events.contains_key(event)
    }

    /// The allowed subset of `props`, or `None` for an unlisted event.
    pub fn filter(&self, event: &str, props: &Map<String, Value>) -> Option<Map<String, Value>> {
        let spec = self.events.get(event)?;
        let mut out = Map::new();
        for (key, value) in props {
            let Some(rule) = spec.props.get(key) else {
                continue;
            };
            if let Some(v) = allowed_value(rule, value) {
                out.insert(key.clone(), v);
            }
        }
        Some(out)
    }
}

fn allowed_value(rule: &PropSpec, value: &Value) -> Option<Value> {
    match (rule, value) {
        (PropSpec::OneOf(values), Value::String(s)) => values.contains(s).then(|| value.clone()),
        (PropSpec::Kind(k), Value::Number(_)) if k == "number" => Some(value.clone()),
        (PropSpec::Kind(k), Value::Bool(_)) if k == "bool" => Some(value.clone()),
        (PropSpec::Kind(k), Value::String(s)) if k == "string" => {
            // No paths, names or anything long enough to be content.
            if s.len() > MAX_STRING || s.contains('/') || s.contains('\\') || s.contains('@') {
                return None;
            }
            let clean = Redactor::global().redact(s);
            (clean == *s).then(|| value.clone())
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CapturedEvent {
    pub event: String,
    pub distinct_id: String,
    pub properties: Map<String, Value>,
    pub timestamp: String,
}

/// Holds the two toggles, the analytics id and the outgoing queue. The
/// uploader in the app drains the queue; nothing here talks to the network.
pub struct Telemetry {
    allow: Allowlist,
    id_path: PathBuf,
    usage: AtomicBool,
    crash: AtomicBool,
    forced_off: AtomicBool,
    queue: Mutex<Vec<CapturedEvent>>,
}

impl Telemetry {
    /// `dir` is `$DATA/telemetry`; the analytics id lives at `dir/analytics-id`.
    pub fn new(dir: &Path, allow: Allowlist) -> Self {
        Self {
            allow,
            id_path: dir.join("analytics-id"),
            usage: AtomicBool::new(false),
            crash: AtomicBool::new(false),
            forced_off: AtomicBool::new(false),
            queue: Mutex::new(Vec::new()),
        }
    }

    pub fn id_path(&self) -> &Path {
        &self.id_path
    }

    pub fn usage_enabled(&self) -> bool {
        self.usage.load(Ordering::Acquire) && !self.forced_off.load(Ordering::Acquire)
    }

    pub fn crash_enabled(&self) -> bool {
        self.crash.load(Ordering::Acquire) && !self.forced_off.load(Ordering::Acquire)
    }

    /// Org policy `forceTelemetryOff`.
    pub fn set_forced_off(&self, forced: bool) {
        self.forced_off.store(forced, Ordering::Release);
    }

    /// On creates the analytics id if missing; off deletes it and the queue.
    pub fn set_usage(&self, on: bool) -> std::io::Result<()> {
        self.usage.store(on, Ordering::Release);
        if on {
            if self.analytics_id().is_none() {
                if let Some(parent) = self.id_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                crate::atomic::atomic_write(
                    &self.id_path,
                    uuid::Uuid::new_v4().to_string().as_bytes(),
                    Some(0o600),
                )?;
            }
        } else {
            if let Ok(mut q) = self.queue.lock() {
                q.clear();
            }
            match std::fs::remove_file(&self.id_path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        Ok(())
    }

    pub fn set_crash(&self, on: bool) {
        self.crash.store(on, Ordering::Release);
    }

    pub fn analytics_id(&self) -> Option<String> {
        std::fs::read_to_string(&self.id_path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Queues an allowlisted event. Returns false (and does nothing) when
    /// usage statistics are off or the event isn't listed.
    pub fn capture(&self, event: &str, props: &Map<String, Value>) -> bool {
        if !self.usage_enabled() {
            return false;
        }
        let (Some(properties), Some(distinct_id)) =
            (self.allow.filter(event, props), self.analytics_id())
        else {
            return false;
        };
        let captured = CapturedEvent {
            event: event.to_string(),
            distinct_id,
            properties,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        match self.queue.lock() {
            Ok(mut q) => {
                q.push(captured);
                if q.len() > 500 {
                    q.remove(0);
                }
                true
            }
            Err(_) => false,
        }
    }

    pub fn drain(&self) -> Vec<CapturedEvent> {
        self.queue
            .lock()
            .map(|mut q| std::mem::take(&mut *q))
            .unwrap_or_default()
    }
}

/// Prepares a crash report for upload: the home directory becomes `~`, every
/// environment variable value is removed, and secrets are redacted.
pub fn scrub_crash(report: &str, home: Option<&Path>, env: &[(String, String)]) -> String {
    let mut out = report.to_string();
    // Longest values first so a value containing another is removed whole.
    let mut values: Vec<&str> = env
        .iter()
        .map(|(_, v)| v.as_str())
        .filter(|v| v.len() >= 4)
        .collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    if let Some(home) = home
        .map(|h| h.to_string_lossy().to_string())
        .filter(|h| h.len() > 1)
    {
        // Home first: env values like HOME and PWD would otherwise erase the path shape.
        out = out.replace(&home, "~");
        values.retain(|v| *v != home);
    }
    for v in values {
        out = out.replace(v, "[env]");
    }
    Redactor::global().redact(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn props(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn f_tl_01_1_off_by_default_captures_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let t = Telemetry::new(dir.path(), Allowlist::bundled());
        assert!(!t.usage_enabled() && !t.crash_enabled());
        assert!(!t.capture("app_started", &props(json!({"platform": "linux"}))));
        assert!(t.drain().is_empty());
        assert!(
            t.analytics_id().is_none(),
            "no id exists until the user opts in"
        );
    }

    #[test]
    fn f_tl_01_2_unlisted_properties_and_events_are_dropped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let t = Telemetry::new(dir.path(), Allowlist::bundled());
        t.set_usage(true).expect("on");
        assert!(t.capture(
            "app_started",
            &props(json!({
                "platform": "linux",
                "version": "0.1.0",
                "email": "ana@example.com",
                "cwd": "/home/ana/code/secret-project",
            }))
        ));
        assert!(
            !t.capture("prompt_sent", &props(json!({"text": "hello"}))),
            "unlisted event"
        );
        // Values that break the rules are dropped even for listed keys.
        assert!(t.capture(
            "app_started",
            &props(json!({"platform": "beos", "version": "/home/ana"}))
        ));
        let events = t.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].properties,
            props(json!({"platform": "linux", "version": "0.1.0"}))
        );
        assert!(events[1].properties.is_empty());
    }

    #[test]
    fn f_tl_01_opt_out_deletes_the_analytics_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let t = Telemetry::new(&dir.path().join("telemetry"), Allowlist::bundled());
        t.set_usage(true).expect("on");
        let id = t.analytics_id().expect("id");
        assert_eq!(id.len(), 36);
        t.set_usage(true).expect("still on");
        assert_eq!(
            t.analytics_id().as_deref(),
            Some(id.as_str()),
            "the id is stable while on"
        );
        t.set_usage(false).expect("off");
        assert!(!t.id_path().exists());
        t.set_usage(true).expect("on again");
        assert_ne!(
            t.analytics_id().expect("new id"),
            id,
            "a fresh id after opting back in"
        );
    }

    #[test]
    fn f_tl_01_org_policy_forces_telemetry_off() {
        let dir = tempfile::tempdir().expect("tempdir");
        let t = Telemetry::new(dir.path(), Allowlist::bundled());
        t.set_usage(true).expect("on");
        t.set_crash(true);
        t.set_forced_off(true);
        assert!(!t.usage_enabled() && !t.crash_enabled());
        assert!(!t.capture("signed_out", &Map::new()));
    }

    #[test]
    fn f_tl_01_3_crash_reports_scrub_home_and_env_values() {
        let env = vec![
            ("HOME".to_string(), "/home/ana".to_string()),
            (
                "DATABASE_URL".to_string(),
                "postgres://u:hunter22@db/prod".to_string(),
            ),
            ("PROJECT_CODENAME".to_string(), "bluefalcon".to_string()),
        ];
        let report = "panicked at /home/ana/code/tandem/src/main.rs:10\n\
                      context: bluefalcon connecting to postgres://u:hunter22@db/prod";
        let out = scrub_crash(report, Some(Path::new("/home/ana")), &env);
        assert!(out.contains("~/code/tandem/src/main.rs:10"), "{out}");
        assert!(!out.contains("/home/ana"));
        assert!(!out.contains("bluefalcon"));
        assert!(!out.contains("hunter22"));
    }

    #[test]
    fn bundled_allowlist_parses() {
        let allow = Allowlist::bundled();
        assert!(allow.has_event("app_started"));
        assert!(allow.has_event("upgrade_sheet_opened"));
    }
}
