//! What worktree lanes share between this computer and an SSH host's helper
//! (F-CD-15, D-1602): slugs, the `.git/info/exclude` line and where lanes
//! live. `tandem_git::lanes` runs them here; `tandem-host` on a host.

pub mod exclude;
pub mod slug;

/// Under the main checkout's root.
pub const WORKTREES_DIR: &str = ".tandem/worktrees";
pub const BRANCH_PREFIX: &str = "tandem/";
/// A merged lane is listed for clean-up after this many idle days.
pub const STALE_AFTER_DAYS: i64 = 7;
