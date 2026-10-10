//! Who asked for an action (ARCHITECTURE §3 "Origin", F-TL-06 rule 5).
//!
//! Local-review-only actions (forced lane removal, enabling Full, running a
//! shell command for someone) take a [`LocalReview`], which only a person in
//! the `main` window can produce. Voice (Conductor), the phone and
//! unattended runs never can.

/// The Tauri label of the app's main window.
pub const MAIN_WINDOW: &str = "main";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A person at this computer, in the window with this label.
    Local { window: String },
    /// The voice assistant.
    Conductor,
    /// The phone companion.
    Phone,
    /// Routines, tasks, mail and Auto.
    Unattended,
}

impl Origin {
    pub fn main_window() -> Self {
        Origin::Local {
            window: MAIN_WINDOW.to_string(),
        }
    }
}

/// Proof that a person in the main window asked for this. It has no public
/// constructor besides [`LocalReview::check`].
#[derive(Debug)]
pub struct LocalReview(());

impl LocalReview {
    pub fn check(origin: &Origin) -> Option<LocalReview> {
        match origin {
            Origin::Local { window } if window == MAIN_WINDOW => Some(LocalReview(())),
            _ => None,
        }
    }
}

// F-TL-06 rule 5's test is tests/invariants/tests/rule_5.rs.
