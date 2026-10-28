-- F-EX-07: the activity ledger's own events (turns sent, tools, subagents,
-- pane approvals, vibezeo run). Commands, checkpoints and thread approvals
-- are read from their own tables. Never prompt text, file contents, output
-- or secret values: `summary` is redacted, `detail_json` holds names and
-- numbers only.
CREATE TABLE activity (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  source_kind   TEXT NOT NULL CHECK (source_kind IN ('pane','thread')),
  source_id     TEXT NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('turn','tool','subagent','approval','run','note')),
  at            INTEGER NOT NULL,
  summary       TEXT NOT NULL,
  detail_json   TEXT NOT NULL DEFAULT '{}',
  checkpoint_id TEXT,
  -- How it's pruned besides by age (repo::activity::Keep). 0: by the
  -- per-session and total counts. 1: under a per-source ceiling instead
  -- (what an agent can still drive: rule runs, answers it caused by going
  -- away, notes). 2: never by count (the person's own decisions).
  kept          INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX activity_source ON activity(source_kind, source_id, at);
CREATE INDEX activity_at ON activity(at);
-- Pruning by count reads only the rows that aren't kept.
CREATE INDEX activity_kept ON activity(kept, source_kind, source_id, at);

-- The ledger reads these by pane or thread, newest first.
CREATE INDEX IF NOT EXISTS command_history_pane ON command_history(pane_id, started_at);
CREATE INDEX IF NOT EXISTS approvals_source ON approvals(source_kind, source_id, created_at);
CREATE INDEX IF NOT EXISTS checkpoints_pane ON checkpoints(pane_id, created_at);
