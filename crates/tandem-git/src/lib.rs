//! Git reads, worktree lanes, commit and push (ARCHITECTURE §2.6, §7). Built in M6 and M9.

pub mod cli;
mod error;
pub mod exclude;
pub mod lanes;
pub mod repo;
pub mod slug;
pub mod types;

pub use cli::{Git, Mode};
pub use error::GitError;
pub use lanes::{briefing, ForceConfirm, Lane, LaneManager};
pub use repo::Repo;
pub use slug::slugify;
pub use types::{FileChange, FileDiff, FileStatus, GitStatus, MergeReport, StaleLane};
