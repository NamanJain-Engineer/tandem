//! Shapes that cross IPC (ARCHITECTURE §3.2 "Launch, handoff, lanes, git").

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One letter in the UI: A, M, D, R, T, U (untracked) and C (conflicted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FileChange {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChanged,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileStatus {
    /// Relative to the repository root, `/`-separated.
    pub path: String,
    pub change: FileChange,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: Vec<FileStatus>,
    /// Worktree changes, untracked files and conflicts.
    pub unstaged: Vec<FileStatus>,
    pub no_commits: bool,
}

impl GitStatus {
    /// No staged, modified, untracked or conflicted file.
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty()
    }

    /// Changes to tracked files only (untracked files don't block a merge).
    pub fn has_tracked_changes(&self) -> bool {
        !self.staged.is_empty()
            || self
                .unstaged
                .iter()
                .any(|f| f.change != FileChange::Untracked)
    }
}

/// One file of a diff, as the DiffView renders it (UI_SPEC §2.25).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileDiff {
    pub path: String,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub change: FileChange,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
    /// Unified diff text with its `diff --git` header; empty when binary.
    pub patch: String,
    /// The patch was cut at [`crate::repo::MAX_PATCH_BYTES`].
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MergeReport {
    pub merged: bool,
    /// Files left conflicted in the parent checkout. Nothing was committed.
    pub conflicts: Vec<String>,
    /// The merge commit, when one was made ("Already up to date" makes none).
    pub commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StaleLane {
    pub branch: String,
    pub path: String,
    /// Days since the last commit or checkout activity in the lane.
    pub idle_days: u32,
}
