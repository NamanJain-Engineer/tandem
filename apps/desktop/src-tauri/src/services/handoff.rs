//! Handoff between engines (F-CD-16, UI-13): a brief from the source, the
//! target engine headless or the raw lines, then the target opens beside
//! the source (or in a new worktree) with the brief as its first prompt.

use std::path::Path;
use std::time::Duration;

use rand::RngCore;
use serde::{Deserialize, Serialize};
use tandem_core::ids::{AccountId, PaneId, TabId, WorkspaceId};
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::handoff::{self, Reply};
use tandem_engines::launch::MAX_ARG_INPUT;
use tandem_store::repo::panes::{self, PaneRecord};
use tandem_store::repo::tabs;
use tandem_store::repo::threads::{ThreadKind, ThreadRecord};
use tandem_store::repo::workspaces;
use ts_rs::TS;

use crate::commands::terminal::{close_pane, create_pane};
use crate::services::lanes;
use crate::services::launch::FIRST_PROMPT_WAIT;
use crate::state::AppState;

/// How long the source may take to write its brief (ARCHITECTURE §6.11).
pub const ASK_WAIT: Duration = Duration::from_secs(120);
/// How long a headless brief may take.
pub const HEADLESS_WAIT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(400);
/// Lines of the session the headless brief reads, and the fallback shows.
const HEADLESS_LINES: u16 = 400;
const FALLBACK_LINES: usize = 200;

/// Where the brief came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum BriefSource {
    /// The source agent wrote it.
    Source,
    /// The target engine wrote it from the session's end.
    Headless,
    /// The raw last lines under the template's headings.
    Fallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HandoffBrief {
    pub text: String,
    pub source: BriefSource,
}

/// UI-13's "Where".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HandoffPlace {
    /// The source's folder or worktree, split right of the source.
    Same,
    /// A new worktree of the source's repository.
    NewWorktree,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HandoffResult {
    pub pane: PaneRecord,
    pub workspace_id: WorkspaceId,
    pub tab_id: TabId,
    pub lane_branch: Option<String>,
    /// The source pane was closed ("Close the original pane after handoff").
    pub source_closed: bool,
}

fn nonce() -> String {
    let mut bytes = [0u8; 3];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A source that can be asked: a live agent of a chat engine whose screen
/// doesn't show a usage limit.
fn can_ask(state: &AppState, source: &PaneRecord) -> bool {
    if source.kind != "agent" || !state.terminals.pty.is_live(source.id.as_str()) {
        return false;
    }
    let Some(engine) = source
        .engine_id
        .as_deref()
        .and_then(|id| state.terminals.engines.get(id))
    else {
        return false;
    };
    if engine.chat_adapter.is_none() {
        return false;
    }
    let screen = state
        .terminals
        .pty
        .screen_text(source.id.as_str())
        .unwrap_or_default();
    !limited(&screen, &engine.limit_patterns)
}

/// True when the screen matches one of the engine's limit patterns.
fn limited(screen: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| {
        regex::RegexBuilder::new(p)
            .case_insensitive(true)
            .multi_line(true)
            .build()
            .is_ok_and(|re| re.is_match(screen))
    })
}

/// Strategy 1: types the request into the source and waits for the reply.
async fn ask_source(state: &AppState, pane: &PaneId) -> Option<String> {
    let nonce = nonce();
    state
        .terminals
        .type_prompt(pane, &handoff::ask_prompt(&nonce))
        .await
        .ok()?;
    let pty = &state.terminals.pty;
    let started = tokio::time::Instant::now();
    while started.elapsed() < ASK_WAIT && pty.is_live(pane.as_str()) {
        tokio::time::sleep(POLL).await;
        let screen = pty.plain_tail(pane.as_str(), HEADLESS_LINES).ok()?;
        match handoff::read_reply(&screen, &nonce) {
            Reply::Brief(text) => return Some(text),
            Reply::Malformed => return None,
            Reply::Pending => {}
        }
    }
    None
}

/// Strategy 2: the target engine, headless, over the session's end.
async fn ask_headless(
    state: &AppState,
    engine_id: &str,
    account: Option<&AccountId>,
    root: &Path,
    session: &str,
) -> Option<String> {
    let engines = &state.terminals.engines;
    let engine = engines.get(engine_id)?;
    let args = handoff::headless_args(&engine)?;
    let program = engines.installed(engine_id)?;
    let launch = engines
        .agent_launch(&engine, program.clone(), root, None, account)
        .await
        .ok()?;
    let login = state.terminals.login_env().await;
    // What a pane gets: the sanitized login env and the account's home.
    let mut env: Vec<(String, String)> = login
        .vars
        .iter()
        .chain(launch.spec.env.iter())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    env.extend(crate::services::fake_engine_env());
    let out = handoff::run_headless(
        &program,
        &args,
        &env,
        root,
        &handoff::headless_prompt(session),
        HEADLESS_WAIT,
    )
    .await;
    match out {
        Ok(text) if handoff::well_formed(&text) => Some(text.trim().to_string()),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!(code = "handoff.headless", error = %e, "headless brief failed");
            None
        }
    }
}

/// `handoff_brief`: the brief for handing `pane` to `engine_id`.
pub async fn brief(
    state: &AppState,
    pane: &PaneId,
    engine_id: &str,
    account: Option<&AccountId>,
) -> Result<HandoffBrief, IpcError> {
    let source = panes::get_open(&state.store, pane).await?;
    let ws = workspaces::get(&state.store, &source.workspace_id).await?;
    let (text, from) = 'brief: {
        if can_ask(state, &source) {
            if let Some(text) = ask_source(state, pane).await {
                break 'brief (text, BriefSource::Source);
            }
        }
        let session = tandem_core::redact::redact(
            &state
                .terminals
                .pty
                .plain_tail(pane.as_str(), HEADLESS_LINES)
                .unwrap_or_default(),
        );
        if let Some(text) =
            ask_headless(state, engine_id, account, Path::new(&ws.root), &session).await
        {
            break 'brief (text, BriefSource::Headless);
        }
        (
            handoff::fallback(&handoff::last_lines(&session, FALLBACK_LINES), None),
            BriefSource::Fallback,
        )
    };
    tracing::info!(code = "handoff.brief", source = ?from, "handoff brief ready");
    Ok(HandoffBrief { text, source: from })
}

/// The first prompt for a brief: the brief itself, or, past the inline
/// limit, a pointer to it in `$DATA/briefings/<pane>.md` (0600; F-CD-16
/// AC2).
pub fn deliver(brief: &str, data_dir: &Path, pane: &PaneId) -> Result<String, IpcError> {
    if brief.len() <= MAX_ARG_INPUT {
        return Ok(brief.to_string());
    }
    let path = handoff::briefing_path(data_dir, pane.as_str());
    let io = |e: std::io::Error| IpcError::new(ErrorCode::Io, e.to_string()).with_stage("briefing");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    opts.open(&path)
        .and_then(|mut f| f.write_all(brief.as_bytes()))
        .map_err(io)?;
    Ok(format!(
        "Read the handoff brief in {} and continue.",
        path.display()
    ))
}

/// The worktree name for a handoff: the brief's goal, else "handoff".
pub fn slug_source(brief: &str) -> String {
    let mut lines = brief.lines().map(str::trim);
    lines
        .by_ref()
        .find(|l| {
            l.trim_start_matches(['#', '*', ' '])
                .to_ascii_lowercase()
                .starts_with("goal")
        })
        .and_then(|_| lines.find(|l| !l.is_empty()))
        .map(|l| l.split_whitespace().take(6).collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|| "handoff".into())
}

/// `handoff_start`: opens the target with the (possibly edited) brief.
#[allow(clippy::too_many_arguments)]
pub async fn start(
    state: &AppState,
    pane: &PaneId,
    engine_id: &str,
    account: Option<AccountId>,
    brief: &str,
    place: HandoffPlace,
    close_source: bool,
) -> Result<HandoffResult, IpcError> {
    let brief = brief.trim();
    if brief.is_empty() {
        return Err(IpcError::new(ErrorCode::Invalid, "Write a summary first"));
    }
    let source = panes::get_open(&state.store, pane).await?;
    let engines = &state.terminals.engines;
    let engine = engines
        .get(engine_id)
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "No engine with that id"))?;
    if engines.installed(engine_id).is_none() {
        return Err(IpcError::new(
            ErrorCode::EngineMissing,
            format!("{} isn't installed", engine.label),
        ));
    }
    state
        .terminals
        .check_live_limit(state.entitlements.limits().panes)?;
    let (home, tab) = match place {
        HandoffPlace::Same => (
            workspaces::get(&state.store, &source.workspace_id).await?,
            source.tab_id.clone(),
        ),
        HandoffPlace::NewWorktree => {
            let login = state.terminals.login_env().await;
            let git = lanes::git_for(&state.data, &login)?;
            let lane = lanes::create(
                &state.store,
                &git,
                &source.workspace_id,
                &slug_source(brief),
            )
            .await?;
            let tab = tabs::list(&state.store, &lane.id)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "The worktree has no tab"))?;
            (lane, tab.id)
        }
    };
    let target = create_pane(
        state,
        &tab,
        "agent",
        Some(home.root.clone()),
        Some(engine_id.to_string()),
        account,
    )
    .await?;
    let prompt = deliver(brief, state.data.root(), &target.id)?;
    let terminals = state.terminals.clone();
    let id = target.id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = terminals
            .type_prompt_when_live(&id, &prompt, FIRST_PROMPT_WAIT)
            .await
        {
            tracing::warn!(code = "handoff.prompt", error = %e.message, "handoff brief not sent");
        }
    });
    let source_closed = close_source && close_pane(state, pane).await.is_ok();
    tracing::info!(code = "handoff.start", place = ?place, "handed off");
    Ok(HandoffResult {
        pane: target,
        workspace_id: home.id.clone(),
        tab_id: tab,
        lane_branch: home.lane_branch.clone(),
        source_closed,
    })
}

/// `handoff_to_thread`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HandoffThread {
    pub thread: ThreadRecord,
    /// The source pane was closed ("Close the original pane after handoff").
    pub source_closed: bool,
}

/// A handoff to a chat target (F-CD-16, §6.11 delivery): a new Thread in
/// the source's folder whose first message is the brief.
pub async fn start_thread(
    state: &AppState,
    pane: &PaneId,
    engine_id: &str,
    account: Option<AccountId>,
    brief: &str,
    close_source: bool,
) -> Result<HandoffThread, IpcError> {
    let brief = brief.trim();
    if brief.is_empty() {
        return Err(IpcError::new(ErrorCode::Invalid, "Write a summary first"));
    }
    let source = panes::get_open(&state.store, pane).await?;
    let engines = &state.terminals.engines;
    let engine = engines
        .get(engine_id)
        .ok_or_else(|| IpcError::new(ErrorCode::NotFound, "No engine with that id"))?;
    if engines.installed(engine_id).is_none() {
        return Err(IpcError::new(
            ErrorCode::EngineMissing,
            format!("{} isn't installed", engine.label),
        ));
    }
    if !crate::services::threads::has_chat_adapter(&engine) {
        return Err(IpcError::new(
            ErrorCode::EngineUnsupported,
            format!(
                "{} can't run in Thread mode yet. Hand off to a pane instead.",
                engine.label
            ),
        ));
    }
    let thread = crate::commands::threads::create_thread(
        state,
        crate::commands::threads::ThreadCreate {
            kind: ThreadKind::Thread,
            workspace_id: Some(source.workspace_id.clone()),
            engine_id: Some(engine_id.to_string()),
            account_id: account,
            model: None,
            effort: None,
            access: None,
            mode: None,
        },
    )
    .await?;
    state
        .threads
        .send(&thread.id, brief.to_string(), Vec::new(), false)
        .await?;
    let source_closed = close_source && close_pane(state, pane).await.is_ok();
    tracing::info!(code = "handoff.start", place = "thread", "handed off");
    Ok(HandoffThread {
        thread,
        source_closed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cd_16_2_a_200_kb_brief_is_handed_over_as_a_file_not_typed() {
        let dir = tempfile::tempdir().expect("tmp");
        let pane = PaneId::from_db("pn_target".to_string());
        assert_eq!(
            deliver("## Goal\nx", dir.path(), &pane).expect("short"),
            "## Goal\nx"
        );
        let big = "## Goal\n".to_string() + &"y".repeat(200 * 1024);
        let prompt = deliver(&big, dir.path(), &pane).expect("long");
        let path = dir.path().join("briefings").join("pn_target.md");
        assert_eq!(
            prompt,
            format!("Read the handoff brief in {} and continue.", path.display())
        );
        assert!(prompt.len() < 512);
        assert_eq!(std::fs::read_to_string(&path).expect("written"), big);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn f_cd_16_a_limit_on_screen_means_the_source_is_not_asked() {
        let patterns = vec!["usage limit reached".to_string(), r"\b429\b".to_string()];
        assert!(limited("Claude usage limit reached. Resets 3pm", &patterns));
        assert!(limited("error 429 Too Many Requests", &patterns));
        assert!(!limited("> fix the parser", &patterns));
    }

    #[test]
    fn f_cd_15_a_new_worktree_for_a_handoff_is_named_after_its_goal() {
        assert_eq!(
            slug_source(
                "## Goal\nFix the flaky login test in CI today please\n\n## What's done\nx"
            ),
            "Fix the flaky login test in"
        );
        assert_eq!(slug_source("no headings here"), "handoff");
    }
}
