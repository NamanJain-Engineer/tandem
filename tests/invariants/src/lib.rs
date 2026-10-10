//! The threat-model rules (PRODUCT_SPEC F-TL-06), each enforced by a test
//! in `tests/rule_<n>.rs` that checks the shipped mechanism through the
//! crates' public APIs. This library is empty; the crate exists for its tests.
//!
//! | Rule | What holds | Mechanism | Built in |
//! |---|---|---|---|
//! | 1 | Repo-level engine config can't raise permissions | `tandem_engines::chat::claude::args` | M7 |
//! | 2 | Status reports and shell marks need a per-launch secret | `StatusServer`, `MarkFilter` | M3/M4 |
//! | 3 | Credentials go only to pinned hosts | the gateway's loopback-only lease; plugin credentials (F-PL-04) join in M12 | M7, M12 |
//! | 4 | Unattended runs never get Full and can never approve | `tandem_core::access::resolve`; F-PL-06 approvals join in M12 | M7, M12 |
//! | 5 | Voice and phone can't do local-review-only actions | `tandem_core::origin::LocalReview`; the phone's command set and step-up queue (`tandem_core::remote::messages`, `services::phone`); live voice in M16 | M6, M16 |
//! | 6 | Git reads never run repo-configured helpers | `tandem_git::Repo` reads, `Mode::Read` | M6/M9 |
//! | 7 | Nothing leaves the machine without opt-in | the Privacy defaults and telemetry here; M13's hosted features in `tandem_app::services::hosted` tests; cloud STT, Conductor and Enhance in the app join with M8; the phone relay is used only while paired | M2, M7, M13, M16 |
//! | 8 | Untrusted content is labelled in every assembled prompt | `tandem_core::untrusted`; tasks, memory, skills and plugin responses use it from M10–M12 | M9 |
//!
//! A session that builds a new path for a rule adds its case to that rule's
//! file (MILESTONES D-9).
