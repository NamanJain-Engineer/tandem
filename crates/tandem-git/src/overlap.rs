//! Overlap check (F-WT-03): which files parallel worktrees share, and a
//! conflict forecast for merging one into its repository's HEAD.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cli::{Git, Mode};
use crate::GitError;

/// The result of `git merge-tree --write-tree` for a worktree's branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case", tag = "kind")]
#[ts(export)]
pub enum Forecast {
    Clean,
    Conflicts { files: Vec<String> },
    /// This git can't forecast (it needs 2.38 or later), or the merge-tree
    /// run failed for another reason.
    Unavailable { reason: String },
}

pub const OLD_GIT: &str = "The forecast needs git 2.38 or later";

/// Files changed in the checkout at `root` since `base`: committed,
/// staged, unstaged and untracked (ignored files aren't changes).
pub async fn changed_files(git: &Git, root: &Path, base: &str) -> Result<BTreeSet<String>, GitError> {
    let diff = git
        .ok(
            Mode::Read,
            root,
            &[
                OsStr::new("diff"),
                OsStr::new("--name-only"),
                OsStr::new("-z"),
                OsStr::new("--no-renames"),
                OsStr::new(base),
            ],
        )
        .await?;
    let untracked = git
        .ok(
            Mode::Read,
            root,
            &[
                OsStr::new("ls-files"),
                OsStr::new("--others"),
                OsStr::new("--exclude-standard"),
                OsStr::new("-z"),
            ],
        )
        .await?;
    Ok(split_z(&diff.stdout)
        .chain(split_z(&untracked.stdout))
        .collect())
}

fn split_z(bytes: &[u8]) -> impl Iterator<Item = String> + '_ {
    bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
}

/// `git merge-tree --write-tree --name-only --no-messages <into> <branch>`
/// in the repository at `root`. It merges in memory: the working tree,
/// index and refs stay as they are.
pub async fn forecast(git: &Git, root: &Path, into: &str, branch: &str) -> Result<Forecast, GitError> {
    let out = git
        .run(
            Mode::Read,
            root,
            &[
                OsStr::new("merge-tree"),
                OsStr::new("--write-tree"),
                OsStr::new("--name-only"),
                OsStr::new("--no-messages"),
                OsStr::new("-z"),
                OsStr::new(into),
                OsStr::new(branch),
            ],
        )
        .await?;
    Ok(parse_forecast(out.code, &out.stdout, &out.stderr))
}

/// Exit 0 is clean; exit 1 lists the conflicted files after the tree id.
fn parse_forecast(code: Option<i32>, stdout: &[u8], stderr: &str) -> Forecast {
    match code {
        Some(0) => Forecast::Clean,
        Some(1) => {
            // With -z: "<tree>\0<path>\0<path>\0\0" (the empty field ends the
            // list; --no-messages leaves nothing after it).
            let mut fields = stdout.split(|b| *b == 0);
            let _tree = fields.next();
            let files: BTreeSet<String> = fields
                .take_while(|f| !f.is_empty())
                .map(|f| String::from_utf8_lossy(f).into_owned())
                .collect();
            if files.is_empty() {
                Forecast::Unavailable {
                    reason: "git reported a conflict without naming files".into(),
                }
            } else {
                Forecast::Conflicts {
                    files: files.into_iter().collect(),
                }
            }
        }
        _ if stderr.contains("--write-tree") || stderr.contains("usage: git merge-tree") => {
            Forecast::Unavailable {
                reason: OLD_GIT.into(),
            }
        }
        _ => Forecast::Unavailable {
            reason: stderr.lines().last().unwrap_or("git merge-tree failed").to_string(),
        },
    }
}

/// The files both sets share, sorted.
pub fn shared(a: &BTreeSet<String>, b: &BTreeSet<String>) -> Vec<String> {
    a.intersection(b).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_wt_03_2_parses_merge_tree_output() {
        assert_eq!(parse_forecast(Some(0), b"abc\0", ""), Forecast::Clean);
        assert_eq!(
            parse_forecast(Some(1), b"abc\0src/a.ts\0README.md\0src/a.ts\0\0", ""),
            Forecast::Conflicts {
                files: vec!["README.md".into(), "src/a.ts".into()]
            }
        );
        assert_eq!(
            parse_forecast(Some(129), b"", "error: unknown option `write-tree'\nusage: git merge-tree"),
            Forecast::Unavailable {
                reason: OLD_GIT.into()
            }
        );
        assert!(matches!(
            parse_forecast(Some(128), b"", "fatal: bad revision"),
            Forecast::Unavailable { .. }
        ));
    }

    #[test]
    fn f_wt_03_1_shared_files_are_the_intersection() {
        let a: BTreeSet<String> = ["src/a.ts", "b.ts"].iter().map(|s| s.to_string()).collect();
        let b: BTreeSet<String> = ["src/a.ts", "c.ts"].iter().map(|s| s.to_string()).collect();
        let c: BTreeSet<String> = ["d.ts"].iter().map(|s| s.to_string()).collect();
        assert_eq!(shared(&a, &b), vec!["src/a.ts".to_string()]);
        assert!(shared(&a, &c).is_empty());
    }
}
