use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tandem_core::paths::Paths;
use tandem_core::Error;
use uuid::Uuid;

const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init", include_str!("../migrations/0001_init.sql")),
    ("0002_domain", include_str!("../migrations/0002_domain.sql")),
];

pub struct Store {
    conn: Connection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub path: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub workspace_id: String,
    pub kind: String,
    pub title: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    pub session_id: String,
    pub engine_id: String,
    pub cwd: String,
    pub position: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub engine_id: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub id: i64,
    pub workspace_id: String,
    pub kind: String,
    pub payload: String,
    pub created_at: String,
}

impl Store {
    pub fn open(paths: &Paths) -> Result<Self, Error> {
        std::fs::create_dir_all(&paths.data).map_err(|err| {
            Error::new(
                "store",
                format!("create data dir: {err}"),
                "check TANDEM_HOME permissions",
            )
        })?;
        let conn = Connection::open(paths.data.join("tandem.sqlite")).map_err(|err| {
            Error::new("store", format!("open sqlite: {err}"), "check disk space")
        })?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|err| db(err, "wal"))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|err| db(err, "fk"))?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), Error> {
        self.conn
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(|err| db(err, "set setting"))?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, Error> {
        self.conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| db(err, "get setting"))
    }

    pub fn create_workspace(&self, name: &str, path: &str) -> Result<Workspace, Error> {
        require_text("name", name, 80)?;
        require_text("path", path, 1024)?;
        let workspace = Workspace {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            path: path.to_string(),
            created_at: now(),
        };
        self.conn
            .execute(
                "INSERT INTO workspaces (id, name, path, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    workspace.id,
                    workspace.name,
                    workspace.path,
                    workspace.created_at
                ],
            )
            .map_err(|err| db(err, "create workspace"))?;
        Ok(workspace)
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>, Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, path, created_at FROM workspaces ORDER BY created_at, id")
            .map_err(|err| db(err, "list workspaces"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Workspace {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|err| db(err, "list workspaces"))?;
        collect(rows)
    }

    pub fn create_session(
        &self,
        workspace_id: &str,
        kind: &str,
        title: &str,
    ) -> Result<Session, Error> {
        if !matches!(kind, "thread" | "code" | "agent") {
            return Err(Error::new(
                "store",
                format!("unknown session kind {kind}"),
                "use thread, code, or agent",
            ));
        }
        require_text("title", title, 200)?;
        let session = Session {
            id: Uuid::new_v4().to_string(),
            workspace_id: workspace_id.to_string(),
            kind: kind.to_string(),
            title: title.to_string(),
            created_at: now(),
        };
        self.conn
            .execute(
                "INSERT INTO sessions (id, workspace_id, kind, title, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    session.id,
                    session.workspace_id,
                    session.kind,
                    session.title,
                    session.created_at
                ],
            )
            .map_err(|err| db(err, "create session"))?;
        Ok(session)
    }

    pub fn list_sessions(&self, workspace_id: &str) -> Result<Vec<Session>, Error> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, workspace_id, kind, title, created_at FROM sessions
                 WHERE workspace_id = ?1 ORDER BY created_at, id",
            )
            .map_err(|err| db(err, "list sessions"))?;
        let rows = stmt
            .query_map(params![workspace_id], |row| {
                Ok(Session {
                    id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    kind: row.get(2)?,
                    title: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|err| db(err, "list sessions"))?;
        collect(rows)
    }

    pub fn create_pane(
        &self,
        session_id: &str,
        engine_id: &str,
        cwd: &str,
        position: i64,
    ) -> Result<Pane, Error> {
        require_text("engine_id", engine_id, 64)?;
        require_text("cwd", cwd, 1024)?;
        let pane = Pane {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            engine_id: engine_id.to_string(),
            cwd: cwd.to_string(),
            position,
        };
        self.conn
            .execute(
                "INSERT INTO panes (id, session_id, engine_id, cwd, position)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    pane.id,
                    pane.session_id,
                    pane.engine_id,
                    pane.cwd,
                    pane.position
                ],
            )
            .map_err(|err| db(err, "create pane"))?;
        Ok(pane)
    }

    pub fn list_panes(&self, session_id: &str) -> Result<Vec<Pane>, Error> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, engine_id, cwd, position FROM panes
                 WHERE session_id = ?1 ORDER BY position, id",
            )
            .map_err(|err| db(err, "list panes"))?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok(Pane {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    engine_id: row.get(2)?,
                    cwd: row.get(3)?,
                    position: row.get(4)?,
                })
            })
            .map_err(|err| db(err, "list panes"))?;
        collect(rows)
    }

    pub fn create_agent(
        &self,
        workspace_id: &str,
        name: &str,
        engine_id: &str,
    ) -> Result<Agent, Error> {
        require_text("name", name, 80)?;
        require_text("engine_id", engine_id, 64)?;
        let agent = Agent {
            id: Uuid::new_v4().to_string(),
            workspace_id: workspace_id.to_string(),
            name: name.to_string(),
            engine_id: engine_id.to_string(),
            created_at: now(),
        };
        self.conn
            .execute(
                "INSERT INTO agents (id, workspace_id, name, engine_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    agent.id,
                    agent.workspace_id,
                    agent.name,
                    agent.engine_id,
                    agent.created_at
                ],
            )
            .map_err(|err| db(err, "create agent"))?;
        Ok(agent)
    }

    pub fn append_event(
        &self,
        workspace_id: &str,
        kind: &str,
        payload: &str,
    ) -> Result<Event, Error> {
        require_text("kind", kind, 64)?;
        let created_at = now();
        self.conn
            .execute(
                "INSERT INTO events (workspace_id, kind, payload, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![workspace_id, kind, payload, created_at],
            )
            .map_err(|err| db(err, "append event"))?;
        let id = self.conn.last_insert_rowid();
        Ok(Event {
            id,
            workspace_id: workspace_id.to_string(),
            kind: kind.to_string(),
            payload: payload.to_string(),
            created_at,
        })
    }

    pub fn events_after(
        &self,
        workspace_id: &str,
        after_id: i64,
        limit: i64,
    ) -> Result<Vec<Event>, Error> {
        let limit = limit.clamp(1, 500);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, workspace_id, kind, payload, created_at FROM events
                 WHERE workspace_id = ?1 AND id > ?2
                 ORDER BY id ASC
                 LIMIT ?3",
            )
            .map_err(|err| db(err, "events after"))?;
        let rows = stmt
            .query_map(params![workspace_id, after_id, limit], |row| {
                Ok(Event {
                    id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    kind: row.get(2)?,
                    payload: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|err| db(err, "events after"))?;
        collect(rows)
    }
}

fn migrate(conn: &Connection) -> Result<(), Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            id TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )
    .map_err(|err| db(err, "migration table"))?;
    for (id, sql) in MIGRATIONS {
        let applied: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| db(err, "migration lookup"))?;
        if applied.is_some() {
            continue;
        }
        let tx = conn
            .unchecked_transaction()
            .map_err(|err| db(err, "migration"))?;
        tx.execute_batch(sql).map_err(|err| db(err, id))?;
        tx.execute(
            "INSERT INTO schema_migrations (id, applied_at) VALUES (?1, ?2)",
            params![id, now()],
        )
        .map_err(|err| db(err, id))?;
        tx.commit().map_err(|err| db(err, id))?;
    }
    Ok(())
}

fn now() -> String {
    DateTime::<Utc>::from(std::time::SystemTime::now()).to_rfc3339()
}

fn require_text(field: &str, value: &str, max: usize) -> Result<(), Error> {
    if value.is_empty() || value.chars().count() > max {
        return Err(Error::new(
            "store",
            format!("{field} must be 1..={max} characters"),
            "shorten the value and retry",
        ));
    }
    Ok(())
}

fn db(err: rusqlite::Error, action: &str) -> Error {
    Error::new("store", format!("{action}: {err}"), "retry the operation")
}

fn collect<T, I>(rows: I) -> Result<Vec<T>, Error>
where
    I: Iterator<Item = Result<T, rusqlite::Error>>,
{
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|err| db(err, "read row"))?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::paths::Paths;

    fn open() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::from_data(dir.path().to_path_buf());
        let store = Store::open(&paths).expect("open");
        (dir, store)
    }

    #[test]
    fn settings_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::from_data(dir.path().to_path_buf());
        let store = Store::open(&paths).expect("open");
        store.set_setting("theme", "light").expect("set");
        drop(store);
        let store = Store::open(&paths).expect("reopen");
        assert_eq!(
            store.get_setting("theme").expect("get").as_deref(),
            Some("light")
        );
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("count");
        assert_eq!(count, 2);
    }

    #[test]
    fn domain_round_trip_and_event_cursor() {
        let (_dir, store) = open();
        let workspace = store
            .create_workspace("demo", "/tmp/demo")
            .expect("workspace");
        assert!(store.create_session("missing", "thread", "nope").is_err());
        assert!(store.create_session(&workspace.id, "nope", "bad").is_err());
        let session = store
            .create_session(&workspace.id, "code", "Build the shell")
            .expect("session");
        let pane = store
            .create_pane(&session.id, "claude", "/tmp/demo", 0)
            .expect("pane");
        assert_eq!(store.list_panes(&session.id).expect("panes").len(), 1);
        assert_eq!(pane.engine_id, "claude");
        let agent = store
            .create_agent(&workspace.id, "builder", "codex")
            .expect("agent");
        assert_eq!(agent.name, "builder");
        let first = store
            .append_event(&workspace.id, "session.created", "{\"ok\":true}")
            .expect("event");
        let second = store
            .append_event(&workspace.id, "pane.opened", "{}")
            .expect("event");
        let page = store
            .events_after(&workspace.id, first.id, 10)
            .expect("page");
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].id, second.id);
        assert_eq!(store.list_workspaces().expect("list").len(), 1);
        assert_eq!(
            store.list_sessions(&workspace.id).expect("sessions").len(),
            1
        );
    }
}
