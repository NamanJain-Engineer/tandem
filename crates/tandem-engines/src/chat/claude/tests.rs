use super::*;
use std::time::Duration;

use tandem_core::ids::ThreadId;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude")
}

/// 2026-10-08 10:00 UTC, so reset times are stable.
fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-10-08T10:00:00+00:00").expect("now")
}

fn as_json(p: &Parsed) -> Value {
    match p {
        Parsed::Event(e) => serde_json::to_value(e).expect("event json"),
        Parsed::Session(s) => json!({"session": s}),
        Parsed::Ack => json!({"ack": true}),
    }
}

fn claude_patterns() -> Vec<String> {
    crate::registry::EngineRegistry::bundled()
        .get("claude")
        .expect("claude")
        .limit_patterns
        .clone()
}

/// F-TH-03 AC1: each fixture maps to its golden event sequence. With
/// `UPDATE_GOLDENS=1` the goldens are rewritten (then reviewed by hand).
#[test]
fn f_th_03_1_claude_fixtures_map_to_their_golden_events() {
    let mut checked = 0;
    let mut names: Vec<PathBuf> = std::fs::read_dir(fixtures())
        .expect("fixtures")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    names.sort();
    for path in names {
        let mut parser = ClaudeParser::new(PathBuf::from("/work/shop"), &claude_patterns(), false);
        let text = std::fs::read_to_string(&path).expect("fixture");
        let got: Vec<Value> = text
            .lines()
            .flat_map(|l| parser.line(l, now()))
            .map(|p| as_json(&p))
            .collect();
        let golden = path.with_extension("golden.json");
        if std::env::var_os("UPDATE_GOLDENS").is_some() {
            std::fs::write(
                &golden,
                serde_json::to_string_pretty(&got).expect("json") + "\n",
            )
            .expect("write golden");
        }
        let want: Vec<Value> =
            serde_json::from_str(&std::fs::read_to_string(&golden).expect("golden"))
                .expect("golden json");
        assert_eq!(got, want, "{}", path.display());
        checked += 1;
    }
    assert!(checked >= 7, "fixtures checked: {checked}");
}

#[test]
fn f_th_04_3_a_limit_fixture_gives_a_limit_event_with_its_reset_time() {
    let mut parser = ClaudeParser::new(PathBuf::from("/w"), &claude_patterns(), false);
    let text = std::fs::read_to_string(fixtures().join("limit.jsonl")).expect("fixture");
    let events: Vec<Parsed> = text.lines().flat_map(|l| parser.line(l, now())).collect();
    assert!(events.contains(&Parsed::Event(ThreadEvent::Limit {
        kind: LimitKind::Session,
        resets_at: Some(1_791_418_440_000),
        message: "Claude AI usage limit reached".into(),
    })));
    assert!(events.contains(&Parsed::Event(ThreadEvent::TurnEnd {
        reason: TurnEndReason::Limit,
        blocked_actions: 0
    })));
}

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
        resume_session: None,
        mcp_config: None,
        mcp: None,
        engine_version: None,
        briefing: None,
        stdio_servers: Vec::new(),
    }
}

fn flag<'a>(args: &'a [OsString], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == name)
        .and_then(|w| w[1].to_str())
}

/// F-TH-04 AC1/AC2 and invariant rule 1: whatever the access, mode or
/// session, repo settings can't grant anything and only Vibezeo's MCP
/// servers load; below Full, Bash goes through the approval card.
#[test]
fn invariant_rule_1_repo_settings_cannot_grant() {
    for access in [Access::Read, Access::Edits, Access::Full] {
        for mode in [ThreadMode::Plan, ThreadMode::Act] {
            for attended in [true, false] {
                let mut req = start(access, mode, attended);
                req.resume_session = Some("sess-1".into());
                req.mcp_config = Some("/data/mcp/th_1.json".into());
                let a = args(&req);
                assert_eq!(
                    flag(&a, "--setting-sources"),
                    Some("user"),
                    "{access:?} {mode:?}"
                );
                assert!(a.iter().any(|x| x == "--strict-mcp-config"));
                assert_eq!(flag(&a, "--mcp-config"), Some("/data/mcp/th_1.json"));
                assert_eq!(flag(&a, "--permission-prompt-tool"), Some(PERMISSION_TOOL));
                assert_eq!(flag(&a, "--resume"), Some("sess-1"));
                assert!(!a.iter().any(|x| x == "--dangerously-skip-permissions"));
            }
        }
    }
}

#[test]
fn f_th_04_2_access_maps_to_the_permission_mode() {
    let mode = |access, m, attended| {
        let a = args(&start(access, m, attended));
        (
            flag(&a, "--permission-mode").map(str::to_string),
            flag(&a, "--disallowedTools").map(str::to_string),
        )
    };
    let no_writes = Some(NO_WRITES.to_string());
    assert_eq!(
        mode(Access::Read, ThreadMode::Act, true),
        (Some("default".into()), no_writes.clone())
    );
    // Edits: Bash isn't granted; it asks through the permission tool.
    assert_eq!(
        mode(Access::Edits, ThreadMode::Act, true),
        (Some("acceptEdits".into()), None)
    );
    assert_eq!(
        mode(Access::Full, ThreadMode::Act, true),
        (Some("bypassPermissions".into()), None)
    );
    // F-TH-09 AC1: a teammate chat at Full started by mail runs at Edits.
    let mailed = tandem_core::access::resolve(
        Access::Full,
        ThreadMode::Act,
        tandem_core::access::Origin::Mail,
    );
    assert_eq!(
        mode(mailed, ThreadMode::Act, false),
        (Some("acceptEdits".into()), None)
    );
    // Plan mode reads only, at any access.
    assert_eq!(
        mode(Access::Full, ThreadMode::Plan, true),
        (Some("plan".into()), no_writes)
    );
}

#[test]
fn f_th_04_attachments_are_text_references_and_inline_images() {
    let dir = tempfile::tempdir().expect("tmp");
    let png = dir.path().join("shot.png");
    std::fs::write(&png, [0x89, b'P', b'N', b'G']).expect("png");
    let line = user_message(&TurnInput {
        turn_id: TurnId::new(),
        text: "Fix this".into(),
        images: vec![png],
        files: vec!["src/app.ts".into()],
    })
    .expect("message");
    let v: Value = serde_json::from_str(&line).expect("json");
    assert_eq!(v["type"], "user");
    assert_eq!(v["message"]["content"][0]["text"], "Fix this @src/app.ts");
    assert_eq!(
        v["message"]["content"][1]["source"]["media_type"],
        "image/png"
    );
    assert_eq!(v["message"]["content"][1]["source"]["data"], "iVBORw==");
}

fn sh(script: &str) -> ChatStart {
    let mut req = start(Access::Edits, ThreadMode::Act, true);
    req.program = "/bin/sh".into();
    req.cwd = std::env::temp_dir();
    req.env = vec![("PATH".into(), "/usr/bin:/bin".into())];
    // The stand-in ignores claude's flags: `sh -c <script>` comes first.
    req.model = None;
    let _ = script;
    req
}

/// Starts a stand-in engine: `sh -c script` with claude's flags as `$@`.
async fn session(script: &str) -> (Box<dyn ChatSession>, mpsc::Receiver<AdapterEvent>) {
    struct Sh(String);
    #[async_trait]
    impl ChatAdapter for Sh {
        fn engine_id(&self) -> &str {
            "claude"
        }
        async fn start(
            &self,
            req: ChatStart,
            events: mpsc::Sender<AdapterEvent>,
        ) -> Result<Box<dyn ChatSession>, AdapterError> {
            // ClaudeAdapter::start with the script in front of the flags.
            let mut a: Vec<OsString> = vec!["-c".into(), self.0.clone().into(), "claude".into()];
            a.extend(args(&req));
            let spec = ProcessSpec::new(req.program.clone(), a, req.env.clone(), req.cwd.clone());
            let (process, lines) = ChatProcess::spawn(spec)?;
            let session = ClaudeSession {
                process: Arc::new(process),
                turn: Arc::new(Mutex::new(None)),
                session_id: Arc::new(Mutex::new(None)),
                acked: Arc::new(Notify::new()),
            };
            let parser = ClaudeParser::new(req.cwd.clone(), &[], false);
            tokio::spawn(session.clone().read(parser, lines, events));
            Ok(Box::new(session))
        }
    }
    let (tx, rx) = mpsc::channel(64);
    let s = Sh(script.to_string())
        .start(sh(script), tx)
        .await
        .expect("start");
    (s, rx)
}

async fn next(rx: &mut mpsc::Receiver<AdapterEvent>) -> AdapterEvent {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("an event in time")
        .expect("open")
}

#[cfg(unix)]
#[tokio::test]
async fn f_th_04_a_session_tags_the_turn_and_reports_the_engine_session() {
    // Reads the user message, checks it, answers one turn, then waits.
    let script = r#"read line
case "$line" in *'"text":"hi"'*) ;; *) exit 9;; esac
case "$*" in *'--setting-sources user'*) ;; *) exit 8;; esac
echo '{"type":"system","subtype":"init","session_id":"s-1","model":"claude-sonnet-4-5"}'
echo '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hey"}},"parent_tool_use_id":null}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"Hey","usage":{"input_tokens":1,"output_tokens":1}}'
read more"#;
    let (s, mut rx) = session(script).await;
    let turn = TurnId::new();
    s.send_turn(TurnInput {
        turn_id: turn.clone(),
        text: "hi".into(),
        images: Vec::new(),
        files: Vec::new(),
    })
    .await
    .expect("send");
    assert_eq!(next(&mut rx).await, AdapterEvent::SessionId("s-1".into()));
    let mut kinds = Vec::new();
    loop {
        match next(&mut rx).await {
            AdapterEvent::Event { turn_id, event } => {
                assert_eq!(turn_id, turn);
                let done = matches!(event, ThreadEvent::TurnEnd { .. });
                kinds.push(event.tag());
                if done {
                    break;
                }
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(kinds, vec!["note", "assistant_delta", "usage", "turn_end"]);
    assert_eq!(s.engine_session_id().as_deref(), Some("s-1"));
    s.shutdown().await.expect("shutdown");
}

#[cfg(unix)]
#[tokio::test]
async fn f_th_03_2_an_answered_interrupt_is_acknowledged() {
    let script = r#"read user
read control
case "$control" in *'"subtype":"interrupt"'*) echo '{"type":"control_response","response":{"subtype":"success"}}';; esac
read more"#;
    let (s, _rx) = session(script).await;
    let turn = TurnId::new();
    s.send_turn(TurnInput {
        turn_id: turn.clone(),
        text: "long job".into(),
        images: Vec::new(),
        files: Vec::new(),
    })
    .await
    .expect("send");
    let started = std::time::Instant::now();
    let out = s.interrupt(&turn).await.expect("interrupt");
    assert_eq!(
        out,
        InterruptOutcome {
            acked: true,
            killed: false
        }
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    s.shutdown().await.expect("shutdown");
}

#[cfg(unix)]
#[tokio::test]
async fn f_th_03_4_an_engine_exit_is_reported_with_its_code_and_stderr() {
    let (s, mut rx) = session("read user; echo 'auth expired' >&2; exit 3").await;
    s.send_turn(TurnInput {
        turn_id: TurnId::new(),
        text: "go".into(),
        images: Vec::new(),
        files: Vec::new(),
    })
    .await
    .expect("send");
    match next(&mut rx).await {
        AdapterEvent::Exited { code, stderr_tail } => {
            assert_eq!(code, Some(3));
            assert_eq!(stderr_tail, "auth expired");
        }
        other => panic!("unexpected {other:?}"),
    }
}

/// F-AG-06 AC2: a 100 KB briefing reaches Claude as a file named in argv,
/// never as argv text.
#[test]
fn f_ag_06_2_a_100_kb_briefing_goes_by_file_not_argv() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("th_1.md");
    let text = format!("<brief>{}</brief>", "b".repeat(100 * 1024));
    std::fs::write(&path, &text).expect("write");
    let mut req = start(Access::Edits, ThreadMode::Act, true);
    req.briefing = Some(path.clone());
    let a = args(&req);
    let at = a
        .iter()
        .position(|x| x == "--append-system-prompt-file")
        .expect("the flag");
    assert_eq!(a[at + 1], path.as_os_str());
    let total: usize = a.iter().map(|x| x.len()).sum();
    assert!(total < 4096, "argv is {total} bytes");
    assert!(!a.iter().any(|x| x.to_string_lossy().contains("bbbb")));
}
