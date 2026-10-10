//! F-TL-06 rule 5: voice (Conductor) and the phone can't perform
//! local-review-only actions (F-CN-03, F-RM-03). Only a person in the main
//! window produces a `LocalReview`. M16 adds the live voice and phone paths.

use tandem_core::origin::{LocalReview, Origin};

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
