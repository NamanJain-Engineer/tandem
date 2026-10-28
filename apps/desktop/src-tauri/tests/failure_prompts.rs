//! F-WT-08: what Send failure to agent types, for a few failed checks. The
//! website's CI scene types the same prompt through its own port
//! (apps/web/lib/scenes.ts), and its test checks that port against these
//! cases, so the scene stays word for word what the app sends (D-3001).
//! After a change to the prompt, `UPDATE_FIXTURES=1 cargo test -p
//! tandem-desktop --test failure_prompts` rewrites them; then the port.

use serde::{Deserialize, Serialize};
use tandem_app::services::pr_checks::{failure_prompt, CheckBucket, Log, PrCheck};

/// A failed check, its log's last lines, and the prompt for them.
#[derive(Deserialize, Serialize)]
struct Case {
    why: String,
    pr: u32,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workflow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    link: Option<String>,
    log: Vec<String>,
    prompt: String,
}

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/failure_prompts.json"
);

#[test]
fn failure_prompts_are_the_fixtures() {
    let text = std::fs::read_to_string(FIXTURE).expect("the fixture");
    let mut cases: Vec<Case> = serde_json::from_str(&text).expect("the fixture's JSON");
    assert!(cases.len() >= 5, "{} cases", cases.len());
    let update = std::env::var_os("UPDATE_FIXTURES").is_some();
    for case in &mut cases {
        let check = PrCheck {
            name: case.name.clone(),
            bucket: CheckBucket::Fail,
            link: case.link.clone(),
            workflow: case.workflow.clone(),
            job_id: None,
        };
        let prompt = failure_prompt(case.pr, &check, &Log::Tail(case.log.join("\n")));
        if update {
            case.prompt = prompt;
        } else {
            assert_eq!(prompt, case.prompt, "{}", case.why);
        }
    }
    if update {
        let json = serde_json::to_string_pretty(&cases).expect("JSON");
        std::fs::write(FIXTURE, json + "\n").expect("the fixture written");
    }
}
