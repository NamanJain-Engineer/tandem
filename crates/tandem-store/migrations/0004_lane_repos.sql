-- 0004 · F-CD-22 (D-2531): a multi-repo worktree's repositories. Its workspaces row
-- is the parent folder (lane_base ''); each repository's git worktree is
-- here, all on the row's lane_branch.
-- IF NOT EXISTS: development databases ran this as 0003 before F-EX-06's
-- 0003_bestof_queued took that number.

CREATE TABLE IF NOT EXISTS lane_repos (
  lane_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  repo_root  TEXT NOT NULL,
  root       TEXT NOT NULL,
  base       TEXT NOT NULL,
  PRIMARY KEY (lane_id, name)
);
