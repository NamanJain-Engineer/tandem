//! The task tracker in the app (F-TK-01..03): its store at `$DATA/tasks/`,
//! and the `tandem-tasks` MCP server agents get (F-TK-02). Each engine
//! session gets its own connection, revoked when the session ends, so a
//! server left behind can't write any more. A session in a git working
//! tree can claim files there (F-TK-05); its edits claim them too.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tandem_core::contain;
use tandem_core::settings::SettingKey;
use tandem_core::DataDir;
use tandem_engines::chat::StdioServer;
use tandem_store::Store;
use tandem_tasks::store::claim_key;
use tandem_tasks::{Claim, ClaimOverlap, ClaimScope, TaskStore};

/// The name engines know the tasks server by (`mcp__tandem_tasks__…`).
pub const SERVER_NAME: &str = "tandem_tasks";
/// The env var a terminal agent's pane carries its connection in.
pub const CONNECTION_ENV: &str = "TANDEM_TASKS_CONNECTION";
/// Revoked connections are kept this long (diagnostics), then dropped.
const KEEP_REVOKED_MS: i64 = 7 * 24 * 60 * 60 * 1000;

pub struct TasksService {
    /// `None` when `$DATA/tasks` can't be opened (logged once).
    pub tracker: Option<TaskStore>,
    /// The `tandem-tasks` binary, when it ships next to the app.
    bin: Option<PathBuf>,
    store: Store,
    /// The live session per key (`th:<id>`, `pn:<id>`): its connection, if
    /// it has one, and its working tree.
    sessions: Mutex<HashMap<String, Live>>,
    /// The `issues/` and `claims.json` watcher, while it runs.
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    /// Overlaps already announced, by [`overlap_keys`].
    announced: Mutex<HashSet<String>>,
    /// Chats that get no tasks server, by session key: Lead's planning
    /// chats (F-TK-06), so nothing reaches the tracker before you approve.
    withheld: Mutex<HashSet<String>>,
    /// Held by a check from reading the claims to telling the sink, and by
    /// a session's start: one at a time (F-TK-05).
    checking: Mutex<()>,
    on_overlap: std::sync::OnceLock<OverlapSink>,
    on_notice: std::sync::OnceLock<NoticeSink>,
}

/// A session's connection and the working tree it claims in.
#[derive(Clone)]
struct Live {
    /// `None` while the session has no tasks server (a terminal agent with
    /// "Expose tasks to terminal agents" off, or no provider home of
    /// Vibezeo's): its edits still claim and its pane still gets notices.
    connection: Option<String>,
    scope: Option<ClaimScope>,
}

/// `(new, all)`: overlaps not announced before (only their new paths), and
/// every overlap there is now (F-TK-05 AC1).
pub type OverlapSink = Arc<dyn Fn(&[ClaimOverlap], &[ClaimOverlap]) + Send + Sync>;

/// `(pane, notices)`: a pane's session began, and these are the overlap
/// notices it's told first (F-TK-05 AC1).
pub type NoticeSink = Arc<dyn Fn(&str, &[String]) + Send + Sync>;

/// Where a session in `folder` claims: its working tree, the repository it
/// belongs to (a lane's worktree claims against its main checkout), and
/// where in the tree the session is. Both folders are resolved as
/// [`contain::canonical`] resolves them (`C:\…` on Windows, not `\\?\C:\…`),
/// as releases resolve a worktree's path; git says which is the main
/// checkout.
pub fn claim_scope(folder: &Path) -> Option<ClaimScope> {
    let repo = tandem_git::repo::Repo::discover(folder).ok()??;
    Some(ClaimScope {
        repo: resolved(&repo.main_root()),
        folder: resolved(repo.root()),
        prefix: repo.prefix().to_string_lossy().into_owned(),
        main: !repo.is_linked_worktree(),
    })
}

/// A folder as claims name it: [`contain::canonical`], else as given.
fn resolved(path: &Path) -> String {
    contain::canonical(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// An edited file's path relative to the working tree, or `None` outside
/// it, and in git's data (`.git`) or Vibezeo's (`.tandem`), compared as file
/// names compare here.
pub fn claim_path(folder: &Path, edited: &Path) -> Option<String> {
    let rel = if edited.is_relative() {
        edited.to_path_buf()
    } else if let Ok(r) = edited.strip_prefix(folder) {
        r.to_path_buf()
    } else {
        // A new file in new folders, a symlinked path (macOS /tmp) or
        // Windows' `\\?\`: the deepest folder that exists resolves.
        contain::canonical_existing(edited)?
            .strip_prefix(folder)
            .ok()?
            .to_path_buf()
    };
    let rel = tandem_tasks::store::normalize_relative_claim_path(&rel.to_string_lossy()).ok()?;
    let top = contain::fold(rel.split('/').next().unwrap_or_default());
    (top != ".git" && top != ".tandem").then_some(rel)
}

/// One key per overlapping path, so a pair is announced again only for a
/// path it didn't overlap on before. Paths key as claims compare
/// ([`claim_key`]), so one file spelled another way isn't new.
fn overlap_keys(o: &ClaimOverlap) -> impl Iterator<Item = (String, &str)> + '_ {
    o.paths.iter().map(move |p| {
        (
            format!(
                "{}\0{}\0{}\0{}",
                o.repo,
                o.folders[0],
                o.folders[1],
                claim_key(p)
            ),
            p.as_str(),
        )
    })
}

/// The overlaps in `all` with paths not in `announced` (recorded now);
/// keys of overlaps that ended are forgotten, so a later one is new again.
pub fn fresh_overlaps(announced: &mut HashSet<String>, all: &[ClaimOverlap]) -> Vec<ClaimOverlap> {
    let mut now = HashSet::new();
    let mut out = Vec::new();
    for o in all {
        let new: Vec<String> = overlap_keys(o)
            .filter_map(|(k, p)| {
                let was = announced.contains(&k);
                now.insert(k);
                (!was).then(|| p.to_string())
            })
            .collect();
        if !new.is_empty() {
            out.push(ClaimOverlap {
                paths: new,
                ..o.clone()
            });
        }
    }
    *announced = now;
    out
}

/// The overlaps in `all` that `folder` is in, with only their paths in
/// `announced`: what a session beginning there missed. Paths not announced
/// yet reach it from the next check, as they reach everyone.
pub fn announced_overlaps(
    announced: &HashSet<String>,
    all: &[ClaimOverlap],
    folder: &str,
) -> Vec<ClaimOverlap> {
    all.iter()
        .filter(|o| o.folders.iter().any(|f| f == folder))
        .filter_map(|o| {
            let paths: Vec<String> = overlap_keys(o)
                .filter(|(k, _)| announced.contains(k))
                .map(|(_, p)| p.to_string())
                .collect();
            (!paths.is_empty()).then(|| ClaimOverlap { paths, ..o.clone() })
        })
        .collect()
}

/// "add-oauth and fix-auth both claim src/auth/session.ts" (up to 3 paths).
pub fn overlap_notice(o: &ClaimOverlap) -> String {
    let shown: Vec<&str> = o.paths.iter().take(3).map(String::as_str).collect();
    let more = o.paths.len().saturating_sub(shown.len());
    let tail = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    format!(
        "{} and {} both claim {}{tail}",
        o.agents[0],
        o.agents[1],
        shown.join(", ")
    )
}

/// A claim nothing will release after a restart (F-TK-05): the main
/// checkout's, or a folder's that's gone. `main` is the only test of the
/// main checkout: a worktree of a repository whose git data lives elsewhere
/// (`--separate-git-dir`, bare) has itself as its repository too. No build
/// that wrote claims without `main` has shipped.
fn stale_at_start(c: &Claim) -> bool {
    c.main || !Path::new(&c.folder).is_dir()
}

/// How long changes are gathered before `tasks:changed` goes out.
pub const WATCH_BATCH: std::time::Duration = std::time::Duration::from_millis(120);

/// `ISSUE-12.json` (or its history and comments) → `TAN-12`.
pub fn issue_of(path: &std::path::Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let n = name.strip_prefix("ISSUE-")?;
    let digits: String = n.chars().take_while(char::is_ascii_digit).collect();
    let rest = &n[digits.len()..];
    let known = [".json", ".history.jsonl", ".comments.jsonl"];
    (!digits.is_empty() && known.contains(&rest)).then(|| format!("TAN-{digits}"))
}

/// `tandem-tasks` next to the app binary (`TANDEM_TASKS_BIN` in debug
/// builds), like `tandem-hook` (D-68).
pub fn tasks_bin() -> Option<PathBuf> {
    let dev = if cfg!(debug_assertions) {
        std::env::var_os("TANDEM_TASKS_BIN").map(PathBuf::from)
    } else {
        None
    };
    tasks_bin_from(dev, &std::env::current_exe().ok()?)
}

/// The sidecar's file name: what Tauri installs from `bundle.externalBin`
/// (`binaries/tandem-tasks-<target>`) without the target suffix.
pub const TASKS_BIN_NAME: &str = if cfg!(windows) {
    "tandem-tasks.exe"
} else {
    "tandem-tasks"
};

/// The dev override, else the bundled sidecar beside the app's executable.
pub fn tasks_bin_from(dev: Option<PathBuf>, exe: &std::path::Path) -> Option<PathBuf> {
    if let Some(p) = dev {
        return Some(p);
    }
    let p = exe.parent()?.join(TASKS_BIN_NAME);
    p.is_file().then_some(p)
}

impl TasksService {
    pub fn new(data: &DataDir, store: Store, bin: Option<PathBuf>) -> Arc<Self> {
        let tracker = match TaskStore::open(data.tasks()) {
            Ok(t) => {
                // Every session of an earlier run has ended: its connections
                // go (a stray server from a crash can't write any more).
                if let Ok(conns) = t.connections() {
                    for c in conns.iter().filter(|c| c.revoked_at.is_none()) {
                        let _ = t.revoke_connection(&c.id);
                    }
                }
                let _ = t.prune_connections(KEEP_REVOKED_MS);
                // So did the main checkouts' claims with their last sessions
                // (D-2427), and a worktree deleted meanwhile keeps none.
                if let Err(e) = t.prune_claims(|c| !stale_at_start(c)) {
                    tracing::warn!(code = "tasks.prune_failed", error = %e, "old claims weren't dropped");
                }
                Some(t)
            }
            Err(e) => {
                tracing::warn!(code = "tasks.open_failed", error = %e, "the task store didn't open");
                None
            }
        };
        Arc::new(Self {
            tracker,
            bin,
            store,
            sessions: Mutex::default(),
            watcher: Mutex::default(),
            announced: Mutex::default(),
            withheld: Mutex::default(),
            checking: Mutex::default(),
            on_overlap: std::sync::OnceLock::new(),
            on_notice: std::sync::OnceLock::new(),
        })
    }

    /// Told of claim overlaps as they change (set once, at startup).
    pub fn set_overlap_sink(&self, sink: OverlapSink) {
        let _ = self.on_overlap.set(sink);
    }

    /// Told what a pane hears as its session begins (set once, at startup).
    pub fn set_notice_sink(&self, sink: NoticeSink) {
        let _ = self.on_notice.set(sink);
    }

    /// The session `key` begins in `folder`, with or without a connection
    /// (F-TK-05): its edits claim there, and a pane gets the overlap notices
    /// of its working tree, the ones announced before it began included
    /// (AC1). The key's previous connection is revoked. Blocking file IO.
    pub fn track_scope(&self, key: &str, folder: Option<&Path>) {
        self.track(key, folder.and_then(claim_scope));
    }

    fn track(&self, key: &str, scope: Option<ClaimScope>) {
        let Some(tracker) = &self.tracker else {
            return;
        };
        // In turn with the checks: the pane hears each overlap once, either
        // here or from the next check.
        let turn = self.checking.lock().unwrap_or_else(|e| e.into_inner());
        let previous = match self.sessions.lock() {
            Ok(mut m) => m.insert(
                key.to_string(),
                Live {
                    connection: None,
                    scope: scope.clone(),
                },
            ),
            Err(_) => return,
        };
        if let (Some(pane), Some(sink)) = (key.strip_prefix("pn:"), self.on_notice.get()) {
            let missed = match &scope {
                Some(s) => self.announced_in(tracker, &s.folder),
                None => Vec::new(),
            };
            let notices: Vec<String> = missed.iter().map(overlap_notice).collect();
            sink(pane, &notices);
        }
        drop(turn);
        if let Some(old) = previous.and_then(|l| l.connection) {
            let _ = tracker.revoke_connection(&old);
        }
    }

    /// The overlaps announced so far that `folder` is in. Under `checking`.
    fn announced_in(&self, tracker: &TaskStore, folder: &str) -> Vec<ClaimOverlap> {
        let all = match tracker.claim_overlaps() {
            Ok(all) => all,
            Err(e) => {
                tracing::warn!(code = "tasks.claims_unreadable", error = %e, "claims couldn't be read");
                return Vec::new();
            }
        };
        match self.announced.lock() {
            Ok(a) => announced_overlaps(&a, &all, folder),
            Err(_) => Vec::new(),
        }
    }

    /// Reads the claims and tells the sink about new overlaps. Checks take
    /// turns (the startup one and the watcher's can meet).
    pub fn check_overlaps(&self) {
        self.check_with(TaskStore::claim_overlaps);
    }

    /// [`Self::check_overlaps`], with the claims read by `read` (tests slow
    /// it down).
    fn check_with(
        &self,
        read: impl FnOnce(&TaskStore) -> Result<Vec<ClaimOverlap>, tandem_tasks::TaskError>,
    ) {
        let (Some(tracker), Some(sink)) = (&self.tracker, self.on_overlap.get()) else {
            return;
        };
        // From the read to the sink: a check that read older claims could
        // forget a newer check's overlaps (announcing them again later) and
        // deliver its older list last.
        let _turn = self.checking.lock().unwrap_or_else(|e| e.into_inner());
        let all = match read(tracker) {
            Ok(all) => all,
            Err(e) => {
                tracing::warn!(code = "tasks.claims_unreadable", error = %e, "claims couldn't be read");
                return;
            }
        };
        let new = match self.announced.lock() {
            Ok(mut a) => fresh_overlaps(&mut a, &all),
            Err(_) => return,
        };
        sink(&new, &all);
    }

    /// The panes whose agents work in `folder`.
    pub fn panes_in(&self, folder: &str) -> Vec<String> {
        let Ok(sessions) = self.sessions.lock() else {
            return Vec::new();
        };
        sessions
            .iter()
            .filter(|(_, l)| l.scope.as_ref().is_some_and(|s| s.folder == folder))
            .filter_map(|(k, _)| k.strip_prefix("pn:").map(str::to_string))
            .collect()
    }

    /// A pane's agent is about to edit `paths`: its working tree claims
    /// them (F-TK-05 AC2). Blocking file IO.
    pub fn claim_edits(&self, pane: &str, paths: &[String]) {
        let Some(tracker) = &self.tracker else {
            return;
        };
        let scope = self
            .sessions
            .lock()
            .ok()
            .and_then(|m| m.get(&format!("pn:{pane}")).and_then(|l| l.scope.clone()));
        let Some(scope) = scope else {
            return;
        };
        let folder = Path::new(&scope.folder);
        let rel: Vec<String> = paths
            .iter()
            .filter_map(|p| claim_path(folder, Path::new(p)))
            .collect();
        if rel.is_empty() {
            return;
        }
        if let Err(e) = tracker.claim_inferred(&scope, &rel) {
            tracing::warn!(code = "tasks.claim_failed", error = %e, "an edit wasn't claimed");
        }
    }

    /// A workspace or worktree at `root` left the app (F-TK-05 AC3): the
    /// claims of its working tree go, unless that's a main checkout, whose
    /// claims go with its last session (D-2427). Blocking file IO.
    pub fn release_removed(&self, root: &Path) {
        if let Some(scope) = claim_scope(root).filter(|s| !s.main) {
            self.release_folder(Path::new(&scope.folder));
        }
    }

    /// Claims of folders that are gone (a worktree deleted outside the app)
    /// go, since nothing would release them (F-TK-05). Blocking file IO.
    pub fn prune_gone(&self) {
        let Some(tracker) = &self.tracker else {
            return;
        };
        if let Err(e) = tracker.prune_claims(|c| Path::new(&c.folder).is_dir()) {
            tracing::warn!(code = "tasks.prune_failed", error = %e, "gone worktrees' claims weren't dropped");
        }
    }

    /// A worktree was merged or removed: its claims go (F-TK-05 AC3).
    pub fn release_folder(&self, folder: &Path) {
        let Some(tracker) = &self.tracker else {
            return;
        };
        if let Err(e) = tracker.release_folder(&folder.to_string_lossy()) {
            tracing::warn!(code = "tasks.release_failed", error = %e, "a worktree's claims weren't released");
        }
    }

    /// Watches `issues/` and publishes `tasks:changed {ids}` within
    /// [`WATCH_BATCH`] of a change by anyone: the UI, an MCP server, another
    /// process (F-TK-03, "within 500 ms").
    pub fn watch(self: &Arc<Self>, bus: tandem_core::bus::EventBus) {
        use notify::Watcher;
        let Some(tracker) = &self.tracker else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel::<Vec<String>>();
        let (claims_tx, claims_rx) = std::sync::mpsc::channel::<()>();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                let ids: Vec<String> = ev.paths.iter().filter_map(|p| issue_of(p)).collect();
                if !ids.is_empty() {
                    let _ = tx.send(ids);
                }
                if ev
                    .paths
                    .iter()
                    .any(|p| p.file_name().is_some_and(|n| n == "claims.json"))
                {
                    let _ = claims_tx.send(());
                }
            }
        });
        let mut watcher = match watcher {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(code = "tasks.watch_failed", error = %e, "tasks won't refresh by themselves");
                return;
            }
        };
        if let Err(e) = watcher.watch(&tracker.issues_dir(), notify::RecursiveMode::NonRecursive) {
            tracing::warn!(code = "tasks.watch_failed", error = %e, "tasks won't refresh by themselves");
            return;
        }
        // Claims change in any session's server process (F-TK-05).
        if let Err(e) = watcher.watch(tracker.dir(), notify::RecursiveMode::NonRecursive) {
            tracing::warn!(code = "tasks.watch_failed", error = %e, "claim overlaps won't be noticed");
        }
        std::thread::spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut ids = first;
                let until = std::time::Instant::now() + WATCH_BATCH;
                while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
                    match rx.recv_timeout(left) {
                        Ok(more) => ids.extend(more),
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                ids.sort();
                ids.dedup();
                bus.publish(tandem_core::bus::BusEvent::new(
                    tandem_core::bus::Topic::Tasks,
                    "tasks:changed",
                    serde_json::json!({ "ids": ids }),
                ));
            }
        });
        let me = Arc::downgrade(self);
        std::thread::spawn(move || {
            while claims_rx.recv().is_ok() {
                // A burst of writes is read once.
                std::thread::sleep(WATCH_BATCH);
                while claims_rx.try_recv().is_ok() {}
                let Some(me) = me.upgrade() else {
                    return;
                };
                me.check_overlaps();
            }
        });
        if let Ok(mut w) = self.watcher.lock() {
            *w = Some(watcher);
        }
    }

    /// A new connection for the session `key`; the session's previous one
    /// is revoked. A terminal pane's launch began the session with
    /// [`Self::track_scope`] already, so here it only gets its connection;
    /// any other session begins here.
    pub fn begin_session(
        &self,
        key: &str,
        label: &str,
        actor: &str,
        folder: Option<&Path>,
    ) -> Option<String> {
        let tracker = self.tracker.as_ref()?;
        let scope = folder.and_then(claim_scope);
        let tracked = self.sessions.lock().ok().is_some_and(|m| {
            m.get(key)
                .is_some_and(|l| l.connection.is_none() && l.scope == scope)
        });
        if !tracked {
            self.track(key, scope.clone());
        }
        let conn = match tracker.create_connection_in(label, actor, scope.clone()) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(code = "tasks.connection_failed", error = %e, "no task connection for the session");
                return None;
            }
        };
        let previous = self.sessions.lock().ok().and_then(|mut m| {
            m.insert(
                key.to_string(),
                Live {
                    connection: Some(conn.id.clone()),
                    scope,
                },
            )
        });
        // Another start for the key in the meantime: its connection goes.
        if let Some(old) = previous.and_then(|l| l.connection) {
            let _ = tracker.revoke_connection(&old);
        }
        Some(conn.id)
    }

    /// The session ended: its connection is revoked (F-TK-02 AC2).
    pub fn end_session(&self, key: &str) {
        let Some(tracker) = &self.tracker else {
            return;
        };
        let (ended, others_there) = match self.sessions.lock() {
            Ok(mut m) => {
                let ended = m.remove(key);
                let folder = ended
                    .as_ref()
                    .and_then(|l| l.scope.as_ref())
                    .map(|s| &s.folder);
                let others = folder.is_some_and(|f| {
                    m.values()
                        .any(|l| l.scope.as_ref().is_some_and(|s| &s.folder == f))
                });
                (ended, others)
            }
            Err(_) => return,
        };
        let Some(ended) = ended else {
            return;
        };
        if let Some(conn) = &ended.connection {
            let _ = tracker.revoke_connection(conn);
        }
        // A lane's claims last until it's merged or removed; the main
        // checkout's go with its last session (D-2427).
        if let Some(scope) = ended.scope {
            if scope.main && !others_there {
                let _ = tracker.release_folder(&scope.folder);
            }
        }
    }

    /// The session's live connection, when it has one.
    pub fn connection_of(&self, key: &str) -> Option<String> {
        self.sessions.lock().ok()?.get(key)?.connection.clone()
    }

    fn server(&self, connection_args: [String; 2]) -> Option<StdioServer> {
        let tracker = self.tracker.as_ref()?;
        let bin = self.bin.clone()?;
        let [flag, value] = connection_args;
        let env_vars = if flag == "--connection-env" {
            vec![value.clone()]
        } else {
            Vec::new()
        };
        Some(StdioServer {
            name: SERVER_NAME.into(),
            command: bin,
            args: vec![
                "mcp".into(),
                "--dir".into(),
                tracker.dir().to_string_lossy().into_owned(),
                flag,
                value,
            ],
            env_vars,
        })
    }

    /// The tasks server for a chat session, with a fresh connection.
    pub fn chat_server(
        &self,
        key: &str,
        label: &str,
        actor: &str,
        folder: Option<&Path>,
    ) -> Option<StdioServer> {
        if self
            .withheld
            .lock()
            .map(|w| w.contains(key))
            .unwrap_or(true)
        {
            return None;
        }
        self.bin.as_ref()?;
        let conn = self.begin_session(key, label, actor, folder)?;
        self.server(["--connection".into(), conn])
    }

    /// The chat's sessions get no tasks server from now on (`th:<id>`).
    pub fn withhold(&self, key: &str) {
        if let Ok(mut w) = self.withheld.lock() {
            w.insert(key.to_string());
        }
    }

    /// The server entry for terminal engines' own config: the connection
    /// comes from each pane's env.
    pub fn terminal_server(&self) -> Option<StdioServer> {
        self.server(["--connection-env".into(), CONNECTION_ENV.into()])
    }

    /// "Expose tasks to terminal agents" (default on).
    pub async fn expose_to_terminals(&self) -> bool {
        tandem_store::repo::settings::get(&self.store, SettingKey::ExposeTasksToTerminalAgents)
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn f_tk_02_the_bundled_server_is_found_beside_the_apps_executable() {
        let dir = tempfile::tempdir().expect("dir");
        let exe = dir.path().join("tandem");
        assert_eq!(tasks_bin_from(None, &exe), None, "not bundled: none");
        std::fs::write(dir.path().join(TASKS_BIN_NAME), b"").expect("sidecar");
        assert_eq!(
            tasks_bin_from(None, &exe),
            Some(dir.path().join(TASKS_BIN_NAME))
        );
        // The dev override wins.
        let dev = std::path::PathBuf::from("/dev/tandem-tasks");
        assert_eq!(tasks_bin_from(Some(dev.clone()), &exe), Some(dev));
    }

    #[test]
    fn f_tk_02_release_builds_bundle_the_tasks_server_as_a_sidecar() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let conf: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join("tauri.sidecars.conf.json")).expect("conf"),
        )
        .expect("json");
        // Beside tandem-hook since D-828 (services::engines checks both).
        let bins = conf["bundle"]["externalBin"]
            .as_array()
            .expect("externalBin");
        assert!(
            bins.iter().any(|b| b == "binaries/tandem-tasks"),
            "{bins:?}"
        );
        // Both release paths build the sidecar and pass that config.
        let repo = root.join("../../..");
        for script in ["scripts/release-local.sh", ".github/workflows/release.yml"] {
            let text = std::fs::read_to_string(repo.join(script)).expect(script);
            assert!(text.contains("build-sidecars.sh"), "{script}");
            assert!(
                text.contains("--config src-tauri/tauri.sidecars.conf.json"),
                "{script}"
            );
        }
    }
    use super::*;

    async fn service(bin: Option<PathBuf>) -> (tempfile::TempDir, Arc<TasksService>) {
        let dir = tempfile::tempdir().expect("dir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        (dir, TasksService::new(&data, store, bin))
    }

    #[tokio::test]
    async fn f_tk_02_2_each_session_gets_a_connection_revoked_when_it_ends() {
        let (_d, tasks) = service(Some("/opt/v/tandem-tasks".into())).await;
        let tracker = tasks.tracker.clone().expect("tracker");
        let server = tasks
            .chat_server("th:1", "Thread", "me", None)
            .expect("server");
        assert_eq!(server.name, "tandem_tasks");
        let conn = tasks.connection_of("th:1").expect("conn");
        assert_eq!(server.args.last(), Some(&conn));
        assert!(tracker.connection(&conn).is_ok());
        // A new session for the same chat revokes the last one.
        tasks
            .chat_server("th:1", "Thread", "me", None)
            .expect("again");
        let next = tasks.connection_of("th:1").expect("conn");
        assert_ne!(next, conn);
        assert!(tracker.connection(&conn).is_err());
        tasks.end_session("th:1");
        assert!(matches!(
            tracker.connection(&next),
            Err(tandem_tasks::TaskError::ConnectionRevoked)
        ));
        // Terminal agents read their connection from the pane's env.
        let t = tasks.terminal_server().expect("terminal");
        assert_eq!(t.env_vars, vec![CONNECTION_ENV.to_string()]);
        assert!(t.args.contains(&"--connection-env".to_string()));
        assert!(tasks.expose_to_terminals().await, "on by default");
    }

    #[tokio::test]
    async fn f_tk_06_lead_planning_gets_no_tasks_server() {
        let (_d, tasks) = service(Some("/opt/v/tandem-tasks".into())).await;
        tasks.withhold("th:plan");
        assert!(tasks
            .chat_server("th:plan", "Lead", "tm_lead", None)
            .is_none());
        assert!(
            tasks.connection_of("th:plan").is_none(),
            "no connection made"
        );
        assert!(tasks
            .chat_server("th:other", "Lead", "tm_lead", None)
            .is_some());
    }

    #[tokio::test]
    async fn f_tk_02_without_the_binary_no_server_is_offered() {
        let (_d, tasks) = service(None).await;
        assert!(tasks.chat_server("th:1", "Thread", "me", None).is_none());
        assert!(tasks.connection_of("th:1").is_none(), "no connection made");
    }

    #[tokio::test]
    async fn f_tk_02_a_restart_revokes_the_last_runs_connections() {
        let dir = tempfile::tempdir().expect("dir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let first = TasksService::new(&data, store.clone(), Some("/x".into()));
        first
            .chat_server("th:1", "Thread", "me", None)
            .expect("server");
        let conn = first.connection_of("th:1").expect("conn");
        let second = TasksService::new(&data, store, Some("/x".into()));
        assert!(second
            .tracker
            .as_ref()
            .expect("tracker")
            .connection(&conn)
            .is_err());
    }

    fn git_repo(dir: &std::path::Path) -> std::path::PathBuf {
        let root = dir.join("app");
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .expect("git")
            .success();
        assert!(ok, "git init");
        std::fs::canonicalize(root).expect("canonical")
    }

    #[test]
    fn f_tk_05_edited_paths_are_relative_to_the_working_tree() {
        let dir = tempfile::tempdir().expect("dir");
        let root = git_repo(dir.path());
        let scope = claim_scope(&root.join("src")).expect("scope");
        assert_eq!(scope.folder, root.to_string_lossy());
        assert_eq!(
            scope.repo, scope.folder,
            "the main checkout is its own repo"
        );
        assert_eq!(
            claim_path(&root, &root.join("src/a.ts")).as_deref(),
            Some("src/a.ts")
        );
        assert_eq!(
            claim_path(&root, &root.join("src/new/b.ts")).as_deref(),
            Some("src/new/b.ts"),
            "a new file in a new folder"
        );
        assert_eq!(claim_path(&root, Path::new("/elsewhere/a.ts")), None);
        assert_eq!(claim_path(&root, &root.join(".git/config")), None);
        assert_eq!(
            claim_path(&root, Path::new("src/c.ts")).as_deref(),
            Some("src/c.ts")
        );
        assert!(claim_scope(&dir.path().join("nowhere")).is_none());
    }

    fn overlap(paths: &[&str]) -> ClaimOverlap {
        ClaimOverlap {
            repo: "/r".into(),
            folders: [
                "/r/.tandem/worktrees/add-oauth".into(),
                "/r/.tandem/worktrees/fix-auth".into(),
            ],
            agents: ["add-oauth".into(), "fix-auth".into()],
            paths: paths.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn f_tk_05_1_an_overlap_is_announced_once_and_again_after_it_ended() {
        let mut seen = HashSet::new();
        let first = fresh_overlaps(&mut seen, &[overlap(&["a.ts"])]);
        assert_eq!(first, vec![overlap(&["a.ts"])]);
        assert!(fresh_overlaps(&mut seen, &[overlap(&["a.ts"])]).is_empty());
        let more = fresh_overlaps(&mut seen, &[overlap(&["a.ts", "b.ts"])]);
        assert_eq!(more, vec![overlap(&["b.ts"])], "only the new path");
        assert!(fresh_overlaps(&mut seen, &[]).is_empty());
        assert_eq!(fresh_overlaps(&mut seen, &[overlap(&["a.ts"])]).len(), 1);
    }

    #[test]
    fn f_tk_05_1_the_notice_names_both_worktrees_and_the_paths() {
        assert_eq!(
            overlap_notice(&overlap(&["src/auth/session.ts"])),
            "add-oauth and fix-auth both claim src/auth/session.ts"
        );
        assert_eq!(
            overlap_notice(&overlap(&["a", "b", "c", "d", "e"])),
            "add-oauth and fix-auth both claim a, b, c and 2 more"
        );
    }

    #[tokio::test]
    async fn f_tk_05_2_a_panes_edit_claims_its_file_and_overlaps_reach_the_sink() {
        let (_d, tasks) = service(Some("/x".into())).await;
        let dir = tempfile::tempdir().expect("dir");
        let root = git_repo(dir.path());
        let seen: Arc<Mutex<Vec<(usize, usize)>>> = Arc::default();
        let sink = seen.clone();
        tasks.set_overlap_sink(Arc::new(move |new, all| {
            sink.lock().expect("seen").push((new.len(), all.len()));
        }));
        tasks
            .begin_session("pn:p1", "Claude Code", "me", Some(&root))
            .expect("p1");
        tasks.claim_edits(
            "p1",
            &[root.join("src/a.ts").to_string_lossy().into_owned()],
        );
        let tracker = tasks.tracker.as_ref().expect("tracker");
        let claims = tracker.claims().expect("claims");
        assert_eq!(claims.len(), 1);
        assert!(claims[0].inferred);
        assert_eq!(claims[0].path, "src/a.ts");
        assert_eq!(
            tasks.panes_in(&root.to_string_lossy()),
            vec!["p1".to_string()]
        );
        // Another working tree of the repository edits the same file.
        tracker
            .claim_inferred(
                &ClaimScope {
                    repo: root.to_string_lossy().into_owned(),
                    folder: format!("{}/other", root.to_string_lossy()),
                    prefix: String::new(),
                    main: false,
                },
                &["src/a.ts".into()],
            )
            .expect("other");
        tasks.check_overlaps();
        tasks.check_overlaps();
        assert_eq!(*seen.lock().expect("seen"), vec![(1, 1), (0, 1)]);
        // The main checkout's last session ending releases its claims.
        tasks.end_session("pn:p1");
        assert!(tracker
            .claims()
            .expect("claims")
            .iter()
            .all(|c| c.folder != root.to_string_lossy()));
    }

    #[test]
    fn f_tk_03_issue_files_map_to_their_ids() {
        use std::path::Path;
        assert_eq!(
            issue_of(Path::new("/t/issues/ISSUE-12.json")).as_deref(),
            Some("TAN-12")
        );
        assert_eq!(
            issue_of(Path::new("ISSUE-3.history.jsonl")).as_deref(),
            Some("TAN-3")
        );
        assert_eq!(issue_of(Path::new(".ISSUE-3.json.1.0.tmp")), None);
        assert_eq!(issue_of(Path::new("ISSUE-.json")), None);
        assert_eq!(issue_of(Path::new("meta.json")), None);
    }

    #[tokio::test]
    async fn f_tk_03_a_write_by_anyone_is_announced_within_500_ms() {
        let (_d, tasks) = service(None).await;
        let bus = tandem_core::bus::EventBus::new();
        let mut rx = bus.subscribe(tandem_core::bus::Topic::Tasks);
        tasks.watch(bus.clone());
        let tracker = tasks.tracker.clone().expect("tracker");
        // Another process's write looks the same: a file in issues/.
        let started = std::time::Instant::now();
        tracker
            .save_issue(
                tandem_tasks::SaveIssue {
                    title: Some("Agent wrote this".into()),
                    ..Default::default()
                },
                &tandem_tasks::Writer::agent("me", "tc_1"),
            )
            .expect("save");
        let ev = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
            .await
            .expect("within 500 ms")
            .expect("event");
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        assert_eq!(ev.name, "tasks:changed");
        assert_eq!(ev.payload["ids"], serde_json::json!(["TAN-1"]));
    }

    /// `app` with one commit, and its worktree `fix-auth`, as a lane's:
    /// `(main checkout, worktree)`, resolved.
    fn repo_with_lane(dir: &Path) -> (PathBuf, PathBuf) {
        let main = git_repo(dir);
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(&main)
                .status()
                .expect("git")
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ]);
        let lane = main.join(".tandem/worktrees/fix-auth");
        git(&[
            "worktree",
            "add",
            "-q",
            lane.to_str().expect("utf8"),
            "-b",
            "tandem/fix-auth",
        ]);
        let lane = std::fs::canonicalize(lane).expect("canonical");
        (main, lane)
    }

    /// What a sink was given, in order.
    type Seen<T> = Arc<Mutex<Vec<T>>>;

    /// `(pane, notice)` for each new overlap, routed as the app routes them
    /// (`lib.rs`): to the panes of both working trees.
    fn route_overlaps(tasks: &Arc<TasksService>) -> Seen<(String, String)> {
        let routed: Seen<(String, String)> = Arc::default();
        let (seen, weak) = (routed.clone(), Arc::downgrade(tasks));
        tasks.set_overlap_sink(Arc::new(move |new, _all| {
            let Some(tasks) = weak.upgrade() else {
                return;
            };
            for o in new {
                for folder in &o.folders {
                    for pane in tasks.panes_in(folder) {
                        seen.lock().expect("routed").push((pane, overlap_notice(o)));
                    }
                }
            }
        }));
        routed
    }

    #[tokio::test]
    async fn f_tk_05_1_concurrent_checks_announce_each_overlap_once() {
        let (_d, tasks) = service(None).await;
        let dir = tempfile::tempdir().expect("dir");
        let (main, lane) = repo_with_lane(dir.path());
        // (new paths, how many overlaps there are) per delivery.
        let lists: Seen<(Vec<String>, usize)> = Arc::default();
        let sink = lists.clone();
        tasks.set_overlap_sink(Arc::new(move |new, all| {
            let paths = new.iter().flat_map(|o| o.paths.clone()).collect();
            sink.lock().expect("lists").push((paths, all.len()));
        }));
        let tracker = tasks.tracker.clone().expect("tracker");
        let edit = |root: &Path| {
            tracker
                .claim_inferred(&claim_scope(root).expect("scope"), &["src/a.ts".into()])
                .expect("claim");
        };
        edit(&main);
        // A check (at startup, say) reads the claims before they overlap, then
        // is slow to go on.
        let (read_tx, read_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let slow = {
            let tasks = tasks.clone();
            std::thread::spawn(move || {
                tasks.check_with(|t| {
                    let all = t.claim_overlaps();
                    let _ = read_tx.send(());
                    let _ = go_rx.recv();
                    all
                })
            })
        };
        read_rx.recv().expect("read");
        edit(&lane);
        // The watcher's check of the overlapping claims, meanwhile.
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let quick = {
            let tasks = tasks.clone();
            std::thread::spawn(move || {
                tasks.check_overlaps();
                let _ = done_tx.send(());
            })
        };
        // It finishes now unless checks take turns; then it waits for the slow one.
        let _ = done_rx.recv_timeout(std::time::Duration::from_millis(300));
        go_tx.send(()).expect("go");
        slow.join().expect("slow");
        quick.join().expect("quick");
        // A later check.
        tasks.check_overlaps();
        let lists = lists.lock().expect("lists").clone();
        let announced = lists.iter().filter(|(new, _)| !new.is_empty()).count();
        assert_eq!(announced, 1, "announced once: {lists:?}");
        assert_eq!(
            lists.iter().map(|(_, all)| *all).collect::<Vec<_>>(),
            [0, 1, 1],
            "an older list is never delivered last: {lists:?}"
        );
    }

    #[tokio::test]
    async fn f_tk_05_2_with_tasks_not_exposed_edits_still_claim_and_reach_the_pane() {
        let (_d, tasks) = service(Some("/x".into())).await;
        let dir = tempfile::tempdir().expect("dir");
        let (main, lane) = repo_with_lane(dir.path());
        let routed = route_overlaps(&tasks);
        // "Expose tasks to terminal agents" is off: the panes' sessions are
        // tracked, and get no connection.
        tasks.track_scope("pn:p1", Some(&main));
        tasks.track_scope("pn:p2", Some(&lane));
        assert_eq!(tasks.connection_of("pn:p1"), None);
        for (pane, root) in [("p1", &main), ("p2", &lane)] {
            tasks.claim_edits(
                pane,
                &[root.join("src/a.ts").to_string_lossy().into_owned()],
            );
        }
        let tracker = tasks.tracker.as_ref().expect("tracker");
        let claims = tracker.claims().expect("claims");
        assert_eq!(claims.len(), 2, "{claims:?}");
        assert!(claims.iter().all(|c| c.inferred && c.path == "src/a.ts"));
        tasks.check_overlaps();
        let mut got = routed.lock().expect("routed").clone();
        got.sort();
        let msg = "app and fix-auth both claim src/a.ts".to_string();
        assert_eq!(got, [("p1".into(), msg.clone()), ("p2".into(), msg)]);
        // A session with no connection ends as one with: the main
        // checkout's last session releases its claims, a lane keeps them.
        tasks.end_session("pn:p1");
        tasks.end_session("pn:p2");
        let left = tracker.claims().expect("claims");
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(left[0].agent, "fix-auth");
    }

    #[tokio::test]
    async fn f_tk_05_1_overlaps_from_before_a_panes_session_reach_it_as_it_begins() {
        let (_d, tasks) = service(Some("/x".into())).await;
        let dir = tempfile::tempdir().expect("dir");
        let (main, lane) = repo_with_lane(dir.path());
        let told: Seen<(String, Vec<String>)> = Arc::default();
        let seen = told.clone();
        tasks.set_notice_sink(Arc::new(move |pane, notices| {
            seen.lock()
                .expect("told")
                .push((pane.to_string(), notices.to_vec()));
        }));
        let routed = route_overlaps(&tasks);
        let tracker = tasks.tracker.clone().expect("tracker");
        let edit = |root: &Path, path: &str| {
            tracker
                .claim_inferred(&claim_scope(root).expect("scope"), &[path.into()])
                .expect("claim");
        };
        // The last run left both worktrees claiming a.ts, and the startup
        // check announced it to no pane.
        edit(&main, "src/a.ts");
        edit(&lane, "src/a.ts");
        tasks.check_overlaps();
        assert!(routed.lock().expect("routed").is_empty());
        // A pane's session begins in the lane: it's told.
        tasks.track_scope("pn:p1", Some(&lane));
        let a = "app and fix-auth both claim src/a.ts".to_string();
        assert_eq!(
            *told.lock().expect("told"),
            [("p1".to_string(), vec![a.clone()])]
        );
        // An overlap not announced yet reaches a beginning session once,
        // from the next check.
        edit(&main, "src/b.ts");
        edit(&lane, "src/b.ts");
        tasks
            .begin_session("pn:p2", "Claude Code", "me", Some(&main))
            .expect("p2");
        tasks.check_overlaps();
        let b = "app and fix-auth both claim src/b.ts".to_string();
        let mut got = routed.lock().expect("routed").clone();
        got.sort();
        assert_eq!(got, [("p1".into(), b.clone()), ("p2".into(), b)]);
        // A terminal pane's launch tracks its session, then connects it: it's
        // told once. A chat isn't a pane; one outside a working tree is told
        // nothing.
        tasks.track_scope("pn:p3", Some(&lane));
        tasks
            .begin_session("pn:p3", "Claude Code", "me", Some(&lane))
            .expect("p3");
        tasks
            .begin_session("th:1", "Thread", "me", Some(&lane))
            .expect("thread");
        tasks.track_scope("pn:p4", Some(dir.path()));
        assert_eq!(
            *told.lock().expect("told"),
            [
                ("p1".to_string(), vec![a.clone()]),
                ("p2".to_string(), vec![a]),
                (
                    "p3".to_string(),
                    vec!["app and fix-auth both claim src/a.ts, src/b.ts".to_string()]
                ),
                ("p4".to_string(), vec![]),
            ]
        );
    }

    /// `split`, whose git data is in `split.git` beside it (`git init
    /// --separate-git-dir`), and its worktree `fix-auth`: `(main checkout,
    /// worktree)`, resolved.
    fn split_repo_with_lane(dir: &Path) -> (PathBuf, PathBuf) {
        let main = dir.join("split");
        std::fs::create_dir_all(main.join("src")).expect("mkdir");
        let git = |cwd: &Path, args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(cwd)
                .status()
                .expect("git")
                .success();
            assert!(ok, "git {args:?}");
        };
        let data = dir.join("split.git");
        git(
            dir,
            &[
                "init",
                "-q",
                "--separate-git-dir",
                data.to_str().expect("utf8"),
                main.to_str().expect("utf8"),
            ],
        );
        git(
            &main,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        let lane = main.join(".tandem/worktrees/fix-auth");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                lane.to_str().expect("utf8"),
                "-b",
                "tandem/fix-auth",
            ],
        );
        (
            std::fs::canonicalize(main).expect("canonical"),
            std::fs::canonicalize(lane).expect("canonical"),
        )
    }

    #[tokio::test]
    async fn f_tk_05_3_only_the_main_checkouts_claims_go_with_its_last_session() {
        let (_d, tasks) = service(None).await;
        let dir = tempfile::tempdir().expect("dir");
        let (main, lane) = repo_with_lane(dir.path());
        let (split, split_lane) = split_repo_with_lane(dir.path());
        // Told by git, not by comparing the folders' spellings (Windows
        // spells a main checkout's two differently).
        for (root, is_main) in [
            (&main, true),
            (&lane, false),
            (&split, true),
            (&split_lane, false),
        ] {
            assert_eq!(claim_scope(root).expect("scope").main, is_main, "{root:?}");
        }
        for (pane, root) in [
            ("p1", &main),
            ("p2", &lane),
            ("p3", &split),
            ("p4", &split_lane),
        ] {
            tasks.track_scope(&format!("pn:{pane}"), Some(root));
            tasks.claim_edits(
                pane,
                &[root.join("src/a.ts").to_string_lossy().into_owned()],
            );
            tasks.end_session(&format!("pn:{pane}"));
        }
        let left: Vec<String> = tasks
            .tracker
            .as_ref()
            .expect("tracker")
            .claims()
            .expect("claims")
            .into_iter()
            .map(|c| c.folder)
            .collect();
        let folder = |root: &Path| claim_scope(root).expect("scope").folder;
        assert_eq!(
            left,
            [folder(&lane), folder(&split_lane)],
            "the worktrees keep theirs until they're merged or removed"
        );
    }

    #[test]
    fn f_tk_05_a_sessions_scope_has_its_folder_in_the_working_tree() {
        let dir = tempfile::tempdir().expect("dir");
        let root = git_repo(dir.path());
        std::fs::create_dir_all(root.join("src/web")).expect("mkdir");
        assert_eq!(claim_scope(&root).expect("top").prefix, "");
        let sub = claim_scope(&root.join("src/web")).expect("sub");
        assert_eq!(Path::new(&sub.prefix), Path::new("src").join("web"));
        assert_eq!(sub.folder, claim_scope(&root).expect("top").folder);
    }

    #[test]
    fn f_tk_05_1_a_path_is_announced_once_however_it_is_spelled() {
        let mut seen = HashSet::new();
        assert_eq!(
            fresh_overlaps(&mut seen, &[overlap(&["src/auth/"])]).len(),
            1
        );
        // A folder claimed before its trailing `/` stopped mattering.
        assert!(fresh_overlaps(&mut seen, &[overlap(&["src/auth"])]).is_empty());
        // Where file names ignore case, so does the announcement.
        let again = fresh_overlaps(&mut seen, &[overlap(&["SRC/Auth"])]);
        assert_eq!(
            again.is_empty(),
            tandem_core::contain::case_insensitive_fs(),
            "{again:?}"
        );
    }

    #[test]
    fn f_tk_05_2_edits_of_git_or_vibezeo_data_never_claim() {
        let dir = tempfile::tempdir().expect("dir");
        let root = git_repo(dir.path());
        for inside in [
            ".git/config",
            "./.git/config",
            ".git",
            ".tandem/worktrees/x/a.ts",
            ".tandem",
        ] {
            assert_eq!(claim_path(&root, &root.join(inside)), None, "{inside}");
            assert_eq!(claim_path(&root, Path::new(inside)), None, "{inside}");
        }
        // Where file names ignore case, `.GIT` is that folder too.
        assert_eq!(
            claim_path(&root, Path::new(".GIT/config")).is_none(),
            tandem_core::contain::case_insensitive_fs()
        );
        assert_eq!(
            claim_path(&root, Path::new(".gitignore")).as_deref(),
            Some(".gitignore")
        );
    }

    #[cfg(unix)]
    #[test]
    fn f_tk_05_2_a_new_file_in_new_folders_claims_through_a_symlink() {
        let dir = tempfile::tempdir().expect("dir");
        let root = git_repo(dir.path());
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&root, &link).expect("symlink");
        assert_eq!(
            claim_path(&root, &link.join("src/new/deeper/b.ts")).as_deref(),
            Some("src/new/deeper/b.ts")
        );
        assert_eq!(
            claim_path(&root, &link.join("src/c.ts")).as_deref(),
            Some("src/c.ts")
        );
        assert_eq!(
            claim_path(&root, &dir.path().join("elsewhere/x/y.ts")),
            None
        );
    }

    #[tokio::test]
    async fn f_tk_05_3_a_restart_drops_the_main_checkouts_and_deleted_worktrees_claims() {
        let dir = tempfile::tempdir().expect("dir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let repos = tempfile::tempdir().expect("repos");
        let (main, lane) = repo_with_lane(repos.path());
        let first = TasksService::new(&data, store.clone(), None);
        let tracker = first.tracker.clone().expect("tracker");
        let claim = |scope: &ClaimScope| {
            tracker
                .claim_inferred(scope, &["src/a.ts".into()])
                .expect("claim");
        };
        let main_scope = claim_scope(&main).expect("main");
        claim(&main_scope);
        // A main checkout's, and a worktree's deleted outside the app.
        let other = repos.path().join("other");
        let deleted = repos.path().join("deleted");
        for d in [&other, &deleted] {
            std::fs::create_dir_all(d).expect("mkdir");
        }
        claim(&ClaimScope {
            folder: other.to_string_lossy().into_owned(),
            main: true,
            ..main_scope.clone()
        });
        claim(&ClaimScope {
            folder: deleted.to_string_lossy().into_owned(),
            main: false,
            ..main_scope.clone()
        });
        std::fs::remove_dir(&deleted).expect("rmdir");
        claim(&claim_scope(&lane).expect("lane"));
        drop(first);
        let second = TasksService::new(&data, store, None);
        let left: Vec<String> = second
            .tracker
            .as_ref()
            .expect("tracker")
            .claims()
            .expect("claims")
            .into_iter()
            .map(|c| c.folder)
            .collect();
        assert_eq!(left, [claim_scope(&lane).expect("lane").folder]);
    }

    #[tokio::test]
    async fn f_tk_05_3_a_restart_keeps_a_worktrees_claims_when_its_git_data_lives_elsewhere() {
        // A worktree of a repository made with `--separate-git-dir` (or a
        // bare one) has itself as its repository (`main_root` can't name a
        // main checkout), yet it's still a worktree, not the main checkout.
        let dir = tempfile::tempdir().expect("dir");
        let data = DataDir::at(dir.path().join("data")).expect("data");
        let store = Store::open(&data).expect("store");
        let repos = tempfile::tempdir().expect("repos");
        let (_split, split_lane) = split_repo_with_lane(repos.path());
        let scope = claim_scope(&split_lane).expect("scope");
        assert!(!scope.main);
        let first = TasksService::new(&data, store.clone(), None);
        first
            .tracker
            .as_ref()
            .expect("tracker")
            .claim_inferred(&scope, &["src/a.ts".into()])
            .expect("claim");
        drop(first);
        let second = TasksService::new(&data, store, None);
        let left: Vec<String> = second
            .tracker
            .as_ref()
            .expect("tracker")
            .claims()
            .expect("claims")
            .into_iter()
            .map(|c| c.folder)
            .collect();
        assert_eq!(left, [scope.folder]);
    }
}
