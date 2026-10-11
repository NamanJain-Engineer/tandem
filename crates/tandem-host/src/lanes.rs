//! Worktree lanes on an SSH host (F-CD-15 there, D-1602): the same lanes as
//! `tandem_git::lanes` locally (`<repo>/.tandem/worktrees/<slug>` on
//! `tandem/<slug>`, the same slugs, exclude line and rules), made with the
//! host's own git as the host user.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tandem_core::lanes::exclude::{ensure_excluded, TANDEM_DIR_LINE};
use tandem_core::lanes::slug::{dedupe, slugify};
use tandem_core::lanes::{BRANCH_PREFIX, STALE_AFTER_DAYS, WORKTREES_DIR};

use crate::gitops::{self, git_err, read, stderr_tail, toplevel, FileChange, FileDiff};
use crate::protocol::{
    ErrorKind, LaneCreate, LaneCreated, LaneDiff, LaneMerge, LaneMergeReport, LaneRemove,
    LaneStale, RpcError,
};

fn text(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ok_text(cwd: &Path, args: &[&str]) -> Result<String, RpcError> {
    let out = read(cwd, args)?;
    if out.status.success() {
        Ok(text(&out))
    } else {
        Err(git_err(stderr_tail(&out.stderr)))
    }
}

/// The repository at `root`: its checkout's top, the main checkout's root
/// (where `.tandem/worktrees` lives, also from inside a lane) and the
/// shared git dir.
struct Located {
    top: PathBuf,
    main: PathBuf,
    common: PathBuf,
}

fn locate(root: &str) -> Result<Located, RpcError> {
    let top = toplevel(Path::new(root))?
        .ok_or_else(|| RpcError::new(ErrorKind::Invalid, "Not a git repository"))?;
    let common = PathBuf::from(ok_text(
        &top,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    // `worktree list` names the main checkout first.
    let list = ok_text(&top, &["worktree", "list", "--porcelain"])?;
    let main = list
        .lines()
        .find_map(|l| l.strip_prefix("worktree "))
        .map(PathBuf::from)
        .unwrap_or_else(|| top.clone());
    Ok(Located { top, main, common })
}

fn head(top: &Path) -> Option<String> {
    let out = read(top, &["rev-parse", "--verify", "-q", "HEAD"]).ok()?;
    out.status
        .success()
        .then(|| text(&out))
        .filter(|h| !h.is_empty())
}

fn dirty(root: &Path) -> Result<bool, RpcError> {
    Ok(gitops::status(root)?
        .map(|s| !s.status.staged.is_empty() || !s.status.unstaged.is_empty())
        .unwrap_or(false))
}

fn tracked_changes(root: &Path) -> Result<bool, RpcError> {
    Ok(gitops::status(root)?
        .map(|s| {
            !s.status.staged.is_empty()
                || s.status
                    .unstaged
                    .iter()
                    .any(|f| f.change != FileChange::Untracked)
        })
        .unwrap_or(false))
}

/// `lane.create`: `git worktree add .tandem/worktrees/<slug> -b tandem/<slug>
/// <HEAD>` from the checkout at `root` (a lane's own HEAD when it's one).
pub fn create(p: &LaneCreate) -> Result<LaneCreated, RpcError> {
    let at = locate(&p.root)?;
    let base = head(&at.top).ok_or_else(|| {
        RpcError::new(
            ErrorKind::Invalid,
            "Lanes start from a commit. Make a first commit, then try again.",
        )
    })?;
    let mut taken: HashSet<String> = ok_text(
        &at.top,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads/tandem",
        ],
    )?
    .lines()
    .filter_map(|b| b.strip_prefix(BRANCH_PREFIX).map(str::to_string))
    .collect();
    let dir = at.main.join(WORKTREES_DIR);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            taken.insert(e.file_name().to_string_lossy().into_owned());
        }
    }
    taken.extend(p.also_taken.iter().cloned());
    let slug = dedupe(&slugify(&p.slug_source), &taken);
    let branch = format!("{BRANCH_PREFIX}{slug}");
    ensure_excluded(&at.common, TANDEM_DIR_LINE).map_err(|e| git_err(e.to_string()))?;
    std::fs::create_dir_all(&dir).map_err(|e| git_err(e.to_string()))?;
    let path = dir.join(&slug);
    let path_s = path.to_string_lossy().into_owned();
    let out = read(&at.top, &["worktree", "add", &path_s, "-b", &branch, &base])?;
    if !out.status.success() {
        let err = stderr_tail(&out.stderr);
        return Err(if err.contains("already exists") {
            RpcError::new(ErrorKind::Conflict, format!("{branch} already exists"))
        } else {
            git_err(err)
        });
    }
    let path = path.canonicalize().unwrap_or(path);
    Ok(LaneCreated {
        slug,
        branch,
        path: path.to_string_lossy().into_owned(),
        base,
    })
}

/// A lane's folder: under the repository's `.tandem/worktrees`, nothing else
/// (`lane.remove` never touches another folder, whatever it's sent).
fn lane_path(main: &Path, path: &str) -> Result<PathBuf, RpcError> {
    let dir = main
        .join(WORKTREES_DIR)
        .canonicalize()
        .unwrap_or_else(|_| main.join(WORKTREES_DIR));
    let wanted = Path::new(path);
    let resolved = wanted
        .canonicalize()
        .unwrap_or_else(|_| wanted.to_path_buf());
    let inside = resolved.parent() == Some(dir.as_path())
        && resolved
            .file_name()
            .is_some_and(|n| !n.is_empty() && n != "." && n != "..");
    if inside {
        Ok(resolved)
    } else {
        Err(RpcError::new(
            ErrorKind::PathEscape,
            "That folder isn't one of this repository's worktrees",
        ))
    }
}

/// `lane.remove`: `git worktree remove` (with `--force` only when asked,
/// which only a person in the main window can do). Uncommitted or
/// untracked files without `force` refuse with Conflict. The branch stays.
pub fn remove(p: &LaneRemove) -> Result<(), RpcError> {
    let at = locate(&p.root)?;
    let path = lane_path(&at.main, &p.path)?;
    if !path.exists() {
        ok_text(&at.main, &["worktree", "prune"])?;
        return Ok(());
    }
    if !p.force && dirty(&path)? {
        return Err(RpcError::new(ErrorKind::Conflict, "Uncommitted changes"));
    }
    let path_s = path.to_string_lossy().into_owned();
    let mut args = vec!["worktree", "remove"];
    if p.force {
        args.push("--force");
    }
    args.push(&path_s);
    let out = read(&at.main, &args)?;
    if out.status.success() {
        Ok(())
    } else {
        let err = stderr_tail(&out.stderr);
        Err(if err.contains("modified or untracked files") {
            RpcError::new(ErrorKind::Conflict, "Uncommitted changes")
        } else {
            git_err(err)
        })
    }
}

/// `lane.merge`: `git merge --no-ff --no-edit tandem/<slug>` in the checkout
/// at `root`. Refuses with tracked changes there; a conflict stops it,
/// commits nothing and is reported with its files.
pub fn merge(p: &LaneMerge) -> Result<LaneMergeReport, RpcError> {
    if !p.branch.starts_with(BRANCH_PREFIX) || p.branch.starts_with('-') {
        return Err(RpcError::new(
            ErrorKind::Invalid,
            "That isn't a worktree's branch",
        ));
    }
    let at = locate(&p.root)?;
    if tracked_changes(&at.top)? {
        return Err(RpcError::new(ErrorKind::Conflict, "Uncommitted changes"));
    }
    let before = head(&at.top);
    let out = crate::gitops::write_output(&at.top, &["merge", "--no-ff", "--no-edit", &p.branch])?;
    if out.status.success() {
        let after = head(&at.top);
        return Ok(LaneMergeReport {
            merged: true,
            conflicts: Vec::new(),
            commit: after.filter(|a| Some(a) != before.as_ref()),
        });
    }
    let conflicts: Vec<String> = ok_text(&at.top, &["diff", "--name-only", "--diff-filter=U"])?
        .lines()
        .map(str::to_string)
        .filter(|l| !l.is_empty())
        .collect();
    if conflicts.is_empty() {
        return Err(git_err(stderr_tail(&out.stderr)));
    }
    Ok(LaneMergeReport {
        merged: false,
        conflicts,
        commit: None,
    })
}

fn is_sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `lane.diff`: everything the lane at `root` changed since `base`, the
/// commit it started from, uncommitted and untracked work included.
pub fn diff(p: &LaneDiff) -> Result<Vec<FileDiff>, RpcError> {
    if !is_sha(&p.base) {
        return Err(RpcError::new(ErrorKind::Invalid, "That isn't a commit"));
    }
    let at = locate(&p.root)?;
    let raw = read(&at.top, &["diff", "--name-status", "-z", "-M", &p.base])?;
    if !raw.status.success() {
        return Err(git_err(stderr_tail(&raw.stderr)));
    }
    let fields: Vec<String> = String::from_utf8_lossy(&raw.stdout)
        .split('\0')
        .map(str::to_string)
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < fields.len() {
        let code = fields[i].clone();
        let Some(letter) = code.chars().next() else {
            i += 1;
            continue;
        };
        let (old, path, step) = if letter == 'R' || letter == 'C' {
            (fields.get(i + 1).cloned(), fields.get(i + 2).cloned(), 3)
        } else {
            (None, fields.get(i + 1).cloned(), 2)
        };
        i += step;
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            continue;
        };
        let change = match letter {
            'A' => FileChange::Added,
            'D' => FileChange::Deleted,
            'R' => FileChange::Renamed,
            'T' => FileChange::TypeChanged,
            _ => FileChange::Modified,
        };
        let mut args = vec!["diff", "-M", p.base.as_str(), "--"];
        if let Some(o) = &old {
            args.push(o);
        }
        args.push(&path);
        let patch = read(&at.top, &args)?;
        out.push(gitops::parse_patch(
            &path,
            &patch.stdout,
            Some(change),
            old.clone(),
        ));
    }
    let untracked = read(
        &at.top,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    for path in String::from_utf8_lossy(&untracked.stdout).split('\0') {
        if path.is_empty() {
            continue;
        }
        let patch = read(&at.top, &["diff", "--no-index", "--", "/dev/null", path])?;
        out.push(gitops::parse_patch(
            path,
            &patch.stdout,
            Some(FileChange::Untracked),
            None,
        ));
    }
    Ok(out)
}

fn modified_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// `lane.sweep`: `git worktree prune`, then this repository's lanes merged
/// into the main checkout's HEAD and idle over 7 days. Deletes nothing.
pub fn sweep(root: &str) -> Result<Vec<LaneStale>, RpcError> {
    let at = locate(root)?;
    ok_text(&at.main, &["worktree", "prune"])?;
    let Some(main_head) = head(&at.main) else {
        return Ok(Vec::new());
    };
    let dir = at.main.join(WORKTREES_DIR);
    let dir = dir.canonicalize().unwrap_or(dir);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let list = ok_text(&at.main, &["worktree", "list", "--porcelain"])?;
    let mut stale = Vec::new();
    for block in list.split("\n\n") {
        let path = block.lines().find_map(|l| l.strip_prefix("worktree "));
        let branch = block
            .lines()
            .find_map(|l| l.strip_prefix("branch refs/heads/"));
        let (Some(path), Some(branch)) = (path, branch) else {
            continue;
        };
        let path = PathBuf::from(path);
        if !branch.starts_with(BRANCH_PREFIX) || path.parent() != Some(dir.as_path()) {
            continue;
        }
        let merged = read(
            &at.main,
            &["merge-base", "--is-ancestor", branch, &main_head],
        )?
        .status
        .success();
        if !merged {
            continue;
        }
        let mut last: u64 = ok_text(&at.main, &["log", "-1", "--format=%ct", branch])?
            .parse()
            .unwrap_or(0);
        if let Ok(gitdir) = ok_text(&path, &["rev-parse", "--absolute-git-dir"]) {
            for file in ["index", "HEAD"] {
                if let Some(t) = modified_secs(&Path::new(&gitdir).join(file)) {
                    last = last.max(t);
                }
            }
        }
        let idle = now.saturating_sub(last) / 86_400;
        if idle as i64 > STALE_AFTER_DAYS {
            stale.push(LaneStale {
                branch: branch.to_string(),
                path: path.to_string_lossy().into_owned(),
                idle_days: u32::try_from(idle).unwrap_or(u32::MAX),
            });
        }
    }
    Ok(stale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().join("shop");
        std::fs::create_dir_all(&root).expect("mkdir");
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "T"]);
        git(&root, &["config", "user.email", "t@example.com"]);
        std::fs::write(root.join("a.txt"), "one\n").expect("w");
        git(&root, &["add", "a.txt"]);
        git(&root, &["commit", "-qm", "one"]);
        let root = root.canonicalize().expect("canon");
        (dir, root)
    }

    fn new_lane(root: &Path, name: &str) -> LaneCreated {
        create(&LaneCreate {
            root: root.to_string_lossy().into(),
            slug_source: name.into(),
            also_taken: Vec::new(),
        })
        .expect("create")
    }

    #[test]
    fn f_rm_01_f_cd_15_lanes_on_a_host_are_made_and_kept_like_local_ones() {
        let (_dir, root) = repo();
        let lane = new_lane(&root, "Fix auth!");
        assert_eq!(lane.slug, "fix-auth");
        assert_eq!(lane.branch, "tandem/fix-auth");
        assert_eq!(
            PathBuf::from(&lane.path),
            root.join(".tandem/worktrees/fix-auth")
        );
        assert_eq!(lane.base, git(&root, &["rev-parse", "HEAD"]));
        // The same name again gets the next free slug; .tandem/ stays out of status.
        assert_eq!(new_lane(&root, "fix auth").slug, "fix-auth-2");
        let exclude = std::fs::read_to_string(root.join(".git/info/exclude")).expect("exclude");
        assert!(exclude.lines().any(|l| l == ".tandem/"));
        assert!(git(&root, &["status", "--porcelain"]).is_empty());

        // The lane's work, committed and not, against where it started.
        let lp = PathBuf::from(&lane.path);
        std::fs::write(lp.join("a.txt"), "one\ntwo\n").expect("w");
        git(&lp, &["commit", "-qam", "two"]);
        std::fs::write(lp.join("new.txt"), "fresh\n").expect("w");
        let d = diff(&LaneDiff {
            root: lane.path.clone(),
            base: lane.base.clone(),
        })
        .expect("diff");
        let paths: Vec<(&str, FileChange)> =
            d.iter().map(|f| (f.path.as_str(), f.change)).collect();
        assert_eq!(
            paths,
            vec![
                ("a.txt", FileChange::Modified),
                ("new.txt", FileChange::Untracked)
            ]
        );
        assert!(d[0].patch.contains("+two"));
        assert!(diff(&LaneDiff {
            root: lane.path.clone(),
            base: "--output=/tmp/x".into(),
        })
        .is_err());

        // Dirty: removing refuses until forced; outside .tandem/worktrees never.
        let rm = |path: &str, force: bool| {
            remove(&LaneRemove {
                root: root.to_string_lossy().into(),
                path: path.into(),
                force,
            })
        };
        assert_eq!(
            rm(&lane.path, false).expect_err("dirty").kind,
            ErrorKind::Conflict
        );
        assert_eq!(
            rm(&root.to_string_lossy(), true)
                .expect_err("not a lane")
                .kind,
            ErrorKind::PathEscape
        );
        assert_eq!(
            rm(&format!("{}/../../..", lane.path), true)
                .expect_err("escape")
                .kind,
            ErrorKind::PathEscape
        );
        std::fs::remove_file(lp.join("new.txt")).expect("rm");

        // Merge into the main checkout: a merge commit.
        let m = merge(&LaneMerge {
            root: root.to_string_lossy().into(),
            branch: lane.branch.clone(),
        })
        .expect("merge");
        assert!(m.merged && m.conflicts.is_empty() && m.commit.is_some());
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).expect("r"),
            "one\ntwo\n"
        );
        assert!(merge(&LaneMerge {
            root: root.to_string_lossy().into(),
            branch: "main".into(),
        })
        .is_err());

        // A conflicting lane stops, commits nothing, names the file.
        let other = new_lane(&root, "other");
        let op = PathBuf::from(&other.path);
        std::fs::write(op.join("a.txt"), "theirs\n").expect("w");
        git(&op, &["commit", "-qam", "theirs"]);
        std::fs::write(root.join("a.txt"), "ours\n").expect("w");
        git(&root, &["commit", "-qam", "ours"]);
        let before = git(&root, &["rev-parse", "HEAD"]);
        let c = merge(&LaneMerge {
            root: root.to_string_lossy().into(),
            branch: other.branch.clone(),
        })
        .expect("report");
        assert!(!c.merged);
        assert_eq!(c.conflicts, vec!["a.txt".to_string()]);
        assert_eq!(git(&root, &["rev-parse", "HEAD"]), before);
        git(&root, &["merge", "--abort"]);

        // A clean lane is removed; its branch stays.
        rm(&lane.path, false).expect("remove");
        assert!(!lp.exists());
        assert!(git(&root, &["branch", "--list", "tandem/fix-auth"]).contains("tandem/fix-auth"));
        rm(&other.path, true).expect("forced");
    }

    #[test]
    fn f_rm_01_f_cd_15_the_sweep_lists_merged_lanes_idle_over_a_week() {
        let (_dir, root) = repo();
        let old = new_lane(&root, "old");
        let fresh = new_lane(&root, "fresh");
        let op = PathBuf::from(&old.path);
        // Old work: a commit dated ten days ago, and its index and HEAD too.
        std::fs::write(op.join("a.txt"), "old\n").expect("w");
        let then = "2000-01-01T00:00:00";
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@example.com",
                "commit",
                "-qam",
                "old",
            ])
            .env("GIT_AUTHOR_DATE", then)
            .env("GIT_COMMITTER_DATE", then)
            .current_dir(&op)
            .output()
            .expect("git");
        assert!(out.status.success());
        let gitdir = git(&op, &["rev-parse", "--absolute-git-dir"]);
        for f in ["index", "HEAD"] {
            Command::new("touch")
                .args(["-d", then, &format!("{gitdir}/{f}")])
                .status()
                .expect("touch");
        }
        git(&root, &["merge", "-q", "--no-ff", "--no-edit", &old.branch]);
        let stale = sweep(&root.to_string_lossy()).expect("sweep");
        assert_eq!(stale.len(), 1, "{stale:?}");
        assert_eq!(stale[0].branch, old.branch);
        assert!(stale[0].idle_days > 7);
        // Nothing was removed.
        assert!(op.exists() && PathBuf::from(&fresh.path).exists());
    }
}
