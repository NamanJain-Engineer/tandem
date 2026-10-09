//! Follows the plan as it changes (`entitlement:changed`): the Team org's
//! telemetry lock (F-BL-06) and the credit-low item in "Waiting on you"
//! (F-DB-02: at most 10% of the monthly grant left).

use std::sync::Arc;

use tandem_core::bus::{EventBus, Topic};
use tandem_core::source::SourceRef;
use tandem_store::repo::notifications::{self as repo, NotificationKind};
use tandem_store::Store;

use super::entitlement::{EntitlementService, EntitlementView};
use super::notifications::NotificationService;
use super::telemetry::TelemetryService;

pub const CREDIT_LOW_KEY: &str = "credit_low";

/// The credit-low item's title and body, when credits are low: a signed-in
/// plan with a monthly grant and at most 10% of it left to spend.
pub fn credit_low(view: &EntitlementView) -> Option<(String, String)> {
    let credits = view.credits.as_ref()?;
    let grant = credits.grant.unwrap_or(view.monthly_grant);
    if grant <= 0 {
        return None;
    }
    let left = credits.available();
    if left * 10 > grant {
        return None;
    }
    let n = format_count(left);
    let title = if left == 0 {
        "You're out of credits".to_string()
    } else {
        format!("{n} credits left")
    };
    let body = if credits.cap.is_some() {
        "Your organization's monthly cap for you is nearly used. Cloud dictation, Enhance and teammate drafts need credits.".to_string()
    } else {
        "Cloud dictation, Enhance and teammate drafts need credits. Buy more or wait for the monthly reset.".to_string()
    };
    Some((title, body))
}

fn format_count(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// Applies one entitlement view: the telemetry lock, then the credit item
/// (raised once when credits fall low, resolved when they recover).
pub async fn apply(
    view: &EntitlementView,
    store: &Store,
    telemetry: &TelemetryService,
    notifications: &NotificationService,
) {
    telemetry
        .core
        .set_forced_off(view.policy.as_ref().is_some_and(|p| p.force_telemetry_off));
    match credit_low(view) {
        Some((title, body)) => {
            if !repo::has_open(store, CREDIT_LOW_KEY).await.unwrap_or(true) {
                notifications
                    .raise_in_app(
                        NotificationKind::CreditLow,
                        title,
                        body,
                        SourceRef::Account,
                        CREDIT_LOW_KEY.to_string(),
                    )
                    .await;
            }
        }
        None => notifications.resolve_key(CREDIT_LOW_KEY).await,
    }
}

/// Runs [`apply`] now and on every `entitlement:changed`.
pub async fn run(
    entitlements: Arc<EntitlementService>,
    store: Store,
    telemetry: Arc<TelemetryService>,
    notifications: Arc<NotificationService>,
    bus: EventBus,
) {
    let mut rx = bus.subscribe(Topic::Entitlement);
    apply(&entitlements.view(), &store, &telemetry, &notifications).await;
    loop {
        match rx.recv().await {
            Ok(ev) if ev.name == "entitlement:changed" => {
                apply(&entitlements.view(), &store, &telemetry, &notifications).await;
            }
            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::entitlement::CreditsView;
    use tandem_core::entitlement::{LimitSource, Limits, Plan};

    fn view(monthly: i64, purchased: i64, grant: Option<i64>, cap: Option<(i64, i64)>) -> EntitlementView {
        EntitlementView {
            plan: Plan::Pro,
            status: "active".into(),
            limits: Limits::free(),
            source: LimitSource::Token,
            credits: Some(CreditsView {
                monthly,
                purchased,
                resets_at: None,
                cap: cap.map(|c| c.0),
                spent: cap.map(|c| c.1),
                grant,
            }),
            policy: None,
            org: None,
            paid_until: None,
            cancel_at_period_end: false,
            online: true,
            confirmed_until: None,
            monthly_grant: 10_000,
        }
    }

    #[test]
    fn f_db_02_credit_low_at_ten_percent_of_the_grant() {
        assert_eq!(credit_low(&view(1_001, 0, Some(10_000), None)), None);
        let (title, _) = credit_low(&view(1_000, 0, Some(10_000), None)).expect("low");
        assert_eq!(title, "1,000 credits left");
        // Purchased credits count: 600 + 500 is above 10%.
        assert_eq!(credit_low(&view(600, 500, Some(10_000), None)), None);
        let (title, _) = credit_low(&view(0, 0, Some(10_000), None)).expect("out");
        assert_eq!(title, "You're out of credits");
        // Team: 30,000 for three seats; 2,500 left is under 10%.
        assert!(credit_low(&view(2_500, 0, Some(30_000), None)).is_some());
        // A member capped at 1,000 with 950 spent has 50 left of a big pool.
        let (title, body) = credit_low(&view(25_000, 0, Some(1_000), Some((1_000, 950)))).expect("cap");
        assert_eq!(title, "50 credits left");
        assert!(body.contains("cap"));
        // Free: no grant, no item.
        assert_eq!(credit_low(&view(0, 0, Some(0), None)), None);
    }

    /// Signs the auth rig in through its scripted login.
    async fn sign_in(r: &crate::services::auth::tests::Rig) {
        r.auth.sign_in().await.expect("start");
        let url = r.opener.opened.lock().expect("lock").last().cloned().unwrap_or_default();
        let state = url::Url::parse(&url)
            .expect("url")
            .query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        r.auth.handle_callback(crate::services::auth::Callback {
            code: "good-code".into(),
            state,
        });
        for _ in 0..100 {
            if r.auth.is_signed_in() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    /// Threat-model rule 7 (MILESTONES M13 task 11): with every opt-in off, a
    /// signed-in session sends nothing to the hosted features (AI titles,
    /// audit, telemetry), checked against `TANDEM_NET_LOG`. The same calls
    /// with the opt-in on do show up there, so the log is a real witness.
    #[tokio::test]
    async fn invariant_rule_7_no_egress_without_optin() {
        use crate::services::api::{ApiClient, Endpoints, HttpApi};
        use crate::services::thread_titles::{ApiTitler, Titler};
        use tandem_core::net::Net;

        let r = crate::services::auth::tests::rig();
        sign_in(&r).await;
        assert!(r.auth.is_signed_in());
        let log = r.dir.path().join("net.log");
        let net = Net::with_log(Some(log.clone()));
        // Port 9 (discard) on loopback: nothing answers, but every attempt is logged.
        let http: Arc<dyn ApiClient> = Arc::new(HttpApi::new(
            net.clone(),
            Endpoints {
                api: "http://127.0.0.1:9".into(),
                web: "http://127.0.0.1:9".into(),
                hosted: true,
            },
        ));
        let entitlements = EntitlementService::new(
            r.auth.clone(),
            r.api.clone(),
            tandem_core::entitlement::PublicKeySet::default(),
            r.data.clone(),
            Arc::new(r.clock.clone()),
            r.bus.clone(),
        );
        let telemetry = TelemetryService::new(&r.data, net.clone());
        telemetry.apply(false, false);
        let audit = crate::services::audit::AuditService::new(
            r.auth.clone(),
            http.clone(),
            entitlements.clone(),
            Arc::new(r.clock.clone()),
        );
        let off = ApiTitler {
            auth: r.auth.clone(),
            api: http.clone(),
            enabled: Arc::new(|| false),
        };

        // A session: a thread's first message, a Full-access start, app events.
        assert_eq!(off.title("refactor the billing module").await, None);
        assert!(!audit.record(crate::services::audit::FULL_ACCESS_USED, &[("engine", "claude")]));
        audit.flush().await;
        assert!(!crate::services::telemetry::capture(
            &telemetry,
            "app_started",
            &serde_json::json!({ "platform": "linux" })
        ));
        telemetry.flush().await;
        telemetry.upload_crashes().await;
        let sent = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(sent.trim().is_empty(), "nothing may leave with every opt-in off:\n{sent}");

        // The witness works: with the titles opt-in on, the attempt is logged.
        let on = ApiTitler {
            auth: r.auth.clone(),
            api: http.clone(),
            enabled: Arc::new(|| true),
        };
        let _ = on.title("refactor the billing module").await;
        let sent = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(sent.contains("POST http://127.0.0.1:9/v1/ai/title"), "{sent}");
        assert!(!sent.contains("billing"), "bodies never reach the log: {sent}");
    }

    /// Every opt-in row in hosted-features.json is off by default; only update
    /// checks (on, as before M13) and the account rows aren't opt-ins.
    #[test]
    fn invariant_rule_7_every_hosted_feature_defaults_to_off() {
        let json: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../packages/protocol/hosted-features.json"
        ))
        .expect("hosted-features.json");
        let rows = json["features"].as_array().expect("features");
        assert!(rows.len() >= 10);
        for row in rows {
            let id = row["id"].as_str().unwrap_or_default();
            let state = &row["state"];
            if state["kind"] != "setting" {
                continue;
            }
            let key = tandem_core::settings::SettingKey::parse(state["key"].as_str().unwrap_or_default())
                .unwrap_or_else(|| panic!("{id}: unknown setting {}", state["key"]));
            let on_by_default = tandem_core::settings::default_value(key) == state["equals"];
            assert_eq!(on_by_default, id == "updates", "{id} must be off until the user turns it on");
        }
    }
}
