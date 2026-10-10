//! F-TL-06 rule 1: repo-level engine config can't raise permissions. Chats
//! read user-level settings only and load Vibezeo's MCP config only.

use std::ffi::OsString;

use tandem_core::access::{Access, ThreadMode};
use tandem_core::ids::ThreadId;
use tandem_engines::chat::claude::{args, PERMISSION_TOOL};
use tandem_engines::chat::ChatStart;

fn start(access: Access, mode: ThreadMode, attended: bool) -> ChatStart {
    ChatStart {
        thread_id: ThreadId::new(),
        engine_id: "claude".into(),
        program: "/bin/claude".into(),
        cwd: "/work/shop".into(),
        env: Vec::new(),
        model: None,
        effort: None,
        access,
        mode,
        attended,
        resume_session: Some("sess-1".into()),
        mcp_config: Some("/data/mcp/th_1.json".into()),
        mcp: None,
        engine_version: None,
    }
}

fn flag<'a>(a: &'a [OsString], name: &str) -> Option<&'a str> {
    a.windows(2)
        .find(|w| w[0] == name)
        .and_then(|w| w[1].to_str())
}

#[test]
fn invariant_rule_1_repo_settings_cannot_grant() {
    for access in [Access::Read, Access::Edits, Access::Full] {
        for mode in [ThreadMode::Plan, ThreadMode::Act] {
            for attended in [true, false] {
                let a = args(&start(access, mode, attended));
                let case = format!("{access:?} {mode:?} attended={attended}");
                assert_eq!(flag(&a, "--setting-sources"), Some("user"), "{case}");
                assert!(a.iter().any(|x| x == "--strict-mcp-config"), "{case}");
                assert_eq!(
                    flag(&a, "--mcp-config"),
                    Some("/data/mcp/th_1.json"),
                    "{case}"
                );
                assert_eq!(
                    flag(&a, "--permission-prompt-tool"),
                    Some(PERMISSION_TOOL),
                    "{case}"
                );
                assert!(
                    !a.iter().any(|x| x == "--dangerously-skip-permissions"),
                    "{case}"
                );
                assert_eq!(flag(&a, "--resume"), Some("sess-1"), "{case}");
            }
        }
    }
}
