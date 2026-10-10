//! F-TL-06 rule 5: voice (Conductor) and the phone can't perform
//! local-review-only actions (F-CN-03, F-RM-03). Only a person in the main
//! window produces a `LocalReview`. M16 adds the live voice and phone paths:
//! the voice path is Conductor's executor, below.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tandem_core::origin::{LocalReview, Origin};
use tandem_core::settings::SettingKey;
use tandem_core::voice_tools::{self, Review};
use tandem_voice::conductor::confirm::Confirms;
use tandem_voice::conductor::exec::{
    ExecTiming, Executor, PaneFacts, PaneKind, ToolCall, ToolHost,
};
use tandem_voice::conductor::view::UiAction;
use tokio_util::sync::CancellationToken;

#[test]
fn invariant_rule_5_local_review_origin() {
    assert!(LocalReview::check(&Origin::main_window()).is_some());
    for origin in [
        Origin::Conductor,
        Origin::Phone,
        Origin::Unattended,
        Origin::Local {
            window: "dictate-overlay".into(),
        },
    ] {
        assert!(
            LocalReview::check(&origin).is_none(),
            "{origin:?} must not pass local review"
        );
    }
}

/// The app as Conductor sees it: one shell pane; what ran is recorded.
#[derive(Default)]
struct Host {
    ran: Mutex<Vec<String>>,
}

#[async_trait]
impl ToolHost for Host {
    async fn pane(&self, _pane: &str) -> Option<PaneFacts> {
        Some(PaneFacts {
            title: "zsh".into(),
            kind: PaneKind::Shell,
            waiting: false,
            working: false,
        })
    }
    async fn type_prompt(&self, _p: &str, text: &str, _s: bool) -> Result<(), String> {
        self.ran.lock().push(format!("typed {text}"));
        Ok(())
    }
    async fn run_in_shell(&self, _p: &str, cmd: &str, _r: LocalReview) -> Result<(), String> {
        self.ran.lock().push(format!("ran {cmd}"));
        Ok(())
    }
    async fn read_pane(&self, _p: &str, _l: usize) -> Result<String, String> {
        Ok(String::new())
    }
    async fn get_setting(&self, _k: SettingKey) -> Result<Value, String> {
        Ok(Value::Null)
    }
    async fn set_setting(&self, _k: SettingKey, _v: Value) -> Result<(), String> {
        Ok(())
    }
    async fn run_tool(&self, name: &str, _a: &Value) -> Result<String, String> {
        self.ran.lock().push(name.to_string());
        Ok(String::new())
    }
    fn ui_action(&self, _a: UiAction) {}
}

#[test]
fn invariant_rule_5_local_review_actions_are_not_voice_tools() {
    for name in [
        "remove_lane",
        "force_remove_lane",
        "enable_full_access",
        "set_access",
        "approve_plugin",
        "resolve_approval",
    ] {
        assert!(voice_tools::find(name).is_none(), "{name} is a voice tool");
    }
    // The one local-review action voice can ask for waits for a click.
    let screen_only: Vec<_> = voice_tools::catalog()
        .iter()
        .filter(|t| t.review == Review::ScreenOnly)
        .map(|t| t.name)
        .collect();
    assert_eq!(screen_only, ["run_command_in_shell"]);
}

#[tokio::test]
async fn invariant_rule_5_live_voice_path_never_runs_a_shell_command_without_a_click() {
    let host = Arc::new(Host::default());
    let confirms = Arc::new(Confirms::new(Arc::new(|_| {})));
    let exec = Arc::new(Executor::new(
        host.clone(),
        confirms.clone(),
        CancellationToken::new(),
        ExecTiming {
            poll: Duration::from_millis(10),
            busy_wait: Duration::from_millis(200),
            answer_wait: Duration::from_secs(5),
        },
        "inv",
    ));
    let e = exec.clone();
    let run = tokio::spawn(async move {
        e.run(&ToolCall {
            id: "vc.1.0.run_command_in_shell".into(),
            name: "run_command_in_shell".into(),
            args: json!({"pane": "pn_1", "cmd": "rm -rf build"}),
        })
        .await
    });
    let q = loop {
        if let Some(q) = confirms.head() {
            break q;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    // A spoken "yes" is refused; the voice and phone origins can't make the
    // click's proof; another window's click can't either.
    assert!(confirms.answer_by_voice(&q.request_id, true).is_err());
    for origin in [
        Origin::Conductor,
        Origin::Phone,
        Origin::Local {
            window: "dictate-widget".into(),
        },
    ] {
        assert!(LocalReview::check(&origin).is_none(), "{origin:?}");
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(host.ran.lock().is_empty());
    assert!(!run.is_finished());
    // Only the main window's click runs it.
    let review = LocalReview::check(&Origin::main_window()).expect("main");
    confirms
        .answer_on_screen(&q.request_id, Some(review))
        .expect("click");
    assert!(run.await.expect("join").result.ok);
    assert_eq!(
        host.ran.lock().clone(),
        vec!["ran rm -rf build".to_string()]
    );
}
