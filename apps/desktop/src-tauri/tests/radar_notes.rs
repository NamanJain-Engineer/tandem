//! F-WT-10: what Warn both agents types into a worktree's agent, for a few
//! overlaps. The website's hero types the same note through its own port
//! (apps/web/lib/scenes.ts), and its test checks that port against these
//! cases, so the scene stays word for word what the app sends (D-2702).
//! After a change to the note, `UPDATE_FIXTURES=1 cargo test -p
//! tandem-desktop --test radar_notes` rewrites them and fails, so a rewrite
//! never passes: check the diff, run it again without the variable, then
//! change the port.

mod common;

use serde::{Deserialize, Serialize};
use tandem_app::services::radar::note_text;
use tandem_git::symbols::SharedSymbol;

/// The sibling worktree's branch, the symbols both change, and the note.
#[derive(Deserialize, Serialize)]
struct Case {
    why: String,
    other: String,
    symbols: Vec<SharedSymbol>,
    note: String,
}

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/radar_notes.json"
);

#[test]
fn f_wt_10_radar_notes_are_the_fixtures() {
    let text = std::fs::read_to_string(FIXTURE).expect("the fixture");
    let mut cases: Vec<Case> = serde_json::from_str(&text).expect("the fixture's JSON");
    assert!(cases.len() >= 5, "{} cases", cases.len());
    let update = common::updating();
    for case in &mut cases {
        let note = note_text(&case.other, &case.symbols);
        if update {
            case.note = note;
        } else {
            assert_eq!(note, case.note, "{}", case.why);
        }
    }
    if update {
        let json = serde_json::to_string_pretty(&cases).expect("JSON");
        common::rewrite(FIXTURE, &(json + "\n"));
    }
}
