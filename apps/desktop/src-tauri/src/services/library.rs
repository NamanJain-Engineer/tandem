//! The session library (F-TH-17, UI-162): every Claude Code and Codex
//! session on this computer, from each account's provider home and each
//! engine's own home; Resume and Continue with another engine; and, opt-in
//! (no account needed, D-2900), a local full-text index at
//! `$DATA/session-index.db`.
//!
//! Session text is read only through `tandem_engines::library` and is never
//! logged; the index stays on this computer (D-2413).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use tandem_core::ids::{AccountId, TabId, WorkspaceId};
use tandem_core::settings::SettingKey;
use tandem_core::{ErrorCode, IpcError};
use tandem_engines::library::{
    self, Found, LibraryEngine, LibrarySource, RealFs, SessionFs, TranscriptItem,
};
use tandem_store::repo::panes::PaneRecord;
use tandem_store::repo::sessions::{self, LaunchConfig};
use tandem_store::repo::{accounts, settings, tabs, workspaces as repo};

use crate::commands::terminal::create_pane_with;
use crate::services::handoff;
use crate::services::workspaces;
use crate::state::AppState;

pub const INDEX_FILE: &str = "session-index.db";
/// Search results at most.
const MAX_HITS: u32 = 200;

/// One session, as listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LibrarySession {
    /// What the other commands take: the session file's path.
    pub key: String,
    pub engine_id: String,
    /// `None`: the engine's own home, outside Vibezeo.
    pub account_id: Option<String>,
    pub account_label: Option<String>,
    pub session_id: String,
    pub folder: Option<String>,
    /// The git root of the folder, else the folder (the grouping).
    pub repository: Option<String>,
    pub title: Option<String>,
    #[ts(type = "number")]
    pub size: u64,
    /// Last activity, Unix ms.
    pub modified_at: i64,
    pub folder_exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LibraryListing {
    pub sessions: Vec<LibrarySession>,
    /// Session files that couldn't be read.
    pub skipped: u32,
}

/// Where Resume or Continue opened its pane.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LibraryOpened {
    pub pane: PaneRecord,
    pub workspace_id: WorkspaceId,
    pub tab_id: TabId,
}

fn not_found() -> IpcError {
    IpcError::new(
        ErrorCode::NotFound,
        "That session isn't on this computer any more",
    )
}

fn internal(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Internal, e.to_string())
}

/// The homes to read: every Claude Code and Codex account's, then each
/// engine's own (`env` and `user_home` from the login environment).
pub fn sources(
    accounts: &[accounts::Account],
    user_home: &Path,
    env: impl Fn(&str) -> Option<String>,
) -> Vec<LibrarySource> {
    let mut out: Vec<LibrarySource> = accounts
        .iter()
        .filter(|a| !a.home_path.is_empty())
        .filter_map(|a| {
            Some(LibrarySource {
                engine: LibraryEngine::of(&a.engine_id)?,
                account_id: Some(a.id.as_str().to_string()),
                account_label: Some(a.label.clone()),
                home: PathBuf::from(&a.home_path),
            })
        })
        .collect();
    for engine in [LibraryEngine::Claude, LibraryEngine::Codex] {
        let home = engine.own_home(user_home, &env);
        if !out.iter().any(|s| s.engine == engine && s.home == home) {
            out.push(LibrarySource {
                engine,
                account_id: None,
                account_label: None,
                home,
            });
        }
    }
    out
}

pub fn view(found: &Found) -> LibrarySession {
    let exists = |p: &Path| p.exists();
    LibrarySession {
        key: found.file.to_string_lossy().into_owned(),
        engine_id: found.engine.id().to_string(),
        account_id: found.account_id.clone(),
        account_label: found.account_label.clone(),
        session_id: found.session_id.clone(),
        folder: found.folder.clone(),
        repository: found
            .folder
            .as_deref()
            .map(|f| library::repository(f, exists)),
        title: found.title.clone(),
        size: found.size,
        modified_at: found.modified_ms,
        folder_exists: found
            .folder
            .as_deref()
            .is_some_and(|f| Path::new(f).is_dir()),
    }
}

async fn current_sources(state: &AppState) -> Result<Vec<LibrarySource>, IpcError> {
    let accounts = accounts::list(&state.store).await?;
    let login = state.terminals.login_env().await;
    let user_home = login
        .vars
        .get("HOME")
        .map(PathBuf::from)
        .or_else(tandem_core::paths::user_home)
        .unwrap_or_default();
    Ok(sources(&accounts, &user_home, |k| {
        login.vars.get(k).cloned()
    }))
}

/// How long a listing serves the commands that follow it (Open, Resume,
/// Continue, Search) before the homes are read again.
const SCAN_FRESH: Duration = Duration::from_secs(30);

type Cached = (Instant, Vec<LibrarySource>, Arc<library::Scan>);
static LAST_SCAN: Mutex<Option<Cached>> = Mutex::new(None);

async fn scan_fresh(state: &AppState, reuse: bool) -> Result<Arc<library::Scan>, IpcError> {
    let sources = current_sources(state).await?;
    if reuse {
        if let Ok(last) = LAST_SCAN.lock() {
            if let Some((at, srcs, scan)) = last.as_ref() {
                if *srcs == sources && at.elapsed() < SCAN_FRESH {
                    return Ok(scan.clone());
                }
            }
        }
    }
    let srcs = sources.clone();
    let scan = Arc::new(
        tokio::task::spawn_blocking(move || library::scan(&RealFs, &srcs))
            .await
            .map_err(internal)?,
    );
    if let Ok(mut last) = LAST_SCAN.lock() {
        *last = Some((Instant::now(), sources, scan.clone()));
    }
    Ok(scan)
}

/// `library_list`. With search off, a leftover index is deleted.
pub async fn list(state: &AppState) -> Result<LibraryListing, IpcError> {
    if !search_on(state).await {
        delete_index(state.data.root());
    }
    let scan = scan_fresh(state, false).await?;
    tracing::info!(
        code = "library.listed",
        sessions = scan.sessions.len(),
        skipped = scan.skipped,
        "session library listed"
    );
    Ok(LibraryListing {
        sessions: scan.sessions.iter().map(view).collect(),
        skipped: scan.skipped,
    })
}

/// The listed session with this key: nothing else is ever read.
pub async fn find(state: &AppState, key: &str) -> Result<Found, IpcError> {
    scan_fresh(state, true)
        .await?
        .sessions
        .iter()
        .find(|s| s.file.to_string_lossy() == key)
        .cloned()
        .ok_or_else(not_found)
}

async fn read_transcript(found: &Found) -> Result<Vec<TranscriptItem>, IpcError> {
    let (engine, file) = (found.engine, found.file.clone());
    tokio::task::spawn_blocking(move || library::transcript(&RealFs, engine, &file))
        .await
        .map_err(internal)?
        .ok_or_else(not_found)
}

/// `library_transcript`.
pub async fn transcript(state: &AppState, key: &str) -> Result<Vec<TranscriptItem>, IpcError> {
    read_transcript(&find(state, key).await?).await
}

/// The workspace and tab a session opens in: the deepest open local
/// workspace containing its folder, else its repository added as one.
async fn home_for(
    state: &AppState,
    found: &Found,
) -> Result<(repo::Workspace, TabId, String), IpcError> {
    let folder = found
        .folder
        .clone()
        .filter(|f| Path::new(f).is_dir())
        .ok_or_else(|| {
            IpcError::new(
                ErrorCode::NotFound,
                "This session's folder isn't on this computer any more",
            )
        })?;
    let open = repo::list(&state.store)
        .await?
        .into_iter()
        .filter(|w| w.kind == "local" && Path::new(&folder).starts_with(&w.root))
        .max_by_key(|w| w.root.len());
    let ws = match open {
        Some(ws) => ws,
        None => {
            let root = library::repository(&folder, |p| p.exists());
            workspaces::add(&state.store, &root, state.entitlements.limits().workspaces)
                .await?
                .workspace
        }
    };
    let tab = match tabs::list(&state.store, &ws.id).await?.into_iter().next() {
        Some(t) => t.id,
        None => tabs::create(&state.store, &ws.id, None).await?.id,
    };
    Ok((ws, tab, folder))
}

fn account_of(found: &Found) -> Option<AccountId> {
    found.account_id.clone().map(AccountId::from_db)
}

/// `library_resume`: an agent pane in the session's folder whose first
/// start runs the engine's resume in the session's home.
pub async fn resume(state: &AppState, key: &str) -> Result<LibraryOpened, IpcError> {
    let found = find(state, key).await?;
    let engine_id = found.engine.id();
    handoff::check_target(state, engine_id)?;
    let (ws, tab, folder) = home_for(state, &found).await?;
    let account = account_of(&found);
    // A session from the engine's own home resumes as the user runs it
    // there: no account, no provider home, no hooks (D-2412).
    let own_home = account.is_none();
    let pane = create_pane_with(
        state,
        &tab,
        "agent",
        Some(folder),
        Some(engine_id.to_string()),
        account.clone(),
        !own_home,
    )
    .await?;
    let config = LaunchConfig {
        extra_args: Vec::new(),
        home: (!own_home).then(|| found.home.to_string_lossy().into_owned()),
        own_home,
    };
    sessions::start_with_config(
        &state.store,
        &pane.id,
        &ws.id,
        engine_id,
        account.as_ref(),
        &config,
    )
    .await?;
    sessions::set_engine_session(&state.store, &pane.id, &found.session_id).await?;
    state.terminals.resume_on_first_start(&pane.id);
    tracing::info!(
        code = "library.resume",
        engine = engine_id,
        "resuming a library session"
    );
    Ok(LibraryOpened {
        pane,
        workspace_id: ws.id,
        tab_id: tab,
    })
}

/// `library_brief`: the handoff fallback brief from the transcript.
pub async fn brief(state: &AppState, key: &str) -> Result<handoff::HandoffBrief, IpcError> {
    let items = transcript(state, key).await?;
    Ok(handoff::offered(
        handoff::fallback_brief(&library::as_text(&items)),
        handoff::BriefSource::Fallback,
    ))
}

/// `library_continue`: the target engine in the session's folder with the
/// brief as its first prompt.
pub async fn continue_with(
    state: &AppState,
    key: &str,
    engine_id: &str,
    account: Option<AccountId>,
    brief: &str,
) -> Result<LibraryOpened, IpcError> {
    let brief = brief.trim();
    if brief.is_empty() {
        return Err(IpcError::new(ErrorCode::Invalid, "Write a summary first"));
    }
    let found = find(state, key).await?;
    handoff::check_target(state, engine_id)?;
    let (ws, tab, folder) = home_for(state, &found).await?;
    let pane = handoff::open_target(state, &tab, folder, engine_id, account, brief).await?;
    tracing::info!(code = "library.continue", "continued a library session");
    Ok(LibraryOpened {
        pane,
        workspace_id: ws.id,
        tab_id: tab,
    })
}

async fn search_on(state: &AppState) -> bool {
    settings::get(&state.store, SettingKey::SessionSearch)
        .await
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn forbidden() -> IpcError {
    IpcError::new(
        ErrorCode::ForbiddenPlan,
        "Searching session text is turned off for this account.",
    )
}

/// `library_search_set`: on needs `session_search` in the limits; off
/// deletes the index.
pub async fn set_search(state: &AppState, on: bool) -> Result<(), IpcError> {
    if on && !state.entitlements.limits().session_search {
        return Err(forbidden());
    }
    settings::set(
        &state.store,
        SettingKey::SessionSearch,
        serde_json::Value::Bool(on),
    )
    .await?;
    if !on {
        delete_index(state.data.root());
    }
    tracing::info!(code = "library.search_set", on, "session search set");
    Ok(())
}

/// `library_search`: keys whose text matches, best first.
pub async fn search(state: &AppState, query: &str) -> Result<Vec<String>, IpcError> {
    if !state.entitlements.limits().session_search {
        return Err(forbidden());
    }
    if !search_on(state).await {
        return Err(IpcError::new(
            ErrorCode::Unavailable,
            "Turn on Search session text in Settings › Privacy first.",
        ));
    }
    let scan = scan_fresh(state, true).await?;
    let db = state.data.root().join(INDEX_FILE);
    let fs: Arc<dyn SessionFs> = Arc::new(RealFs);
    let build = db.clone();
    tokio::task::spawn_blocking(move || {
        // One writer at a time: a second search waits instead of failing
        // on a locked database.
        let _one = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        update_index(&build, fs.as_ref(), &scan.sessions)
    })
    .await
    .map_err(internal)?
    .map_err(|e| index_error("built", e))?;
    // Turned off while it was indexing: nothing stays behind.
    if !search_on(state).await {
        delete_index(state.data.root());
        return Err(IpcError::new(
            ErrorCode::Unavailable,
            "Search session text was turned off.",
        ));
    }
    let query = query.to_string();
    tokio::task::spawn_blocking(move || {
        let _one = INDEX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        search_index(&db, &query)
    })
    .await
    .map_err(internal)?
    .map_err(|e| index_error("read", e))
}

fn index_error(what: &str, e: rusqlite::Error) -> IpcError {
    IpcError::new(
        ErrorCode::Io,
        format!("The search index couldn't be {what}: {e}"),
    )
}

static INDEX_LOCK: Mutex<()> = Mutex::new(());

/// The index's format: an index from before secrets were redacted (or of
/// any other version) is deleted and built again.
const INDEX_VERSION: i64 = 2;

/// Removes the index and its journal files.
pub fn delete_index(data_root: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let path = data_root.join(format!("{INDEX_FILE}{suffix}"));
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!(code = "library.index_delete_failed", error = %e, "couldn't delete the session index");
            }
        }
    }
}

fn open_index(db: &Path) -> rusqlite::Result<Connection> {
    if db.exists() {
        let version: i64 = Connection::open(db)?
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        if version != INDEX_VERSION {
            if let Some(dir) = db.parent() {
                delete_index(dir);
            }
        }
    }
    let c = Connection::open(db)?;
    c.busy_timeout(Duration::from_secs(5))?;
    // Deleted rows are overwritten, not left in free pages.
    c.pragma_update(None, "secure_delete", "ON")?;
    c.pragma_update(None, "user_version", INDEX_VERSION)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(db, std::fs::Permissions::from_mode(0o600));
    }
    c.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS docs USING fts5(key UNINDEXED, body);
         CREATE TABLE IF NOT EXISTS files (key TEXT PRIMARY KEY, modified INTEGER NOT NULL, size INTEGER NOT NULL);",
    )?;
    Ok(c)
}

/// Brings the index up to date with `sessions`: new or changed files are
/// read again, gone ones dropped.
pub fn update_index(db: &Path, fs: &dyn SessionFs, sessions: &[Found]) -> rusqlite::Result<()> {
    let mut c = open_index(db)?;
    let t = c.transaction()?;
    let known: std::collections::HashMap<String, (i64, i64)> = {
        let mut stmt = t.prepare("SELECT key, modified, size FROM files")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?))))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut live = std::collections::HashSet::new();
    for s in sessions {
        let key = s.file.to_string_lossy().into_owned();
        live.insert(key.clone());
        let stamp = (s.modified_ms, s.size as i64);
        if known.get(&key) == Some(&stamp) {
            continue;
        }
        let Some(items) = library::transcript(fs, s.engine, &s.file) else {
            continue;
        };
        t.execute("DELETE FROM docs WHERE key = ?1", [&key])?;
        t.execute(
            "INSERT INTO docs (key, body) VALUES (?1, ?2)",
            // Already redacted as parsed (D-2416); once more as stored.
            params![key, tandem_core::redact::redact(&library::as_text(&items))],
        )?;
        t.execute(
            "INSERT OR REPLACE INTO files (key, modified, size) VALUES (?1, ?2, ?3)",
            params![key, stamp.0, stamp.1],
        )?;
    }
    for key in known.keys().filter(|k| !live.contains(*k)) {
        t.execute("DELETE FROM docs WHERE key = ?1", [key])?;
        t.execute("DELETE FROM files WHERE key = ?1", [key])?;
    }
    t.commit()
}

/// Keys matching every word of `query`, best first.
pub fn search_index(db: &Path, query: &str) -> rusqlite::Result<Vec<String>> {
    let Some(q) = tandem_store::repo::search::fts_query(query) else {
        return Ok(Vec::new());
    };
    let c = open_index(db)?;
    let mut stmt = c.prepare("SELECT key FROM docs WHERE docs MATCH ?1 ORDER BY rank LIMIT ?2")?;
    let rows = stmt.query_map(params![q, MAX_HITS], |r| r.get::<_, String>(0))?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_engines::library::FsEntry;

    /// One Claude Code session file on the real disk.
    fn claude_home(dir: &Path, name: &str, session: &str, cwd: &str, prompt: &str) -> PathBuf {
        let home = dir.join(name);
        let folder = home.join("projects").join("-src");
        std::fs::create_dir_all(&folder).expect("mkdir");
        let line = serde_json::json!({
            "type": "user", "sessionId": session, "cwd": cwd,
            "message": {"role": "user", "content": prompt}
        });
        std::fs::write(folder.join(format!("{session}.jsonl")), line.to_string()).expect("write");
        home
    }

    fn account(id: &str, engine: &str, home: &Path) -> accounts::Account {
        accounts::Account {
            id: AccountId::from_db(id.to_string()),
            engine_id: engine.into(),
            label: id.into(),
            home_path: home.to_string_lossy().into_owned(),
            is_default: false,
            plan_label: None,
            created_at: 0,
        }
    }

    #[test]
    fn f_th_17_1_every_account_home_and_each_engines_own_home_are_read() {
        let dir = tempfile::tempdir().expect("dir");
        let work = claude_home(dir.path(), "work", "aaaa-1111", "/src/shop", "Fix rounding");
        let home = claude_home(
            dir.path(),
            "home",
            "cccc-3333",
            "/src/blog",
            "Release notes",
        );
        let user = dir.path().join("user");
        let accts = vec![
            account("work", "claude", &work),
            account("home", "claude", &home),
        ];
        let srcs = sources(&accts, &user, |_| None);
        assert_eq!(srcs.len(), 4, "{srcs:?}");
        assert!(srcs
            .iter()
            .any(|s| s.account_id.is_none() && s.home == user.join(".claude")));
        assert!(srcs
            .iter()
            .any(|s| s.account_id.is_none() && s.home == user.join(".codex")));
        // CLAUDE_CONFIG_DIR in the login env names Claude Code's own home.
        let env = |k: &str| (k == "CLAUDE_CONFIG_DIR").then(|| work.to_string_lossy().into_owned());
        let srcs = sources(&accts, &user, env);
        assert_eq!(
            srcs.iter()
                .filter(|s| s.engine == LibraryEngine::Claude)
                .count(),
            2,
            "work isn't read twice"
        );
        let scan = library::scan(&RealFs, &srcs);
        let views: Vec<_> = scan.sessions.iter().map(view).collect();
        assert_eq!(views.len(), 2);
        let shop = views
            .iter()
            .find(|v| v.session_id == "aaaa-1111")
            .expect("shop");
        assert_eq!(shop.account_label.as_deref(), Some("work"));
        assert_eq!(shop.engine_id, "claude");
        assert!(!shop.folder_exists);
    }

    #[test]
    fn f_th_17_4_a_word_from_a_prompt_finds_its_session_and_off_deletes_the_index() {
        let dir = tempfile::tempdir().expect("dir");
        let work = claude_home(
            dir.path(),
            "work",
            "aaaa-1111",
            "/src/shop",
            "Fix the checkout rounding",
        );
        let home = claude_home(
            dir.path(),
            "home",
            "cccc-3333",
            "/src/blog",
            "Write the release notes",
        );
        let accts = vec![
            account("work", "claude", &work),
            account("home", "claude", &home),
        ];
        let scan = library::scan(&RealFs, &sources(&accts, &dir.path().join("u"), |_| None));
        let db = dir.path().join(INDEX_FILE);
        update_index(&db, &RealFs, &scan.sessions).expect("index");
        let hits = search_index(&db, "checkout").expect("search");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].ends_with("aaaa-1111.jsonl"));
        assert!(search_index(&db, "nothing-like-this")
            .expect("search")
            .is_empty());
        assert!(search_index(&db, "\" OR *").expect("no syntax").is_empty());
        // A gone file leaves the index.
        update_index(&db, &RealFs, &scan.sessions[..1]).expect("index");
        let all: Vec<String> = ["checkout", "release"]
            .iter()
            .flat_map(|q| search_index(&db, q).expect("search"))
            .collect();
        assert_eq!(all.len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&db).expect("db").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        delete_index(dir.path());
        assert!(!db.exists());
    }

    #[test]
    fn f_th_17_4_a_session_with_a_token_is_found_by_its_words_and_the_index_never_holds_the_token()
    {
        let token = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";
        let dir = tempfile::tempdir().expect("dir");
        let work = claude_home(
            dir.path(),
            "work",
            "aaaa-1111",
            "/src/shop",
            &format!("Rotate the deploy key {token} before Friday"),
        );
        let srcs = sources(
            &[account("work", "claude", &work)],
            &dir.path().join("u"),
            |_| None,
        );
        let scan = library::scan(&RealFs, &srcs);
        let db = dir.path().join(INDEX_FILE);
        update_index(&db, &RealFs, &scan.sessions).expect("index");
        assert_eq!(search_index(&db, "rotate deploy").expect("search").len(), 1);
        assert!(search_index(&db, token).expect("search").is_empty());
        // Nowhere in the database's files, journal included.
        for entry in std::fs::read_dir(dir.path()).expect("dir").flatten() {
            if entry.file_name().to_string_lossy().starts_with(INDEX_FILE) {
                let bytes = std::fs::read(entry.path()).expect("read");
                assert!(
                    !String::from_utf8_lossy(&bytes).contains(token),
                    "{:?}",
                    entry.path()
                );
            }
        }
        let shown = view(&scan.sessions[0]);
        assert!(!shown.title.unwrap_or_default().contains(token));
    }

    /// The index reads through the same file system as the listing.
    #[test]
    fn f_th_17_5_indexing_opens_only_the_session_files() {
        struct Recording(std::sync::Mutex<Vec<PathBuf>>);
        impl SessionFs for Recording {
            fn entries(&self, dir: &Path) -> Vec<FsEntry> {
                RealFs.entries(dir)
            }
            fn is_real_dir(&self, path: &Path) -> bool {
                RealFs.is_real_dir(path)
            }
            fn open(&self, path: &Path) -> std::io::Result<Box<dyn std::io::Read + Send>> {
                self.0.lock().expect("lock").push(path.to_path_buf());
                RealFs.open(path)
            }
        }
        let dir = tempfile::tempdir().expect("dir");
        let work = claude_home(dir.path(), "work", "aaaa-1111", "/src/shop", "Fix it");
        std::fs::write(work.join(".credentials.json"), "{}").expect("cred");
        let fs = Recording(std::sync::Mutex::new(Vec::new()));
        let srcs = sources(
            &[account("work", "claude", &work)],
            &dir.path().join("u"),
            |_| None,
        );
        let scan = library::scan(&fs, &srcs);
        update_index(&dir.path().join(INDEX_FILE), &fs, &scan.sessions).expect("index");
        let opened = fs.0.lock().expect("lock").clone();
        assert!(!opened.is_empty());
        assert!(
            opened
                .iter()
                .all(|p| p.extension().is_some_and(|e| e == "jsonl")),
            "{opened:?}"
        );
    }

    /// An index from before redaction (any other version) is deleted and
    /// built again, so a token it held doesn't survive.
    #[test]
    fn f_th_17_4_an_old_index_is_rebuilt_without_what_it_held() {
        let token = "sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";
        let dir = tempfile::tempdir().expect("dir");
        let db = dir.path().join(INDEX_FILE);
        {
            let c = Connection::open(&db).expect("open");
            c.execute_batch(&format!(
                "CREATE VIRTUAL TABLE docs USING fts5(key UNINDEXED, body);
                 INSERT INTO docs (key, body) VALUES ('/k', 'User: use {token}');
                 PRAGMA user_version = 1;"
            ))
            .expect("old index");
        }
        update_index(&db, &RealFs, &[]).expect("index");
        let bytes = std::fs::read(&db).expect("read");
        assert!(!String::from_utf8_lossy(&bytes).contains(token));
        let c = Connection::open(&db).expect("open");
        let v: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("version");
        assert_eq!(v, INDEX_VERSION);
        let secure: i64 = open_index(&db)
            .expect("open")
            .query_row("PRAGMA secure_delete", [], |r| r.get(0))
            .expect("secure_delete");
        assert_eq!(secure, 1);
    }
}
