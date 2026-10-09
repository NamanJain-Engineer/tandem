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
}

/// `tandem-tasks` next to the app binary (`TANDEM_TASKS_BIN` in debug
/// builds), like `tandem-hook` (D-68).
pub fn tasks_bin() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        if let Some(p) = std::env::var_os("TANDEM_TASKS_BIN") {
            return Some(PathBuf::from(p));
        }
    }
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) {
        "tandem-tasks.exe"
    } else {
        "tandem-tasks"
    };
    let p = exe.parent()?.join(name);
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
        })
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
}
