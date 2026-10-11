//! F-TL-06 rule 7: nothing leaves the machine without an opt-in (F-TL-01).
//! Every Privacy switch starts off, Dictate starts local, and with
//! telemetry off nothing is sent. M13's hosted features have two more
//! rule-7 tests next to their mechanism, because they need the app's
//! private sign-in rig: `services::hosted::tests::
//! invariant_rule_7_no_egress_without_optin` (a signed-in session with every
//! opt-in off sends nothing: AI titles, audit, telemetry) and
//! `invariant_rule_7_every_hosted_feature_defaults_to_off`
//! (hosted-features.json). Cloud STT, Conductor and Enhance in the app add
//! their cases as M8 builds them.

#[path = "support/phone.rs"]
mod phone;

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

/// Pairing is the phone companion's opt-in: with no pairing nothing dials
/// the relay, and after a revoke the phone's frames reach nothing (F-RM-03).
#[tokio::test]
async fn invariant_rule_7_the_phone_relay_is_used_only_while_paired() {
    let rig = phone::rig();
    rig.svc.start().await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(
        *rig.net.dials.lock().expect("lock"),
        0,
        "no pairing, no relay"
    );
    let mut p = phone::pair(&rig).await;
    rig.svc.revoke(&p.pid, "revoked").await.expect("revoke");
    let frame_count = rig.svc.list().await.expect("list").len();
    assert_eq!(frame_count, 0);
    let reply = p
        .send(
            &rig.svc,
            json!({"t": "cmd", "id": 1, "cmd": "view.snapshot", "args": {}}),
        )
        .await;
    assert!(reply.is_empty(), "a revoked pairing gets no answer");
}
