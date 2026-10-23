//! The task tracker in the app (F-TK-01..03): its store at `$DATA/tasks/`,
//! and the `tandem-tasks` MCP server agents get (F-TK-02). Each engine
//! session gets its own connection, revoked when the session ends, so a
//! server left behind can't write any more. A session in a git working
//! tree can claim files there (F-TK-05); its edits claim them too.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tandem_core::settings::SettingKey;
use tandem_core::DataDir;
use tandem_engines::chat::StdioServer;
use tandem_store::Store;
use tandem_tasks::{ClaimOverlap, ClaimScope, TaskStore};

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
    /// Held by a check from reading the claims to telling the sink: one
    /// check at a time (F-TK-05).
    checking: Mutex<()>,
    on_overlap: std::sync::OnceLock<OverlapSink>,
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

/// Where a session in `folder` claims: its working tree and the repository
/// it belongs to (a lane's worktree claims against its main checkout).
pub fn claim_scope(folder: &Path) -> Option<ClaimScope> {
    let repo = tandem_git::repo::Repo::discover(folder).ok()??;
    Some(ClaimScope {
        repo: repo.main_root().to_string_lossy().into_owned(),
        folder: repo.root().to_string_lossy().into_owned(),
    })
}

/// An edited file's path relative to the working tree, or `None` outside
/// it (and inside `.git` or Vibezeo's own `.tandem`).
pub fn claim_path(folder: &Path, edited: &Path) -> Option<String> {
    let rel = if edited.is_relative() {
        edited.to_path_buf()
    } else if let Ok(r) = edited.strip_prefix(folder) {
        r.to_path_buf()
    } else {
        // A new file's folder exists; symlinked paths (macOS /tmp) resolve.
        let parent = std::fs::canonicalize(edited.parent()?).ok()?;
        parent
            .join(edited.file_name()?)
            .strip_prefix(folder)
            .ok()?
            .to_path_buf()
    };
    let text = rel.to_string_lossy().replace('\\', "/");
    if text.starts_with(".git/") || text.starts_with(".tandem/") {
        return None;
    }
    tandem_tasks::store::normalize_claim_path(&text).ok()
}

/// One key per overlapping path, so a pair is announced again only for a
/// path it didn't overlap on before.
fn overlap_keys(o: &ClaimOverlap) -> impl Iterator<Item = (String, &str)> + '_ {
    o.paths.iter().map(move |p| {
        (
            format!("{}\0{}\0{}\0{p}", o.repo, o.folders[0], o.folders[1]),
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
            checking: Mutex::default(),
            on_overlap: std::sync::OnceLock::new(),
        })
    }

    /// Told of claim overlaps as they change (set once, at startup).
    pub fn set_overlap_sink(&self, sink: OverlapSink) {
        let _ = self.on_overlap.set(sink);
    }

    /// The session `key` begins in `folder`, with or without a connection
    /// (F-TK-05): its edits claim there, and its pane gets the overlap
    /// notices of its working tree. The key's previous connection is
    /// revoked. Blocking file IO.
    pub fn track_scope(&self, key: &str, folder: Option<&Path>) {
        self.track(key, folder.and_then(claim_scope));
    }

    fn track(&self, key: &str, scope: Option<ClaimScope>) {
        let Some(tracker) = &self.tracker else {
            return;
        };
        let previous = match self.sessions.lock() {
            Ok(mut m) => m.insert(
                key.to_string(),
                Live {
                    connection: None,
                    scope,
                },
            ),
            Err(_) => return,
        };
        if let Some(old) = previous.and_then(|l| l.connection) {
            let _ = tracker.revoke_connection(&old);
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
            if scope.folder == scope.repo && !others_there {
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
        self.bin.as_ref()?;
        let conn = self.begin_session(key, label, actor, folder)?;
        self.server(["--connection".into(), conn])
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
}
