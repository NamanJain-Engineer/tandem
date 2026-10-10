use super::*;
use std::collections::HashMap;
use std::path::PathBuf;

use tandem_core::access::{Access, ThreadMode};
use tandem_core::approvals::ApprovalScope;
use tandem_core::ids::ThreadId;

use crate::chat::McpEndpoint;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/acp")
}

fn patterns() -> Vec<String> {
    crate::registry::EngineRegistry::bundled()
        .get("grok")
        .expect("grok")
        .limit_patterns
        .clone()
}

fn subject_json(s: &ApprovalSubject) -> Value {
    match s {
        ApprovalSubject::Command(c) => json!({"command": c}),
        ApprovalSubject::Tool(t) => json!({"tool": t}),
    }
}

fn as_json(a: &Acp) -> Value {
    match a {
        Acp::Event(e) => serde_json::to_value(e).expect("event"),
        Acp::Approval {
            rpc_id,
            kind,
            summary,
            detail,
            subject,
            options,
        } => json!({"approval": {
            "rpcId": rpc_id, "kind": kind, "summary": summary, "detail": detail,
            "subject": subject_json(subject),
            "options": options.iter().map(|o| json!([o.id, o.kind])).collect::<Vec<_>>(),
        }}),
        Acp::Read {
            rpc_id,
            path,
            line,
            limit,
        } => json!({"read": {"rpcId": rpc_id, "path": path, "line": line, "limit": limit}}),
        Acp::Write {
            rpc_id,
            path,
            content,
        } => json!({"write": {"rpcId": rpc_id, "path": path, "content": content}}),
        Acp::Unsupported { rpc_id, method } => {
            json!({"unsupported": {"rpcId": rpc_id, "method": method}})
        }
    }
}

/// Compares with `<name>.golden.json` (`UPDATE_GOLDENS=1` rewrites, then reviewed).
fn golden(fixture: &std::path::Path, got: &[Value]) {
    let golden = fixture.with_extension("golden.json");
    if std::env::var_os("UPDATE_GOLDENS").is_some() {
        std::fs::write(
            &golden,
            serde_json::to_string_pretty(got).expect("json") + "\n",
        )
        .expect("write");
    }
    let want: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&golden).expect("golden"))
        .expect("golden json");
    assert_eq!(got, want.as_slice(), "{}", fixture.display());
}

/// F-TH-06: updates, permission requests and file requests map to their
/// golden events.
#[test]
fn f_th_06_1_acp_messages_map_to_their_golden_events() {
    let mut names: Vec<PathBuf> = std::fs::read_dir(fixtures())
        .expect("fixtures")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 3);
    for path in names {
        let mut parser = AcpParser::new(PathBuf::from("/work/shop"));
        let got: Vec<Value> = std::fs::read_to_string(&path)
            .expect("fixture")
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .flat_map(|m| parser.message(&m))
            .map(|a| as_json(&a))
            .collect();
        golden(&path, &got);
    }
}

#[test]
fn f_th_06_a_decision_picks_the_matching_option() {
    let opts = |kinds: &[&str]| -> Vec<PermissionOption> {
        kinds
            .iter()
            .map(|k| PermissionOption {
                id: format!("opt-{k}"),
                kind: (*k).to_string(),
            })
            .collect()
    };
    let all = opts(&["allow_once", "allow_always", "reject_once", "reject_always"]);
    let once = ApprovalDecision::Allow {
        scope: ApprovalScope::Once,
    };
    let thread = ApprovalDecision::Allow {
        scope: ApprovalScope::Thread,
    };
    let deny = ApprovalDecision::Deny { reason: None };
    assert_eq!(choose(&all, &once).as_deref(), Some("opt-allow_once"));
    assert_eq!(choose(&all, &thread).as_deref(), Some("opt-allow_always"));
    assert_eq!(choose(&all, &deny).as_deref(), Some("opt-reject_once"));
    // Fallbacks, and nothing that fits: the request is answered cancelled.
    let few = opts(&["allow_always", "reject_always"]);
    assert_eq!(choose(&few, &once).as_deref(), Some("opt-allow_always"));
    assert_eq!(choose(&few, &deny).as_deref(), Some("opt-reject_always"));
    assert_eq!(choose(&opts(&["allow_once"]), &deny), None);
}

#[test]
fn f_th_06_stop_reasons_and_failures_end_the_turn() {
    let end = |r| ThreadEvent::TurnEnd {
        reason: r,
        blocked_actions: 0,
    };
    assert_eq!(
        stopped("Grok", &json!({"stopReason": "end_turn"})),
        vec![end(TurnEndReason::Completed)]
    );
    assert_eq!(
        stopped("Grok", &json!({"stopReason": "cancelled"})),
        vec![end(TurnEndReason::Cancelled)]
    );
    for reason in ["max_tokens", "max_turn_requests", "refusal"] {
        let events = stopped("Grok", &json!({"stopReason": reason}));
        assert!(matches!(
            &events[0],
            ThreadEvent::Error { code, stage: ErrorStage::Model, action: Some(ErrorAction::Retry), .. } if code == reason
        ));
        assert_eq!(events[1], end(TurnEndReason::Error));
    }
    let now = limits::local(chrono::Utc::now());
    let compiled = limits::compile(&patterns());
    // Sign-in and model errors have their own stages (F-TH-06).
    let auth = prompt_failed("Grok", -32000, "Authentication required", &compiled, now);
    assert!(matches!(
        &auth[0],
        ThreadEvent::Error { stage: ErrorStage::Auth, action: Some(ErrorAction::SignIn), message, .. } if message == "Grok needs you to sign in."
    ));
    let model = prompt_failed("Grok", -32603, "model grok-9 not found", &compiled, now);
    assert!(matches!(
        &model[0],
        ThreadEvent::Error {
            stage: ErrorStage::Model,
            ..
        }
    ));
    let other = prompt_failed("Grok", -32603, "disk full", &compiled, now);
    assert!(matches!(
        &other[0],
        ThreadEvent::Error {
            stage: ErrorStage::Unknown,
            action: Some(ErrorAction::Retry),
            ..
        }
    ));
}

#[test]
fn f_th_06_a_diff_shows_the_changed_lines_with_context() {
    assert_eq!(
        unified(
            "a.rs",
            "1\n2\n3\n4\n5\nold\n7\n8\n9\n10\n",
            "1\n2\n3\n4\n5\nnew\nadded\n7\n8\n9\n10\n"
        ),
        "--- a/a.rs\n+++ b/a.rs\n@@ -3,7 +3,8 @@\n 3\n 4\n 5\n-old\n+new\n+added\n 7\n 8\n 9\n"
    );
    assert_eq!(
        unified("n.md", "", "# Notes\n"),
        "--- /dev/null\n+++ b/n.md\n@@ -0,0 +1,1 @@\n+# Notes\n"
    );
}

// ---- Sessions against a scripted agent ---------------------------------

/// A stand-in agent from a session fixture: `agent` lines are printed,
/// `client` lines read one message and check it contains every needle.
fn script(name: &str, vars: &[(&str, &str)]) -> String {
    let path = fixtures().join("sessions").join(format!("{name}.jsonl"));
    let mut out = String::new();
    for (n, raw) in std::fs::read_to_string(&path)
        .expect("fixture")
        .lines()
        .enumerate()
    {
        let mut line = raw.to_string();
        for (k, v) in vars {
            line = line.replace(k, v);
        }
        let entry: Value = serde_json::from_str(&line).expect("fixture line");
        if let Some(agent) = entry.get("agent") {
            let text = agent.to_string();
            assert!(!text.contains('\''), "no single quotes in agent lines");
            out.push_str(&format!("printf '%s\\n' '{text}'\n"));
        }
        if let Some(needles) = entry.get("client").and_then(Value::as_array) {
            out.push_str("read -r l || exit 8\n");
            for needle in needles.iter().filter_map(Value::as_str) {
                out.push_str(&format!(
                    "case \"$l\" in *'{needle}'*) ;; *) echo \"line {}: $l\" >&2; exit 9;; esac\n",
                    n + 1
                ));
            }
        }
    }
    out.push_str("read -r more\n");
    out
}

fn start(cwd: &std::path::Path) -> ChatStart {
    ChatStart {
        thread_id: ThreadId::new(),
        engine_id: "grok".into(),
        program: "/bin/sh".into(),
        cwd: cwd.to_path_buf(),
        env: vec![("PATH".into(), "/usr/bin:/bin".into())],
        model: None,
        effort: None,
        access: Access::Edits,
        mode: ThreadMode::Act,
        attended: true,
        resume_session: None,
        mcp_config: None,
        mcp: None,
        engine_version: None,
        briefing: None,
    }
}

async fn session(
    name: &str,
    req: ChatStart,
    vars: &[(&str, &str)],
) -> (
    Result<Box<dyn ChatSession>, AdapterError>,
    mpsc::Receiver<AdapterEvent>,
) {
    let adapter = AcpAdapter::new("grok", "Grok", &["grok".into(), "acp".into()], patterns());
    let spec = ProcessSpec::new(
        "/bin/sh".into(),
        vec!["-c".into(), script(name, vars).into()],
        req.env.clone(),
        req.cwd.clone(),
    );
    let (tx, rx) = mpsc::channel(64);
    (adapter.start_with(spec, req, tx).await, rx)
}

async fn next(rx: &mut mpsc::Receiver<AdapterEvent>) -> AdapterEvent {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("an event in time")
        .expect("open")
}

fn input(turn: &TurnId, text: &str) -> TurnInput {
    TurnInput {
        turn_id: turn.clone(),
        text: text.into(),
        images: Vec::new(),
        files: Vec::new(),
    }
}

/// Events as golden JSON; turns are numbered in the order they were sent.
struct Recorder {
    turns: HashMap<TurnId, usize>,
    got: Vec<Value>,
}

impl Recorder {
    fn new() -> Self {
        Self {
            turns: HashMap::new(),
            got: Vec::new(),
        }
    }

    fn turn(&mut self) -> TurnId {
        let id = TurnId::new();
        self.turns.insert(id.clone(), self.turns.len() + 1);
        id
    }

    fn push(&mut self, e: &AdapterEvent) {
        let v = match e {
            AdapterEvent::Event { turn_id, event } => {
                json!({"turn": self.turns.get(turn_id), "event": event})
            }
            AdapterEvent::SessionId(id) => json!({"sessionId": id}),
            AdapterEvent::Approval {
                turn_id,
                kind,
                summary,
                detail,
                subject,
                ..
            } => json!({"turn": self.turns.get(turn_id), "approval": {
                "kind": kind, "summary": summary, "detail": detail, "subject": subject_json(subject),
            }}),
            AdapterEvent::Models {
                available,
                withdrawn,
            } => json!({"models": {
                "available": available.iter().map(|m| json!([m.id, m.label])).collect::<Vec<_>>(),
                "withdrawn": withdrawn,
            }}),
            AdapterEvent::Exited { code, .. } => json!({"exited": code}),
        };
        self.got.push(v);
    }

    /// Records events until the turn ends, answering approvals with `decide`.
    async fn until_end(
        &mut self,
        s: &dyn ChatSession,
        rx: &mut mpsc::Receiver<AdapterEvent>,
        decide: &ApprovalDecision,
    ) {
        loop {
            let e = next(rx).await;
            self.push(&e);
            match e {
                AdapterEvent::Approval { id, .. } => {
                    s.resolve_approval(&id, decide.clone())
                        .await
                        .expect("answer");
                }
                AdapterEvent::Event {
                    event: ThreadEvent::TurnEnd { .. },
                    ..
                } => return,
                AdapterEvent::Exited { .. } => panic!("the agent exited: {:?}", self.got),
                _ => {}
            }
        }
    }
}

fn tmp() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tmp");
    let cwd = dir.path().canonicalize().expect("canonical");
    (dir, cwd)
}

/// F-TH-06 AC1: initialize, session/new (with the gateway), a prompt with 3
/// updates and a permission request, then a cancelled prompt.
#[cfg(unix)]
#[tokio::test]
async fn f_th_06_1_the_conformance_fixture_produces_the_expected_events() {
    let (_dir, cwd) = tmp();
    let mut req = start(&cwd);
    req.mcp = Some(McpEndpoint {
        url: "http://127.0.0.1:4000/mcp".into(),
        token: "lease-token".into(),
    });
    let cwd_text = cwd.display().to_string();
    let (s, mut rx) = session("conformance", req, &[("{cwd}", &cwd_text)]).await;
    let s = s.expect("start");
    let mut rec = Recorder::new();
    rec.push(&next(&mut rx).await);
    let once = ApprovalDecision::Allow {
        scope: ApprovalScope::Once,
    };

    let first = rec.turn();
    s.send_turn(input(&first, "tidy the tests"))
        .await
        .expect("send");
    rec.until_end(s.as_ref(), &mut rx, &once).await;

    let second = rec.turn();
    s.send_turn(input(&second, "now split the module"))
        .await
        .expect("send");
    let e = next(&mut rx).await;
    rec.push(&e);
    let outcome = s.interrupt(&second).await.expect("stop");
    rec.got
        .push(json!({"interrupt": {"acked": outcome.acked, "killed": outcome.killed}}));
    rec.until_end(s.as_ref(), &mut rx, &once).await;
    assert_eq!(s.engine_session_id().as_deref(), Some("sess_1"));
    s.shutdown().await.expect("shutdown");
    golden(&fixtures().join("sessions/conformance.jsonl"), &rec.got);
}

/// F-TH-06 AC2: the agent no longer offers the thread's model: the picker
/// gets the agent's list, the thread falls back to Automatic, and the next
/// turn says so.
#[cfg(unix)]
#[tokio::test]
async fn f_th_06_2_a_withdrawn_model_is_cleared_with_the_note() {
    let (_dir, cwd) = tmp();
    let mut req = start(&cwd);
    req.model = Some("grok-3".into());
    let (s, mut rx) = session("models_withdrawn", req, &[]).await;
    let s = s.expect("start");
    let mut rec = Recorder::new();
    rec.push(&next(&mut rx).await);
    rec.push(&next(&mut rx).await);
    let turn = rec.turn();
    s.send_turn(input(&turn, "hello")).await.expect("send");
    rec.until_end(
        s.as_ref(),
        &mut rx,
        &ApprovalDecision::Deny { reason: None },
    )
    .await;
    s.shutdown().await.expect("shutdown");
    golden(
        &fixtures().join("sessions/models_withdrawn.jsonl"),
        &rec.got,
    );
}

/// A resumed session is loaded (its replayed history isn't a turn), an
/// offered model is selected, and a rate limit ends the turn as a limit.
#[cfg(unix)]
#[tokio::test]
async fn f_th_06_a_loaded_session_selects_the_model_and_reports_a_limit() {
    let (_dir, cwd) = tmp();
    let mut req = start(&cwd);
    req.model = Some("grok-code-fast".into());
    req.resume_session = Some("sess_old".into());
    let (s, mut rx) = session("models_selected", req, &[]).await;
    let s = s.expect("start");
    let mut rec = Recorder::new();
    rec.push(&next(&mut rx).await);
    rec.push(&next(&mut rx).await);
    let turn = rec.turn();
    s.send_turn(input(&turn, "hello")).await.expect("send");
    rec.until_end(
        s.as_ref(),
        &mut rx,
        &ApprovalDecision::Deny { reason: None },
    )
    .await;
    s.shutdown().await.expect("shutdown");
    golden(&fixtures().join("sessions/models_selected.jsonl"), &rec.got);
}

/// F-TH-06: an agent that needs a sign-in fails the start as an auth error
/// (shown with Sign in), not as a crash.
#[cfg(unix)]
#[tokio::test]
async fn f_th_06_a_sign_in_error_is_an_auth_error() {
    let (_dir, cwd) = tmp();
    let (s, _rx) = session("auth_error", start(&cwd), &[]).await;
    match s {
        Err(AdapterError::Auth(m)) => assert_eq!(m, "Grok needs you to sign in."),
        Err(other) => panic!("unexpected {other}"),
        Ok(_) => panic!("started"),
    }
}

/// Files are read and written only inside the thread's folder; writes and
/// edits are refused at Read access; terminals aren't offered.
#[cfg(unix)]
#[tokio::test]
async fn f_th_09_acp_file_requests_stay_in_the_folder_and_respect_read_access() {
    for access in [Access::Edits, Access::Read] {
        let (_dir, cwd) = tmp();
        std::fs::write(cwd.join("notes.txt"), "one\ntwo\nthree\n").expect("write");
        let mut req = start(&cwd);
        req.access = access;
        let cwd_text = cwd.display().to_string();
        let write = if access == Access::Read {
            "Read access"
        } else {
            // JSON-escaped: it lands inside a string in the fixture.
            r#"\"result\":null"#
        };
        let (s, mut rx) = session("fs", req, &[("{cwd}", &cwd_text), ("{write}", write)]).await;
        let s = s.expect("start");
        let mut rec = Recorder::new();
        rec.push(&next(&mut rx).await);
        let turn = rec.turn();
        s.send_turn(input(&turn, "go")).await.expect("send");
        rec.until_end(
            s.as_ref(),
            &mut rx,
            &ApprovalDecision::Deny { reason: None },
        )
        .await;
        let approvals = rec
            .got
            .iter()
            .filter(|v| v.get("approval").is_some())
            .count();
        let ended = rec.got.last().cloned().unwrap_or_default();
        assert_eq!(
            ended["event"]["reason"], "completed",
            "{access:?}: {:?}",
            rec.got
        );
        let written = std::fs::read_to_string(cwd.join("out/new.txt")).ok();
        if access == Access::Read {
            // The edit was declined without a card, and nothing was written.
            assert_eq!(approvals, 0);
            assert_eq!(written, None);
        } else {
            assert_eq!(approvals, 1);
            assert_eq!(written.as_deref(), Some("written"));
        }
        s.shutdown().await.expect("shutdown");
    }
}

/// Waits for `turn`'s end; a script whose check failed exits instead.
async fn ended(rx: &mut mpsc::Receiver<AdapterEvent>, turn: &TurnId) {
    loop {
        match next(rx).await {
            AdapterEvent::Event {
                turn_id,
                event: ThreadEvent::TurnEnd { .. },
            } if &turn_id == turn => return,
            AdapterEvent::Exited { code, stderr_tail } => {
                panic!("the agent exited ({code:?}): {stderr_tail}")
            }
            _ => {}
        }
    }
}

/// F-AG-06: an ACP agent gets the teammate briefing at the start of the
/// session's first prompt, and only there.
#[cfg(unix)]
#[tokio::test]
async fn f_ag_06_the_briefing_opens_the_first_acp_prompt_only() {
    let (_dir, cwd) = tmp();
    let briefing = cwd.join("briefing.md");
    std::fs::write(&briefing, "<brief>Keep the changelog.</brief>").expect("briefing");
    let mut req = start(&cwd);
    req.briefing = Some(briefing);
    let (s, mut rx) = session("briefing", req, &[]).await;
    let s = s.expect("start");
    let first = TurnId::new();
    s.send_turn(input(&first, "hello")).await.expect("send");
    ended(&mut rx, &first).await;
    let second = TurnId::new();
    s.send_turn(input(&second, "again")).await.expect("send");
    ended(&mut rx, &second).await;
    s.shutdown().await.expect("shutdown");
}
