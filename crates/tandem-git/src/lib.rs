//! Git reads, worktree lanes, commit and push (ARCHITECTURE §2.6, §7). Built in M6 and M9.

pub mod checkpoint;
pub mod cli;
mod error;
pub use tandem_core::lanes::exclude;
pub mod lanes;
pub mod ops;
pub mod repo;
pub use tandem_core::lanes::slug;
pub mod types;

pub use cli::{Git, Mode};
pub use error::GitError;
pub use lanes::{briefing, ForceConfirm, Lane, LaneManager};
pub use repo::{diff_texts, Repo};
pub use slug::slugify;
pub use types::{Branch, FileChange, FileDiff, FileStatus, GitStatus, MergeReport, StaleLane};
