//! Lead's answer session (F-TK-08, D-2838): one fresh session of Lead's
//! engine for each question, with every tool off, no MCP server, no
//! briefing and an empty folder of its own outside the repository. Its one
//! message is all it knows: the plan's own task, the question and the
//! instructions files from the plan's base commit. The first tool row or
//! approval request ends it, and nothing it wrote is used.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tandem_core::access::{Access, ThreadMode};
use tandem_core::approvals::ApprovalDecision;
use tandem_core::ids::{ThreadId, TurnId};
use tandem_engines::chat::{AdapterError, AdapterEvent, ChatAdapter, ChatStart, TurnInput};
use tandem_engines::events::{ThreadEvent, TurnEndReason};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Why an answer session gave no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerFailed {
    /// Lead reached for a tool or asked for an approval.
    ToolUse,
    /// Lead's engine can't run with every tool off.
    Unsupported,
    /// The engine didn't start, stopped, or ended its turn with an error.
    Failed,
    TimedOut,
    /// Lead's answers were turned off, or the plan was removed.
    Cancelled,
}

/// An answer session's start: Lead's engine, account and model, read-only
/// in plan mode with every tool off, in `cwd`, and nothing else: no MCP
/// server, tasks server, briefing or session to resume.
pub fn answer_start(
    engine_id: &str,
    program: PathBuf,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    model: Option<String>,
    effort: Option<String>,
    engine_version: Option<String>,
) -> ChatStart {
    ChatStart {
        thread_id: ThreadId::new(),
        engine_id: engine_id.to_string(),
        program,
        cwd,
        env,
        model,
        effort,
        access: Access::Read,
        mode: ThreadMode::Plan,
        attended: false,
        resume_session: None,
        mcp_config: None,
        mcp: None,
        engine_version,
        relocate: None,
        briefing: None,
        stdio_servers: Vec::new(),
        tools_off: true,
    }
}

/// Runs one answer session: `prompt` as its only turn, and the engine's
/// reply when the turn completes. A tool row or an approval request ends
/// it with no answer, and so do `limit` and `cancel`. The session is shut
/// down whatever happens.
pub async fn run(
    adapter: Arc<dyn ChatAdapter>,
    start: ChatStart,
    prompt: String,
    limit: Duration,
    cancel: CancellationToken,
) -> Result<String, AnswerFailed> {
    if !start.tools_off || !adapter.answers_without_tools() {
        return Err(AnswerFailed::Unsupported);
    }
    let (tx, mut rx) = mpsc::channel(256);
    let session = match adapter.start(start, tx).await {
        Ok(s) => s,
        Err(AdapterError::Unsupported(_)) => return Err(AnswerFailed::Unsupported),
        Err(e) => {
            tracing::info!(code = "plan.answer_start_failed", error = %e, "Lead's answer session didn't start");
            return Err(AnswerFailed::Failed);
        }
    };
    let turn = TurnId::new();
    let outcome = async {
        session
            .send_turn(TurnInput {
                turn_id: turn.clone(),
                text: prompt,
                images: Vec::new(),
                files: Vec::new(),
            })
            .await
            .map_err(|_| AnswerFailed::Failed)?;
        let mut blocks: Vec<String> = Vec::new();
        let mut streamed = String::new();
        let deadline = tokio::time::sleep(limit);
        tokio::pin!(deadline);
        loop {
            let ev = tokio::select! {
                () = cancel.cancelled() => return Err(AnswerFailed::Cancelled),
                () = &mut deadline => return Err(AnswerFailed::TimedOut),
                ev = rx.recv() => ev,
            };
            match ev {
                None | Some(AdapterEvent::Exited { .. }) => return Err(AnswerFailed::Failed),
                Some(AdapterEvent::Approval { id, .. }) => {
                    let _ = session
                        .resolve_approval(
                            &id,
                            ApprovalDecision::Deny {
                                reason: Some("Lead answers without tools.".into()),
                            },
                        )
                        .await;
                    return Err(AnswerFailed::ToolUse);
                }
                Some(AdapterEvent::Event { event, .. }) => match event {
                    ThreadEvent::Tool { .. } | ThreadEvent::ApprovalRequest { .. } => {
                        return Err(AnswerFailed::ToolUse)
                    }
                    ThreadEvent::AssistantDelta { text } => streamed.push_str(&text),
                    ThreadEvent::Assistant { markdown } => {
                        blocks.push(markdown);
                        streamed.clear();
                    }
                    ThreadEvent::TurnEnd { reason, .. } => {
                        if reason != TurnEndReason::Completed {
                            return Err(AnswerFailed::Failed);
                        }
                        if !streamed.is_empty() {
                            blocks.push(std::mem::take(&mut streamed));
                        }
                        return Ok(blocks.join("\n"));
                    }
                    _ => {}
                },
                Some(_) => {}
            }
        }
    }
    .await;
    if outcome.is_err() {
        let _ = session.interrupt(&turn).await;
    }
    let _ = session.shutdown().await;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use tandem_core::allow_rules::ApprovalSubject;
    use tandem_core::approvals::ApprovalKind;
    use tandem_core::ids::ApprovalId;
    use tandem_engines::chat::{ChatSession, InterruptOutcome};
    use tandem_engines::events::{ToolStatus, ToolVerb};

    /// What the scripted engine does once the turn starts.
    enum Step {
        Event(ThreadEvent),
        Ask,
        Hang,
    }

    /// An engine that plays its steps and records what it was asked.
    struct Engine {
        steps: Mutex<Vec<Step>>,
        log: Arc<Mutex<Vec<String>>>,
        tool_free: bool,
    }

    struct Session {
        steps: Mutex<Vec<Step>>,
        tx: mpsc::Sender<AdapterEvent>,
        log: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl ChatAdapter for Engine {
        fn engine_id(&self) -> &str {
            "claude"
        }
        fn answers_without_tools(&self) -> bool {
            self.tool_free
        }
        async fn start(
            &self,
            req: ChatStart,
            tx: mpsc::Sender<AdapterEvent>,
        ) -> Result<Box<dyn ChatSession>, AdapterError> {
            self.log
                .lock()
                .expect("log")
                .push(format!("start tools_off={}", req.tools_off));
            let steps = std::mem::take(&mut *self.steps.lock().expect("steps"));
            Ok(Box::new(Session {
                steps: Mutex::new(steps),
                tx,
                log: self.log.clone(),
            }))
        }
    }

    #[async_trait]
    impl ChatSession for Session {
        fn engine_session_id(&self) -> Option<String> {
            None
        }
        async fn send_turn(&self, turn: TurnInput) -> Result<TurnId, AdapterError> {
            self.log.lock().expect("log").push("turn".into());
            let steps = std::mem::take(&mut *self.steps.lock().expect("steps"));
            let tx = self.tx.clone();
            let id = turn.turn_id.clone();
            tokio::spawn(async move {
                for step in steps {
                    let ev = match step {
                        Step::Event(event) => AdapterEvent::Event {
                            turn_id: id.clone(),
                            event,
                        },
                        Step::Ask => AdapterEvent::Approval {
                            turn_id: id.clone(),
                            id: ApprovalId::new(),
                            kind: ApprovalKind::Command,
                            summary: "cat .env".into(),
                            detail: String::new(),
                            subject: ApprovalSubject::Command("cat .env".into()),
                        },
                        Step::Hang => std::future::pending().await,
                    };
                    if tx.send(ev).await.is_err() {
                        return;
                    }
                }
            });
            Ok(turn.turn_id)
        }
        async fn interrupt(&self, _turn: &TurnId) -> Result<InterruptOutcome, AdapterError> {
            self.log.lock().expect("log").push("interrupt".into());
            Ok(InterruptOutcome {
                acked: true,
                killed: false,
            })
        }
        async fn resolve_approval(
            &self,
            _id: &ApprovalId,
            decision: ApprovalDecision,
        ) -> Result<(), AdapterError> {
            let denied = matches!(decision, ApprovalDecision::Deny { .. });
            self.log
                .lock()
                .expect("log")
                .push(format!("approval denied={denied}"));
            Ok(())
        }
        async fn set_model(
            &self,
            _model: Option<String>,
            _effort: Option<String>,
        ) -> Result<(), AdapterError> {
            Ok(())
        }
        async fn shutdown(&self) -> Result<(), AdapterError> {
            self.log.lock().expect("log").push("shutdown".into());
            Ok(())
        }
    }

    fn engine(steps: Vec<Step>) -> (Arc<Engine>, Arc<Mutex<Vec<String>>>) {
        let log: Arc<Mutex<Vec<String>>> = Arc::default();
        (
            Arc::new(Engine {
                steps: Mutex::new(steps),
                log: log.clone(),
                tool_free: true,
            }),
            log,
        )
    }

    fn start() -> ChatStart {
        answer_start(
            "claude",
            "/bin/claude".into(),
            "/data/run/answers/ans_1".into(),
            vec![(
                "CLAUDE_CONFIG_DIR".into(),
                "/home/u/.tandem/providers/claude/work".into(),
            )],
            Some("opus".into()),
            None,
            None,
        )
    }

    fn end() -> Step {
        Step::Event(ThreadEvent::TurnEnd {
            reason: TurnEndReason::Completed,
            blocked_actions: 0,
        })
    }

    const MINUTES_30: Duration = Duration::from_secs(30 * 60);

    #[test]
    fn f_tk_08_1_an_answer_session_starts_with_nothing_but_its_engine() {
        let s = start();
        assert!(s.tools_off);
        assert_eq!(
            (s.access, s.mode, s.attended),
            (Access::Read, ThreadMode::Plan, false)
        );
        assert!(s.mcp_config.is_none() && s.mcp.is_none());
        assert!(s.stdio_servers.is_empty());
        assert!(s.briefing.is_none() && s.resume_session.is_none() && s.relocate.is_none());
        assert_eq!(s.cwd, PathBuf::from("/data/run/answers/ans_1"));
        assert_eq!(s.model.as_deref(), Some("opus"));
    }

    #[tokio::test]
    async fn f_tk_08_2_an_answer_session_gives_its_reply_and_is_shut_down() {
        let (e, log) = engine(vec![
            Step::Event(ThreadEvent::AssistantDelta {
                text: "ANSWER-".into(),
            }),
            Step::Event(ThreadEvent::Assistant {
                markdown: "ANSWER-ab12cd34\nUse Vitest.\nEND-ab12cd34".into(),
            }),
            end(),
        ]);
        let got = run(
            e,
            start(),
            "the question".into(),
            MINUTES_30,
            CancellationToken::new(),
        )
        .await
        .expect("answer");
        assert_eq!(got, "ANSWER-ab12cd34\nUse Vitest.\nEND-ab12cd34");
        assert_eq!(
            *log.lock().expect("log"),
            vec!["start tools_off=true", "turn", "shutdown"]
        );
    }

    #[tokio::test]
    async fn f_tk_08_1_a_session_that_reaches_for_a_file_gives_no_answer() {
        // Lead reads the .env beside the main checkout and writes it into its
        // answer: the tool row ends the session, and the answer is dropped.
        let mut read =
            ThreadEvent::tool("t1", ToolVerb::Read, "/work/shop/.env", ToolStatus::Running);
        if let ThreadEvent::Tool { output, .. } = &mut read {
            *output = Some("API_KEY=hunter2".into());
        }
        let (e, log) = engine(vec![
            Step::Event(read),
            Step::Event(ThreadEvent::Assistant {
                markdown: "ANSWER-ab12cd34\nAPI_KEY=hunter2\nEND-ab12cd34".into(),
            }),
            end(),
        ]);
        let got = run(e, start(), "q".into(), MINUTES_30, CancellationToken::new()).await;
        assert_eq!(got, Err(AnswerFailed::ToolUse));
        assert_eq!(
            *log.lock().expect("log"),
            vec!["start tools_off=true", "turn", "interrupt", "shutdown"]
        );
    }

    #[tokio::test]
    async fn f_tk_08_1_an_approval_request_is_denied_and_gives_no_answer() {
        let (e, log) = engine(vec![Step::Ask, end()]);
        let got = run(e, start(), "q".into(), MINUTES_30, CancellationToken::new()).await;
        assert_eq!(got, Err(AnswerFailed::ToolUse));
        assert!(log
            .lock()
            .expect("log")
            .contains(&"approval denied=true".to_string()));
    }

    #[tokio::test]
    async fn f_tk_08_5_turning_answers_off_ends_the_session_at_once() {
        let (e, log) = engine(vec![Step::Hang]);
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            stop.cancel();
        });
        let got = run(e, start(), "q".into(), MINUTES_30, cancel).await;
        assert_eq!(got, Err(AnswerFailed::Cancelled));
        assert_eq!(
            log.lock().expect("log").last().map(String::as_str),
            Some("shutdown")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn f_tk_08_3_an_answer_session_ends_after_its_time() {
        let (e, _log) = engine(vec![Step::Hang]);
        let got = run(e, start(), "q".into(), MINUTES_30, CancellationToken::new()).await;
        assert_eq!(got, Err(AnswerFailed::TimedOut));
    }

    #[tokio::test]
    async fn f_tk_08_1_an_engine_that_cant_turn_tools_off_never_starts() {
        let log: Arc<Mutex<Vec<String>>> = Arc::default();
        let codex_like = Arc::new(Engine {
            steps: Mutex::new(vec![end()]),
            log: log.clone(),
            tool_free: false,
        });
        let got = run(
            codex_like,
            start(),
            "q".into(),
            MINUTES_30,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(got, Err(AnswerFailed::Unsupported));
        assert!(log.lock().expect("log").is_empty(), "never started");
    }
}
