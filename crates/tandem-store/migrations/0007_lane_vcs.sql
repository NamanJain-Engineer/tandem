-- 0007 · F-CD-24 (D-2598): worktrees made by another version control system
-- than git. A row here marks a jj workspace: its workspaces row's
-- lane_branch is the jj workspace's name, not a git branch.

CREATE TABLE IF NOT EXISTS lane_vcs (
  lane_id    TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
  vcs        TEXT NOT NULL
);
