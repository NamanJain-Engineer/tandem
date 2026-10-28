//! F-WT-05: a second opinion on a worktree's diff from another engine. A
//! Read only thread (F-TH-09) gets a fixed review prompt; when its turn
//! ends, the reply's `path:line: comment` lines become review-bar comments
//! (F-WT-02) and everything else one General comment (D-2420).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::access::{Access, ThreadMode};
use tandem_core::bus::{BusEvent, EventBus, Topic};
use tandem_core::ids::{AccountId, ThreadId, WorkspaceId};
use tandem_core::{untrusted, ErrorCode, IpcError};
use tandem_engines::events::ThreadEvent;
use tandem_git::FileDiff;
use tandem_store::repo::threads::{ThreadKind, ThreadState};
use tandem_store::Store;

/// The diff the reviewer reads is cut here, with a note.
pub const DIFF_MAX: usize = 150_000;
const POLL: Duration = Duration::from_millis(500);
pub const EVENT: &str = "review:second_opinion";

/// One finding placed on a line the diff shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OpinionComment {
    pub path: String,
    pub line: u32,
    /// The quoted line.
    pub text: String,
    pub comment: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OpinionOutcome {
    Done,
    Failed,
}

/// `review:second_opinion`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SecondOpinion {
    pub lane_id: WorkspaceId,
    pub thread_id: ThreadId,
    pub engine_id: String,
    pub outcome: OpinionOutcome,
    pub comments: Vec<OpinionComment>,
    pub general: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SecondOpinionStarted {
    pub thread_id: ThreadId,
}

/// Worktrees with a reviewer running: one at a time each.
#[derive(Default, Clone)]
pub struct Running(Arc<Mutex<HashSet<String>>>);

impl Running {
    fn claim(&self, lane: &WorkspaceId) -> bool {
        self.0
            .lock()
            .map(|mut s| s.insert(lane.as_str().to_string()))
            .unwrap_or(false)
    }

    fn release(&self, lane: &WorkspaceId) {
        if let Ok(mut s) = self.0.lock() {
            s.remove(lane.as_str());
        }
    }
}

/// The diff as one text, cut at [`DIFF_MAX`] on a character boundary.
pub fn diff_text(files: &[FileDiff]) -> (String, bool) {
    let all = files
        .iter()
        .map(|f| f.patch.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if all.len() <= DIFF_MAX {
        return (all, false);
    }
    let mut end = DIFF_MAX;
    while !all.is_char_boundary(end) {
        end -= 1;
    }
    (all[..end].to_string(), true)
}

/// The reviewer's first message. The words are the engine's instructions,
/// so they stay in English like F-WT-02's prompt. No `@` in the diff names
/// a file for the reviewer to attach (D-3022).
pub fn prompt(branch: &str, task: Option<&str>, diff: &str, cut: bool) -> String {
    let task = task
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| format!("The task they were given:\n{t}\n\n"))
        .unwrap_or_default();
    let note = if cut {
        "\n(The diff was cut at 150,000 characters.)"
    } else {
        ""
    };
    format!(
        "You are giving a second opinion on another coding agent's changes on the branch {branch}. Review them. Don't change any files.\n\n{task}{label}\n{diff}{note}\n\nReply in this format:\n## Findings\nOne line per finding: path:line: comment (the path as the diff shows it, the line number on the new side).\n## Overall\nA few sentences on the change as a whole.",
        label = untrusted::label("the worktree's diff"),
        diff = untrusted::fence_defanged("diff", diff),
    )
}

/// The new-side lines each file's patch shows (added and context), by number.
fn shown_lines(files: &[FileDiff]) -> HashMap<&str, HashMap<u32, String>> {
    let mut out = HashMap::new();
    for f in files {
        let mut lines = HashMap::new();
        let mut next: Option<u32> = None;
        for l in f.patch.lines() {
            if let Some(rest) = l.strip_prefix("@@ ") {
                // "@@ -a,b +c,d @@": the new side starts at c.
                next = rest
                    .split_whitespace()
                    .find_map(|p| p.strip_prefix('+'))
                    .and_then(|p| p.split(',').next())
                    .and_then(|n| n.parse().ok());
                continue;
            }
            let Some(n) = next else { continue };
            match l.chars().next() {
                Some('+') | Some(' ') => {
                    lines.insert(n, l[1..].to_string());
                    next = Some(n + 1);
                }
                Some('-') | Some('\\') => {}
                _ => next = None,
            }
        }
        out.insert(f.path.as_str(), lines);
    }
    out
}

/// `path:line: comment`, list markers and backticks around the place
/// ignored.
fn split_finding(line: &str) -> Option<(&str, u32, &str)> {
    let l = line.trim();
    let l = ["- ", "* ", "+ "]
        .iter()
        .find_map(|m| l.strip_prefix(m))
        .unwrap_or(l)
        .trim_start();
    let (path, rest) = l.split_once(':')?;
    let path = path.trim_matches('`');
    if path.is_empty() || path.contains(char::is_whitespace) {
        return None;
    }
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let n = rest[..digits].parse().ok()?;
    let comment = rest[digits..]
        .trim_start_matches('`')
        .strip_prefix(':')?
        .trim();
    (!comment.is_empty()).then_some((path, n, comment))
}

/// A reply's findings on lines the diff shows, and one General text with
/// everything else that isn't a heading (nothing is dropped).
pub fn parse(reply: &str, files: &[FileDiff]) -> (Vec<OpinionComment>, Option<String>) {
    let shown = shown_lines(files);
    let resolve = |p: &str| -> Option<&str> {
        // The diff it read had ＠ for @ (D-3022).
        let p = p.replace('\u{FF20}', "@");
        let p = p.as_str();
        if shown.contains_key(p) {
            return shown.get_key_value(p).map(|(k, _)| *k);
        }
        let suffix = format!("/{}", p.trim_start_matches("./"));
        let mut hits = shown.keys().filter(|k| k.ends_with(&suffix));
        match (hits.next(), hits.next()) {
            (Some(k), None) => Some(*k),
            _ => None,
        }
    };
    let mut comments = Vec::new();
    let mut general = Vec::new();
    for line in reply.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let placed = split_finding(t).and_then(|(p, n, c)| {
            let path = resolve(p)?;
            let text = shown.get(path)?.get(&n)?;
            Some(OpinionComment {
                path: path.to_string(),
                line: n,
                text: text.clone(),
                comment: c.to_string(),
            })
        });
        match placed {
            Some(c) => comments.push(c),
            None => general.push(t.to_string()),
        }
    }
    let general = (!general.is_empty()).then(|| general.join("\n"));
    (comments, general)
}

/// What starting needs from the app, so the flow is tested with fakes.
#[async_trait::async_trait]
pub trait OpinionHost: Send + Sync {
    async fn lane(&self, lane: &WorkspaceId) -> Result<(String, Vec<FileDiff>), IpcError>;
    /// Whether the engine has a chat adapter (can run read-only).
    fn chat_capable(&self, engine: &str) -> Option<bool>;
    async fn create_thread(
        &self,
        lane: &WorkspaceId,
        engine: &str,
        account: Option<AccountId>,
        title: String,
    ) -> Result<ThreadId, IpcError>;
    async fn send(&self, thread: &ThreadId, text: String) -> Result<(), IpcError>;
    async fn state(&self, thread: &ThreadId) -> Option<ThreadState>;
    async fn last_reply(&self, thread: &ThreadId) -> Option<String>;
}

pub struct StartArgs {
    pub lane: WorkspaceId,
    pub engine: String,
    pub account: Option<AccountId>,
    pub task: Option<String>,
}

/// Starts the reviewer, then waits for its turn in the background and
/// publishes `review:second_opinion`.
pub async fn start(
    host: Arc<dyn OpinionHost>,
    bus: EventBus,
    running: Running,
    a: StartArgs,
) -> Result<SecondOpinionStarted, IpcError> {
    match host.chat_capable(&a.engine) {
        None => {
            return Err(IpcError::new(
                ErrorCode::EngineMissing,
                "This engine isn't known to Vibezeo",
            ))
        }
        Some(false) => {
            return Err(IpcError::new(
                ErrorCode::EngineUnsupported,
                "Can't run read-only",
            ))
        }
        Some(true) => {}
    }
    let (branch, files) = host.lane(&a.lane).await?;
    if files.is_empty() {
        return Err(IpcError::new(ErrorCode::Invalid, "No changes to review"));
    }
    if !running.claim(&a.lane) {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "A second opinion is already running for this worktree",
        ));
    }
    let started = async {
        let thread = host
            .create_thread(
                &a.lane,
                &a.engine,
                a.account.clone(),
                format!("Second opinion: {branch}"),
            )
            .await?;
        let (diff, cut) = diff_text(&files);
        host.send(&thread, prompt(&branch, a.task.as_deref(), &diff, cut))
            .await?;
        Ok::<_, IpcError>(thread)
    }
    .await;
    let thread = match started {
        Ok(t) => t,
        Err(e) => {
            running.release(&a.lane);
            return Err(e);
        }
    };
    tracing::info!(code = "review.second_opinion_started", engine = %a.engine, "second opinion started");
    let t = thread.clone();
    tokio::spawn(async move {
        let done = loop {
            tokio::time::sleep(POLL).await;
            match host.state(&t).await {
                Some(ThreadState::Working) | Some(ThreadState::Waiting) => {}
                Some(ThreadState::Done) => break true,
                _ => break false,
            }
        };
        let (comments, general) = if done {
            parse(&host.last_reply(&t).await.unwrap_or_default(), &files)
        } else {
            (Vec::new(), None)
        };
        tracing::info!(
            code = "review.second_opinion_done",
            ok = done,
            comments = comments.len(),
            "second opinion ended"
        );
        running.release(&a.lane);
        bus.publish(BusEvent::new(
            Topic::Threads,
            EVENT,
            SecondOpinion {
                lane_id: a.lane,
                thread_id: t,
                engine_id: a.engine,
                outcome: if done {
                    OpinionOutcome::Done
                } else {
                    OpinionOutcome::Failed
                },
                comments,
                general,
            },
        ));
    });
    Ok(SecondOpinionStarted { thread_id: thread })
}

/// The app's host: worktrees, engines and threads.
pub struct AppHost {
    pub store: Store,
    pub threads: Arc<crate::services::threads::ThreadService>,
    pub engines: Arc<crate::services::engines::EngineService>,
}

#[async_trait::async_trait]
impl OpinionHost for AppHost {
    async fn lane(&self, lane: &WorkspaceId) -> Result<(String, Vec<FileDiff>), IpcError> {
        let ws = tandem_store::repo::workspaces::get(&self.store, lane)
            .await
            .map_err(|e| IpcError::new(ErrorCode::NotFound, e.to_string()))?;
        crate::services::lanes::single_repo(&self.store, &ws).await?;
        let files = crate::services::lanes::diff(&self.store, lane).await?;
        Ok((ws.lane_branch.unwrap_or(ws.name), files))
    }

    fn chat_capable(&self, engine: &str) -> Option<bool> {
        self.engines
            .get(engine)
            .map(|s| crate::services::threads::has_chat_adapter(&s))
    }

    async fn create_thread(
        &self,
        lane: &WorkspaceId,
        engine: &str,
        account: Option<AccountId>,
        title: String,
    ) -> Result<ThreadId, IpcError> {
        let ws = tandem_store::repo::workspaces::get(&self.store, lane)
            .await
            .map_err(|e| IpcError::new(ErrorCode::NotFound, e.to_string()))?;
        let thread = self
            .threads
            .create(tandem_store::repo::threads::NewThread {
                id: ThreadId::new(),
                kind: ThreadKind::Thread,
                workspace_id: Some(lane.clone()),
                cwd: ws.root,
                engine_id: engine.to_string(),
                account_id: account,
                model: None,
                effort: None,
                // F-TH-09: the reviewer reads; any write is denied.
                access: Access::Read,
                mode: ThreadMode::Act,
                attended: true,
            })
            .await?;
        let _ = self
            .threads
            .update(
                &thread.id,
                tandem_store::repo::threads::ThreadPatch {
                    title: Some(title),
                    ..Default::default()
                },
            )
            .await;
        Ok(thread.id)
    }

    async fn send(&self, thread: &ThreadId, text: String) -> Result<(), IpcError> {
        self.threads
            .send(thread, text, Vec::new(), false)
            .await
            .map(|_| ())
    }

    async fn state(&self, thread: &ThreadId) -> Option<ThreadState> {
        tandem_store::repo::threads::get(&self.store, thread)
            .await
            .ok()
            .flatten()
            .map(|t| t.state)
    }

    async fn last_reply(&self, thread: &ThreadId) -> Option<String> {
        last_reply(&self.store, thread).await
    }
}

/// A thread's last assistant message (also the review queue's closing
/// message, F-DB-04).
pub async fn last_reply(store: &Store, thread: &ThreadId) -> Option<String> {
    let rows = tandem_store::repo::threads::all_messages(store, thread)
        .await
        .ok()?;
    rows.iter().rev().find_map(|r| {
        match serde_json::from_str::<ThreadEvent>(&r.payload_json).ok()? {
            ThreadEvent::Assistant { markdown } => Some(markdown),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, patch: &str) -> FileDiff {
        FileDiff {
            path: path.into(),
            old_path: None,
            change: tandem_git::FileChange::Modified,
            additions: 0,
            deletions: 0,
            binary: false,
            patch: patch.into(),
            truncated: false,
        }
    }

    fn files() -> Vec<FileDiff> {
        vec![
            file(
                "src/a.ts",
                "diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -40,3 +40,4 @@ fn\n ctx 40\n-old 41\n+if (token == null) return;\n+const t = token;\n ctx 43\n",
            ),
            file(
                "src/b.ts",
                "--- a/src/b.ts\n+++ b/src/b.ts\n@@ -1,2 +1,3 @@\n one\n two\n+thre\n",
            ),
        ]
    }

    #[test]
    fn f_wt_05_1_the_prompt_has_the_branch_the_fenced_diff_and_the_headings() {
        let p = prompt(
            "tandem/fix-auth",
            Some("Handle expired tokens"),
            "+x",
            false,
        );
        assert!(p.contains("tandem/fix-auth"));
        assert!(p.contains("Handle expired tokens"));
        assert!(p.contains("```diff\n+x"), "{p}");
        assert!(p.contains("## Findings") && p.contains("## Overall"));
        assert!(p.contains("Don't change any files"));
        assert!(!p.contains("cut at"));
        assert!(prompt("b", None, "+x", true).contains("cut at 150,000"));
        assert!(!prompt("b", Some("  "), "+x", false).contains("task they were given"));
    }

    #[test]
    fn f_wt_05_1_no_at_in_the_diff_names_a_file_for_the_reviewer() {
        // An agent's diff can name any file; Claude Code would attach it.
        let diff =
            "@@ -1 +1,2 @@\n+// see @~/.ssh/id_rsa and @/etc/passwd\n+import x from \"@/lib/x\";\n";
        let p = prompt("b", None, diff, false);
        assert!(
            !p.contains("@~/") && !p.contains("@/etc") && !p.contains("\"@/lib"),
            "{p}"
        );
        assert!(p.contains("\u{FF20}~/.ssh/id_rsa") && p.contains("\"\u{FF20}/lib/x\""));
        // Hunk headers keep theirs, and the reviewer is told to read ＠ as @.
        assert!(p.contains("@@ -1 +1,2 @@"));
        assert!(p.contains(untrusted::DEFANGED));
        assert!(!prompt("b", None, "+x", false).contains(untrusted::DEFANGED));
    }

    #[test]
    fn f_wt_05_2_a_finding_on_a_path_with_an_at_still_lands_on_its_line() {
        // The reviewer read ＠ for @ (D-3022) and may write the path that way.
        let routes = vec![file(
            "app/@modal/page.tsx",
            "--- a/app/@modal/page.tsx\n+++ b/app/@modal/page.tsx\n@@ -1 +1,2 @@\n one\n+two\n",
        )];
        let reply = "## Findings\napp/\u{FF20}modal/page.tsx:2: say why\n";
        let (comments, general) = parse(reply, &routes);
        assert_eq!(comments.len(), 1, "{general:?}");
        assert_eq!(
            (comments[0].path.as_str(), comments[0].line),
            ("app/@modal/page.tsx", 2)
        );
    }

    #[test]
    fn f_wt_05_1_a_long_diff_is_cut_on_a_character_boundary() {
        let big = file("a", &"é".repeat(DIFF_MAX));
        let (text, cut) = diff_text(&[big]);
        assert!(cut);
        assert!(text.len() <= DIFF_MAX);
        let (text, cut) = diff_text(&files());
        assert!(!cut && text.contains("src/b.ts"));
    }

    #[test]
    fn f_wt_05_2_findings_on_shown_lines_become_line_comments() {
        let reply =
            "## Findings\n- `src/a.ts:42`: handle expired tokens\nb.ts:3: typo\n## Overall\nFine.";
        let (comments, general) = parse(reply, &files());
        assert_eq!(
            comments,
            vec![
                OpinionComment {
                    path: "src/a.ts".into(),
                    line: 42,
                    text: "const t = token;".into(),
                    comment: "handle expired tokens".into(),
                },
                OpinionComment {
                    path: "src/b.ts".into(),
                    line: 3,
                    text: "thre".into(),
                    comment: "typo".into(),
                },
            ]
        );
        assert_eq!(general.as_deref(), Some("Fine."));
    }

    #[test]
    fn f_wt_05_3_everything_else_is_kept_in_general() {
        let reply = "Looks careful.\nsrc/a.ts:9999: not in the diff\nnope.ts:1: no such file\na.ts:40-44: a range\n\n### Overall\nsrc/a.ts:39: above the hunk";
        let (comments, general) = parse(reply, &files());
        assert!(comments.is_empty());
        assert_eq!(
            general.as_deref(),
            Some("Looks careful.\nsrc/a.ts:9999: not in the diff\nnope.ts:1: no such file\na.ts:40-44: a range\nsrc/a.ts:39: above the hunk")
        );
        assert_eq!(parse("", &files()), (Vec::new(), None));
        // An ambiguous suffix isn't guessed.
        let two = vec![
            file("x/c.ts", "@@ -0,0 +1 @@\n+a\n"),
            file("y/c.ts", "@@ -0,0 +1 @@\n+b\n"),
        ];
        assert!(parse("c.ts:1: which?", &two).0.is_empty());
        assert_eq!(parse("x/c.ts:1: this", &two).0.len(), 1);
    }

    struct Fake {
        files: Vec<FileDiff>,
        created: Mutex<Vec<(String, Option<AccountId>, String)>>,
        sent: Mutex<Vec<String>>,
        state: Mutex<ThreadState>,
        reply: String,
    }

    #[async_trait::async_trait]
    impl OpinionHost for Fake {
        async fn lane(&self, _: &WorkspaceId) -> Result<(String, Vec<FileDiff>), IpcError> {
            Ok(("tandem/fix-auth".into(), self.files.clone()))
        }
        fn chat_capable(&self, engine: &str) -> Option<bool> {
            match engine {
                "codex" => Some(true),
                "aider" => Some(false),
                _ => None,
            }
        }
        async fn create_thread(
            &self,
            _: &WorkspaceId,
            engine: &str,
            account: Option<AccountId>,
            title: String,
        ) -> Result<ThreadId, IpcError> {
            self.created
                .lock()
                .expect("lock")
                .push((engine.into(), account, title));
            Ok(ThreadId::new())
        }
        async fn send(&self, _: &ThreadId, text: String) -> Result<(), IpcError> {
            self.sent.lock().expect("lock").push(text);
            Ok(())
        }
        async fn state(&self, _: &ThreadId) -> Option<ThreadState> {
            Some(*self.state.lock().expect("lock"))
        }
        async fn last_reply(&self, _: &ThreadId) -> Option<String> {
            Some(self.reply.clone())
        }
    }

    fn fake(files: Vec<FileDiff>) -> Arc<Fake> {
        Arc::new(Fake {
            files,
            created: Mutex::new(Vec::new()),
            sent: Mutex::new(Vec::new()),
            state: Mutex::new(ThreadState::Working),
            reply: "src/a.ts:42: handle expired tokens\nb.ts:3: typo\nOverall fine.".into(),
        })
    }

    fn args(engine: &str) -> StartArgs {
        StartArgs {
            lane: WorkspaceId::from_db("lane-1".into()),
            engine: engine.into(),
            account: None,
            task: None,
        }
    }

    #[tokio::test]
    async fn f_wt_05_1_starts_a_review_and_publishes_the_parsed_reply() {
        let host = fake(files());
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::Threads);
        let running = Running::default();
        start(host.clone(), bus.clone(), running.clone(), args("codex"))
            .await
            .expect("start");
        let created = host.created.lock().expect("lock").clone();
        assert_eq!(created[0].0, "codex");
        assert_eq!(created[0].2, "Second opinion: tandem/fix-auth");
        assert!(host.sent.lock().expect("lock")[0].contains("+if (token == null) return;"));
        // One at a time per worktree.
        let again = start(host.clone(), bus.clone(), running.clone(), args("codex")).await;
        assert_eq!(again.expect_err("busy").code, ErrorCode::Invalid);
        *host.state.lock().expect("lock") = ThreadState::Done;
        let ev = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("event")
            .expect("recv");
        assert_eq!(ev.name, EVENT);
        let op: SecondOpinion = serde_json::from_value(ev.payload).expect("payload");
        assert_eq!(op.outcome, OpinionOutcome::Done);
        assert_eq!(op.comments.len(), 2);
        assert_eq!(op.general.as_deref(), Some("Overall fine."));
        // Released: another can start.
        assert!(running.claim(&WorkspaceId::from_db("lane-1".into())));
    }

    #[tokio::test]
    async fn f_wt_05_a_failed_turn_adds_nothing() {
        let host = fake(files());
        *host.state.lock().expect("lock") = ThreadState::Failed;
        let bus = EventBus::new();
        let mut rx = bus.subscribe(Topic::Threads);
        start(host, bus, Running::default(), args("codex"))
            .await
            .expect("start");
        let ev = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("event")
            .expect("recv");
        let op: SecondOpinion = serde_json::from_value(ev.payload).expect("payload");
        assert_eq!(op.outcome, OpinionOutcome::Failed);
        assert!(op.comments.is_empty() && op.general.is_none());
    }

    #[tokio::test]
    async fn f_wt_05_4_6_terminal_engines_and_empty_diffs_are_refused() {
        let bus = EventBus::new();
        let host = fake(files());
        let err = start(host.clone(), bus.clone(), Running::default(), args("aider"))
            .await
            .expect_err("terminal");
        assert_eq!(
            (err.code, err.message.as_str()),
            (ErrorCode::EngineUnsupported, "Can't run read-only")
        );
        assert_eq!(
            start(host.clone(), bus.clone(), Running::default(), args("nope"))
                .await
                .expect_err("unknown")
                .code,
            ErrorCode::EngineMissing
        );
        let empty = fake(Vec::new());
        let err = start(empty.clone(), bus, Running::default(), args("codex"))
            .await
            .expect_err("empty");
        assert_eq!(err.message, "No changes to review");
        assert!(empty.created.lock().expect("lock").is_empty());
    }
}
