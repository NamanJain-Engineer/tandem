-- Tandem local schema (ARCHITECTURE §4.2). Forward-only: never edit a shipped migration.
-- ── settings, rail, onboarding ───────────────────────────────── F-SH-03/04/08
CREATE TABLE settings (
  key         TEXT PRIMARY KEY,               -- SettingKey (snake_case)
  value_json  TEXT NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE TABLE rail_items (
  mode     TEXT NOT NULL CHECK (mode IN ('agent','code','thread')),
  item_id  TEXT NOT NULL,                      -- tm_/ws_/th_ id
  pinned   INTEGER NOT NULL DEFAULT 0,
  sort_key TEXT NOT NULL,
  PRIMARY KEY (mode, item_id)
);
CREATE INDEX rail_items_order ON rail_items(mode, pinned DESC, sort_key);

-- ── hosts, workspaces, tabs, layout ─────────────────────────── F-CD-01/02, F-RM-01/02, F-SH-10, F-TH-14
CREATE TABLE hosts (
  id            TEXT PRIMARY KEY,              -- 'local' | host_<ulid>
  kind          TEXT NOT NULL CHECK (kind IN ('local','ssh','wsl')),
  label         TEXT NOT NULL,
  ssh_target    TEXT,                          -- alias or user@host
  ssh_port      INTEGER,
  identity_file TEXT,
  wsl_distro    TEXT,
  host_key_fp   TEXT,                          -- SHA256:… pinned at first approval
  host_protocol INTEGER,                       -- tandem-host protocol in use
  arch          TEXT,
  created_at    INTEGER NOT NULL
);
CREATE TABLE workspaces (
  id                 TEXT PRIMARY KEY,
  kind               TEXT NOT NULL CHECK (kind IN ('local','ssh','wsl')),
  host_id            TEXT NOT NULL REFERENCES hosts(id),
  root               TEXT NOT NULL,            -- absolute, normalized
  wsl_distro         TEXT,
  name               TEXT NOT NULL,
  parent_id          TEXT REFERENCES workspaces(id) ON DELETE CASCADE,  -- lanes
  lane_branch        TEXT,                     -- tandem/<slug> when a lane
  lane_base          TEXT,                     -- base commit sha
  sort_key           TEXT NOT NULL,
  pinned             INTEGER NOT NULL DEFAULT 0,
  collapsed          INTEGER NOT NULL DEFAULT 0,
  sessions_collapsed INTEGER NOT NULL DEFAULT 0,       -- "Show terminal sessions"
  account_overrides  TEXT NOT NULL DEFAULT '{}',       -- {engine_id: account_id}
  removed_at         INTEGER,                  -- soft remove; re-add restores
  created_at         INTEGER NOT NULL,
  UNIQUE (host_id, root)
);
CREATE INDEX workspaces_parent ON workspaces(parent_id);
CREATE TABLE tabs (
  id           TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name         TEXT NOT NULL,
  sort_key     TEXT NOT NULL,
  created_at   INTEGER NOT NULL
);
CREATE INDEX tabs_ws ON tabs(workspace_id, sort_key);
CREATE TABLE layout_trees (
  tab_id            TEXT PRIMARY KEY REFERENCES tabs(id) ON DELETE CASCADE,
  tree_json         TEXT NOT NULL,             -- LayoutNode
  active_pane_id    TEXT,
  maximized_pane_id TEXT,
  flipped           INTEGER NOT NULL DEFAULT 0,
  updated_at        INTEGER NOT NULL
);
CREATE TABLE layout_sidebar (
  id         INTEGER PRIMARY KEY CHECK (id = 1),
  state_json TEXT NOT NULL,                    -- tabs, order, active, width, fullscreen, open, browser tabs
  updated_at INTEGER NOT NULL
);
CREATE TABLE layout_thread_grid (
  id         INTEGER PRIMARY KEY CHECK (id = 1),
  cells_json TEXT NOT NULL,                    -- [thread_id] ≤ 6
  updated_at INTEGER NOT NULL
);

-- ── accounts, engines ───────────────────────────────────────── F-AC-01..03, F-EN-01
CREATE TABLE accounts (
  id          TEXT PRIMARY KEY,
  engine_id   TEXT NOT NULL,
  label       TEXT NOT NULL,
  home_path   TEXT NOT NULL,                   -- $PROVIDERS/<engine>/<id>
  is_default  INTEGER NOT NULL DEFAULT 0,
  one_off     INTEGER NOT NULL DEFAULT 0,
  plan_label  TEXT,                            -- "Max", "Plus" if the engine reports it
  created_at  INTEGER NOT NULL,
  removed_at  INTEGER,
  UNIQUE (engine_id, label)
);
CREATE UNIQUE INDEX accounts_one_default ON accounts(engine_id) WHERE is_default = 1 AND removed_at IS NULL;
CREATE TABLE engine_installs (
  host_id     TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
  engine_id   TEXT NOT NULL,
  path        TEXT NOT NULL,
  version     TEXT,
  detected_at INTEGER NOT NULL,
  PRIMARY KEY (host_id, engine_id)
);
CREATE TABLE engine_detection_reports (
  host_id     TEXT PRIMARY KEY REFERENCES hosts(id) ON DELETE CASCADE,
  report_json TEXT NOT NULL,                   -- shells asked, folders checked, found, fixes, slow_shell
  created_at  INTEGER NOT NULL
);
CREATE TABLE usage_snapshots (
  account_id  TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
  windows_json TEXT NOT NULL, source TEXT NOT NULL, estimated INTEGER NOT NULL, fetched_at INTEGER NOT NULL
);

-- ── panes, sessions, presets ────────────────────────────────── F-CD-03/12/14, F-SH-07
CREATE TABLE panes (
  id            TEXT PRIMARY KEY,
  tab_id        TEXT NOT NULL REFERENCES tabs(id) ON DELETE CASCADE,
  workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  kind          TEXT NOT NULL CHECK (kind IN ('terminal','agent','browser','simulator')),
  cwd           TEXT,                          -- last OSC 7 cwd
  engine_id     TEXT,
  account_id    TEXT REFERENCES accounts(id),
  human_name    TEXT,
  title         TEXT,                          -- live title (engine/OSC)
  user_title    TEXT,
  browser_url   TEXT,
  role          TEXT,                          -- preset seat role
  launch_config TEXT,                          -- JSON LaunchConfig (engine args, extra args, one-off home path, custom flags)
  restore_state TEXT NOT NULL DEFAULT 'none' CHECK (restore_state IN ('none','live','paused','cancelled')),
  created_at    INTEGER NOT NULL,
  closed_at     INTEGER
);
CREATE INDEX panes_tab ON panes(tab_id) WHERE closed_at IS NULL;
CREATE UNIQUE INDEX panes_unique_name ON panes(workspace_id, human_name)
  WHERE kind = 'agent' AND closed_at IS NULL AND human_name IS NOT NULL;
CREATE TABLE sessions (
  id                TEXT PRIMARY KEY,          -- ses_
  kind              TEXT NOT NULL CHECK (kind IN ('pane','thread','saved')),
  pane_id           TEXT REFERENCES panes(id) ON DELETE SET NULL,
  thread_id         TEXT REFERENCES threads(id) ON DELETE CASCADE,
  workspace_id      TEXT REFERENCES workspaces(id) ON DELETE CASCADE,
  host_id           TEXT NOT NULL DEFAULT 'local',
  engine_id         TEXT NOT NULL,
  account_id        TEXT REFERENCES accounts(id),
  provider_home     TEXT,                      -- exact path used (one-off homes too)
  engine_session_id TEXT,                      -- for --resume / resume <id>
  title             TEXT,
  launch_config     TEXT,
  started_at        INTEGER NOT NULL,
  ended_at          INTEGER
);
CREATE INDEX sessions_pane ON sessions(pane_id);
CREATE INDEX sessions_saved ON sessions(workspace_id, kind, ended_at DESC);
CREATE TABLE presets (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  seats_json TEXT NOT NULL,                    -- [Seat {kind, engine_id?, account_id?, role?, task?, command?, url?, count}]
  isolation  TEXT NOT NULL CHECK (isolation IN ('shared','worktree')),
  builtin    INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

-- ── threads & teammate chats (one table) ────────────────────── F-TH-*, F-AG-03, F-RT-02
CREATE TABLE threads (
  id                TEXT PRIMARY KEY,
  kind              TEXT NOT NULL CHECK (kind IN ('thread','teammate_chat')),
  teammate_id       TEXT REFERENCES teammates(id) ON DELETE SET NULL,
  routine_id        TEXT REFERENCES routines(id) ON DELETE SET NULL,
  workspace_id      TEXT REFERENCES workspaces(id) ON DELETE SET NULL,   -- NULL = No folder
  cwd               TEXT NOT NULL,             -- folder or $DATA/scratch/<id> or place path
  host_id           TEXT NOT NULL DEFAULT 'local',
  engine_id         TEXT NOT NULL,
  account_id        TEXT REFERENCES accounts(id),
  model             TEXT,
  effort            TEXT,
  access            TEXT NOT NULL CHECK (access IN ('read','edits','full')),
  mode              TEXT NOT NULL CHECK (mode IN ('plan','act')),       -- UI "Plan | Act"; access values map to "Read only" / "Edits" / "Full access"
  title             TEXT NOT NULL DEFAULT '',
  title_source      TEXT NOT NULL DEFAULT 'none' CHECK (title_source IN ('none','auto','fallback','user','routine','mail')),
  state             TEXT NOT NULL DEFAULT 'idle' CHECK (state IN ('idle','working','waiting','done','failed','interrupted')),
  attended          INTEGER NOT NULL DEFAULT 1,   -- 0 for routine/task/mail/Auto-triggered chats
  engine_session_id TEXT,
  start_commit      TEXT,                      -- for Changes panel
  snapshot_ref      TEXT,                      -- non-git folders: hash manifest file
  conversation_ref  TEXT,                      -- mail threading
  pinned            INTEGER NOT NULL DEFAULT 0,
  archived          INTEGER NOT NULL DEFAULT 0,
  unread            INTEGER NOT NULL DEFAULT 0,
  sort_key          TEXT NOT NULL,
  draft_json        TEXT NOT NULL DEFAULT '{}',
  allow_rules_json  TEXT NOT NULL DEFAULT '[]',    -- "Allow for this thread" rules; cleared on engine change
  plugin_grants_json TEXT NOT NULL DEFAULT '[]',
  tokens_json       TEXT NOT NULL DEFAULT '{}',    -- per-engine {in, out, cache}
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL,
  deleted_at        INTEGER                    -- recovery copy kept 7 days
);
CREATE INDEX threads_rail ON threads(kind, workspace_id, archived, pinned DESC, sort_key);
CREATE INDEX threads_teammate ON threads(teammate_id, sort_key) WHERE kind = 'teammate_chat';
CREATE TABLE thread_turns (
  id         TEXT PRIMARY KEY,                 -- turn_
  thread_id  TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  n          INTEGER NOT NULL,
  engine_id  TEXT NOT NULL,
  account_id TEXT,
  state      TEXT NOT NULL CHECK (state IN ('running','completed','cancelled','limit','error','interrupted')),
  blocked_actions INTEGER NOT NULL DEFAULT 0,  -- "Completed with blocked actions"
  started_at INTEGER NOT NULL,
  ended_at   INTEGER,
  UNIQUE (thread_id, n)
);
CREATE TABLE thread_messages (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  thread_id    TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  turn_id      TEXT NOT NULL REFERENCES thread_turns(id) ON DELETE CASCADE,
  seq          INTEGER NOT NULL,               -- per turn
  type         TEXT NOT NULL,                  -- ThreadEvent tag
  payload_json TEXT NOT NULL,                  -- tool output truncated to 64 KB; full text in attachments dir
  created_at   INTEGER NOT NULL
);
CREATE INDEX thread_messages_by_thread ON thread_messages(thread_id, id);
CREATE TABLE plan_revisions (
  id TEXT PRIMARY KEY, thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  version INTEGER NOT NULL, turn_id TEXT NOT NULL, text TEXT NOT NULL, created_at INTEGER NOT NULL,
  UNIQUE (thread_id, version)
);
CREATE TABLE attachments (
  id         TEXT PRIMARY KEY,
  thread_id  TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  message_id INTEGER REFERENCES thread_messages(id) ON DELETE SET NULL,
  kind       TEXT NOT NULL CHECK (kind IN ('image','file','folder','tool_output')),
  path       TEXT NOT NULL,                    -- $DATA/attachments/<thread>/… or referenced path
  name       TEXT NOT NULL, mime TEXT, bytes INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
-- FTS over teammate chat prose only (user + assistant text) ── F-AG-11
CREATE VIRTUAL TABLE chat_fts USING fts5(text, thread_id UNINDEXED, teammate_id UNINDEXED, created_at UNINDEXED, tokenize = 'unicode61');

-- ── teammates, mail, memory-change log ──────────────────────── F-AG-*
CREATE TABLE teammates (
  id                   TEXT PRIMARY KEY,
  name                 TEXT NOT NULL,
  name_fold            TEXT NOT NULL UNIQUE,   -- lower(NFKC(name))
  avatar_hue           INTEGER,                -- NULL = derived from id hash (12 hues)
  avatar_image         TEXT,
  brief                TEXT NOT NULL DEFAULT '',
  engine_id            TEXT NOT NULL,
  model                TEXT, effort TEXT,
  preferred_account_id TEXT REFERENCES accounts(id),
  home                 TEXT NOT NULL,
  places_json          TEXT NOT NULL DEFAULT '[]',
  settings_json        TEXT NOT NULL,          -- {memory_cap, learn_mode, may_message, may_schedule, default_access}
  is_lead              INTEGER NOT NULL DEFAULT 0,
  pinned               INTEGER NOT NULL DEFAULT 0,
  sort_key             TEXT NOT NULL,
  created_at           INTEGER NOT NULL,
  deleted_at           INTEGER
);
CREATE TABLE mail (
  id               TEXT PRIMARY KEY,
  from_teammate_id TEXT REFERENCES teammates(id) ON DELETE SET NULL,  -- NULL = the user
  to_teammate_id   TEXT NOT NULL REFERENCES teammates(id) ON DELETE CASCADE,
  from_thread_id   TEXT,
  body             TEXT NOT NULL,
  conversation_ref TEXT NOT NULL,
  state            TEXT NOT NULL CHECK (state IN ('queued','delivered','refused','failed')),
  reason           TEXT,                       -- paused|not_permitted|terminal_only|deleted|rate_limit|global_pause|wake_failed
  delivered_thread_id TEXT,
  created_at       INTEGER NOT NULL,
  delivered_at     INTEGER
);
CREATE INDEX mail_to ON mail(to_teammate_id, state, created_at);
CREATE TABLE mail_turn_log (teammate_id TEXT NOT NULL, started_at INTEGER NOT NULL);   -- loop guard window
CREATE INDEX mail_turn_log_idx ON mail_turn_log(teammate_id, started_at);
CREATE TABLE memory_changes (
  id TEXT PRIMARY KEY, teammate_id TEXT NOT NULL, file TEXT NOT NULL, before_hash TEXT, after_hash TEXT,
  before_path TEXT NOT NULL,                   -- copy under <home>/memory/.history/<id>.md for Undo
  created_at INTEGER NOT NULL
);

-- ── routines ────────────────────────────────────────────────── F-RT-*
CREATE TABLE routines (
  id                     TEXT PRIMARY KEY,
  teammate_id            TEXT REFERENCES teammates(id) ON DELETE SET NULL,  -- NULL → error on next run
  name                   TEXT NOT NULL,
  instruction            TEXT NOT NULL,
  schedule_json          TEXT NOT NULL,        -- ScheduleSpec
  timezone               TEXT NOT NULL,
  access                 TEXT NOT NULL DEFAULT 'edits' CHECK (access IN ('read','edits','full')),
  run_mode               TEXT NOT NULL DEFAULT 'in_app' CHECK (run_mode IN ('in_app','background')),
  paused                 INTEGER NOT NULL DEFAULT 0,
  paused_by_plan         INTEGER NOT NULL DEFAULT 0,
  created_by_teammate_id TEXT,
  last_run_chat_id       TEXT, last_status TEXT, last_error TEXT, last_error_kind TEXT,
  next_run_at            INTEGER,
  created_at             INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE INDEX routines_due ON routines(paused, next_run_at);
CREATE TABLE routine_runs (
  id          TEXT PRIMARY KEY,
  routine_id  TEXT NOT NULL REFERENCES routines(id) ON DELETE CASCADE,
  due_at      INTEGER NOT NULL,
  started_at  INTEGER, ended_at INTEGER,
  status      TEXT NOT NULL CHECK (status IN ('running','ok','error','skipped')),
  late        INTEGER NOT NULL DEFAULT 0,
  runner      TEXT NOT NULL CHECK (runner IN ('gui','headless')),
  chat_id     TEXT, error TEXT, held_count INTEGER NOT NULL DEFAULT 0,
  UNIQUE (routine_id, due_at)                  -- a due slot fires at most once, across processes
);
CREATE TABLE error_notice_log (kind TEXT NOT NULL, subject_id TEXT NOT NULL, day TEXT NOT NULL, PRIMARY KEY (kind, subject_id, day));

-- ── plugins, grants, approvals, held drafts ─────────────────── F-PL-*, F-TH-11
CREATE TABLE plugin_connections (
  id            TEXT PRIMARY KEY,              -- pc_
  plugin_id     TEXT NOT NULL,
  account_label TEXT,
  auth_kind     TEXT NOT NULL CHECK (auth_kind IN ('oauth_dcr','oauth_static','api_key','local_port','none')),
  status        TEXT NOT NULL CHECK (status IN ('connected','needs_attention','disconnected')),
  meta_json     TEXT NOT NULL DEFAULT '{}',    -- client_id, auth server URL, scopes, port, mode (test/live). NEVER secrets
  options_json  TEXT NOT NULL DEFAULT '{}',    -- allow_publish_in_full
  created_at    INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE TABLE plugin_grants (
  subject_kind  TEXT NOT NULL CHECK (subject_kind IN ('teammate','thread','workspace','all_threads')),
  subject_id    TEXT NOT NULL,                 -- '*' for all_threads
  connection_id TEXT NOT NULL REFERENCES plugin_connections(id) ON DELETE CASCADE,
  created_at    INTEGER NOT NULL,
  PRIMARY KEY (subject_kind, subject_id, connection_id)
);
CREATE TABLE approvals (
  id           TEXT PRIMARY KEY,
  source_kind  TEXT NOT NULL CHECK (source_kind IN ('thread','teammate_chat','gateway')),
  source_id    TEXT NOT NULL,                  -- thread id
  lease_id     TEXT,
  turn_id      TEXT,
  kind         TEXT NOT NULL,                  -- ApprovalKind
  risk_class   TEXT,                           -- plugin approvals
  summary      TEXT NOT NULL,                  -- redacted
  detail_json  TEXT NOT NULL,                  -- redacted, secrets masked
  options_json TEXT NOT NULL,                  -- offered scopes
  state        TEXT NOT NULL CHECK (state IN ('pending','allowed','denied','expired','cancelled')),
  scope        TEXT,                           -- once|thread|conversation
  decided_via  TEXT,                           -- inline|dashboard|notification|phone|voice|rule|expiry
  deny_reason  TEXT,
  created_at   INTEGER NOT NULL, expires_at INTEGER NOT NULL, decided_at INTEGER
);
CREATE INDEX approvals_pending ON approvals(state, expires_at);
CREATE TABLE held_drafts (
  id            TEXT PRIMARY KEY,
  display_no    INTEGER NOT NULL UNIQUE,       -- shown as D-<n>
  connection_id TEXT NOT NULL REFERENCES plugin_connections(id) ON DELETE CASCADE,
  plugin_id     TEXT NOT NULL, tool TEXT NOT NULL,
  args_json     TEXT NOT NULL,                 -- arguments as requested (no credentials ever present)
  risk_class    TEXT NOT NULL,
  origin_kind   TEXT NOT NULL CHECK (origin_kind IN ('routine','task','mail','auto')),
  origin_id     TEXT NOT NULL, run_id TEXT, thread_id TEXT,
  state         TEXT NOT NULL CHECK (state IN ('held','executing','executed','discarded','failed')),
  result_json   TEXT,
  created_at    INTEGER NOT NULL, decided_at INTEGER
);

-- ── notifications, status snapshot ──────────────────────────── F-DB-*, F-EN-05
CREATE TABLE notifications (
  id          TEXT PRIMARY KEY,
  kind        TEXT NOT NULL,                   -- approval|question|done|failed|routine_ready|task_review|lane_cleanup|update_ready|credit_low|plugin_attention|held_actions|mail_refused
  title       TEXT NOT NULL, body TEXT NOT NULL,   -- redacted
  source_json TEXT NOT NULL,                   -- SourceRef
  state       TEXT NOT NULL CHECK (state IN ('unread','read','resolved')),
  dedupe_key  TEXT,                            -- e.g. approval id; resolves together
  created_at  INTEGER NOT NULL, read_at INTEGER
);
CREATE INDEX notifications_recent ON notifications(created_at DESC);
CREATE INDEX notifications_dedupe ON notifications(dedupe_key);
CREATE TABLE status_snapshot (
  session_key TEXT PRIMARY KEY, status TEXT NOT NULL, reason TEXT, since INTEGER NOT NULL, ts INTEGER NOT NULL, payload_json TEXT
);

-- ── task execution runtime (issue records themselves are files, §9) ── F-TK-04
CREATE TABLE task_attempts (
  id TEXT PRIMARY KEY, issue_id TEXT NOT NULL, workspace_id TEXT NOT NULL, lane_workspace_id TEXT,
  thread_id TEXT, teammate_id TEXT, engine_id TEXT NOT NULL, account_id TEXT,
  access TEXT NOT NULL, review INTEGER NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('queued','setup','running','reviewing','in_review','stopped','failed','done')),
  queued_at INTEGER NOT NULL, started_at INTEGER, ended_at INTEGER, review_verdict TEXT, error TEXT
);
CREATE INDEX task_attempts_active ON task_attempts(state, queued_at);

-- ── Auto, Conductor ─────────────────────────────────────────── F-AU-01, F-CN-04
CREATE TABLE auto_runs (
  id TEXT PRIMARY KEY, pane_id TEXT NOT NULL, mode TEXT NOT NULL, goal TEXT, ceiling INTEGER NOT NULL,
  turns INTEGER NOT NULL DEFAULT 0, state TEXT NOT NULL CHECK (state IN ('running','paused','ended')),
  end_reason TEXT, created_at INTEGER NOT NULL, ended_at INTEGER
);
CREATE TABLE conductor_calls (
  id TEXT PRIMARY KEY, started_at INTEGER NOT NULL, ended_at INTEGER, connected_seconds INTEGER, credits INTEGER, end_reason TEXT
);
CREATE TABLE conductor_call_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT, call_id TEXT NOT NULL REFERENCES conductor_calls(id) ON DELETE CASCADE,
  ts INTEGER NOT NULL, kind TEXT NOT NULL CHECK (kind IN ('user','assistant','action')), text TEXT NOT NULL, ok INTEGER
);   -- retained 7 days

-- ── Dictate ─────────────────────────────────────────────────── F-DI-08/09
CREATE TABLE dictate_takes (
  id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, text TEXT NOT NULL, engine TEXT NOT NULL, model TEXT NOT NULL,
  word_count INTEGER NOT NULL, duration_ms INTEGER NOT NULL, app_id TEXT, deleted INTEGER NOT NULL DEFAULT 0
);
CREATE VIRTUAL TABLE dictate_fts USING fts5(text, content='dictate_takes', content_rowid='rowid');
CREATE TABLE dictate_stats (id INTEGER PRIMARY KEY CHECK (id = 1), words INTEGER NOT NULL, speaking_ms INTEGER NOT NULL, takes INTEGER NOT NULL);
CREATE TABLE dictionary_rules (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK (kind IN ('word','correction')),
  preferred TEXT, variants_json TEXT, phrase TEXT, replacement TEXT,
  match_case INTEGER NOT NULL DEFAULT 0, protect INTEGER NOT NULL DEFAULT 1,
  spacing TEXT NOT NULL DEFAULT 'exact' CHECK (spacing IN ('exact','flexible')),
  enabled INTEGER NOT NULL DEFAULT 1, lang TEXT NOT NULL DEFAULT '*', app_profiles_json TEXT NOT NULL DEFAULT '[]',
  updated_at INTEGER NOT NULL, deleted_at INTEGER, sync_rev INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE dictionary_suggestions (
  id TEXT PRIMARY KEY, term TEXT NOT NULL, kind TEXT NOT NULL, evidence_json TEXT NOT NULL,
  first_seen INTEGER NOT NULL, last_seen INTEGER NOT NULL, state TEXT NOT NULL CHECK (state IN ('open','remembered','dismissed'))
);

-- ── remote, editor, audit ───────────────────────────────────── F-RM-03, F-CD-19, F-TL-10
CREATE TABLE pairings (
  id TEXT PRIMARY KEY, device_name TEXT NOT NULL, grants_json TEXT NOT NULL, epoch INTEGER NOT NULL DEFAULT 0,
  tx_counter INTEGER NOT NULL DEFAULT 0, rx_counter INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL, last_seen INTEGER, revoked_at INTEGER
);
CREATE TABLE phone_queue (
  id TEXT PRIMARY KEY, pairing_id TEXT NOT NULL REFERENCES pairings(id) ON DELETE CASCADE,
  action_json TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('queued','applied','rejected')), created_at INTEGER NOT NULL
);
CREATE TABLE editor_tabs (context_key TEXT PRIMARY KEY, state_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE audit_outbox (id TEXT PRIMARY KEY, event TEXT NOT NULL, meta_json TEXT NOT NULL, created_at INTEGER NOT NULL, sent_at INTEGER);

-- The local machine is always host 'local'.
INSERT INTO hosts (id, kind, label, created_at) VALUES ('local', 'local', 'This computer', 0);
