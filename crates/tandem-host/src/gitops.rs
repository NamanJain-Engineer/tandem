//! `git.*` on the host, through the host's own `git` CLI. Reads are
//! hardened like local ones (ARCHITECTURE §7): no fsmonitor, hooks,
//! credential helpers, pagers, external diff or textconv, and every filter
//! driver the repository's own config names is emptied. Writes (stage,
//! unstage, discard, commit) run the user's git as it is, hooks included.
//! The JSON shapes match `tandem_git::types` so the desktop reads them as
//! its own.

use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::protocol::{ErrorKind, GitOp, GitOpKind, RpcError};

pub const MAX_PATCH_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChange {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChanged,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStatus {
    pub path: String,
    pub change: FileChange,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: Vec<FileStatus>,
    pub unstaged: Vec<FileStatus>,
    pub no_commits: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub old_path: Option<String>,
    pub change: FileChange,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
    pub patch: String,
    pub truncated: bool,
}

/// `git.status`'s reply: the repository's top folder (status paths are
/// relative to it) and its status; `None` outside a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoState {
    pub top: String,
    pub status: GitStatus,
}

const READ_OVERRIDES: &[&str] = &[
    "core.fsmonitor=",
    "core.hooksPath=/dev/null",
    "credential.helper=",
    "core.pager=cat",
    "core.sshCommand=",
    "core.askPass=",
    "uploadpack.packObjectsHook=",
];

fn git_err(msg: impl Into<String>) -> RpcError {
    RpcError::new(ErrorKind::Io, msg)
}

fn base(cwd: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// The `-c` pairs that empty the repository's own filter drivers.
fn driver_overrides(cwd: &Path) -> Vec<String> {
    let out = base(cwd)
        .args([
            "config",
            "--local",
            "--name-only",
            "--get-regexp",
            r"^filter\..+\.",
        ])
        .output();
    let Ok(out) = out else { return Vec::new() };
    let mut drivers = std::collections::BTreeSet::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some((d, _)) = line
            .strip_prefix("filter.")
            .and_then(|r| r.rsplit_once('.'))
        {
            drivers.insert(d.to_string());
        }
    }
    let mut args = Vec::new();
    for d in drivers {
        for kv in ["clean=", "smudge=", "process=", "required=false"] {
            args.push("-c".to_string());
            args.push(format!("filter.{d}.{kv}"));
        }
    }
    args
}

fn read(cwd: &Path, args: &[&str]) -> Result<std::process::Output, RpcError> {
    let mut cmd = base(cwd);
    for kv in READ_OVERRIDES {
        cmd.args(["-c", kv]);
    }
    cmd.args(driver_overrides(cwd))
        .env("GIT_OPTIONAL_LOCKS", "0");
    if let Some((sub, rest)) = args.split_first() {
        cmd.arg(sub);
        if matches!(*sub, "diff" | "log" | "show") {
            cmd.args(["--no-ext-diff", "--no-textconv"]);
        }
        cmd.args(rest);
    }
    cmd.output()
        .map_err(|e| git_err(format!("Couldn't run git on the host: {e}")))
}

/// The repository's top folder, or `None` outside one.
pub fn toplevel(root: &Path) -> Result<Option<PathBuf>, RpcError> {
    if !root.is_dir() {
        return Err(RpcError::new(
            ErrorKind::NotFound,
            "The remote folder doesn't exist",
        ));
    }
    let out = read(root, &["rev-parse", "--show-toplevel"])?;
    if !out.status.success() {
        return Ok(None);
    }
    let top = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(if top.is_empty() {
        None
    } else {
        Some(PathBuf::from(top))
    })
}

/// Keeps Vibezeo's own folder (`.tandem/`, attachments of remote threads)
/// out of the host repository's status, as locally: one line in
/// `info/exclude`, never `.gitignore`. Nothing outside a repository.
pub fn exclude_tandem(root: &Path) {
    let Ok(Some(top)) = toplevel(root) else {
        return;
    };
    let Ok(out) = read(&top, &["rev-parse", "--git-common-dir"]) else {
        return;
    };
    let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if dir.is_empty() {
        return;
    }
    let common = if Path::new(&dir).is_absolute() {
        PathBuf::from(dir)
    } else {
        top.join(dir)
    };
    let path = common.join("info").join("exclude");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current
        .lines()
        .any(|l| l.trim_end_matches('\r') == ".tandem/")
    {
        return;
    }
    let _ = std::fs::create_dir_all(common.join("info"));
    let mut text = current;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(".tandem/\n");
    let _ = std::fs::write(&path, text);
}

pub fn status(root: &Path) -> Result<Option<RepoState>, RpcError> {
    let Some(top) = toplevel(root)? else {
        return Ok(None);
    };
    let out = read(
        &top,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ],
    )?;
    if !out.status.success() {
        return Err(git_err(stderr_tail(&out.stderr)));
    }
    Ok(Some(RepoState {
        top: top.to_string_lossy().into_owned(),
        status: parse_porcelain_v2(&out.stdout),
    }))
}

fn change(c: u8) -> Option<FileChange> {
    Some(match c {
        b'A' | b'C' => FileChange::Added,
        b'M' => FileChange::Modified,
        b'D' => FileChange::Deleted,
        b'R' => FileChange::Renamed,
        b'T' => FileChange::TypeChanged,
        _ => return None,
    })
}

/// Parses `git status --porcelain=v2 -z --branch`.
pub fn parse_porcelain_v2(raw: &[u8]) -> GitStatus {
    let mut st = GitStatus::default();
    let mut fields = raw
        .split(|b| *b == 0)
        .map(|f| String::from_utf8_lossy(f).into_owned());
    while let Some(entry) = fields.next() {
        if entry.is_empty() {
            continue;
        }
        if let Some(h) = entry.strip_prefix("# ") {
            let (key, val) = h.split_once(' ').unwrap_or((h, ""));
            match key {
                "branch.oid" => st.no_commits = val == "(initial)",
                "branch.head" => {
                    st.branch = (val != "(detached)").then(|| val.to_string());
                }
                "branch.upstream" => st.upstream = Some(val.to_string()),
                "branch.ab" => {
                    for part in val.split(' ') {
                        if let Some(n) = part.strip_prefix('+') {
                            st.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            st.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let kind = entry.as_bytes()[0];
        match kind {
            b'?' => st.unstaged.push(FileStatus {
                path: entry[2..].to_string(),
                change: FileChange::Untracked,
                old_path: None,
            }),
            b'u' => {
                let path = entry
                    .splitn(11, ' ')
                    .nth(10)
                    .unwrap_or_default()
                    .to_string();
                st.unstaged.push(FileStatus {
                    path,
                    change: FileChange::Conflicted,
                    old_path: None,
                });
            }
            b'1' | b'2' => {
                let n = if kind == b'1' { 9 } else { 10 };
                let parts: Vec<&str> = entry.splitn(n, ' ').collect();
                let xy = parts.get(1).map(|s| s.as_bytes()).unwrap_or(b"..");
                let path = parts.get(n - 1).copied().unwrap_or_default().to_string();
                let old_path = if kind == b'2' { fields.next() } else { None };
                if let Some(c) = xy.first().copied().and_then(change) {
                    st.staged.push(FileStatus {
                        path: path.clone(),
                        change: c,
                        old_path: if c == FileChange::Renamed {
                            old_path.clone()
                        } else {
                            None
                        },
                    });
                }
                if let Some(c) = xy.get(1).copied().and_then(change) {
                    st.unstaged.push(FileStatus {
                        path,
                        change: c,
                        old_path: None,
                    });
                }
            }
            _ => {}
        }
    }
    st
}

/// A repository-relative path: no absolute paths, no `..`, nothing git
/// could take for an option.
fn checked(path: &str) -> Result<&str, RpcError> {
    let p = Path::new(path);
    let ok = !path.is_empty()
        && !path.starts_with('-')
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if ok {
        Ok(path)
    } else {
        Err(RpcError::new(
            ErrorKind::PathEscape,
            "That file isn't in this repository",
        ))
    }
}

pub fn diff(root: &Path, path: &str, staged: bool) -> Result<FileDiff, RpcError> {
    let path = checked(path)?;
    let top =
        toplevel(root)?.ok_or_else(|| RpcError::new(ErrorKind::Invalid, "Not a git repository"))?;
    let state = status(&top)?.map(|s| s.status).unwrap_or_default();
    let list = if staged {
        &state.staged
    } else {
        &state.unstaged
    };
    let entry = list.iter().find(|f| f.path == path);
    let untracked = entry.is_some_and(|f| f.change == FileChange::Untracked);
    let out = if untracked {
        read(&top, &["diff", "--no-index", "--", "/dev/null", path])?
    } else if staged {
        read(&top, &["diff", "--cached", "-M", "--", path])?
    } else {
        read(&top, &["diff", "--", path])?
    };
    // `--no-index` exits 1 when the files differ.
    if !out.status.success() && out.status.code() != Some(1) {
        return Err(git_err(stderr_tail(&out.stderr)));
    }
    Ok(parse_patch(
        path,
        &out.stdout,
        entry.map(|e| e.change),
        entry.and_then(|e| e.old_path.clone()),
    ))
}

pub fn parse_patch(
    path: &str,
    raw: &[u8],
    known: Option<FileChange>,
    old_path: Option<String>,
) -> FileDiff {
    let truncated = raw.len() > MAX_PATCH_BYTES;
    let text = String::from_utf8_lossy(&raw[..raw.len().min(MAX_PATCH_BYTES)]).into_owned();
    let mut additions = 0;
    let mut deletions = 0;
    let mut binary = false;
    let mut header_change = None;
    let mut in_hunk = false;
    for line in text.lines() {
        if line.starts_with("@@") {
            in_hunk = true;
        } else if line.starts_with("diff --git") {
            in_hunk = false;
        } else if !in_hunk {
            if line.starts_with("new file mode") {
                header_change = Some(FileChange::Added);
            } else if line.starts_with("deleted file mode") {
                header_change = Some(FileChange::Deleted);
            } else if line.starts_with("rename from") {
                header_change = Some(FileChange::Renamed);
            } else if line.starts_with("Binary files") || line.starts_with("GIT binary patch") {
                binary = true;
            }
        } else if line.starts_with('+') {
            additions += 1;
        } else if line.starts_with('-') {
            deletions += 1;
        }
    }
    let change = match known {
        Some(FileChange::Untracked) | None => header_change.unwrap_or(FileChange::Modified),
        Some(c) => c,
    };
    FileDiff {
        path: path.to_string(),
        old_path,
        change: if known == Some(FileChange::Untracked) {
            FileChange::Untracked
        } else {
            change
        },
        additions,
        deletions,
        binary,
        patch: if binary { String::new() } else { text },
        truncated,
    }
}

fn write(cwd: &Path, args: &[&str], paths: &[String], stdin: Option<&str>) -> Result<(), RpcError> {
    let mut cmd = base(cwd);
    cmd.args(["-c", "core.fsmonitor="]).args(args);
    if !paths.is_empty() {
        cmd.arg("--");
        for p in paths {
            cmd.arg(checked(p)?);
        }
    }
    let out = match stdin {
        Some(text) => {
            use std::io::Write;
            cmd.stdin(Stdio::piped());
            let mut child = cmd
                .spawn()
                .map_err(|e| git_err(format!("Couldn't run git on the host: {e}")))?;
            if let Some(mut i) = child.stdin.take() {
                let _ = i.write_all(text.as_bytes());
            }
            child.wait_with_output()
        }
        None => cmd.output(),
    }
    .map_err(|e| git_err(format!("Couldn't run git on the host: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        let msg = stderr_tail(&out.stderr);
        Err(
            if msg.contains("Please tell me who you are") || msg.contains("user.email") {
                RpcError::new(
                ErrorKind::Invalid,
                "git on the host has no identity. Run git config --global user.name and user.email there.",
            )
            } else {
                git_err(msg)
            },
        )
    }
}

/// Runs the action; a commit returns its sha.
pub fn op(p: &GitOp) -> Result<Option<String>, RpcError> {
    let top = toplevel(Path::new(&p.root))?
        .ok_or_else(|| RpcError::new(ErrorKind::Invalid, "Not a git repository"))?;
    match p.op {
        GitOpKind::Stage => write(&top, &["add", "-A"], &p.paths, None).map(|_| None),
        GitOpKind::Unstage => {
            let st = status(&top)?.map(|s| s.status).unwrap_or_default();
            if st.no_commits {
                write(&top, &["rm", "--cached", "-r", "-q"], &p.paths, None)?;
            } else {
                write(&top, &["restore", "--staged"], &p.paths, None)?;
            }
            Ok(None)
        }
        GitOpKind::Discard => {
            let st = status(&top)?.map(|s| s.status).unwrap_or_default();
            let mut tracked = Vec::new();
            for path in &p.paths {
                checked(path)?;
                match st
                    .unstaged
                    .iter()
                    .find(|f| &f.path == path)
                    .map(|f| f.change)
                {
                    Some(FileChange::Conflicted) => {
                        return Err(RpcError::new(
                            ErrorKind::Invalid,
                            format!("Resolve the conflict in {path} first"),
                        ))
                    }
                    Some(FileChange::Untracked) => {
                        let full = top.join(path);
                        let parent = full
                            .parent()
                            .and_then(|d| d.canonicalize().ok())
                            .ok_or_else(|| {
                                RpcError::new(
                                    ErrorKind::PathEscape,
                                    "That file isn't in this repository",
                                )
                            })?;
                        if !parent.starts_with(&top) {
                            return Err(RpcError::new(
                                ErrorKind::PathEscape,
                                "That file isn't in this repository",
                            ));
                        }
                        let res = match std::fs::symlink_metadata(&full) {
                            Ok(m) if m.is_dir() => std::fs::remove_dir_all(&full),
                            Ok(_) => std::fs::remove_file(&full),
                            Err(_) => Ok(()),
                        };
                        res.map_err(|e| git_err(e.to_string()))?;
                    }
                    Some(_) => tracked.push(path.clone()),
                    None => {}
                }
            }
            if !tracked.is_empty() {
                write(&top, &["restore", "--worktree"], &tracked, None)?;
            }
            Ok(None)
        }
        GitOpKind::Commit => {
            let msg = p.message.as_deref().unwrap_or_default();
            if msg.trim().is_empty() || msg.contains('\0') {
                return Err(RpcError::new(
                    ErrorKind::Invalid,
                    "Write a commit message first",
                ));
            }
            write(&top, &["commit", "-F", "-"], &[], Some(msg))?;
            let out = read(&top, &["rev-parse", "HEAD"])?;
            Ok(Some(
                String::from_utf8_lossy(&out.stdout).trim().to_string(),
            ))
        }
    }
}

fn stderr_tail(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(4);
    lines[start..].join("\n").chars().take(600).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git")
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn f_rm_01_porcelain_v2_parses_like_the_local_status() {
        let raw = b"# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 M. N... 100644 100644 100644 a b src/a.rs\0\
1 .M N... 100644 100644 100644 a b src/b c.rs\0\
2 R. N... 100644 100644 100644 a b R100 new.rs\0old.rs\0\
u UU N... 100644 100644 100644 100644 a b c conflict.txt\0\
? notes/todo.md\0";
        let st = parse_porcelain_v2(raw);
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!(st.upstream.as_deref(), Some("origin/main"));
        assert_eq!((st.ahead, st.behind), (2, 1));
        assert!(!st.no_commits);
        assert_eq!(st.staged.len(), 2);
        assert_eq!(st.staged[1].change, FileChange::Renamed);
        assert_eq!(st.staged[1].old_path.as_deref(), Some("old.rs"));
        assert_eq!(st.unstaged[0].path, "src/b c.rs");
        assert_eq!(st.unstaged[1].change, FileChange::Conflicted);
        assert_eq!(st.unstaged[2].change, FileChange::Untracked);
    }

    #[test]
    fn f_rm_01_git_status_diff_stage_commit_on_the_host() {
        let dir = tempfile::tempdir().expect("tmp");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join("sub")).expect("mkdir");
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "one\n").expect("w");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "first"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\n").expect("w");
        std::fs::write(repo.join("sub/new.txt"), "new\n").expect("w");
        // The workspace may be a sub-folder: status covers the repository.
        let st = status(&repo.join("sub")).expect("status").expect("repo");
        assert_eq!(st.status.branch.as_deref(), Some("main"));
        assert_eq!(st.status.unstaged.len(), 2);
        let d = diff(&repo, "a.txt", false).expect("diff");
        assert_eq!((d.additions, d.deletions), (1, 0));
        assert!(d.patch.contains("+two"));
        let u = diff(&repo, "sub/new.txt", false).expect("diff");
        assert_eq!(u.change, FileChange::Untracked);
        assert_eq!(u.additions, 1);
        let paths = vec!["a.txt".to_string()];
        op(&GitOp {
            root: repo.to_string_lossy().into(),
            op: GitOpKind::Stage,
            paths: paths.clone(),
            message: None,
        })
        .expect("stage");
        let st = status(&repo).expect("s").expect("r").status;
        assert_eq!(st.staged.len(), 1);
        let escape = op(&GitOp {
            root: repo.to_string_lossy().into(),
            op: GitOpKind::Stage,
            paths: vec!["../x".into()],
            message: None,
        })
        .expect_err("refused");
        assert_eq!(escape.kind, ErrorKind::PathEscape);
        git(&repo, &["config", "user.name", "T"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        let sha = op(&GitOp {
            root: repo.to_string_lossy().into(),
            op: GitOpKind::Commit,
            paths: vec![],
            message: Some("second".into()),
        })
        .expect("commit")
        .expect("sha");
        assert_eq!(sha.len(), 40);
        op(&GitOp {
            root: repo.to_string_lossy().into(),
            op: GitOpKind::Discard,
            paths: vec!["sub/new.txt".into()],
            message: None,
        })
        .expect("discard");
        assert!(!repo.join("sub/new.txt").exists());
        assert!(status(dir.path()).expect("not a repo").is_none());
        // Attachments under .tandem/ never show as changes.
        std::fs::create_dir_all(repo.join(".tandem/attachments/th_1")).expect("mkdir");
        std::fs::write(repo.join(".tandem/attachments/th_1/a.txt"), "x").expect("w");
        exclude_tandem(&repo.join("sub"));
        exclude_tandem(&repo);
        let st = status(&repo).expect("s").expect("r").status;
        assert!(
            st.unstaged.iter().all(|f| !f.path.starts_with(".tandem")),
            "{st:?}"
        );
        let exclude = std::fs::read_to_string(repo.join(".git/info/exclude")).expect("exclude");
        assert_eq!(exclude.matches(".tandem/").count(), 1);
    }

    #[test]
    fn f_rm_01_a_repo_filter_driver_never_runs_on_reads() {
        let dir = tempfile::tempdir().expect("tmp");
        let repo = dir.path();
        git(repo, &["init", "-q", "-b", "main"]);
        let marker = repo.join("pwned");
        git(
            repo,
            &[
                "config",
                "filter.evil.clean",
                &format!("touch {}", marker.display()),
            ],
        );
        git(
            repo,
            &[
                "config",
                "core.fsmonitor",
                &format!("touch {}", marker.display()),
            ],
        );
        std::fs::write(repo.join(".gitattributes"), "*.txt filter=evil\n").expect("w");
        std::fs::write(repo.join("a.txt"), "x\n").expect("w");
        let _ = status(repo).expect("status");
        let _ = diff(repo, "a.txt", false);
        assert!(!marker.exists(), "a repository helper ran during a read");
    }
}
