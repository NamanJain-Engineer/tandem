//! F-TL-06 rule 5: voice (Conductor) and the phone can't perform
//! local-review-only actions (F-CN-03, F-RM-03). Only a person in the main
//! window produces a `LocalReview`. M16 adds the live voice and phone paths.

#[path = "support/phone.rs"]
mod phone;

use serde_json::json;
use tandem_core::origin::{LocalReview, Origin};
use tandem_core::remote::messages::{
    approval_route, ApprovalRoute, CommandError, PhoneCommand, PhoneDecision, PhoneGrants,
    PHONE_COMMANDS,
};

#[test]
fn invariant_rule_5_local_review_origin() {
    assert!(LocalReview::check(&Origin::main_window()).is_some());
    for origin in [
        Origin::Conductor,
        Origin::Phone,
        Origin::Unattended,
        Origin::Local {
            window: "dictate-overlay".into(),
        },
    ] {
        assert!(
            LocalReview::check(&origin).is_none(),
            "{origin:?} must not pass local review"
        );
    }
}

/// The phone's whole command set: none of it forces a lane removal, enables
/// Full access, runs a shell command or answers a local-review prompt.
#[test]
fn invariant_rule_5_the_phone_has_no_local_review_command() {
    assert_eq!(
        PHONE_COMMANDS,
        [
            "view.snapshot",
            "thread.read",
            "thread.send",
            "pane.type",
            "approval.decide",
            "routine.run_now",
            "push.register",
            "push.unregister"
        ]
    );
    for cmd in [
        "lane.remove",
        "lane.force_remove",
        "access.full",
        "thread.set_access",
        "shell.run",
        "settings.set",
        "phone.arm",
        "phone.grants",
        "queue.decide",
    ] {
        assert_eq!(
            PhoneCommand::parse(cmd, json!({})),
            Err(CommandError::Unknown),
            "{cmd}"
        );
    }
    // An approval from the phone is once, never a wider scope.
    assert!(PhoneCommand::parse(
        "approval.decide",
        json!({"approvalId": "ap_1", "decision": "allow", "scope": "conversation"})
    )
    .is_err());
}

/// Allow from the phone is a step-up: it applies only while someone at the
/// desktop armed it, and otherwise waits for them. Deny always applies.
#[test]
fn invariant_rule_5_a_phone_allow_needs_local_arming() {
    assert_eq!(
        approval_route(PhoneDecision::Allow, false),
        ApprovalRoute::Queue
    );
    assert_eq!(
        approval_route(PhoneDecision::Deny, false),
        ApprovalRoute::Apply
    );
}

/// The live path: a paired phone with the Approvals grant asks to allow,
/// and nothing reaches the app until the main window confirms it.
#[tokio::test]
async fn invariant_rule_5_live_phone_allow_is_queued_until_the_main_window_confirms() {
    let rig = phone::rig();
    let mut p = phone::pair(&rig).await;
    let main = LocalReview::check(&Origin::main_window()).expect("main window");
    rig.svc
        .set_grants(
            &main,
            &p.pid,
            PhoneGrants {
                approvals: true,
                ..PhoneGrants::default()
            },
        )
        .await
        .expect("grants");
    let ap = phone::approval();
    let id = ap.id.to_string();
    rig.host.approvals.lock().expect("lock").push(ap);
    let reply = p
        .send(
            &rig.svc,
            json!({"t": "cmd", "id": 1, "cmd": "approval.decide", "args": {"approvalId": id, "decision": "allow"}}),
        )
        .await;
    assert_eq!(reply[0]["data"]["state"], "queued");
    assert!(rig.host.resolved.lock().expect("lock").is_empty());
    let queued = rig.svc.queue_list().await.expect("queue");
    rig.svc
        .queue_decide(&main, queued[0].id.as_str(), true)
        .await
        .expect("confirm");
    assert_eq!(
        *rig.host.resolved.lock().expect("lock"),
        vec![(id, PhoneDecision::Allow)]
    );
}
