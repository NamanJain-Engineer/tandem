-- F-EX-07: the activity ledger's own events (turns sent, tools, subagents,
-- pane approvals, vibezeo run). Commands, checkpoints and thread approvals
-- are read from their own tables. Never prompt text, file contents, output
-- or secret values: `summary` is redacted, `detail_json` holds names and
-- numbers only.
CREATE TABLE activity (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  source_kind   TEXT NOT NULL CHECK (source_kind IN ('pane','thread')),
  source_id     TEXT NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('turn','tool','subagent','approval','run')),
  at            INTEGER NOT NULL,
  summary       TEXT NOT NULL,
  detail_json   TEXT NOT NULL DEFAULT '{}',
  checkpoint_id TEXT
);
CREATE INDEX activity_source ON activity(source_kind, source_id, at);
CREATE INDEX activity_at ON activity(at);

-- The ledger reads these by pane or thread, newest first.
CREATE INDEX IF NOT EXISTS command_history_pane ON command_history(pane_id, started_at);
CREATE INDEX IF NOT EXISTS approvals_source ON approvals(source_kind, source_id, created_at);
CREATE INDEX IF NOT EXISTS checkpoints_pane ON checkpoints(pane_id, created_at);
