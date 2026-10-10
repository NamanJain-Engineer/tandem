//! F-TL-06 rule 7: nothing leaves the machine without an opt-in (F-TL-01).
//! Every Privacy switch starts off, Dictate starts local, and with
//! telemetry off nothing is sent. Auto, cloud STT, Conductor and Enhance
//! add their cases here as M8 and M13 build them.

use serde_json::{json, Value};
use tandem_app::services::telemetry::{capture, TelemetryService};
use tandem_core::net::Net;
use tandem_core::settings::{default_value, SettingKey, SettingsPage, Validator, SCHEMA};
use tandem_core::DataDir;

#[test]
fn invariant_rule_7_every_privacy_switch_starts_off() {
    let switches: Vec<_> = SCHEMA
        .iter()
        .filter(|s| s.page == SettingsPage::Privacy && matches!(s.validator, Validator::Bool))
        .collect();
    assert!(switches.len() >= 4, "privacy switches: {}", switches.len());
    for s in switches {
        assert_eq!((s.default)(), Value::Bool(false), "{:?}", s.key);
    }
    for key in [
        SettingKey::TelemetryUsage,
        SettingKey::TelemetryCrash,
        SettingKey::AiThreadTitles,
    ] {
        assert_eq!(default_value(key), json!(false), "{key:?}");
    }
    assert_eq!(default_value(SettingKey::DictateEngine), json!("local"));
}

#[tokio::test]
async fn invariant_rule_7_telemetry_off_sends_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data = DataDir::at(dir.path().to_path_buf()).expect("data");
    let log = dir.path().join("net.log");
    let svc = TelemetryService::new(&data, Net::with_log(Some(log.clone())));
    svc.apply(false, false);
    assert!(!capture(&svc, "app_started", &json!({"platform": "linux"})));
    svc.flush().await;
    svc.upload_crashes().await;
    assert_eq!(
        std::fs::read_to_string(&log).unwrap_or_default(),
        "",
        "requests while off"
    );
}
