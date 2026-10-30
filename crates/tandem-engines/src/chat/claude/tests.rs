// The session tests drive `/bin/sh` fake engines, so they and their helpers
// are Unix only; the parsing and mapping tests run everywhere.
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

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
        Parsed::Permission {
            request_id,
            tool_name,
            input,
        } => json!({"permission": request_id, "tool": tool_name, "input": input}),
        Parsed::ToolStart { id, name } => json!({"tool_start": id, "name": name}),
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
        relocate: None,
        briefing: None,
        stdio_servers: Vec::new(),
        tools_off: false,
    }
}

fn flag<'a>(args: &'a [OsString], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == name)
        .and_then(|w| w[1].to_str())
}

#[test]
fn f_tk_08_1_an_answer_session_has_every_tool_off_and_no_mcp_server() {
    // Even a request that carries a gateway config, the tasks server, a
    // briefing and a session to resume gets none of them with tools off.
    let mut req = start(Access::Full, ThreadMode::Act, true);
    req.tools_off = true;
    req.mcp_config = Some("/data/mcp/s.json".into());
    req.stdio_servers = vec![crate::chat::StdioServer {
        name: "tandem_tasks".into(),
        command: "/bin/tasks".into(),
        args: Vec::new(),
        env_vars: Vec::new(),
    }];
    req.briefing = Some("/data/briefings/t.md".into());
    req.resume_session = Some("sess-1".into());
    req.model = Some("opus".into());
    let a = args(&req);
    // No built-in tool at all, and nobody to ask for one.
    assert_eq!(flag(&a, "--tools"), Some(""));
    assert_eq!(flag(&a, "--permission-mode"), Some("dontAsk"));
    assert_eq!(flag(&a, "--permission-prompts"), Some("none"));
    for on in [
        "--safe-mode",
        "--disable-slash-commands",
        "--no-session-persistence",
        "--strict-mcp-config",
    ] {
        assert!(a.iter().any(|x| x == on), "{on}");
    }
    // Every tool is denied too, 2.1.310's newer ones among them.
    let off: Vec<&str> = flag(&a, "--disallowedTools")
        .expect("disallowed")
        .split(',')
        .collect();
    for tool in [
        "Read",
        "Glob",
        "Grep",
        "LS",
        "WebFetch",
        "WebSearch",
        "Bash",
        "PowerShell",
        "Monitor",
        "CronCreate",
        "CronDelete",
        "CronList",
        "ScheduleWakeup",
        "RemoteTrigger",
        "ToolSearch",
        "SendMessage",
        "ListAgents",
        "TeamCreate",
        "Workflow",
        "REPL",
        "Config",
        "EnterWorktree",
        "ExitWorktree",
        "TaskCreate",
        "TaskStop",
        "Edit",
        "Write",
        "MultiEdit",
        "NotebookEdit",
        "Task",
        "Agent",
        "TodoWrite",
        "Skill",
        "SlashCommand",
    ] {
        assert!(off.contains(&tool), "{tool}");
    }
    assert_eq!(flag(&a, "--setting-sources"), Some("user"));
    for gone in [
        "--mcp-config",
        "--permission-prompt-tool",
        "--append-system-prompt-file",
        "--resume",
    ] {
        assert!(!a.iter().any(|x| x == gone), "{gone}");
    }
    assert_eq!(flag(&a, "--model"), Some("opus"));
    let claude = ClaudeAdapter::new(Vec::new());
    assert!(claude.answers_without_tools(Some("2.1.310")));
    assert!(claude.answers_without_tools(Some("2.2.0 (Claude Code)")));
    for old in [None, Some("2.1.309"), Some("1.9.999"), Some("garbled")] {
        assert!(!claude.answers_without_tools(old), "{old:?}");
    }
}

#[test]
fn f_tk_08_1_a_pre_release_of_2_1_310_cant_answer_with_every_tool_off() {
    let claude = ClaudeAdapter::new(Vec::new());
    for early in [
        "2.1.310-rc.1",
        "2.1.310-rc.1 (Claude Code)",
        "claude 2.1.310-beta",
        // A run that isn't a version before it doesn't hide the suffix.
        "v.9.9.9 2.1.310-rc.1",
    ] {
        assert!(!claude.answers_without_tools(Some(early)), "{early}");
    }
    for ok in [
        "2.1.310 (Claude Code)",
        "2.1.310+build.5",
        "2.1.311-rc.1",
        "claude 2.1.310",
    ] {
        assert!(claude.answers_without_tools(Some(ok)), "{ok}");
    }
}

#[test]
fn f_tk_08_1_the_tools_off_check_reads_the_version_parse_version_reads() {
    for version in [
        "2.1.310",
        "v.9.9.9 2.1.310-rc.1",
        "1.2 3.4.5-beta",
        "claude 2.1.310 (Claude Code) 9.9.9-rc",
        "2.1.310.4-rc",
        "garbled",
        "",
        "—2.1.310—",
    ] {
        assert_eq!(
            claude_version(version).map(|(v, _)| v),
            crate::chat::codex::protocol::parse_version(version),
            "{version}"
        );
    }
    assert_eq!(claude_version("1.2 3.4.5-beta"), Some(((3, 4, 5), true)));
    assert_eq!(claude_version("—2.1.310—"), Some(((2, 1, 310), false)));
}

#[tokio::test]
async fn f_tk_08_1_a_claude_too_old_for_every_tool_off_is_refused_before_it_starts() {
    let mut req = start(Access::Read, ThreadMode::Plan, false);
    req.tools_off = true;
    for version in [None, Some("2.1.309"), Some("2.1.310-rc.1")] {
        req.engine_version = version.map(str::to_string);
        let (tx, _rx) = mpsc::channel(4);
        // `/bin/claude` isn't run: a spawn would fail with another error.
        match ClaudeAdapter::new(Vec::new()).start(req.clone(), tx).await {
            Err(AdapterError::Unsupported(why)) => {
                assert!(why.contains("2.1.310 or newer"), "{why}")
            }
            Err(e) => panic!("{version:?}: {e}"),
            Ok(_) => panic!("{version:?} started"),
        }
    }
}

#[test]
fn f_tk_08_1_a_tool_block_starting_is_reported_at_once_with_every_tool_off() {
    let start_of = |kind: &str| {
        json!({"type": "stream_event", "parent_tool_use_id": null, "event": {
            "type": "content_block_start", "index": 1,
            "content_block": {"type": kind, "id": "toolu_1", "name": "CronCreate", "input": {}}
        }})
        .to_string()
    };
    let mut off = ClaudeParser::new("/w".into(), &[], false).tools_off();
    assert_eq!(
        off.line(&start_of("tool_use"), now()),
        vec![Parsed::ToolStart {
            id: "toolu_1".into(),
            name: "CronCreate".into()
        }]
    );
    assert_eq!(off.line(&start_of("server_tool_use"), now()).len(), 1);
    for prose in ["text", "thinking", "redacted_thinking"] {
        assert!(off.line(&start_of(prose), now()).is_empty(), "{prose}");
    }
    // A thread with tools reports its tools as before, when complete.
    let mut on = ClaudeParser::new("/w".into(), &[], false);
    assert!(on.line(&start_of("tool_use"), now()).is_empty());
}

// F-TL-06 rule 1 is tests/invariants/tests/rule_1.rs.

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
    session_with(script, false).await
}

/// [`session`], with every tool off when `tools_off`.
async fn session_with(
    script: &str,
    tools_off: bool,
) -> (Box<dyn ChatSession>, mpsc::Receiver<AdapterEvent>) {
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
                asks: Arc::new(Mutex::new(HashMap::new())),
                cwd: req.cwd.clone(),
                tools_off: req.tools_off,
            };
            let parser = ClaudeParser::new(req.cwd.clone(), &[], false);
            let parser = if req.tools_off {
                parser.tools_off()
            } else {
                parser
            };
            tokio::spawn(session.clone().read(parser, lines, events));
            Ok(Box::new(session))
        }
    }
    let (tx, rx) = mpsc::channel(64);
    let mut req = sh(script);
    req.tools_off = tools_off;
    let s = Sh(script.to_string()).start(req, tx).await.expect("start");
    (s, rx)
}

#[cfg(unix)]
#[tokio::test]
async fn f_tk_08_1_with_every_tool_off_a_tool_starting_kills_the_engine_before_the_tool_runs() {
    // The stand-in starts a tool block, then "runs" it a second later: the
    // session kills it first, so the mark is never written.
    let mark = tempfile::tempdir().expect("dir");
    let file = mark.path().join("ran");
    let script = format!(
        r#"read line
echo '{{"type":"system","subtype":"init","session_id":"s-1","model":"m"}}'
echo '{{"type":"stream_event","parent_tool_use_id":null,"event":{{"type":"content_block_start","index":0,"content_block":{{"type":"tool_use","id":"toolu_1","name":"CronCreate","input":{{}}}}}}}}'
sleep 1
touch '{}'
read more"#,
        file.display()
    );
    let (s, mut rx) = session_with(&script, true).await;
    let turn = TurnId::new();
    s.send_turn(TurnInput {
        turn_id: turn.clone(),
        text: "hi".into(),
        images: Vec::new(),
        files: Vec::new(),
    })
    .await
    .expect("send");
    let tool = loop {
        match next(&mut rx).await {
            AdapterEvent::Event {
                event: ThreadEvent::Tool { arg_summary, .. },
                ..
            } => break arg_summary,
            AdapterEvent::Exited { .. } => panic!("exited before the tool row"),
            _ => {}
        }
    };
    assert_eq!(tool, "CronCreate");
    // The engine is gone: it reports its exit, and never ran the tool.
    loop {
        if let AdapterEvent::Exited { .. } = next(&mut rx).await {
            break;
        }
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(!file.exists(), "the tool ran");
    s.shutdown().await.expect("shutdown");
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

#[test]
fn f_rm_01_a_thread_on_a_host_asks_permission_in_band() {
    // Local, with the gateway: its permission tool.
    let mut req = start(Access::Edits, ThreadMode::Act, true);
    req.mcp_config = Some("/data/mcp/s.json".into());
    assert_eq!(
        flag(&args(&req), "--permission-prompt-tool"),
        Some(PERMISSION_TOOL)
    );
    // On an SSH host or in WSL: no gateway there, so stdio.
    req.mcp_config = None;
    req.relocate = Some(crate::chat::Relocate(Arc::new(|spec| spec)));
    assert_eq!(flag(&args(&req), "--permission-prompt-tool"), Some("stdio"));
    assert!(!args(&req).iter().any(|a| a == "--mcp-config"));
    // Local without a gateway: unchanged (no prompt tool at all).
    req.relocate = None;
    assert_eq!(flag(&args(&req), "--permission-prompt-tool"), None);
}

#[test]
fn f_rm_01_a_thread_on_a_host_names_its_relayed_servers_inline() {
    // D-1604: the relays to this machine's gateway and tasks server, given
    // inline (no file on the host), still asking permission in band.
    let mut req = start(Access::Edits, ThreadMode::Act, true);
    req.relocate = Some(crate::chat::Relocate(Arc::new(|spec| spec)));
    req.stdio_servers = ["tandem", "tandem_tasks"]
        .iter()
        .enumerate()
        .map(|(i, name)| crate::chat::StdioServer {
            name: (*name).into(),
            command: "/home/me/.tandem-host/1/tandem-host".into(),
            args: vec!["mcp".into(), "--channel".into(), format!("{i:032x}")],
            env_vars: Vec::new(),
        })
        .collect();
    let a = args(&req);
    let cfg: Value =
        serde_json::from_str(flag(&a, "--mcp-config").expect("inline config")).expect("json");
    assert_eq!(
        cfg["mcpServers"]["tandem"]["command"],
        "/home/me/.tandem-host/1/tandem-host"
    );
    assert_eq!(
        cfg["mcpServers"]["tandem_tasks"]["args"],
        json!(["mcp", "--channel", format!("{:032x}", 1)])
    );
    assert_eq!(cfg["mcpServers"].as_object().map(|m| m.len()), Some(2));
    // No URL, header or token: only the relays.
    assert!(!cfg.to_string().contains("http"), "{cfg}");
    assert_eq!(flag(&a, "--permission-prompt-tool"), Some("stdio"));
    assert!(a.iter().any(|x| x == "--strict-mcp-config"));
}

#[test]
fn f_rm_01_an_in_band_request_describes_the_card_like_the_gateway_does() {
    let mut p = ClaudeParser::new("/repo".into(), &[], false);
    let line = r#"{"type":"control_request","request_id":"r-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"npm test"},"tool_use_id":"toolu_1"}}"#;
    let parsed = p.line(line, now());
    assert_eq!(
        parsed,
        vec![Parsed::Permission {
            request_id: "r-1".into(),
            tool_name: "Bash".into(),
            input: json!({"command": "npm test"}),
        }]
    );
    let (kind, summary, _, subject) =
        approval_card("Bash", &json!({"command": "npm test"}), Path::new("/repo"));
    assert_eq!(kind, ApprovalKind::Command);
    assert_eq!(summary, "Run `npm test`");
    assert_eq!(subject, ApprovalSubject::Command("npm test".into()));
    // Other control requests from Claude aren't approvals.
    assert!(p
        .line(r#"{"type":"control_request","request_id":"r-2","request":{"subtype":"hook_callback"}}"#, now())
        .is_empty());
    let allow = permission_reply(
        "r-1",
        &json!({"command": "npm test"}),
        &ApprovalDecision::Allow {
            scope: tandem_core::approvals::ApprovalScope::Once,
        },
    );
    let v: Value = serde_json::from_str(&allow).expect("json");
    assert_eq!(v["type"], "control_response");
    assert_eq!(v["response"]["request_id"], "r-1");
    assert_eq!(v["response"]["response"]["behavior"], "allow");
    assert_eq!(
        v["response"]["response"]["updatedInput"]["command"],
        "npm test"
    );
    let deny = permission_reply("r-1", &json!({}), &ApprovalDecision::Deny { reason: None });
    let v: Value = serde_json::from_str(&deny).expect("json");
    assert_eq!(v["response"]["response"]["behavior"], "deny");
    assert_eq!(v["response"]["response"]["message"], "Denied by the user");
}

#[cfg(unix)]
#[tokio::test]
async fn f_rm_01_an_in_band_approval_reaches_the_engine() {
    // Asks about `npm test`, then reports what the answer said.
    let script = r#"read user
echo '{"type":"system","subtype":"init","session_id":"s-1","model":"m"}'
echo '{"type":"control_request","request_id":"r-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"npm test"}}}'
read answer
case "$answer" in
  *'"request_id":"r-1"'*'"behavior":"allow"'*'"command":"npm test"'*) word=allowed;;
  *) word=refused;;
esac
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"'"$word"'"}]},"parent_tool_use_id":null}'
echo '{"type":"result","subtype":"success","is_error":false,"result":"x","usage":{"input_tokens":1,"output_tokens":1}}'
read more"#;
    let (s, mut rx) = session(script).await;
    let turn = TurnId::new();
    s.send_turn(TurnInput {
        turn_id: turn.clone(),
        text: "test it".into(),
        images: Vec::new(),
        files: Vec::new(),
    })
    .await
    .expect("send");
    let asked = loop {
        match next(&mut rx).await {
            AdapterEvent::Approval {
                turn_id,
                id,
                summary,
                subject,
                ..
            } => {
                assert_eq!(turn_id, turn);
                assert_eq!(summary, "Run `npm test`");
                assert_eq!(subject, ApprovalSubject::Command("npm test".into()));
                break id;
            }
            AdapterEvent::SessionId(_) | AdapterEvent::Event { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    };
    s.resolve_approval(
        &asked,
        ApprovalDecision::Allow {
            scope: tandem_core::approvals::ApprovalScope::Once,
        },
    )
    .await
    .expect("answer");
    let mut said = String::new();
    loop {
        if let AdapterEvent::Event { event, .. } = next(&mut rx).await {
            if let ThreadEvent::Assistant { markdown } = &event {
                said = markdown.clone();
            }
            if matches!(event, ThreadEvent::TurnEnd { .. }) {
                break;
            }
        }
    }
    assert_eq!(said, "allowed");
    s.shutdown().await.expect("shutdown");
}

/// F-EN-08 AC3: Claude Code's generic error result (an API overload, say)
/// ends the turn in error, but isn't the engine's own failure, so it never
/// counts against its version.
#[test]
fn f_en_08_3_a_generic_error_result_isnt_the_engines_fault() {
    let mut parser = ClaudeParser::new(PathBuf::from("/w"), &claude_patterns(), false);
    let line = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"API Error: 529 Overloaded"}"#;
    let events = parser.line(line, now());
    assert!(events.iter().any(|e| matches!(
        e,
        Parsed::Event(ThreadEvent::TurnEnd {
            reason: TurnEndReason::Error,
            ..
        })
    )));
    let faults: Vec<bool> = events
        .iter()
        .filter_map(|e| match e {
            Parsed::Event(ThreadEvent::Error { stage, code, .. }) => Some(stage.engine_fault(code)),
            _ => None,
        })
        .collect();
    assert_eq!(faults, vec![false], "{events:?}");
}

/// The `paths` of every tool row in `parsed`, in order.
fn tool_paths(parsed: Vec<Parsed>) -> Vec<Option<Vec<String>>> {
    parsed
        .into_iter()
        .filter_map(|p| match p {
            Parsed::Event(ThreadEvent::Tool { paths, .. }) => Some(paths),
            _ => None,
        })
        .collect()
}

#[test]
fn f_tk_07_1_an_edit_names_its_files_absolute_and_its_result_keeps_them() {
    let mut p = ClaudeParser::new(PathBuf::from("/work/shop"), &[], false);
    let use_line = |name: &str, input: Value, id: &str| {
        json!({"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": id, "name": name, "input": input}
        ]}})
        .to_string()
    };
    let one = |p: &str| Some(vec![p.to_string()]);
    let edit = use_line(
        "Edit",
        json!({"file_path": "/work/shop/src/a.ts", "old_string": "x", "new_string": "y"}),
        "t1",
    );
    assert_eq!(
        tool_paths(p.line(&edit, now())),
        vec![one("/work/shop/src/a.ts")]
    );
    // A relative path counts from the thread's folder.
    let write = use_line(
        "Write",
        json!({"file_path": "notes.md", "content": "hi"}),
        "t2",
    );
    assert_eq!(
        tool_paths(p.line(&write, now())),
        vec![one("/work/shop/notes.md")]
    );
    let multi = use_line(
        "MultiEdit",
        json!({"file_path": "/work/shop/b.ts", "edits": []}),
        "t3",
    );
    assert_eq!(
        tool_paths(p.line(&multi, now())),
        vec![one("/work/shop/b.ts")]
    );
    let notebook = use_line(
        "NotebookEdit",
        json!({"notebook_path": "/work/shop/n.ipynb"}),
        "t4",
    );
    assert_eq!(
        tool_paths(p.line(&notebook, now())),
        vec![one("/work/shop/n.ipynb")]
    );
    // Reading names no file to claim, nor does a command.
    let read = use_line("Read", json!({"file_path": "/work/shop/src/a.ts"}), "t5");
    assert_eq!(tool_paths(p.line(&read, now())), vec![None]);
    let bash = use_line("Bash", json!({"command": "touch x"}), "t6");
    assert_eq!(tool_paths(p.line(&bash, now())), vec![None]);
    // The edit's result row keeps its files.
    let result = json!({"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": "t1", "content": "ok"}
    ]}})
    .to_string();
    assert_eq!(
        tool_paths(p.line(&result, now())),
        vec![one("/work/shop/src/a.ts")]
    );
    // So does MultiEdit's: the row a claim is made from (D-2836).
    let multi_done = json!({"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": "t3", "content": "Applied 2 edits"}
    ]}})
    .to_string();
    assert_eq!(
        tool_paths(p.line(&multi_done, now())),
        vec![one("/work/shop/b.ts")]
    );
    // Rows without files serialize as before.
    let row = serde_json::to_value(ThreadEvent::tool(
        "t7",
        ToolVerb::Read,
        "a.ts",
        ToolStatus::Ok,
    ))
    .expect("json");
    assert!(row.get("paths").is_none(), "{row}");
}

/// Every finished tool row in `parsed`: its id, status, lines and files.
type Finished = (String, ToolStatus, Option<LineCounts>, Option<Vec<String>>);

fn finished(parsed: Vec<Parsed>) -> Vec<Finished> {
    parsed
        .into_iter()
        .filter_map(|p| match p {
            Parsed::Event(ThreadEvent::Tool {
                id,
                status,
                lines,
                paths,
                ..
            }) if status != ToolStatus::Running => Some((id, status, lines, paths)),
            _ => None,
        })
        .collect()
}

fn edits_fixture() -> Vec<Parsed> {
    let text = std::fs::read_to_string(fixtures().join("edits.jsonl")).expect("fixture");
    let mut p = ClaudeParser::new(PathBuf::from("/work/shop"), &[], false);
    text.lines().flat_map(|l| p.line(l, now())).collect()
}

const fn lines(added: u32, removed: u32) -> Option<LineCounts> {
    Some(LineCounts { added, removed })
}

#[test]
fn f_th_12_4_claude_codes_edits_count_the_lines_their_inputs_describe() {
    let parsed = edits_fixture();
    // A call that's still running claims no lines yet.
    for p in &parsed {
        if let Parsed::Event(ThreadEvent::Tool {
            status: ToolStatus::Running,
            lines,
            ..
        }) = p
        {
            assert_eq!(*lines, None);
        }
    }
    let rows = finished(parsed);
    let at = |id: &str| rows.iter().find(|r| r.0 == id).expect(id).clone();
    // Two changed lines with an unchanged one between them: 2 and 2, not
    // the 3 and 3 of the span from the first change to the last.
    assert_eq!(at("toolu_e1").2, lines(2, 2));
    // A MultiEdit counts each of its edits.
    assert_eq!(at("toolu_e2").2, lines(2, 2));
    // A new file: every line of it is added.
    assert_eq!(at("toolu_e3").2, lines(3, 0));
    // A Write over a file that existed: what it replaced isn't in the
    // event, so it names its file and counts no lines.
    assert_eq!(at("toolu_e4").2, None);
    assert_eq!(at("toolu_e4").3, Some(vec!["/work/shop/README.md".into()]));
    // replace_all without the result's patch: how many places it changed
    // isn't said, so it names its file and counts no lines.
    assert_eq!(at("toolu_e7").2, None);
    assert_eq!(
        at("toolu_e7").3,
        Some(vec!["/work/shop/src/checkout/next.ts".into()])
    );
}

#[test]
fn f_th_12_4_the_results_patch_counts_a_replace_all_and_a_rewrite_exactly() {
    let rows = finished(edits_fixture());
    let at = |id: &str| rows.iter().find(|r| r.0 == id).expect(id).clone();
    // `tool_use_result.structuredPatch`: three occurrences replaced.
    assert_eq!(at("toolu_e8").2, lines(3, 3));
    // A rewrite whose result carries its patch.
    assert_eq!(at("toolu_e9").2, lines(2, 1));
}

#[test]
fn f_th_12_4_a_notebook_edit_names_its_file_without_lines() {
    let rows = finished(edits_fixture());
    let nb = rows.iter().find(|r| r.0 == "toolu_e10").expect("notebook");
    assert_eq!(nb.1, ToolStatus::Ok);
    assert_eq!(nb.2, None);
    assert_eq!(nb.3, Some(vec!["/work/shop/notebooks/prices.ipynb".into()]));
}

#[test]
fn f_th_12_4_a_subagents_edits_count_on_its_task() {
    let rows = finished(edits_fixture());
    let task = rows.iter().find(|r| r.0 == "toolu_e11").expect("task");
    assert_eq!(
        task.3,
        Some(vec![
            "/work/shop/docs/checkout.md".into(),
            "/work/shop/docs/faq.md".into()
        ])
    );
    // An Edit's (1, 1) and a new file's three lines.
    assert_eq!(task.2, lines(4, 1));
    // The subagent's own rows stay inside the Task's.
    assert!(rows
        .iter()
        .all(|r| r.0 != "toolu_e12" && r.0 != "toolu_e13"));
}

/// Parses `lines` (stream-json values) as one session in /work/shop.
fn parse(lines: &[Value]) -> Vec<Finished> {
    let mut p = ClaudeParser::new(PathBuf::from("/work/shop"), &[], false);
    finished(
        lines
            .iter()
            .flat_map(|l| p.line(&l.to_string(), now()))
            .collect(),
    )
}

fn tool_use(id: &str, name: &str, input: Value, task: Option<&str>) -> Value {
    json!({"type": "assistant", "parent_tool_use_id": task, "message": {"role": "assistant",
        "content": [{"type": "tool_use", "id": id, "name": name, "input": input}]}})
}

fn tool_result(id: &str, text: &str, error: bool, task: Option<&str>) -> Value {
    json!({"type": "user", "parent_tool_use_id": task, "message": {"role": "user",
        "content": [{"type": "tool_result", "tool_use_id": id, "content": text, "is_error": error}]}})
}

#[test]
fn f_th_12_4_a_task_names_its_subagents_files_without_lines_when_one_edit_cant_say() {
    let edit = |file: &str, all: bool| json!({"file_path": file, "old_string": "a", "new_string": "b", "replace_all": all});
    let rows = parse(&[
        tool_use("t1", "Task", json!({"description": "Rename"}), None),
        tool_use("s1", "Edit", edit("src/a.ts", false), Some("t1")),
        tool_result(
            "s1",
            "The file src/a.ts has been updated.",
            false,
            Some("t1"),
        ),
        // replace_all with no patch: its lines aren't known.
        tool_use("s2", "Edit", edit("src/b.ts", true), Some("t1")),
        tool_result(
            "s2",
            "The file src/b.ts has been updated.",
            false,
            Some("t1"),
        ),
        // A refused edit adds nothing.
        tool_use("s3", "Edit", edit("src/c.ts", false), Some("t1")),
        tool_result("s3", "Denied by the user", true, Some("t1")),
        tool_result("t1", "Renamed.", false, None),
        // A Task whose subagent edited nothing names no files.
        tool_use("t2", "Task", json!({"description": "Look"}), None),
        tool_result("t2", "Nothing to change.", false, None),
    ]);
    let at = |id: &str| rows.iter().find(|r| r.0 == id).expect(id).clone();
    assert_eq!(
        at("t1").3,
        Some(vec![
            "/work/shop/src/a.ts".into(),
            "/work/shop/src/b.ts".into()
        ])
    );
    assert_eq!(at("t1").2, None);
    assert_eq!(at("t2").3, None);
    assert_eq!(at("t2").2, None);
}

#[test]
fn f_th_12_6_a_task_that_ends_in_error_still_names_what_its_subagent_edited() {
    let edit = |file: &str| json!({"file_path": file, "old_string": "a", "new_string": "b"});
    let rows = parse(&[
        tool_use("t1", "Task", json!({"description": "Rename"}), None),
        tool_use("s1", "Edit", edit("src/a.ts"), Some("t1")),
        tool_result(
            "s1",
            "The file src/a.ts has been updated.",
            false,
            Some("t1"),
        ),
        // The subagent's edit happened; then the Task itself failed.
        tool_result("t1", "Agent stopped: it ran out of turns.", true, None),
    ]);
    let task = rows.iter().find(|r| r.0 == "t1").expect("task");
    assert_eq!(task.1, ToolStatus::Error);
    assert_eq!(task.3, Some(vec!["/work/shop/src/a.ts".into()]));
    assert_eq!(task.2, lines(1, 1));
}

#[test]
fn f_th_12_6_an_unknown_tool_that_writes_a_file_names_it_and_counts_no_lines() {
    let rows = parse(&[
        // A local MCP server's write: an absolute path and what goes in it.
        tool_use(
            "m1",
            "mcp__fs__write_file",
            json!({"path": "/work/shop/src/new.ts", "content": "export {};\n"}),
            None,
        ),
        tool_result("m1", "Wrote /work/shop/src/new.ts", false, None),
        // A forge's: a path in a repository somewhere else, so no local file.
        tool_use(
            "m4",
            "mcp__github__create_or_update_file",
            json!({"owner": "acme", "repo": "shop", "path": "src/a.ts", "content": "x", "message": "Update a.ts", "branch": "main"}),
            None,
        ),
        tool_result("m4", "Updated src/a.ts on main", false, None),
        tool_use(
            "m5",
            "mcp__gitlab__create_or_update_file",
            json!({"project_id": "acme/shop", "file_path": "src/a.ts", "content": "x", "commit_message": "Update a.ts", "branch": "main"}),
            None,
        ),
        tool_result("m5", "Updated src/a.ts on main", false, None),
        // A tool that names a file but writes nothing, and one that names none.
        tool_use(
            "m2",
            "mcp__fs__read_file",
            json!({"path": "src/a.ts"}),
            None,
        ),
        tool_result("m2", "export {};", false, None),
        tool_use(
            "m3",
            "TodoWrite",
            json!({"todos": [{"content": "x", "status": "pending"}]}),
            None,
        ),
        tool_result("m3", "Todos updated", false, None),
    ]);
    let at = |id: &str| rows.iter().find(|r| r.0 == id).expect(id).clone();
    // Named, so the turn says it changed the file; uncounted, so the turn's
    // total says no lines rather than a partial one (D-2622).
    assert_eq!(at("m1").3, Some(vec!["/work/shop/src/new.ts".into()]));
    assert_eq!(at("m1").2, None);
    // A relative path from a tool this list doesn't know names nothing: a
    // forge's write changed no file here (D-2622).
    assert_eq!(at("m4").3, None);
    assert_eq!(at("m5").3, None);
    assert_eq!(at("m2").3, None);
    assert_eq!(at("m3").3, None);
}

#[test]
fn f_th_12_6_a_path_is_absolute_in_either_form_whatever_this_computer_is() {
    use crate::events::is_absolute;
    assert!(is_absolute("/work/shop/src/a.ts"));
    assert!(is_absolute(r"C:\work\a.ts"));
    assert!(is_absolute("c:/work/a.ts"));
    assert!(is_absolute(r"\\server\share\a.ts"));
    assert!(!is_absolute("src/a.ts"));
    assert!(!is_absolute("./a.ts"));
    assert!(!is_absolute("C:a.ts"));
    assert!(!is_absolute(""));
}

#[test]
fn f_th_12_6_a_windows_path_names_one_file_whatever_its_case_and_slashes() {
    // How a Task's subagent edits count once (claude.rs) and a turn's files
    // too (rows.ts fileKey).
    assert!(crate::events::same_file(
        r"C:\work\shop\src\A.ts",
        "c:/Work/Shop/src/a.ts"
    ));
    assert!(crate::events::same_file(
        r"\\server\share\A.ts",
        r"\\SERVER\share\a.ts"
    ));
    assert!(!crate::events::same_file(r"C:\work\a.ts", r"C:\work\b.ts"));
    // Elsewhere the text is the file: Linux's names are case-sensitive, and
    // a Mac's can be.
    assert!(!crate::events::same_file(
        "/work/shop/src/A.ts",
        "/work/shop/src/a.ts"
    ));
    assert!(crate::events::same_file(
        "/work/shop/src/a.ts",
        "/work/shop/src/a.ts"
    ));
}

#[test]
fn f_th_12_4_a_patch_counts_only_when_its_message_answers_one_tool() {
    let edit = |file: &str| json!({"file_path": file, "old_string": "a", "new_string": "b"});
    let patch = json!({"structuredPatch": [{"oldStart": 1, "oldLines": 2, "newStart": 1, "newLines": 3,
        "lines": ["-a", "+b", "+c", " d"]}]});
    let mut two = json!({"type": "user", "parent_tool_use_id": null, "message": {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "e1", "content": "The file has been updated."},
        {"type": "tool_result", "tool_use_id": "e2", "content": "The file has been updated."}]}});
    two["tool_use_result"] = patch.clone();
    let mut one = tool_result("e3", "The file has been updated.", false, None);
    one["tool_use_result"] = patch;
    let rows = parse(&[
        tool_use("e1", "Edit", edit("a.ts"), None),
        tool_use("e2", "Edit", edit("b.ts"), None),
        two,
        tool_use("e3", "Edit", edit("c.ts"), None),
        one,
    ]);
    let at = |id: &str| rows.iter().find(|r| r.0 == id).expect(id).clone();
    // Two answers share the message's patch, so each keeps its input's count.
    assert_eq!(at("e1").2, lines(1, 1));
    assert_eq!(at("e2").2, lines(1, 1));
    assert_eq!(at("e3").2, lines(2, 1));
}

#[test]
fn f_tk_07_an_edits_path_is_named_one_way() {
    let cwd = Path::new("/work/shop");
    let named = |p: &str| edited_paths("Edit", &json!({"file_path": p}), cwd);
    let one = Some(vec!["/work/shop/src/a.ts".to_string()]);
    assert_eq!(named("src/a.ts"), one);
    assert_eq!(named("./src/../src/a.ts"), one);
    assert_eq!(named("/work/shop/./src/a.ts"), one);
    assert_eq!(named("/work/x/../shop/src/a.ts"), one);
    // Never above the root.
    assert_eq!(named("/../a.ts"), Some(vec!["/a.ts".to_string()]));
}

#[test]
fn f_th_12_5_a_refused_or_failed_edit_counts_no_lines() {
    let rows = finished(edits_fixture());
    let at = |id: &str| rows.iter().find(|r| r.0 == id).expect(id).clone();
    assert_eq!(at("toolu_e5").1, ToolStatus::Denied);
    assert_eq!(at("toolu_e5").2, None);
    assert_eq!(at("toolu_e6").1, ToolStatus::Error);
    assert_eq!(at("toolu_e6").2, None);
}

#[test]
fn f_th_12_4_lines_come_from_a_line_diff_not_the_changed_span() {
    let c = |old: &str, new: &str| {
        let l = LineCounts::edit(old, new).expect("within the budget");
        (l.added, l.removed)
    };
    assert_eq!(c("a\nb\nc", "a\nB\nc"), (1, 1));
    assert_eq!(c("a\nX\nb\nY", "a\nX2\nb\nY2"), (2, 2));
    // An insertion between context lines.
    assert_eq!(c("fn a() {\n}", "fn a() {\n    b();\n}"), (1, 0));
    // A deletion.
    assert_eq!(c("a\nb\nc", "a\nc"), (0, 1));
    // Uneven sides, either way round: one line kept between the changes.
    assert_eq!(c("a\nX\nb\nY\nc", "a\nZ\nb\nc"), (1, 2));
    assert_eq!(c("a\nZ\nb\nc", "a\nX\nb\nY\nc"), (2, 1));
    // A change inside one line changes that line.
    assert_eq!(c("foo(1)", "bar(1)"), (1, 1));
    assert_eq!(c("a\nb", "a\nb"), (0, 0));
    // Two lines taken out without their newline leave one line, empty.
    assert_eq!(c("a\nb", ""), (1, 2));
    // An empty old string writes a new file's content.
    assert_eq!(c("", "a\nb\n"), (2, 0));
}

#[test]
fn f_th_12_4_a_snippets_edges_count_as_git_does() {
    // Each against `git diff --numstat`, the snippet a whole line or lines.
    let c = |old: &str, new: &str| {
        let l = LineCounts::edit(old, new).expect("within the budget");
        (l.added, l.removed)
    };
    // The line is left empty.
    assert_eq!(c("foo();", ""), (1, 1));
    // A new line after it.
    assert_eq!(c("foo();", "foo();\n"), (1, 0));
    // The line and its newline go.
    assert_eq!(c("foo();\n", ""), (0, 1));
    // A line join: "foo();" and the line after become one.
    assert_eq!(c("foo();\n", "foo();"), (1, 2));
    assert_eq!(c("a\nb\n", "a\nb"), (1, 2));
    // Whole lines with their newlines, in and out.
    assert_eq!(c("a\n", "a\nb\n"), (1, 0));
    assert_eq!(c("a\nb\nc\n", "c\n"), (0, 2));
}

#[test]
fn f_th_12_4_a_huge_edit_counts_no_lines_rather_than_a_guess() {
    let c = |old: &str, new: &str| LineCounts::edit(old, new).map(|l| (l.added, l.removed));
    // 2,000 lines a side, the first and last changed: exact, within the
    // comparison budget.
    let mid = |n: usize| -> String { (0..n).map(|i| format!("m{i}\n")).collect() };
    assert_eq!(
        c(&format!("x\n{}y", mid(1998)), &format!("X\n{}Y", mid(1998))),
        Some((2, 2))
    );
    // Past it, no count (the changed span would say 2,102 and 2,102 where
    // git says 2 and 2).
    assert_eq!(
        c(&format!("x\n{}y", mid(2100)), &format!("X\n{}Y", mid(2100))),
        None
    );
    let old: String = (0..5000).map(|i| format!("a{i}\n")).collect();
    let new: String = (0..5000).map(|i| format!("b{i}\n")).collect();
    assert_eq!(c(&old, &new), None);
    // A MultiEdit with one such edit counts none.
    let input = json!({"edits": [{"old_string": "a", "new_string": "b"},
        {"old_string": old, "new_string": new}]});
    assert_eq!(input_lines("MultiEdit", &input), None);
}
