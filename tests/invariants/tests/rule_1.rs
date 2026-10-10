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
        relocate: None,
        briefing: None,
        stdio_servers: Vec::new(),
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

/// On an SSH host (F-RM-01) the same flags reach the engine: the remote
/// command line `tandem-host run` gets is the local argv, each word quoted,
/// so a repository on the host can't raise permissions either.
#[test]
fn invariant_rule_1_remote_threads_keep_the_flags() {
    use tandem_engines::chat::process::ProcessSpec;
    use tandem_host::ssh::{Ssh, SshTarget};

    let ssh = Ssh::new("/data/run".into(), "/data/ssh/known_hosts".into());
    let target = SshTarget::parse("me@build-box").expect("target");
    for access in [Access::Read, Access::Edits, Access::Full] {
        let req = start(access, ThreadMode::Act, true);
        let local = args(&req);
        let spec = ProcessSpec::new(
            req.program.clone(),
            local.clone(),
            Vec::new(),
            req.cwd.clone(),
        );
        let mut argv = vec![spec.program.to_string_lossy().into_owned()];
        argv.extend(spec.args.iter().map(|a| a.to_string_lossy().into_owned()));
        let (_, ssh_args) = ssh.run_args(&target, "/home/me/repo", &Default::default(), &argv);
        let remote = ssh_args.last().cloned().unwrap_or_default();
        assert!(remote.contains(" -- /bin/claude "), "{remote}");
        assert!(remote.contains("--setting-sources user"), "{remote}");
        assert!(remote.contains("--strict-mcp-config"), "{remote}");
        assert!(
            !remote.contains("--dangerously-skip-permissions"),
            "{remote}"
        );
        // Every local word is there, in order.
        let mut at = 0;
        for word in &local {
            let quoted = tandem_host::quote(&word.to_string_lossy());
            let found = remote[at..]
                .find(&quoted)
                .unwrap_or_else(|| panic!("{quoted} missing from {remote}"));
            at += found + quoted.len();
        }
    }
}

/// Rule 1 for WSL threads (F-RM-02): the engine inside the distro gets the
/// same flags, in order, and nothing of Windows' environment.
#[test]
fn invariant_rule_1_wsl_threads_keep_the_flags() {
    use tandem_engines::chat::process::ProcessSpec;

    for access in [Access::Read, Access::Edits, Access::Full] {
        let req = start(access, ThreadMode::Act, true);
        let local = args(&req);
        let spec = ProcessSpec::new(
            req.program.clone(),
            local.clone(),
            Vec::new(),
            req.cwd.clone(),
        );
        let windows = [
            ("PATH".to_string(), r"C:\Windows".to_string()),
            (
                "CLAUDE_CONFIG_DIR".to_string(),
                r"C:\Users\a\.tandem\providers\claude\x".to_string(),
            ),
        ];
        let out = tandem_app::services::wsl::thread_spec(
            "Ubuntu",
            "/home/me/repo",
            spec,
            windows.into_iter(),
        );
        let words: Vec<String> = out
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let at = words
            .iter()
            .position(|w| w == "/bin/claude")
            .expect("engine");
        let expected: Vec<String> = local
            .iter()
            .map(|w| w.to_string_lossy().into_owned())
            .collect();
        assert_eq!(words[at + 1..], expected[..]);
        assert!(words.iter().any(|w| w == "--strict-mcp-config"));
        assert!(!words.iter().any(|w| w == "--dangerously-skip-permissions"));
        assert!(out.env.iter().all(|(k, _)| k != "CLAUDE_CONFIG_DIR"));
    }
}
