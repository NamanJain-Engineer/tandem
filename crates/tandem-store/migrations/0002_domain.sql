ALTER TABLE workspaces ADD COLUMN created_at TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    kind TEXT NOT NULL CHECK (kind IN ('thread', 'code', 'agent')),
    title TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS sessions_workspace_idx ON sessions (workspace_id, created_at);

CREATE TABLE IF NOT EXISTS panes (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    engine_id TEXT NOT NULL,
    cwd TEXT NOT NULL,
    position INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS panes_session_idx ON panes (session_id, position);

CREATE TABLE IF NOT EXISTS agents (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    name TEXT NOT NULL,
    engine_id TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS agents_workspace_idx ON agents (workspace_id, created_at);

CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS events_workspace_idx ON events (workspace_id, id);
