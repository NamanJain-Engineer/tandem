//! Find work by meaning (F-CN-05, ARCHITECTURE §12.6): open panes, threads
//! and teammates as candidates, ranked on this machine by
//! `tandem_core::find_work`. A pane is known by its name, title, engine,
//! folder or page, and its last 20 lines of output; a thread by its title
//! and last message; a teammate by its name, its brief's first line and its
//! latest chat's last line. Output and messages go through the secret
//! redactor before they're read. Nothing leaves the machine unless the user
//! turned on the hosted ranking (Settings › Privacy, D-1302): then the same
//! redacted text, never file contents, goes to `/v1/voice/resolve`, and the
//! local ranking answers whenever that fails.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::find_work::{self, Candidate, Decision};
use tandem_core::redact::redact;
use tandem_core::source::SourceRef;
use tandem_store::repo::panes::PaneRecord;
use tandem_store::repo::threads::ThreadRecord;

/// The lines of a pane's output a search reads.
pub const OUTPUT_LINES: usize = 20;
/// The most of a thread's last message a search reads.
const MESSAGE_CHARS: usize = 1_000;
/// The most of what it was last doing a match shows.
const DETAIL_CHARS: usize = 120;
/// The most matches a search returns.
const MAX_MATCHES: usize = 20;
/// The longest search (ARCHITECTURE §3).
pub const MAX_QUERY_CHARS: usize = 500;
/// The most candidates the hosted ranking takes, and the most text of each
/// (ARCHITECTURE §14 `/v1/voice/resolve`).
pub const HOSTED_CANDIDATES: usize = 200;
pub const HOSTED_TEXT_CHARS: usize = 2_000;
/// The hosted ranking scores 0..1; times this, its scores read like the
/// local ranking's (a name that matches every word scores 3), so one clear
/// match is decided the same way.
const HOSTED_SCALE: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FindKind {
    Pane,
    Thread,
    Teammate,
}

/// Which ranking ordered a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FindRanking {
    /// `tandem_core::find_work`, on this machine.
    Local,
    /// Vibezeo's hosted ranking (`/v1/voice/resolve`), with the opt-in on.
    Hosted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FindMatch {
    pub source: SourceRef,
    pub kind: FindKind,
    pub label: String,
    /// What it was last doing: its latest activity line or message, redacted.
    pub detail: Option<String>,
    /// A pane's workspace and tab, so the window can show it.
    pub workspace_id: Option<String>,
    pub tab_id: Option<String>,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FindWorkResult {
    /// Best first.
    pub matches: Vec<FindMatch>,
    /// The first match is one clear match to go to. Otherwise the close
    /// matches lead the list for the user to choose from.
    pub clear: bool,
    pub ranking: FindRanking,
}

/// A teammate as Find work sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeammateFacts {
    pub id: String,
    pub name: String,
    pub brief: String,
    /// The last message of its most recent chat, not yet redacted.
    pub last_message: Option<String>,
}

/// The hosted ranking's answer: ranked ids, or why there isn't one.
pub type HostedAnswer = Result<Vec<(String, f32)>, String>;

/// The app around a search; faked in tests.
#[async_trait]
pub trait FindHost: Send + Sync {
    /// Open panes in workspaces that aren't removed.
    async fn panes(&self) -> Vec<PaneRecord>;
    /// A pane's recent plain text (ANSI stripped), not yet redacted.
    fn output(&self, pane: &str) -> String;
    /// The latest activity an agent reported in a pane (its preview or tool).
    fn activity(&self, pane: &str) -> Option<String>;
    /// The output of a shell's last command (shell integration marks), not
    /// yet redacted.
    fn last_output(&self, pane: &str) -> Option<String>;
    /// A pane's live terminal title.
    fn live_title(&self, pane: &str) -> Option<String>;
    /// An engine's display name.
    fn engine_label(&self, engine: &str) -> Option<String>;
    /// Threads that aren't archived.
    async fn threads(&self) -> Vec<ThreadRecord>;
    /// A thread's last message, the user's or the agent's.
    async fn last_message(&self, thread: &ThreadRecord) -> Option<String>;
    /// Teammates that aren't deleted.
    async fn teammates(&self) -> Vec<TeammateFacts> {
        Vec::new()
    }
    /// The hosted ranking: `None` while the user hasn't turned it on.
    async fn hosted_rank(
        &self,
        _query: &str,
        _candidates: &[crate::services::api::ResolveCandidate],
    ) -> Option<HostedAnswer> {
        None
    }
}

/// The first `n` characters, on a character boundary.
fn clip(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((cut, _)) => format!("{}…", s[..cut].trim_end()),
        None => s.to_string(),
    }
}

/// The last line with something on it.
fn last_line(s: &str) -> Option<String> {
    s.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| clip(l, DETAIL_CHARS))
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

async fn pane_candidate(host: &dyn FindHost, pane: PaneRecord) -> (Candidate, FindMatch) {
    let id = pane.id.as_str().to_string();
    let title = non_empty(pane.title.clone()).or_else(|| non_empty(host.live_title(&id)));
    let engine = pane.engine_id.as_deref().and_then(|e| host.engine_label(e));
    let label = non_empty(pane.human_name.clone())
        .or_else(|| title.clone())
        .or_else(|| engine.clone())
        .unwrap_or_else(|| match pane.kind.as_str() {
            "browser" => "Browser".into(),
            "simulator" => "Simulator".into(),
            "agent" => "Agent".into(),
            _ => "Terminal".into(),
        });
    let output = redact(&tandem_engines::handoff::last_lines(
        &host.output(&id),
        OUTPUT_LINES,
    ));
    let folder = pane
        .cwd
        .as_deref()
        .and_then(|c| Path::new(c).file_name())
        .map(|f| f.to_string_lossy().into_owned());
    let text = [
        title,
        engine,
        folder,
        pane.browser_url.clone(),
        Some(output.clone()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n");
    // An agent's own report, else how a shell's last command ended, else
    // the last line on screen.
    let detail = non_empty(host.activity(&id).map(|a| redact(&a)))
        .map(|a| clip(&a, DETAIL_CHARS))
        .or_else(|| host.last_output(&id).and_then(|o| last_line(&redact(&o))))
        .or_else(|| last_line(&output));
    let found = FindMatch {
        source: SourceRef::Pane {
            pane_id: id.clone(),
        },
        kind: FindKind::Pane,
        label: label.clone(),
        detail,
        workspace_id: Some(pane.workspace_id.as_str().to_string()),
        tab_id: Some(pane.tab_id.as_str().to_string()),
        score: 0.0,
    };
    (Candidate { id, label, text }, found)
}

async fn thread_candidate(host: &dyn FindHost, thread: ThreadRecord) -> (Candidate, FindMatch) {
    let id = thread.id.as_str().to_string();
    let message = host
        .last_message(&thread)
        .await
        .map(|m| redact(&clip(&m, MESSAGE_CHARS)));
    let label = non_empty(Some(thread.title.clone()))
        .or_else(|| message.as_deref().and_then(last_line))
        .unwrap_or_else(|| "New thread".into());
    let text = [Some(thread.title.clone()), message.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    let found = FindMatch {
        source: SourceRef::Thread {
            thread_id: id.clone(),
        },
        kind: FindKind::Thread,
        label: label.clone(),
        detail: message.as_deref().and_then(last_line),
        workspace_id: thread.workspace_id.map(|w| w.as_str().to_string()),
        tab_id: None,
        score: 0.0,
    };
    (Candidate { id, label, text }, found)
}

fn teammate_candidate(t: TeammateFacts) -> (Candidate, FindMatch) {
    let brief = t
        .brief
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| clip(l, MESSAGE_CHARS));
    let last = t
        .last_message
        .as_deref()
        .map(|m| redact(&clip(m, MESSAGE_CHARS)))
        .as_deref()
        .and_then(last_line);
    let text = [brief.clone(), last.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    let found = FindMatch {
        source: SourceRef::Teammate {
            teammate_id: t.id.clone(),
        },
        kind: FindKind::Teammate,
        label: t.name.clone(),
        detail: last.or_else(|| brief.map(|b| clip(&b, DETAIL_CHARS))),
        workspace_id: None,
        tab_id: None,
        score: 0.0,
    };
    (
        Candidate {
            id: t.id,
            label: t.name,
            text,
        },
        found,
    )
}

fn kind_name(kind: FindKind) -> &'static str {
    match kind {
        FindKind::Pane => "pane",
        FindKind::Thread => "thread",
        FindKind::Teammate => "teammate",
    }
}

/// What the hosted ranking is sent: each candidate's name and text, clipped,
/// the locally likelier ones first when there are too many.
/// A pane on a local model is never sent (F-EN-11): it's ranked here only.
fn hosted_candidates(
    candidates: &[Candidate],
    by_id: &HashMap<String, FindMatch>,
    local: &[find_work::Ranked],
    stay_here: &HashSet<String>,
) -> Vec<crate::services::api::ResolveCandidate> {
    let mut order: Vec<&Candidate> = local
        .iter()
        .filter_map(|r| candidates.iter().find(|c| c.id == r.id))
        .collect();
    for c in candidates {
        if !order.iter().any(|o| o.id == c.id) {
            order.push(c);
        }
    }
    order
        .into_iter()
        .filter(|c| !stay_here.contains(&c.id))
        .take(HOSTED_CANDIDATES)
        .filter_map(|c| {
            let kind = by_id.get(&c.id)?.kind;
            let text = format!("{}\n{}", c.label, c.text);
            let text = match text.char_indices().nth(HOSTED_TEXT_CHARS) {
                Some((cut, _)) => text[..cut].to_string(),
                None => text,
            };
            Some(crate::services::api::ResolveCandidate {
                id: c.id.clone(),
                kind: kind_name(kind).into(),
                text,
            })
        })
        .collect()
}

/// Ranks the open panes, threads and teammates for `query`, best first:
/// with the hosted ranking when it's on and answers, else on this machine.
pub async fn find(host: &dyn FindHost, query: &str) -> FindWorkResult {
    let mut candidates = Vec::new();
    let mut by_id = HashMap::new();
    let mut stay_here = HashSet::new();
    for pane in host.panes().await {
        if pane.local_model.is_some() {
            stay_here.insert(pane.id.as_str().to_string());
        }
        let (candidate, found) = pane_candidate(host, pane).await;
        by_id.insert(candidate.id.clone(), found);
        candidates.push(candidate);
    }
    for thread in host.threads().await {
        let (candidate, found) = thread_candidate(host, thread).await;
        by_id.insert(candidate.id.clone(), found);
        candidates.push(candidate);
    }
    for teammate in host.teammates().await {
        let (candidate, found) = teammate_candidate(teammate);
        by_id.insert(candidate.id.clone(), found);
        candidates.push(candidate);
    }
    let local = find_work::rank(query, &candidates);
    let mut ranking = FindRanking::Local;
    let mut ranked = local.clone();
    if !candidates.is_empty() && !query.trim().is_empty() {
        let sent = hosted_candidates(&candidates, &by_id, &local, &stay_here);
        match host.hosted_rank(query, &sent).await {
            None => {}
            Some(Ok(matches)) => {
                // Only ids it was offered, each once.
                let mut hosted: Vec<find_work::Ranked> = Vec::new();
                for (id, score) in matches {
                    if by_id.contains_key(&id)
                        && score.is_finite()
                        && score > 0.0
                        && !hosted.iter().any(|r| r.id == id)
                    {
                        hosted.push(find_work::Ranked {
                            id,
                            score: score.min(1.0) * HOSTED_SCALE,
                        });
                    }
                }
                if !hosted.is_empty() {
                    // A local model's pane wasn't sent: it keeps its place
                    // from the local ranking among the hosted answer
                    // (F-EN-11).
                    for r in &local {
                        if stay_here.contains(&r.id) && !hosted.iter().any(|h| h.id == r.id) {
                            hosted.push(r.clone());
                        }
                    }
                }
                hosted.sort_by(|a, b| b.score.total_cmp(&a.score));
                if hosted.is_empty() {
                    tracing::info!(
                        code = "find_work.hosted_empty",
                        "hosted ranking found nothing; ranked locally"
                    );
                } else {
                    ranked = hosted;
                    ranking = FindRanking::Hosted;
                }
            }
            Some(Err(reason)) => {
                tracing::info!(code = "find_work.hosted_failed", reason = %reason, "hosted ranking unavailable; ranked locally");
            }
        }
    }
    let clear = matches!(find_work::decide(&ranked), Decision::One(_));
    let matches = ranked
        .into_iter()
        .take(MAX_MATCHES)
        .filter_map(|r| {
            by_id.remove(&r.id).map(|mut found| {
                found.score = r.score;
                found
            })
        })
        .collect();
    FindWorkResult {
        matches,
        clear,
        ranking,
    }
}

/// A thread's last message text from its stored events (newest last): the
/// user's text or the agent's finished block.
pub fn message_text(kind: &str, payload_json: &str) -> Option<String> {
    let field = match kind {
        "user" => "text",
        "assistant" => "markdown",
        _ => return None,
    };
    let value: serde_json::Value = serde_json::from_str(payload_json).ok()?;
    non_empty(value.get(field)?.as_str().map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::access::{Access, ThreadMode};
    use tandem_core::ids::{PaneId, TabId, ThreadId, WorkspaceId};
    use tandem_store::repo::threads::{ThreadKind, ThreadState};

    #[derive(Default)]
    struct FakeHost {
        panes: Vec<PaneRecord>,
        output: HashMap<String, String>,
        activity: HashMap<String, String>,
        last_output: HashMap<String, String>,
        threads: Vec<ThreadRecord>,
        messages: HashMap<String, String>,
        teammates: Vec<TeammateFacts>,
        /// None: the hosted ranking is off.
        hosted: Option<HostedAnswer>,
        sent: std::sync::Mutex<Vec<crate::services::api::ResolveCandidate>>,
    }

    #[async_trait]
    impl FindHost for FakeHost {
        async fn panes(&self) -> Vec<PaneRecord> {
            self.panes.clone()
        }
        fn output(&self, pane: &str) -> String {
            self.output.get(pane).cloned().unwrap_or_default()
        }
        fn activity(&self, pane: &str) -> Option<String> {
            self.activity.get(pane).cloned()
        }
        fn last_output(&self, pane: &str) -> Option<String> {
            self.last_output.get(pane).cloned()
        }
        fn live_title(&self, _pane: &str) -> Option<String> {
            None
        }
        fn engine_label(&self, engine: &str) -> Option<String> {
            (engine == "claude").then(|| "Claude Code".to_string())
        }
        async fn threads(&self) -> Vec<ThreadRecord> {
            self.threads.clone()
        }
        async fn last_message(&self, thread: &ThreadRecord) -> Option<String> {
            self.messages.get(thread.id.as_str()).cloned()
        }
        async fn teammates(&self) -> Vec<TeammateFacts> {
            self.teammates.clone()
        }
        async fn hosted_rank(
            &self,
            _query: &str,
            candidates: &[crate::services::api::ResolveCandidate],
        ) -> Option<HostedAnswer> {
            // Off: nothing is sent.
            if self.hosted.is_some() {
                if let Ok(mut sent) = self.sent.lock() {
                    *sent = candidates.to_vec();
                }
            }
            self.hosted.clone()
        }
    }

    fn pane(id: &str, name: Option<&str>) -> PaneRecord {
        PaneRecord {
            id: PaneId::from_db(id.into()),
            tab_id: TabId::from_db("tab_1".into()),
            workspace_id: WorkspaceId::from_db("ws_1".into()),
            kind: "agent".into(),
            cwd: Some("/home/ana/acme".into()),
            engine_id: Some("claude".into()),
            account_id: None,
            human_name: name.map(str::to_string),
            title: None,
            restore_state: "none".into(),
            created_at: 1,
            browser_url: None,
            local_model: None,
        }
    }

    fn thread(id: &str, title: &str) -> ThreadRecord {
        ThreadRecord {
            id: ThreadId::from_db(id.into()),
            kind: ThreadKind::Thread,
            workspace_id: Some(WorkspaceId::from_db("ws_1".into())),
            cwd: "/home/ana/acme".into(),
            engine_id: "claude".into(),
            account_id: None,
            model: None,
            effort: None,
            access: Access::Edits,
            mode: ThreadMode::Act,
            title: title.into(),
            state: ThreadState::Idle,
            attended: true,
            engine_session_id: None,
            pinned: false,
            archived: false,
            unread: false,
            sort_key: "a0".into(),
            created_at: 1,
            updated_at: 1,
            teammate_id: None,
            conversation_ref: None,
        }
    }

    fn host() -> FakeHost {
        FakeHost {
            panes: vec![
                pane("pan_auth", Some("auth tests")),
                pane("pan_billing", Some("billing UI")),
                pane("pan_docs", Some("docs")),
            ],
            output: HashMap::from([(
                "pan_auth".to_string(),
                "ana@box:~/acme$ npm test -- auth\n  12 passing\nana@box:~/acme$ ".to_string(),
            )]),
            last_output: HashMap::from([("pan_auth".to_string(), "  12 passing\n".to_string())]),
            ..FakeHost::default()
        }
    }

    #[tokio::test]
    async fn f_cn_05_1_login_testing_finds_the_auth_tests_pane_and_says_what_it_was_doing() {
        let result = find(&host(), "login testing").await;
        assert!(result.clear);
        let first = &result.matches[0];
        assert_eq!(
            first.source,
            SourceRef::Pane {
                pane_id: "pan_auth".into()
            }
        );
        assert_eq!(first.label, "auth tests");
        assert_eq!(first.detail.as_deref(), Some("12 passing"));
        assert_eq!(first.workspace_id.as_deref(), Some("ws_1"));
        assert_eq!(first.tab_id.as_deref(), Some("tab_1"));
        assert_eq!(result.matches.len(), 1);
    }

    #[tokio::test]
    async fn f_cn_05_threads_are_found_by_title_and_last_message() {
        let mut host = host();
        host.threads = vec![thread("thr_1", "Fix flaky checkout")];
        host.messages = HashMap::from([(
            "thr_1".to_string(),
            "The Stripe webhook retries twice".to_string(),
        )]);
        let result = find(&host, "stripe webhook").await;
        let first = &result.matches[0];
        assert_eq!(
            first.source,
            SourceRef::Thread {
                thread_id: "thr_1".into()
            }
        );
        assert_eq!(first.kind, FindKind::Thread);
        assert_eq!(
            first.detail.as_deref(),
            Some("The Stripe webhook retries twice")
        );
        // "billing UI" means the same as "checkout", so it's found too, lower.
        let found = find(&host, "payments").await;
        assert_eq!(found.matches[0].label, "billing UI");
        assert!(found
            .matches
            .iter()
            .any(|m| m.label == "Fix flaky checkout"));
    }

    #[tokio::test]
    async fn f_cn_05_output_is_redacted_and_only_its_last_20_lines_are_read() {
        let mut host = host();
        let key = "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let mut lines: Vec<String> = (0..30).map(|i| format!("line {i}")).collect();
        lines[2] = "an early line about kubernetes".into();
        lines.push(format!("export KEY={key}"));
        host.output.insert("pan_docs".into(), lines.join("\n"));
        let result = find(&host, "line").await;
        let docs = result
            .matches
            .iter()
            .find(|m| m.label == "docs")
            .expect("docs");
        let detail = docs.detail.clone().expect("detail");
        assert!(!detail.contains(key), "{detail}");
        assert!(find(&host, "kubernetes").await.matches.is_empty());
    }

    #[tokio::test]
    async fn f_cn_05_an_agents_reported_activity_is_what_it_was_last_doing() {
        let mut host = host();
        host.activity
            .insert("pan_auth".into(), "Running the auth suite".into());
        let result = find(&host, "auth").await;
        assert_eq!(
            result.matches[0].detail.as_deref(),
            Some("Running the auth suite")
        );
    }

    #[tokio::test]
    async fn f_cn_05_unnamed_panes_go_by_title_engine_or_kind() {
        let mut host = FakeHost::default();
        let mut unnamed = pane("pan_x", None);
        unnamed.engine_id = None;
        unnamed.kind = "terminal".into();
        host.panes = vec![unnamed, pane("pan_y", None)];
        let result = find(&host, "terminal").await;
        assert_eq!(result.matches[0].label, "Terminal");
        let result = find(&host, "claude").await;
        assert_eq!(result.matches[0].label, "Claude Code");
        // The folder counts as well.
        assert_eq!(find(&host, "acme").await.matches.len(), 2);
    }

    #[test]
    fn f_cn_05_a_threads_last_message_is_the_users_text_or_the_agents_block() {
        assert_eq!(
            message_text(
                "user",
                r#"{"type":"user","text":"run the tests","attachments":[]}"#
            ),
            Some("run the tests".into())
        );
        assert_eq!(
            message_text(
                "assistant",
                r#"{"type":"assistant","markdown":"All **12** pass."}"#
            ),
            Some("All **12** pass.".into())
        );
        assert_eq!(message_text("tool", r#"{"type":"tool"}"#), None);
        assert_eq!(message_text("user", "not json"), None);
    }

    #[tokio::test]
    async fn f_cn_05_1_the_hosted_ranking_resolves_login_testing_and_the_local_one_answers_when_its_down(
    ) {
        // The mocked server's answer.
        let mut on = host();
        on.hosted = Some(Ok(vec![
            ("pan_auth".into(), 0.9),
            ("pan_docs".into(), 0.1),
            ("not_offered".into(), 1.0),
        ]));
        let hosted = find(&on, "login testing").await;
        assert_eq!(hosted.ranking, FindRanking::Hosted);
        assert!(hosted.clear);
        assert_eq!(hosted.matches[0].label, "auth tests");
        assert_eq!(hosted.matches.len(), 2, "only offered ids");
        // What was sent: names and redacted text, each with its kind.
        let sent = on.sent.lock().expect("sent").clone();
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[0].id, "pan_auth", "the likelier ones first");
        assert!(sent.iter().all(|c| c.kind == "pane"));
        assert!(sent[0].text.starts_with("auth tests\n"));
        // The server is down: the same answer, ranked here.
        let mut down = host();
        down.hosted = Some(Err("couldn't reach Vibezeo".into()));
        let local = find(&down, "login testing").await;
        assert_eq!(local.ranking, FindRanking::Local);
        assert!(local.clear);
        assert_eq!(local.matches[0].label, "auth tests");
        // Off: nothing is asked.
        let off = host();
        assert_eq!(
            find(&off, "login testing").await.ranking,
            FindRanking::Local
        );
        assert!(off.sent.lock().expect("sent").is_empty());
        // An empty answer falls back too.
        let mut empty = host();
        empty.hosted = Some(Ok(Vec::new()));
        let r = find(&empty, "login testing").await;
        assert_eq!(
            (r.ranking, r.matches[0].label.as_str()),
            (FindRanking::Local, "auth tests")
        );
    }

    /// F-EN-11 review: a pane on a local model is ranked on this computer
    /// only; its name and output never go to the hosted ranking.
    #[tokio::test]
    async fn f_en_11_3_a_local_model_pane_is_never_sent_to_the_hosted_ranking() {
        let mut h = host();
        h.panes[0].local_model = Some("qwen3:8b · Ollama".into());
        h.hosted = Some(Ok(vec![("pan_docs".into(), 0.9)]));
        let r = find(&h, "login testing").await;
        let sent = h.sent.lock().expect("sent").clone();
        assert!(sent.iter().all(|c| c.id != "pan_auth"), "{sent:?}");
        assert_eq!(sent.len(), 2);
        // The hosted ranking answered, and the local pane is still found,
        // ranked here.
        assert_eq!(r.ranking, FindRanking::Hosted);
        let labels: Vec<&str> = r.matches.iter().map(|m| m.label.as_str()).collect();
        assert!(labels.contains(&"auth tests"), "{labels:?}");
        assert!(labels.contains(&"docs"), "{labels:?}");
        // Off: still found, ranked here.
        let mut off = host();
        off.panes[0].local_model = Some("qwen3:8b · Ollama".into());
        let r = find(&off, "login testing").await;
        assert_eq!(r.matches[0].label, "auth tests");
    }

    #[tokio::test]
    async fn f_cn_05_close_hosted_scores_ask_which() {
        let mut h = host();
        h.hosted = Some(Ok(vec![("pan_auth".into(), 0.8), ("pan_docs".into(), 0.7)]));
        let r = find(&h, "tests").await;
        assert_eq!(r.ranking, FindRanking::Hosted);
        assert!(!r.clear);
        // Weak alone is not clear either.
        h.hosted = Some(Ok(vec![("pan_auth".into(), 0.3)]));
        assert!(!find(&h, "tests").await.clear);
    }

    #[tokio::test]
    async fn f_cn_05_teammates_are_found_by_name_brief_and_last_line() {
        let mut h = host();
        let key = "sk-ant-api03-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
        h.teammates = vec![TeammateFacts {
            id: "tm_rel".into(),
            name: "Release scribe".into(),
            brief: "You draft the changelog from merged pull requests.\nKeep a strict tone.".into(),
            last_message: Some(format!("Drafted 0.5.0 notes with {key}")),
        }];
        // ("changelog" also means docs, so the docs pane is found too.)
        let r = find(&h, "merged pull requests").await;
        let first = &r.matches[0];
        assert_eq!(first.kind, FindKind::Teammate);
        assert_eq!(
            first.source,
            SourceRef::Teammate {
                teammate_id: "tm_rel".into()
            }
        );
        assert_eq!(first.label, "Release scribe");
        let detail = first.detail.clone().expect("detail");
        assert!(detail.starts_with("Drafted 0.5.0 notes"), "{detail}");
        assert!(!detail.contains(key), "{detail}");
        // By name, and by what it said last.
        assert_eq!(find(&h, "scribe").await.matches[0].label, "Release scribe");
        assert_eq!(
            find(&h, "drafted notes").await.matches[0].label,
            "Release scribe"
        );
        // Only the brief's first line counts.
        assert!(find(&h, "strict tone").await.matches.is_empty());
        // Sent to the hosted ranking as a teammate, redacted.
        h.hosted = Some(Err("down".into()));
        let _ = find(&h, "pull requests").await;
        let sent = h.sent.lock().expect("sent").clone();
        let tm = sent.iter().find(|c| c.id == "tm_rel").expect("teammate");
        assert_eq!(tm.kind, "teammate");
        assert!(!tm.text.contains(key));
    }

    #[tokio::test]
    async fn f_cn_05_at_most_200_candidates_of_2000_characters_are_sent() {
        let h = FakeHost {
            panes: (0..250)
                .map(|i| pane(&format!("pan_{i}"), Some(&format!("pane {i}"))))
                .collect(),
            output: HashMap::from([("pan_249".to_string(), "x".repeat(5_000))]),
            hosted: Some(Err("down".into())),
            ..FakeHost::default()
        };
        let _ = find(&h, "pane 249").await;
        let sent = h.sent.lock().expect("sent").clone();
        assert_eq!(sent.len(), HOSTED_CANDIDATES);
        assert!(sent
            .iter()
            .all(|c| c.text.chars().count() <= HOSTED_TEXT_CHARS));
        assert!(
            sent.iter().any(|c| c.id == "pan_249"),
            "the local best is kept"
        );
    }
}
