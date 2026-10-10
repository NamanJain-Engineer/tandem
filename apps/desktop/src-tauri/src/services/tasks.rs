//! The task tracker in the app (F-TK-01..03): its store at `$DATA/tasks/`,
//! and the `tandem-tasks` MCP server agents get (F-TK-02). Each engine
//! session gets its own connection, revoked when the session ends, so a
//! server left behind can't write any more.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tandem_core::settings::SettingKey;
use tandem_core::DataDir;
use tandem_engines::chat::StdioServer;
use tandem_store::Store;
use tandem_tasks::TaskStore;

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
    /// The live connection per session key (`th:<id>`, `pn:<id>`).
    sessions: Mutex<HashMap<String, String>>,
    /// The `issues/` watcher, while it runs.
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
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
        })
    }

    /// Watches `issues/` and publishes `tasks:changed {ids}` within
    /// [`WATCH_BATCH`] of a change by anyone: the UI, an MCP server, another
    /// process (F-TK-03, "within 500 ms").
    pub fn watch(&self, bus: tandem_core::bus::EventBus) {
        use notify::Watcher;
        let Some(tracker) = &self.tracker else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel::<Vec<String>>();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                let ids: Vec<String> = ev.paths.iter().filter_map(|p| issue_of(p)).collect();
                if !ids.is_empty() {
                    let _ = tx.send(ids);
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
        if let Ok(mut w) = self.watcher.lock() {
            *w = Some(watcher);
        }
    }

    /// A new connection for the session `key`; the session's previous one
    /// is revoked.
    pub fn begin_session(&self, key: &str, label: &str, actor: &str) -> Option<String> {
        let tracker = self.tracker.as_ref()?;
        let conn = match tracker.create_connection(label, actor) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(code = "tasks.connection_failed", error = %e, "no task connection for the session");
                return None;
            }
        };
        let previous = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut m| m.insert(key.to_string(), conn.id.clone()));
        if let Some(old) = previous {
            let _ = tracker.revoke_connection(&old);
        }
        Some(conn.id)
    }

    /// The session ended: its connection is revoked (F-TK-02 AC2).
    pub fn end_session(&self, key: &str) {
        let Some(tracker) = &self.tracker else {
            return;
        };
        let conn = self.sessions.lock().ok().and_then(|mut m| m.remove(key));
        if let Some(c) = conn {
            let _ = tracker.revoke_connection(&c);
        }
    }

    /// The session's live connection.
    pub fn connection_of(&self, key: &str) -> Option<String> {
        self.sessions.lock().ok()?.get(key).cloned()
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
    pub fn chat_server(&self, key: &str, label: &str, actor: &str) -> Option<StdioServer> {
        self.bin.as_ref()?;
        let conn = self.begin_session(key, label, actor)?;
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
        assert_eq!(
            conf["bundle"]["externalBin"],
            serde_json::json!(["binaries/tandem-tasks"])
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
        let server = tasks.chat_server("th:1", "Thread", "me").expect("server");
        assert_eq!(server.name, "tandem_tasks");
        let conn = tasks.connection_of("th:1").expect("conn");
        assert_eq!(server.args.last(), Some(&conn));
        assert!(tracker.connection(&conn).is_ok());
        // A new session for the same chat revokes the last one.
        tasks.chat_server("th:1", "Thread", "me").expect("again");
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
        assert!(tasks.chat_server("th:1", "Thread", "me").is_none());
        assert!(tasks.connection_of("th:1").is_none(), "no connection made");
    }

    #[tokio::test]
    async fn f_tk_02_a_restart_revokes_the_last_runs_connections() {
        let dir = tempfile::tempdir().expect("dir");
        let data = DataDir::at(dir.path().to_path_buf()).expect("data");
        let store = Store::open(&data).expect("store");
        let first = TasksService::new(&data, store.clone(), Some("/x".into()));
        first.chat_server("th:1", "Thread", "me").expect("server");
        let conn = first.connection_of("th:1").expect("conn");
        let second = TasksService::new(&data, store, Some("/x".into()));
        assert!(second
            .tracker
            .as_ref()
            .expect("tracker")
            .connection(&conn)
            .is_err());
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
}
