-- F-EX-06: a Best of N lane waiting its turn ("One check at a time") reads
-- 'queued'. SQLite can't change a CHECK constraint, so the table is rebuilt.
CREATE TABLE bestof_lanes_new (
  run_id       TEXT NOT NULL REFERENCES bestof_runs(id) ON DELETE CASCADE,
  seat         INTEGER NOT NULL,
  lane_id      TEXT,
  branch       TEXT,
  engine_id    TEXT NOT NULL,
  account_id   TEXT,
  pane_id      TEXT,
  error        TEXT,
  check_state  TEXT NOT NULL DEFAULT 'none' CHECK (check_state IN ('none','queued','running','passed','failed','stopped')),
  check_exit   INTEGER,
  check_output TEXT,
  PRIMARY KEY (run_id, seat)
);
INSERT INTO bestof_lanes_new
  SELECT run_id, seat, lane_id, branch, engine_id, account_id, pane_id, error, check_state, check_exit, check_output
  FROM bestof_lanes;
DROP TABLE bestof_lanes;
ALTER TABLE bestof_lanes_new RENAME TO bestof_lanes;
