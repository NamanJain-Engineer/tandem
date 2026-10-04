CREATE TABLE IF NOT EXISTS sync_events (
  seq bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  id text NOT NULL UNIQUE,
  workspace_id text NOT NULL,
  kind text NOT NULL,
  payload jsonb NOT NULL,
  created_at timestamptz NOT NULL
);

CREATE INDEX IF NOT EXISTS sync_events_workspace_seq_idx ON sync_events (workspace_id, seq);
