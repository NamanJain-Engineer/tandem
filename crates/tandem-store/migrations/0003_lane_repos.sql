-- F-CD-22 (D-2531): a multi-repo worktree's repositories. Its workspaces row
-- is the parent folder (lane_base ''); each repository's git worktree is
-- here, all on the row's lane_branch.

CREATE TABLE lane_repos (
  lane_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  repo_root  TEXT NOT NULL,
  root       TEXT NOT NULL,
  base       TEXT NOT NULL,
  PRIMARY KEY (lane_id, name)
);
