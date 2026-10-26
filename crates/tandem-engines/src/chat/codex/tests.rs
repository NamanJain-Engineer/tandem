// The session tests drive `/bin/sh` fake engines, so they and their helpers
// are Unix only; the parsing and mapping tests run everywhere.
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

use super::*;
use std::path::PathBuf;

use tandem_core::access::{Access, ThreadMode};
use tandem_core::approvals::ApprovalScope;
use tandem_core::ids::ThreadId;

use crate::chat::McpEndpoint;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex")
}

fn now() -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::parse_from_rfc3339("2026-10-08T10:00:00+00:00").expect("now")
}

fn patterns() -> Vec<String> {
    crate::registry::EngineRegistry::bundled()
        .get("codex")
        .expect("codex")
        .limit_patterns
        .clone()
}

fn as_json(c: &Codex) -> Value {
    match c {
        Codex::Event(e) => serde_json::to_value(e).expect("event"),
        Codex::Approval {
            rpc_id,
            kind,
            summary,
            detail,
            subject,
        } => {
            let subject = match subject {
                ApprovalSubject::Command(c) => json!({"command": c}),
                ApprovalSubject::Tool(t) => json!({"tool": t}),
            };
            json!({"approval": {"rpcId": rpc_id, "kind": kind, "summary": summary, "detail": detail, "subject": subject}})
        }
        Codex::TurnStarted(id) => json!({"turnStarted": id}),
    }
}

/// F-TH-05 AC1: create, turn, approval, interrupt and the failures map to
/// their golden events (`UPDATE_GOLDENS=1` rewrites, then reviewed).
#[test]
fn f_th_05_1_codex_fixtures_map_to_their_golden_events() {
    let mut names: Vec<PathBuf> = std::fs::read_dir(fixtures())
        .expect("fixtures")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    names.sort();
    assert!(names.len() >= 6);
    for path in names {
        let mut parser = CodexParser::new(PathBuf::from("/work/shop"), &patterns());
        let got: Vec<Value> = std::fs::read_to_string(&path)
            .expect("fixture")
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .flat_map(|m| parser.message(&m, now()))
            .map(|c| as_json(&c))
            .collect();
        let golden = path.with_extension("golden.json");
        if std::env::var_os("UPDATE_GOLDENS").is_some() {
            std::fs::write(
                &golden,
                serde_json::to_string_pretty(&got).expect("json") + "\n",
            )
            .expect("write");
        }
        let want: Vec<Value> =
            serde_json::from_str(&std::fs::read_to_string(&golden).expect("golden"))
                .expect("golden json");
        assert_eq!(got, want, "{}", path.display());
    }
}

#[test]
fn f_th_05_3_an_unknown_or_old_codex_fails_closed() {
    assert_eq!(protocol::check_version(Some("codex-cli 0.47.0")), Ok(()));
    assert_eq!(
        protocol::check_version(Some("codex_cli_rs/0.40.0 (Linux 6.1)")),
        Ok(())
    );
    assert_eq!(
        protocol::check_version(Some("0.39.2")),
        Err("Codex version 0.39.2 isn't supported yet — update Vibezeo or Codex".into())
    );
    assert_eq!(
        protocol::check_version(None),
        Err("Codex version unknown isn't supported yet — update Vibezeo or Codex".into())
    );
    assert!(protocol::check_version(Some("nightly")).is_err());
}

#[test]
fn f_th_05_access_maps_to_sandbox_and_approval_policy() {
    use protocol::policy;
    assert_eq!(
        policy(Access::Read, ThreadMode::Act),
        ("read-only", "on-request")
    );
    assert_eq!(
        policy(Access::Edits, ThreadMode::Act),
        ("workspace-write", "on-request")
    );
    assert_eq!(
        policy(Access::Full, ThreadMode::Act),
        ("danger-full-access", "never")
    );
    // Unattended runs never get Full (resolved before); Plan mode reads only.
    let mailed = tandem_core::access::resolve(
        Access::Full,
        ThreadMode::Act,
        tandem_core::access::Origin::Mail,
    );
    assert_eq!(
        policy(mailed, ThreadMode::Act),
        ("workspace-write", "on-request")
    );
    assert_eq!(
        policy(Access::Full, ThreadMode::Plan),
        ("read-only", "on-request")
    );
    let d = |scope| V2.decision(&ApprovalDecision::Allow { scope });
    assert_eq!(d(ApprovalScope::Once), json!({"decision": "accept"}));
    assert_eq!(
        d(ApprovalScope::Thread),
        json!({"decision": "acceptForSession"})
    );
    assert_eq!(
        V2.decision(&ApprovalDecision::Deny { reason: None }),
        json!({"decision": "decline"})
    );
}

#[test]
fn f_th_05_long_commands_lose_their_middle_on_the_row() {
    let long = "npm ci && npm run build && npm run lint && npm test -- --coverage --runInBand";
    let shown = middle(long, 40);
    assert!(shown.chars().count() <= 41, "{shown}");
    assert!(shown.starts_with("npm ci && npm"));
    assert!(shown.ends_with("--runInBand"));
    assert!(shown.contains(" … "));
    assert_eq!(middle("ls -la", 40), "ls -la");
}

fn start(script_args: bool) -> ChatStart {
    let _ = script_args;
    ChatStart {
        thread_id: ThreadId::new(),
        engine_id: "codex".into(),
        program: "/bin/sh".into(),
        cwd: std::env::temp_dir(),
        env: vec![("PATH".into(), "/usr/bin:/bin".into())],
        model: Some("gpt-5-codex".into()),
        effort: Some("high".into()),
        access: Access::Edits,
        mode: ThreadMode::Act,
        attended: true,
        resume_session: None,
        mcp_config: None,
        mcp: None,
        engine_version: Some("codex-cli 0.47.0".into()),
        relocate: None,
        briefing: None,
        stdio_servers: Vec::new(),
        tools_off: false,
    }
}

#[tokio::test]
async fn f_tk_08_1_codex_refuses_a_session_with_every_tool_off() {
    // Its read-only sandbox still runs commands that read the disk.
    let adapter = CodexAdapter::new(Vec::new());
    assert!(!adapter.answers_without_tools());
    let mut req = start(false);
    req.tools_off = true;
    let (tx, _rx) = mpsc::channel(4);
    let refused = adapter.start(req, tx).await.err().expect("refused");
    assert!(matches!(refused, AdapterError::Unsupported(_)), "{refused}");
}

#[test]
fn f_pl_01_the_gateway_joins_codex_through_config_overrides() {
    let mut req = start(false);
    req.mcp = Some(McpEndpoint {
        url: "http://127.0.0.1:4100/mcp".into(),
        token: "secret".into(),
    });
    let a: Vec<String> = args(&req)
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        a,
        vec![
            "app-server",
            "-c",
            "mcp_servers.tandem.url=\"http://127.0.0.1:4100/mcp\"",
            "-c",
            "mcp_servers.tandem.bearer_token_env_var=\"TANDEM_GATEWAY_LEASE\"",
        ]
    );
    // The token itself is never an argument (it's in the env).
    assert!(!a.iter().any(|x| x.contains("secret")));
    assert!(!format!("{:?}", req.mcp).contains("secret"));
}

/// Runs `script` as a stand-in app-server (`sh -c script codex app-server …`).
async fn session(
    script: &str,
    mut req: ChatStart,
) -> Result<(Box<dyn ChatSession>, mpsc::Receiver<AdapterEvent>), AdapterError> {
    struct Sh(String);
    #[async_trait]
    impl ChatAdapter for Sh {
        fn engine_id(&self) -> &str {
            "codex"
        }
        async fn start(
            &self,
            req: ChatStart,
            events: mpsc::Sender<AdapterEvent>,
        ) -> Result<Box<dyn ChatSession>, AdapterError> {
            // CodexAdapter::start with the script in front of the arguments.
            if let Some(v) = &req.engine_version {
                protocol::check_version(Some(v)).map_err(AdapterError::Unsupported)?;
            }
            let mut a: Vec<OsString> = vec!["-c".into(), self.0.clone().into(), "codex".into()];
            a.extend(args(&req));
            let spec = ProcessSpec::new(req.program.clone(), a, req.env.clone(), req.cwd.clone());
            let (process, lines) = ChatProcess::spawn(spec)?;
            let session = CodexSession {
                process: Arc::new(process),
                protocol: Arc::new(V2),
                req: Arc::new(req.clone()),
                next_id: Arc::new(AtomicI64::new(1)),
                pending: Arc::default(),
                thread_id: Arc::new(Mutex::new(None)),
                codex_turn: Arc::new(Mutex::new(None)),
                turn: Arc::new(Mutex::new(None)),
                approvals: Arc::default(),
            };
            tokio::spawn(session.clone().read(
                CodexParser::new(req.cwd.clone(), &[]),
                lines,
                events.clone(),
            ));
            session.handshake(&events).await?;
            Ok(Box::new(session))
        }
    }
    let (tx, rx) = mpsc::channel(64);
    req.program = "/bin/sh".into();
    let s = Sh(script.to_string()).start(req, tx).await?;
    Ok((s, rx))
}

async fn next(rx: &mut mpsc::Receiver<AdapterEvent>) -> AdapterEvent {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("an event in time")
        .expect("open")
}

/// The handshake every stand-in starts with: initialize (id 1), initialized,
/// thread/start or resume (id 2).
const HANDSHAKE_SH: &str = r#"read init
echo '{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex_cli_rs/0.47.0 (Linux)"}}'
read initialized
read thread
case "$thread" in *'"method":"thread/resume"'*'"threadId":"thr_old"'*) echo '{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_old"}}}';; *'"method":"thread/start"'*'"sandbox":"workspace-write"'*) echo '{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_1"}}}';; *) exit 7;; esac
"#;

#[cfg(unix)]
#[tokio::test]
async fn f_th_05_1_a_session_runs_a_turn_and_answers_an_approval_in_band() {
    let script = format!(
        r#"{HANDSHAKE_SH}read turn
case "$turn" in *'"method":"turn/start"'*'"effort":"high"'*'"text":"fix it"'*'"threadId":"thr_1"'*) ;; *) exit 8;; esac
echo '{{"jsonrpc":"2.0","id":3,"result":{{"turn":{{"id":"turn_c1"}}}}}}'
echo '{{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{{"delta":"On it"}}}}'
echo '{{"jsonrpc":"2.0","method":"item/started","params":{{"item":{{"type":"commandExecution","id":"call_1","command":"npm test"}}}}}}'
echo '{{"jsonrpc":"2.0","id":50,"method":"item/commandExecution/requestApproval","params":{{"itemId":"call_1"}}}}'
read answer
case "$answer" in *'"id":50'*'"decision":"accept"'*) ;; *) exit 9;; esac
echo '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"turn":{{"id":"turn_c1","status":"completed"}}}}}}'
read more"#
    );
    let (s, mut rx) = session(&script, start(false)).await.expect("start");
    assert_eq!(next(&mut rx).await, AdapterEvent::SessionId("thr_1".into()));
    let turn = TurnId::new();
    s.send_turn(TurnInput {
        turn_id: turn.clone(),
        text: "fix it".into(),
        images: Vec::new(),
        files: Vec::new(),
    })
    .await
    .expect("send");
    assert_eq!(
        next(&mut rx).await,
        AdapterEvent::Event {
            turn_id: turn.clone(),
            event: ThreadEvent::AssistantDelta {
                text: "On it".into()
            }
        }
    );
    assert!(matches!(
        next(&mut rx).await,
        AdapterEvent::Event {
            event: ThreadEvent::Tool { .. },
            ..
        }
    ));
    let id = match next(&mut rx).await {
        AdapterEvent::Approval {
            turn_id,
            id,
            kind,
            summary,
            ..
        } => {
            assert_eq!(turn_id, turn);
            assert_eq!(kind, ApprovalKind::Command);
            assert_eq!(summary, "Run `npm test`");
            id
        }
        other => panic!("unexpected {other:?}"),
    };
    s.resolve_approval(
        &id,
        ApprovalDecision::Allow {
            scope: ApprovalScope::Once,
        },
    )
    .await
    .expect("answer");
    assert_eq!(
        next(&mut rx).await,
        AdapterEvent::Event {
            turn_id: turn,
            event: ThreadEvent::TurnEnd {
                reason: TurnEndReason::Completed,
                blocked_actions: 0
            }
        }
    );
    assert_eq!(s.engine_session_id().as_deref(), Some("thr_1"));
    s.shutdown().await.expect("shutdown");
}

#[cfg(unix)]
#[tokio::test]
async fn f_th_05_2_a_restarted_session_resumes_the_thread_and_interrupts_its_turn() {
    let script = format!(
        r#"{HANDSHAKE_SH}read turn
echo '{{"jsonrpc":"2.0","id":3,"result":{{"turn":{{"id":"turn_x"}}}}}}'
read interrupt
case "$interrupt" in *'"method":"turn/interrupt"'*'"turnId":"turn_x"'*) echo '{{"jsonrpc":"2.0","id":4,"result":{{}}}}';; esac
read more"#
    );
    let mut req = start(false);
    req.resume_session = Some("thr_old".into());
    let (s, mut rx) = session(&script, req).await.expect("start");
    assert_eq!(
        next(&mut rx).await,
        AdapterEvent::SessionId("thr_old".into())
    );
    let turn = TurnId::new();
    s.send_turn(TurnInput {
        turn_id: turn.clone(),
        text: "go".into(),
        images: Vec::new(),
        files: Vec::new(),
    })
    .await
    .expect("send");
    let out = s.interrupt(&turn).await.expect("interrupt");
    assert_eq!(
        out,
        InterruptOutcome {
            acked: true,
            killed: false
        }
    );
    s.shutdown().await.expect("shutdown");
}

#[cfg(unix)]
#[tokio::test]
async fn f_th_05_3_an_old_codex_is_refused_before_or_at_the_handshake() {
    let mut req = start(false);
    req.engine_version = Some("codex-cli 0.31.0".into());
    let err = session("exit 0", req).await.err().expect("refused");
    assert_eq!(
        err.to_string(),
        "Codex version codex-cli 0.31.0 isn't supported yet — update Vibezeo or Codex"
    );
    // Without a detected version, the server's own is checked.
    let mut req = start(false);
    req.engine_version = None;
    let old = r#"read init
echo '{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex_cli_rs/0.20.1"}}'
read more"#;
    let err = session(old, req).await.err().expect("refused");
    assert!(err
        .to_string()
        .starts_with("Codex version codex_cli_rs/0.20.1 isn't supported yet"));
}

/// F-AG-06: Codex gets the briefing as developer instructions (over stdin),
/// ahead of Plan mode's own.
#[test]
fn f_ag_06_codex_gets_the_briefing_as_developer_instructions() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("th_1.md");
    std::fs::write(&path, "<brief>Keep the changelog.</brief>").expect("write");
    let mut req = start(false);
    req.briefing = Some(path);
    let (_, p) = protocol::V2.start_thread(&req);
    assert_eq!(
        p["developerInstructions"],
        "<brief>Keep the changelog.</brief>"
    );
    req.mode = ThreadMode::Plan;
    let (_, p) = protocol::V2.resume_thread("t1", &req);
    let text = p["developerInstructions"].as_str().expect("text");
    assert!(text.starts_with("<brief>Keep the changelog.</brief>\n\n"));
    req.briefing = None;
    req.mode = ThreadMode::Act;
    let (_, p) = protocol::V2.start_thread(&req);
    assert!(p.get("developerInstructions").is_none());
}

#[test]
fn f_tk_02_the_tasks_server_joins_codex_with_its_connection_env() {
    let mut req = start(false);
    req.stdio_servers = vec![crate::chat::StdioServer {
        name: "tandem_tasks".into(),
        command: "/opt/vibezeo/tandem-tasks".into(),
        args: vec![
            "mcp".into(),
            "--dir".into(),
            "/home/u/.tandem/tasks".into(),
            "--connection".into(),
            "tc_1".into(),
        ],
        env_vars: vec!["TANDEM_TASKS_CONNECTION".into()],
    }];
    let a: Vec<String> = args(&req)
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        a,
        vec![
            "app-server",
            "-c",
            "mcp_servers.tandem_tasks.command=\"/opt/vibezeo/tandem-tasks\"",
            "-c",
            "mcp_servers.tandem_tasks.args=[\"mcp\", \"--dir\", \"/home/u/.tandem/tasks\", \"--connection\", \"tc_1\"]",
            "-c",
            "mcp_servers.tandem_tasks.env_vars=[\"TANDEM_TASKS_CONNECTION\"]",
        ]
    );
}

/// F-EN-08 AC3: a failed turn Codex doesn't classify (its catch-all), a
/// network failure and a sign-in failure end the turn in error but aren't
/// the engine's own failure.
#[test]
fn f_en_08_3_codexs_turn_failures_arent_the_engines_fault() {
    for info in [
        serde_json::json!(null),
        serde_json::json!("internalServerError"),
        serde_json::json!("httpConnectionFailed"),
        serde_json::json!("unauthorized"),
    ] {
        let mut parser = CodexParser::new(PathBuf::from("/work/shop"), &patterns());
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "turn/completed",
            "params": {"turn": {"id": "t1", "status": "failed", "error": {"message": "stream error", "codexErrorInfo": info}}}
        });
        let out = parser.message(&msg, now());
        let mut ended = false;
        let mut errors = Vec::new();
        for c in &out {
            match c {
                Codex::Event(ThreadEvent::TurnEnd { reason, .. }) => {
                    ended = *reason == TurnEndReason::Error
                }
                Codex::Event(ThreadEvent::Error { stage, code, .. }) => {
                    errors.push(stage.engine_fault(code))
                }
                _ => {}
            }
        }
        assert!(ended, "{info}");
        assert_eq!(errors, vec![false], "{info}");
    }
}

#[test]
fn f_tk_07_1_a_file_change_names_its_files_absolute() {
    let mut parser = CodexParser::new(PathBuf::from("/work/shop"), &patterns());
    let item = |method: &str, status: &str| {
        json!({"jsonrpc": "2.0", "method": method, "params": {"threadId": "thr_1", "turnId": "turn_1", "item": {
            "type": "fileChange", "id": "fc_9", "status": status,
            "changes": [
                {"path": "/work/shop/src/cart.ts", "kind": {"type": "update"}, "diff": ""},
                {"path": "src/new, odd.ts", "kind": {"type": "add"}, "diff": ""}
            ]
        }}})
    };
    let want = Some(vec![
        "/work/shop/src/cart.ts".to_string(),
        "/work/shop/src/new, odd.ts".to_string(),
    ]);
    for (method, status) in [
        ("item/started", "inProgress"),
        ("item/completed", "completed"),
    ] {
        let paths: Vec<Option<Vec<String>>> = parser
            .message(&item(method, status), now())
            .into_iter()
            .filter_map(|c| match c {
                Codex::Event(ThreadEvent::Tool { paths, .. }) => Some(paths),
                _ => None,
            })
            .collect();
        assert_eq!(paths, vec![want.clone()], "{method}");
    }
    // A result that doesn't list the changes again keeps the files the
    // change started with: it's the row a claim is made from (D-2836).
    let started = json!({"jsonrpc": "2.0", "method": "item/started", "params": {"item": {
        "type": "fileChange", "id": "fc_10", "status": "inProgress",
        "changes": [{"path": "src/lone.ts", "kind": {"type": "update"}, "diff": ""}]
    }}});
    let bare = json!({"jsonrpc": "2.0", "method": "item/completed", "params": {"item": {
        "type": "fileChange", "id": "fc_10", "status": "completed"
    }}});
    let lone = Some(vec!["/work/shop/src/lone.ts".to_string()]);
    for msg in [started, bare] {
        let paths: Vec<Option<Vec<String>>> = parser
            .message(&msg, now())
            .into_iter()
            .filter_map(|c| match c {
                Codex::Event(ThreadEvent::Tool { paths, .. }) => Some(paths),
                _ => None,
            })
            .collect();
        assert_eq!(paths, vec![lone.clone()]);
    }
    // A command names no file.
    let cmd = json!({"jsonrpc": "2.0", "method": "item/started", "params": {"item": {
        "type": "commandExecution", "id": "call_9", "command": "touch x"
    }}});
    let paths: Vec<Option<Vec<String>>> = parser
        .message(&cmd, now())
        .into_iter()
        .filter_map(|c| match c {
            Codex::Event(ThreadEvent::Tool { paths, .. }) => Some(paths),
            _ => None,
        })
        .collect();
    assert_eq!(paths, vec![None]);
}

/// A file change that never completes keeps nothing past its turn.
#[test]
fn f_tk_07_1_a_file_change_that_never_completes_is_dropped_at_the_turns_end() {
    let mut parser = CodexParser::new(PathBuf::from("/work/shop"), &patterns());
    let started = json!({"jsonrpc": "2.0", "method": "item/started", "params": {"item": {
        "type": "fileChange", "id": "fc_11", "status": "inProgress",
        "changes": [{"path": "src/left.ts", "kind": {"type": "update"}, "diff": ""}]
    }}});
    parser.message(&started, now());
    assert_eq!(parser.edits.len(), 1);
    let ended = json!({"jsonrpc": "2.0", "method": "turn/completed",
        "params": {"turn": {"id": "t1", "status": "interrupted"}}});
    parser.message(&ended, now());
    assert!(parser.edits.is_empty());
}
