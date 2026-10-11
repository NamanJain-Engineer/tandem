-- M17 extras (D-702): checkpoints (F-EX-01), command recall (F-EX-02) and
-- Best of N (F-EX-04).

CREATE TABLE checkpoints (
  id           TEXT PRIMARY KEY,           -- cp_<ULID>, also the store's ref name
  folder       TEXT NOT NULL,              -- canonical folder path
  workspace_id TEXT,
  thread_id    TEXT,
  pane_id      TEXT,
  turn_id      TEXT,
  reason       TEXT NOT NULL CHECK (reason IN ('turn','prompt','manual','restore')),
  label        TEXT NOT NULL,              -- never prompt text
  tree         TEXT NOT NULL,              -- the store's tree id
  files        INTEGER NOT NULL DEFAULT 0,
  skipped      TEXT NOT NULL DEFAULT '[]', -- JSON array of relative paths
  partial      INTEGER NOT NULL DEFAULT 0,
  late         INTEGER NOT NULL DEFAULT 0,
  created_at   INTEGER NOT NULL
);
CREATE INDEX checkpoints_folder ON checkpoints(folder, created_at);
CREATE INDEX checkpoints_thread ON checkpoints(thread_id, created_at);

CREATE TABLE command_history (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  pane_id      TEXT NOT NULL,
  workspace_id TEXT,
  cwd          TEXT NOT NULL,
  command      TEXT NOT NULL,              -- redacted (F-TL-02)
  secret       INTEGER NOT NULL DEFAULT 0, -- the redactor replaced something
  exit_code    INTEGER,
  started_at   INTEGER NOT NULL,
  duration_ms  INTEGER
);
CREATE INDEX command_history_started ON command_history(started_at);
CREATE INDEX command_history_command ON command_history(command, cwd);

CREATE TABLE bestof_runs (
  id            TEXT PRIMARY KEY,          -- bo_<ULID>
  workspace_id  TEXT NOT NULL,
  title         TEXT NOT NULL,
  tab_id        TEXT,
  base          TEXT NOT NULL,             -- the commit every lane started from
  check_command TEXT,
  state         TEXT NOT NULL CHECK (state IN ('running','merged','discarded')),
  created_at    INTEGER NOT NULL
);

CREATE TABLE bestof_lanes (
  run_id       TEXT NOT NULL REFERENCES bestof_runs(id) ON DELETE CASCADE,
  seat         INTEGER NOT NULL,
  lane_id      TEXT,
  branch       TEXT,
  engine_id    TEXT NOT NULL,
  account_id   TEXT,
  pane_id      TEXT,
  error        TEXT,
  check_state  TEXT NOT NULL DEFAULT 'none' CHECK (check_state IN ('none','running','passed','failed','stopped')),
  check_exit   INTEGER,
  check_output TEXT,
  PRIMARY KEY (run_id, seat)
);
