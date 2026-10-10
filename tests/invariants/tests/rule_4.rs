//! F-TL-06 rule 4: unattended runs never get Full (F-TH-09). Only a
//! routine's or task's own explicit grant does. "Can never approve"
//! (F-PL-06) joins when M12 adds plugin approvals; today no unattended
//! path answers an approval, and thread allow-rules apply only to attended
//! sessions (the thread service, §6.10).

use tandem_core::access::{resolve, Access, Origin, ThreadMode};

#[test]
fn invariant_rule_4_unattended_never_full() {
    for origin in [
        Origin::Mail,
        Origin::Auto,
        Origin::Routine { grants_full: false },
        Origin::Task { grants_full: false },
    ] {
        assert!(!origin.attended(), "{origin:?}");
        // Full is lowered to Edits, never refused outright or kept.
        assert_eq!(
            resolve(Access::Full, ThreadMode::Act, origin),
            Access::Edits,
            "{origin:?}"
        );
        for mode in [ThreadMode::Act, ThreadMode::Plan] {
            assert_ne!(
                resolve(Access::Full, mode, origin),
                Access::Full,
                "{origin:?} {mode:?}"
            );
        }
    }
    for origin in [
        Origin::Routine { grants_full: true },
        Origin::Task { grants_full: true },
    ] {
        assert_eq!(resolve(Access::Full, ThreadMode::Act, origin), Access::Full);
    }
    // A person at the keyboard keeps the Full they chose.
    assert_eq!(
        resolve(Access::Full, ThreadMode::Act, Origin::Interactive),
        Access::Full
    );
    // Lower levels are never raised.
    assert_eq!(
        resolve(Access::Read, ThreadMode::Act, Origin::Mail),
        Access::Read
    );
}
