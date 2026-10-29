//! The local tool executor (F-CN-02, F-CN-03, ARCHITECTURE §12.6
//! "ActionRegistry"). The doer plans server-side; every tool call it
//! returns runs here, through the rules below, and the result goes back.
//!
//! - A call outside the catalog or its types is refused ([`schema`]).
//! - `set_setting` and `get_setting` reach only the closed list (AC2).
//! - Conductor types into agents, never straight into a shell, and never
//!   into a pane waiting on an approval (F-CN-03 AC1). Text has control
//!   characters stripped; Enter is the host's separate write (F-EN-06).
//! - Closing a pane, executing a task and creating a teammate ask first; a
//!   shell command runs only after a click in the main window (AC3).
//! - Every output passes the secret redactor before the doer or the talker
//!   sees it (AC4), as written and past what's removable, as they read it
//!   (D-3036), capped at 8 KB.
//! - Once the call ends, nothing more runs: each effect checks the call's
//!   cancellation first (F-CN-01 AC2).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_core::origin::LocalReview;
use tandem_core::settings::{self, SettingKey};
use tandem_core::voice_tools::{self, Review};
use tokio_util::sync::CancellationToken;

use super::confirm::{Answer, Confirms};
use super::schema;
use super::view::{middle_truncate, QuestionView, UiAction};

/// The most of a result the doer gets (the API refuses more).
pub const MAX_OUTPUT: usize = 8 * 1024;
/// `read_pane` without `lines`.
pub const DEFAULT_LINES: usize = 50;

/// One tool call from the doer (`POST /v1/voice/act`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

/// Its result, as the next `/voice/act` round sends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    pub id: String,
    pub ok: bool,
    pub output: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    Agent,
    Shell,
}

/// What the executor needs to know about a pane before it types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneFacts {
    pub title: String,
    pub kind: PaneKind,
    /// The agent waits on an approval or a question.
    pub waiting: bool,
    /// The agent is busy; a prompt waits until it's back at its prompt.
    pub working: bool,
}

/// The app the tools act on; the desktop implements it, tests fake it.
#[async_trait]
pub trait ToolHost: Send + Sync {
    async fn pane(&self, pane: &str) -> Option<PaneFacts>;
    /// Types `text` into an agent's prompt; `send` presses Enter after it,
    /// as a separate write.
    async fn type_prompt(&self, pane: &str, text: &str, send: bool) -> Result<(), String>;
    /// Runs `cmd` in a shell pane. Only a click in the main window makes the
    /// [`LocalReview`] this takes.
    async fn run_in_shell(&self, pane: &str, cmd: &str, review: LocalReview) -> Result<(), String>;
    /// The pane's last `lines` lines, plain text, not yet redacted.
    async fn read_pane(&self, pane: &str, lines: usize) -> Result<String, String>;
    async fn get_setting(&self, key: SettingKey) -> Result<Value, String>;
    async fn set_setting(&self, key: SettingKey, value: Value) -> Result<(), String>;
    /// Every other tool: awareness, threads, teammates, tasks, find work,
    /// usage, updates. Returns what to tell the doer.
    async fn run_tool(&self, name: &str, args: &Value) -> Result<String, String>;
    /// A navigation-only tool, for the UI to perform.
    fn ui_action(&self, action: UiAction);
}

/// Tools the UI performs (`conductor:ui-action`).
const UI_TOOLS: &[&str] = &[
    "switch_mode",
    "open_settings",
    "toggle_sidebar",
    "open_sidebar_tab",
    "open_launch_wizard",
    "open_routines",
    "dismiss",
    "tidy",
    "flip",
];

#[derive(Debug, Clone)]
pub struct ExecTiming {
    /// How often a prompt waiting for a busy agent looks again.
    pub poll: Duration,
    /// How long a prompt waits for a busy agent.
    pub busy_wait: Duration,
    /// How long a question waits for an answer.
    pub answer_wait: Duration,
}

impl Default for ExecTiming {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(250),
            busy_wait: Duration::from_secs(60),
            answer_wait: Duration::from_secs(120),
        }
    }
}

pub struct Executor {
    host: Arc<dyn ToolHost>,
    confirms: Arc<Confirms>,
    cancel: CancellationToken,
    timing: ExecTiming,
    questions: AtomicU64,
    call_tag: String,
}

/// What [`Executor::run`] did, for the chip and the call log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub result: ToolResult,
    /// "Confirmed on screen", "Confirmed by voice".
    pub confirmed: Option<&'static str>,
}

type Outcome = Result<String, String>;

const ENDED: &str = "Cancelled: the call ended, so nothing was done.";

impl Executor {
    pub fn new(
        host: Arc<dyn ToolHost>,
        confirms: Arc<Confirms>,
        cancel: CancellationToken,
        timing: ExecTiming,
        call_tag: impl Into<String>,
    ) -> Self {
        Self {
            host,
            confirms,
            cancel,
            timing,
            questions: AtomicU64::new(0),
            call_tag: call_tag.into(),
        }
    }

    pub fn confirms(&self) -> &Arc<Confirms> {
        &self.confirms
    }

    /// The chip's label for a call (before it runs).
    pub async fn label(&self, call: &ToolCall) -> String {
        let a = &call.args;
        let s = |k: &str| a[k].as_str().unwrap_or_default().to_string();
        let label = match call.name.as_str() {
            "switch_mode" => format!("{} mode", capitalized(&s("mode"))),
            "open_settings" => format!("Settings › {}", capitalized(&s("page"))),
            "prompt_pane" => format!("Prompting {}: {}", self.title(&s("pane")).await, s("text")),
            "read_pane" => format!("Read {}", self.title(&s("pane")).await),
            "focus_pane" => format!("Focused {}", self.title(&s("pane")).await),
            "close_pane" => format!("Close {}", self.title(&s("pane")).await),
            "run_command_in_shell" => format!("Run `{}`", s("cmd")),
            "set_setting" => format!(
                "Set {} to {}",
                s("key").replace('_', " "),
                value_text(&a["value"])
            ),
            "get_setting" => format!("Read {}", s("key").replace('_', " ")),
            "find_work" => format!("Find “{}”", s("query")),
            "review_next" => "Next to review".to_string(),
            "review_send_back" => format!("Send back: {}", s("text")),
            "review_approve" => "Approve the review item".to_string(),
            "launch_agents" => format!(
                "Launch {} × {}",
                a["count"].as_u64().unwrap_or(1),
                s("engine")
            ),
            other => humanized(other),
        };
        middle_truncate(&tandem_core::redact::redact(&label), 40)
    }

    async fn title(&self, pane: &str) -> String {
        self.host
            .pane(pane)
            .await
            .map(|p| p.title)
            .unwrap_or_else(|| pane.to_string())
    }

    /// Runs one call through the rules. Never panics; a refusal is a result.
    pub async fn run(&self, call: &ToolCall) -> Ran {
        let mut confirmed = None;
        let outcome = if self.cancel.is_cancelled() {
            Err(ENDED.to_string())
        } else if matches!(call.name.as_str(), "set_setting" | "get_setting")
            && closed_key(&call.args).is_err()
        {
            // Off the closed list (F-CN-02 AC2), whatever else the call says.
            Err("Conductor can't change that setting.".to_string())
        } else if let Err(why) = schema::check(&call.name, &call.args) {
            Err(format!("Refused: {why}."))
        } else {
            self.dispatch(call, &mut confirmed).await
        };
        let (ok, output) = match outcome {
            Ok(out) => (true, out),
            Err(why) => (false, why),
        };
        Ran {
            result: ToolResult {
                id: call.id.clone(),
                ok,
                // Conductor's model reads past what's removable (D-3036).
                output: capped(&tandem_core::untrusted::redacted(&output)),
            },
            confirmed,
        }
    }

    async fn dispatch(&self, call: &ToolCall, confirmed: &mut Option<&'static str>) -> Outcome {
        let a = &call.args;
        let spec = voice_tools::find(&call.name)
            .ok_or_else(|| format!("{} isn't a Conductor tool.", call.name))?;
        match call.name.as_str() {
            "prompt_pane" => {
                let text = a["text"].as_str().unwrap_or_default();
                self.prompt(arg(a, "pane")?, text, a["send"].as_bool().unwrap_or(false))
                    .await
            }
            "prompt_panes" => self.prompt_many(a).await,
            "read_pane" => {
                let lines = a["lines"].as_u64().map_or(DEFAULT_LINES, |n| n as usize);
                let raw = self.host.read_pane(arg(a, "pane")?, lines).await?;
                Ok(last_lines(&raw, lines))
            }
            "get_setting" => {
                let key = closed_key(a)?;
                self.host
                    .get_setting(key)
                    .await
                    .map(|v| format!("{} is {}", key.as_str(), value_text(&v)))
            }
            "set_setting" => {
                let key = closed_key(a)?;
                let value = a["value"].clone();
                settings::validate(key, &value)
                    .map_err(|_| format!("{} can't be set to that.", key.as_str()))?;
                self.effect()?;
                self.host.set_setting(key, value.clone()).await?;
                Ok(format!("{} is now {}", key.as_str(), value_text(&value)))
            }
            "run_command_in_shell" => {
                let (out, how) = self.shell(arg(a, "pane")?, arg(a, "cmd")?).await?;
                *confirmed = Some(how);
                Ok(out)
            }
            name if UI_TOOLS.contains(&name) => {
                self.effect()?;
                self.host.ui_action(UiAction {
                    action: name.to_string(),
                    args: a.clone(),
                });
                Ok("Done.".into())
            }
            name => {
                if spec.review == Review::VoiceOrScreen {
                    let summary = self.summary(call).await;
                    *confirmed = Some(self.ask(call, summary, spec.review).await?.0);
                }
                self.effect()?;
                self.host.run_tool(name, a).await
            }
        }
    }

    /// Checked just before anything with an effect: after hang-up, nothing.
    fn effect(&self) -> Result<(), String> {
        if self.cancel.is_cancelled() {
            Err(ENDED.into())
        } else {
            Ok(())
        }
    }

    async fn prompt(&self, pane: &str, text: &str, send: bool) -> Outcome {
        let mut facts = self.typable(pane).await?;
        // A busy agent gets the prompt once it's back at its prompt.
        let waited = tokio::time::Instant::now();
        while facts.working {
            if waited.elapsed() >= self.timing.busy_wait {
                return Err(format!("{} is still busy; nothing was typed.", facts.title));
            }
            tokio::select! {
                _ = self.cancel.cancelled() => return Err(ENDED.into()),
                _ = tokio::time::sleep(self.timing.poll) => {}
            }
            facts = self.typable(pane).await?;
        }
        self.effect()?;
        // The model reads panes anyone can write to (D-3023, D-3026).
        let clean = words(text);
        if clean.trim().is_empty() {
            return Err("There was nothing to type.".into());
        }
        self.host.type_prompt(pane, &clean, send).await?;
        Ok(if send {
            format!("Sent to {}.", facts.title)
        } else {
            format!("Typed into {} without sending.", facts.title)
        })
    }

    /// The pane as it is now, if Conductor may type into it.
    async fn typable(&self, pane: &str) -> Result<PaneFacts, String> {
        let facts = self
            .host
            .pane(pane)
            .await
            .ok_or_else(|| format!("There's no pane {pane}."))?;
        if facts.kind == PaneKind::Shell {
            return Err(format!(
                "{} is a shell, and Conductor only types into agents. A command there needs run_command_in_shell and a click on screen.",
                facts.title
            ));
        }
        if facts.waiting {
            return Err(format!(
                "{} is waiting for approval, so nothing was typed. Tell the user to answer it first.",
                facts.title
            ));
        }
        Ok(facts)
    }

    async fn prompt_many(&self, a: &Value) -> Outcome {
        let panes: Vec<&str> = a["panes"]
            .as_array()
            .map(|p| p.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let texts: Vec<&str> = a["texts"]
            .as_array()
            .map(|t| t.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if texts.len() != 1 && texts.len() != panes.len() {
            return Err("Give one text, or one per pane.".into());
        }
        let mut lines = Vec::new();
        let mut any = false;
        for (i, pane) in panes.iter().enumerate() {
            let text = texts.get(i).or(texts.first()).copied().unwrap_or_default();
            match self.prompt(pane, text, true).await {
                Ok(out) => {
                    any = true;
                    lines.push(out);
                }
                Err(why) => lines.push(why),
            }
        }
        let out = lines.join("\n");
        if any {
            Ok(out)
        } else {
            Err(out)
        }
    }

    async fn shell(&self, pane: &str, cmd: &str) -> Result<(String, &'static str), String> {
        let facts = self
            .host
            .pane(pane)
            .await
            .ok_or_else(|| format!("There's no pane {pane}."))?;
        if facts.kind != PaneKind::Shell {
            return Err(format!("{} runs an agent, not a shell.", facts.title));
        }
        let clean = strip_controls(cmd);
        let summary = format!("Run `{clean}` in {}?", facts.title);
        let call = ToolCall {
            id: String::new(),
            name: "run_command_in_shell".into(),
            args: Value::Null,
        };
        let (how, review) = self.ask(&call, summary, Review::ScreenOnly).await?;
        let review = review.ok_or("Shell commands need a click on screen.")?;
        // If the pane changed while the question was up, it's stale.
        match self.host.pane(pane).await {
            Some(now) if now.kind == PaneKind::Shell => {}
            _ => return Err(format!("Expired: {} changed.", facts.title)),
        }
        self.effect()?;
        self.host.run_in_shell(pane, &clean, review).await?;
        Ok((format!("Ran it in {}.", facts.title), how))
    }

    async fn summary(&self, call: &ToolCall) -> String {
        let a = &call.args;
        let s = |k: &str| a[k].as_str().unwrap_or_default().to_string();
        match call.name.as_str() {
            "close_pane" => {
                let pane = s("pane");
                let title = self.host.pane(&pane).await.map(|p| p.title).unwrap_or(pane);
                format!("Close {title}?")
            }
            "execute_task" => format!("Execute task {}?", s("task")),
            "create_teammate" => format!(
                "Create a teammate: {}?",
                middle_truncate(&s("description"), 80)
            ),
            other => format!("{}?", humanized(other)),
        }
    }

    /// Asks and waits. Returns how it was confirmed (and the click's proof),
    /// or why not.
    async fn ask(
        &self,
        call: &ToolCall,
        summary: String,
        review: Review,
    ) -> Result<(&'static str, Option<LocalReview>), String> {
        self.effect()?;
        let n = self.questions.fetch_add(1, Ordering::Relaxed) + 1;
        let request_id = format!("q-{}-{n}", self.call_tag);
        let verb = match call.name.as_str() {
            "run_command_in_shell" => "run it",
            "close_pane" => "close it",
            "execute_task" => "execute it",
            "create_teammate" => "create it",
            _ => "do it",
        };
        let rx = self.confirms.ask(QuestionView {
            request_id: request_id.clone(),
            tool: call.name.clone(),
            summary: tandem_core::redact::redact(&summary),
            verb: verb.into(),
            review,
        });
        let answer = tokio::select! {
            a = rx => a.unwrap_or(Answer::Cancelled),
            _ = self.cancel.cancelled() => Answer::Cancelled,
            _ = tokio::time::sleep(self.timing.answer_wait) => {
                self.confirms.withdraw(&request_id);
                return Err("No answer came, so nothing was done.".into());
            }
        };
        match answer {
            Answer::YesOnScreen(r) => Ok(("Confirmed on screen", Some(r))),
            Answer::YesByVoice if review != Review::ScreenOnly => Ok(("Confirmed by voice", None)),
            Answer::YesByVoice => Err("Shell commands need a click on screen.".into()),
            Answer::No => Err("The user said no, so nothing was done.".into()),
            Answer::Cancelled => Err(ENDED.into()),
        }
    }
}

fn arg<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a[k].as_str().ok_or_else(|| format!("{k} is missing"))
}

fn closed_key(a: &Value) -> Result<SettingKey, String> {
    let raw = a["key"].as_str().unwrap_or_default();
    match SettingKey::parse(raw) {
        Some(k) if voice_tools::settable(k) => Ok(k),
        _ => Err("Conductor can't change that setting.".into()),
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "on".into(),
        Value::Bool(false) => "off".into(),
        other => other.to_string(),
    }
}

fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn humanized(tool: &str) -> String {
    capitalized(&tool.replace('_', " "))
}

/// The model's words as Conductor types or sends them: one line first, so
/// no line break `defang` has read can join a paste marker after it
/// (D-3032); then defanged, so no `@` names a file, with the note on the
/// same line (D-3023); then guarded, so it runs no slash or shell command
/// (D-3026).
pub fn words(text: &str) -> String {
    let (safe, wrote) = tandem_core::untrusted::defang_marked(&strip_controls(text));
    let line = if wrote {
        format!("{safe} {}", tandem_core::untrusted::DEFANGED)
    } else {
        safe
    };
    tandem_core::untrusted::no_command(&line)
}

/// Control characters out (newlines become spaces): typed text can't
/// press keys (F-EN-06).
pub fn strip_controls(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else {
                c
            }
        })
        .filter(|c| !c.is_control())
        .collect()
}

fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn capped(s: &str) -> String {
    if s.len() <= MAX_OUTPUT {
        return s.to_string();
    }
    let mut cut = s.len() - MAX_OUTPUT;
    while !s.is_char_boundary(cut) {
        cut += 1;
    }
    s[cut..].to_string()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use parking_lot::Mutex;
    use serde_json::json;
    use std::collections::HashMap;
    use tandem_core::origin::Origin;

    #[derive(Default)]
    pub(crate) struct FakeHost {
        pub panes: Mutex<HashMap<String, PaneFacts>>,
        pub typed: Mutex<Vec<(String, String, bool)>>,
        pub shell: Mutex<Vec<(String, String)>>,
        pub settings: Mutex<HashMap<SettingKey, Value>>,
        pub tools: Mutex<Vec<String>>,
        pub ui: Mutex<Vec<UiAction>>,
        pub tail: Mutex<String>,
    }

    impl FakeHost {
        pub fn with_panes() -> Arc<Self> {
            let h = Arc::new(Self::default());
            for (id, title, kind) in [
                ("agent", "Fix auth guard", PaneKind::Agent),
                ("shell", "zsh", PaneKind::Shell),
            ] {
                h.panes.lock().insert(
                    id.into(),
                    PaneFacts {
                        title: title.into(),
                        kind,
                        waiting: false,
                        working: false,
                    },
                );
            }
            h
        }

        pub fn set(&self, pane: &str, waiting: bool, working: bool) {
            if let Some(p) = self.panes.lock().get_mut(pane) {
                p.waiting = waiting;
                p.working = working;
            }
        }
    }

    #[async_trait]
    impl ToolHost for FakeHost {
        async fn pane(&self, pane: &str) -> Option<PaneFacts> {
            self.panes.lock().get(pane).cloned()
        }
        async fn type_prompt(&self, pane: &str, text: &str, send: bool) -> Result<(), String> {
            self.typed.lock().push((pane.into(), text.into(), send));
            Ok(())
        }
        async fn run_in_shell(&self, pane: &str, cmd: &str, _r: LocalReview) -> Result<(), String> {
            self.shell.lock().push((pane.into(), cmd.into()));
            Ok(())
        }
        async fn read_pane(&self, _pane: &str, _lines: usize) -> Result<String, String> {
            Ok(self.tail.lock().clone())
        }
        async fn get_setting(&self, key: SettingKey) -> Result<Value, String> {
            Ok(self
                .settings
                .lock()
                .get(&key)
                .cloned()
                .unwrap_or_else(|| settings::default_value(key)))
        }
        async fn set_setting(&self, key: SettingKey, value: Value) -> Result<(), String> {
            self.settings.lock().insert(key, value);
            Ok(())
        }
        async fn run_tool(&self, name: &str, _args: &Value) -> Result<String, String> {
            self.tools.lock().push(name.into());
            Ok(format!("{name} done"))
        }
        fn ui_action(&self, action: UiAction) {
            self.ui.lock().push(action);
        }
    }

    pub(crate) fn quick() -> ExecTiming {
        ExecTiming {
            poll: Duration::from_millis(10),
            busy_wait: Duration::from_millis(400),
            answer_wait: Duration::from_secs(5),
        }
    }

    fn exec(host: Arc<FakeHost>) -> (Arc<Executor>, CancellationToken) {
        let cancel = CancellationToken::new();
        let confirms = Arc::new(Confirms::new(Arc::new(|_| {})));
        (
            Arc::new(Executor::new(host, confirms, cancel.clone(), quick(), "c1")),
            cancel,
        )
    }

    fn call(name: &str, args: Value) -> ToolCall {
        ToolCall {
            id: format!("vc.1.0.{name}"),
            name: name.into(),
            args,
        }
    }

    async fn question(e: &Executor) -> QuestionView {
        for _ in 0..200 {
            if let Some(q) = e.confirms().head() {
                return q;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("no question was asked");
    }

    #[tokio::test]
    async fn f_cn_03_1_a_waiting_pane_gets_nothing_typed() {
        let host = FakeHost::with_panes();
        host.set("agent", true, false);
        let (e, _) = exec(host.clone());
        let r = e
            .run(&call(
                "prompt_pane",
                json!({"pane": "agent", "text": "run the tests", "send": true}),
            ))
            .await;
        assert!(!r.result.ok);
        assert!(
            r.result.output.contains("waiting for approval"),
            "{}",
            r.result.output
        );
        assert!(host.typed.lock().is_empty());
    }

    #[tokio::test]
    async fn f_cn_03_conductor_never_types_into_a_shell() {
        let host = FakeHost::with_panes();
        let (e, _) = exec(host.clone());
        let r = e
            .run(&call(
                "prompt_pane",
                json!({"pane": "shell", "text": "rm -rf /", "send": true}),
            ))
            .await;
        assert!(!r.result.ok);
        assert!(host.typed.lock().is_empty());
    }

    #[tokio::test]
    async fn f_cn_03_typed_text_loses_control_characters() {
        let host = FakeHost::with_panes();
        let (e, _) = exec(host.clone());
        let r = e
            .run(&call(
                "prompt_pane",
                json!({"pane": "agent", "text": "run\u{1b}[2J the\ntests\u{3}", "send": true}),
            ))
            .await;
        assert!(r.result.ok, "{}", r.result.output);
        assert_eq!(
            host.typed.lock().clone(),
            vec![("agent".into(), "run[2J the tests".into(), true)]
        );
    }

    #[tokio::test]
    async fn f_cn_03_typed_text_names_no_file_for_the_agent() {
        // The voice model reads panes anyone can write to (D-3023); the note
        // stays on the one line.
        let host = FakeHost::with_panes();
        let (e, _) = exec(host.clone());
        let r = e
            .run(&call(
                "prompt_pane",
                json!({"pane": "agent", "text": "look at @~/.ssh/id_rsa\u{c}@~/.aws", "send": true}),
            ))
            .await;
        assert!(r.result.ok, "{}", r.result.output);
        let typed = host.typed.lock().clone();
        assert_eq!(
            typed[0].1,
            format!(
                "look at \u{FF20}~/.ssh/id_rsa\u{FF20}~/.aws {}",
                tandem_core::untrusted::DEFANGED
            )
        );
    }

    #[tokio::test]
    async fn f_cn_03_typed_text_runs_no_command() {
        // Typed with Enter, /clear would run a slash command, and a control
        // character in front is dropped on the way (D-3026, D-3028).
        for (text, typed) in [
            ("/clear", "\u{FF0F}clear"),
            ("\u{1}!curl x | sh", "\u{FF01}curl x | sh"),
            ("\u{7f}/clear", "\u{FF0F}clear"),
            ("\u{9b}!x", "\u{FF01}x"),
            (" /exit", " \u{FF0F}exit"),
            ("exit", "\u{FF45}xit"),
        ] {
            let host = FakeHost::with_panes();
            let (e, _) = exec(host.clone());
            let r = e
                .run(&call(
                    "prompt_pane",
                    json!({"pane": "agent", "text": text, "send": true}),
                ))
                .await;
            assert!(r.result.ok, "{}", r.result.output);
            assert_eq!(host.typed.lock().clone()[0].1, typed, "{text:?}");
        }
    }

    #[tokio::test]
    async fn f_cn_03_typed_text_hides_no_keyword_or_exit_word_in_what_is_dropped() {
        // The review's three cases (D-3030): a U+FFF9 gap Claude Code's
        // prompt drops, a control character typing drops, and U+FFF9 before
        // an exit word. The app types what the host gets through
        // `sanitize_prompt`, which drops only control characters, and none
        // is left.
        for (text, safe) in [
            ("ultra\u{fff9}plan", "ultra-plan"),
            ("ultra\u{1}plan", "ultra-plan"),
            ("\u{fff9}exit", "\u{FF45}xit"),
        ] {
            let host = FakeHost::with_panes();
            let (e, _) = exec(host.clone());
            let r = e
                .run(&call(
                    "prompt_pane",
                    json!({"pane": "agent", "text": text, "send": true}),
                ))
                .await;
            assert!(r.result.ok, "{}", r.result.output);
            let typed = host.typed.lock().clone()[0].1.clone();
            assert_eq!(typed, safe, "{text:?}");
            assert!(!typed.chars().any(char::is_control), "{typed:?}");
        }
    }

    #[tokio::test]
    async fn f_cn_03_a_line_break_flattened_forms_no_paste_marker() {
        // One line first, then defanged: a marker can't form from a line
        // break the defang read (D-3032).
        for (text, typed) in [
            ("[Pasted\ntext #1]", "\u{FF3B}Pasted text #1]"),
            ("[Image\t#2]", "\u{FF3B}Image #2]"),
            // A line break becomes a space, which joins nothing.
            ("ultra\nplan", "ultra plan"),
        ] {
            let host = FakeHost::with_panes();
            let (e, _) = exec(host.clone());
            let r = e
                .run(&call(
                    "prompt_pane",
                    json!({"pane": "agent", "text": text, "send": true}),
                ))
                .await;
            assert!(r.result.ok, "{}", r.result.output);
            assert_eq!(host.typed.lock().clone()[0].1, typed, "{text:?}");
        }
    }

    #[tokio::test]
    async fn f_cn_01_2_hang_up_during_a_pending_prompt_types_nothing() {
        let host = FakeHost::with_panes();
        host.set("agent", false, true); // busy: the prompt waits
        let (e, cancel) = exec(host.clone());
        let e2 = e.clone();
        let pending = tokio::spawn(async move {
            e2.run(&call(
                "prompt_pane",
                json!({"pane": "agent", "text": "next", "send": true}),
            ))
            .await
        });
        tokio::time::sleep(Duration::from_millis(60)).await;
        cancel.cancel();
        // The agent comes back to its prompt just as the call ends.
        host.set("agent", false, false);
        let r = pending.await.expect("join");
        assert!(!r.result.ok);
        assert!(r.result.output.contains("call ended"));
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(host.typed.lock().is_empty());
        // And nothing at all runs after the end.
        let r = e.run(&call("switch_mode", json!({"mode": "code"}))).await;
        assert!(!r.result.ok);
        assert!(host.ui.lock().is_empty());
    }

    #[tokio::test]
    async fn f_cn_01_a_busy_agent_gets_the_prompt_when_it_is_back() {
        let host = FakeHost::with_panes();
        host.set("agent", false, true);
        let (e, _) = exec(host.clone());
        let h = host.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            h.set("agent", false, false);
        });
        let r = e
            .run(&call(
                "prompt_pane",
                json!({"pane": "agent", "text": "next", "send": false}),
            ))
            .await;
        assert!(r.result.ok, "{}", r.result.output);
        assert_eq!(host.typed.lock().len(), 1);
        // One that never comes back gives up without typing.
        host.set("agent", false, true);
        let r = e
            .run(&call(
                "prompt_pane",
                json!({"pane": "agent", "text": "again", "send": false}),
            ))
            .await;
        assert!(!r.result.ok);
        assert_eq!(host.typed.lock().len(), 1);
    }

    #[tokio::test]
    async fn f_cn_02_2_set_setting_off_the_closed_list_is_rejected() {
        let host = FakeHost::with_panes();
        let (e, _) = exec(host.clone());
        for (key, value) in [
            ("telemetry_usage", json!(true)),
            ("launch_at_login", json!(true)),
            ("update_channel", json!("beta")),
        ] {
            let r = e
                .run(&call("set_setting", json!({"key": key, "value": value})))
                .await;
            assert!(!r.result.ok, "{key}");
        }
        assert!(host.settings.lock().is_empty());
        // On the list, with a bad value: refused too.
        let r = e
            .run(&call(
                "set_setting",
                json!({"key": "theme", "value": "pink"}),
            ))
            .await;
        assert!(!r.result.ok);
        let r = e
            .run(&call(
                "set_setting",
                json!({"key": "theme", "value": "light"}),
            ))
            .await;
        assert!(r.result.ok, "{}", r.result.output);
        assert_eq!(
            host.settings.lock().get(&SettingKey::Theme),
            Some(&json!("light"))
        );
        let r = e.run(&call("get_setting", json!({"key": "theme"}))).await;
        assert_eq!(r.result.output, "theme is light");
    }

    #[tokio::test]
    async fn f_cn_02_3_a_shell_command_never_runs_without_the_click() {
        let host = FakeHost::with_panes();
        let (e, _) = exec(host.clone());
        let e2 = e.clone();
        let run = tokio::spawn(async move {
            e2.run(&call(
                "run_command_in_shell",
                json!({"pane": "shell", "cmd": "pnpm test"}),
            ))
            .await
        });
        let q = question(&e).await;
        assert_eq!(q.review, Review::ScreenOnly);
        assert_eq!(q.summary, "Run `pnpm test` in zsh?");
        // "Yes" by voice: refused, still waiting, nothing ran.
        assert!(e.confirms().answer_by_voice(&q.request_id, true).is_err());
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(host.shell.lock().is_empty());
        assert!(!run.is_finished());
        // The click from the main window.
        let review = LocalReview::check(&Origin::main_window()).expect("main");
        e.confirms()
            .answer_on_screen(&q.request_id, Some(review))
            .expect("click");
        let r = run.await.expect("join");
        assert!(r.result.ok, "{}", r.result.output);
        assert_eq!(r.confirmed, Some("Confirmed on screen"));
        assert_eq!(
            host.shell.lock().clone(),
            vec![("shell".into(), "pnpm test".into())]
        );
    }

    #[tokio::test]
    async fn f_cn_02_3_a_shell_command_said_no_runs_nothing() {
        let host = FakeHost::with_panes();
        let (e, _) = exec(host.clone());
        let e2 = e.clone();
        let run = tokio::spawn(async move {
            e2.run(&call(
                "run_command_in_shell",
                json!({"pane": "shell", "cmd": "ls"}),
            ))
            .await
        });
        let q = question(&e).await;
        e.confirms()
            .answer_on_screen(&q.request_id, None)
            .expect("no");
        let r = run.await.expect("join");
        assert!(!r.result.ok);
        assert!(host.shell.lock().is_empty());
    }

    #[tokio::test]
    async fn f_cn_03_2_voice_allow_for_one_question_never_answers_another() {
        let host = FakeHost::with_panes();
        let (e, _) = exec(host.clone());
        let (e1, e2) = (e.clone(), e.clone());
        let a = tokio::spawn(
            async move { e1.run(&call("close_pane", json!({"pane": "agent"}))).await },
        );
        let qa = question(&e).await;
        let b = tokio::spawn(async move {
            e2.run(&call("execute_task", json!({"task": "TAN-12"})))
                .await
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        e.confirms()
            .answer_by_voice(&qa.request_id, true)
            .expect("A");
        let ra = a.await.expect("join");
        assert!(ra.result.ok);
        assert_eq!(ra.confirmed, Some("Confirmed by voice"));
        // B is still asking.
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!b.is_finished());
        let qb = e.confirms().head().expect("B");
        assert_ne!(qb.request_id, qa.request_id);
        assert_eq!(host.tools.lock().clone(), vec!["close_pane".to_string()]);
        e.confirms()
            .answer_by_voice(&qb.request_id, false)
            .expect("B no");
        assert!(!b.await.expect("join").result.ok);
        assert_eq!(host.tools.lock().len(), 1);
    }

    #[tokio::test]
    async fn f_cn_02_4_read_pane_is_redacted() {
        let host = FakeHost::with_panes();
        *host.tail.lock() =
            "export OPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstuvwxyz0123456789ABCD\nok".into();
        let (e, _) = exec(host.clone());
        let r = e
            .run(&call("read_pane", json!({"pane": "agent", "lines": 20})))
            .await;
        assert!(r.result.ok);
        assert!(
            !r.result.output.contains("sk-proj-abcdef"),
            "{}",
            r.result.output
        );
        assert!(r.result.output.contains("ok"));
    }

    #[tokio::test]
    async fn f_cn_02_unknown_tools_and_bad_arguments_never_reach_the_app() {
        let host = FakeHost::with_panes();
        let (e, _) = exec(host.clone());
        for c in [
            call("remove_lane", json!({"force": true})),
            call("switch_mode", json!({"mode": "root"})),
            call("prompt_pane", json!({"pane": "agent"})),
        ] {
            assert!(!e.run(&c).await.result.ok, "{}", c.name);
        }
        assert!(
            host.typed.lock().is_empty()
                && host.ui.lock().is_empty()
                && host.tools.lock().is_empty()
        );
        // Navigation goes to the UI.
        assert!(
            e.run(&call("switch_mode", json!({"mode": "code"})))
                .await
                .result
                .ok
        );
        assert_eq!(host.ui.lock()[0].action, "switch_mode");
    }

    #[tokio::test]
    async fn f_tk_08_1_a_tools_output_hides_a_credential_behind_what_is_removable() {
        // What a tool found goes to Conductor's cloud model, which reads
        // past what's removable: redacted as written and as it reads it
        // (D-3036). read_thread, find_work and the rest end the same way.
        for secret in [
            "ghp_0123456789\u{200b}abcdefghijABCDEFGHIJ012345",
            "abc\u{200b}ghp_0123456789abcdefghijABCDEFGHIJ012345",
        ] {
            let host = FakeHost::with_panes();
            *host.tail.lock() = format!("pushed with {secret}\nok");
            let (e, _) = exec(host.clone());
            let r = e
                .run(&call("read_pane", json!({"pane": "agent", "lines": 20})))
                .await;
            assert!(r.result.ok);
            let out = &r.result.output;
            assert!(out.contains("pushed with") && out.contains("ok"), "{out}");
            assert!(
                !out.contains("0123456789") && !out.contains("ABCDEFGHIJ012345"),
                "{out}"
            );
        }
    }

    #[tokio::test]
    async fn f_cn_04_labels_name_the_pane_and_stay_short() {
        let host = FakeHost::with_panes();
        let (e, _) = exec(host);
        let l = e
            .label(&call(
                "prompt_pane",
                json!({"pane": "agent", "text": "run the unit tests and report failures", "send": true}),
            ))
            .await;
        assert!(l.starts_with("Prompting Fix auth"), "{l}");
        assert!(l.chars().count() <= 40);
        assert_eq!(
            e.label(&call("switch_mode", json!({"mode": "code"}))).await,
            "Code mode"
        );
    }
}
