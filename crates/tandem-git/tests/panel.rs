//! The git panel's reads and actions (F-CD-20).

mod support;

use std::path::{Path, PathBuf};

use support::{git, Fixture};
use tandem_git::{ops, FileChange, GitError, LaneManager, Repo};

fn repo(path: &Path) -> Repo {
    Repo::discover(path)
        .expect("discover")
        .expect("a repository")
}

/// A bare remote that `f.repo` tracks as `origin`, with `main` pushed.
fn with_remote(f: &Fixture) -> PathBuf {
    let bare = f.dir.path().join("remote.git");
    std::fs::create_dir_all(&bare).expect("mkdir");
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    git(
        &f.repo,
        &["remote", "add", "origin", bare.to_str().expect("utf8")],
    );
    git(&f.repo, &["push", "-q", "-u", "origin", "main"]);
    bare
}

/// F-CD-20 AC1: in a repository whose `.git/config` sets
/// `core.fsmonitor=touch /tmp/pwned`, opening Changes (every read the panel
/// does) never creates `/tmp/pwned`. The same trap is proven live first in
/// another repository with its own marker.
#[cfg(unix)]
#[tokio::test]
async fn f_cd_20_1_opening_changes_never_runs_the_repos_fsmonitor() {
    // Live: plain git runs a `touch` fsmonitor command.
    let live = Fixture::new();
    let marker = live.dir.path().join("live-marker");
    git(
        &live.repo,
        &[
            "config",
            "core.fsmonitor",
            &format!("touch {}", marker.display()),
        ],
    );
    std::fs::write(live.repo.join("a.txt"), "changed\n").expect("write");
    let _ = std::process::Command::new("git")
        .current_dir(&live.repo)
        .args(["status", "--porcelain"])
        .output();
    assert!(
        marker.exists(),
        "the fsmonitor trap didn't fire under plain git"
    );

    // AC1. The trap goes in last: plain git (the fixture's) would spring it.
    let pwned = Path::new("/tmp/pwned");
    let f = Fixture::new();
    std::fs::write(f.repo.join("a.txt"), "changed\n").expect("write");
    std::fs::write(f.repo.join("new.txt"), "new\n").expect("write");
    git(&f.repo, &["add", "new.txt"]);
    git(&f.repo, &["config", "core.fsmonitor", "touch /tmp/pwned"]);
    let _ = std::fs::remove_file(pwned);
    let r = repo(&f.repo);
    let status = r.status().await.expect("status");
    assert_eq!(status.staged.len(), 1);
    assert_eq!(status.unstaged.len(), 1);
    r.diff_file("a.txt", false).await.expect("unstaged diff");
    r.diff_file("new.txt", true).await.expect("staged diff");
    r.branches().await.expect("branches");
    r.local_branches().await.expect("local branches");
    r.head().await.expect("head");
    assert!(!pwned.exists(), "a git panel read ran core.fsmonitor");
}

/// F-CD-20 AC2: stage, commit and push against a local bare remote, and
/// ahead/behind comes back to 0/0.
#[tokio::test]
async fn f_cd_20_2_stage_commit_and_push_to_a_bare_remote_end_at_0_0() {
    let f = Fixture::new();
    let bare = with_remote(&f);
    let cli = f.git();
    let r = repo(&f.repo);
    std::fs::write(f.repo.join("a.txt"), "one\nTWO\nthree\n").expect("write");
    std::fs::write(f.repo.join("b.txt"), "bee\n").expect("write");
    ops::stage(&cli, &r, &["a.txt".into(), "b.txt".into()])
        .await
        .expect("stage");
    let s = r.status().await.expect("status");
    assert_eq!(s.staged.len(), 2, "{s:?}");
    assert!(s.unstaged.is_empty(), "{s:?}");
    let sha = ops::commit(&cli, &r, "Fix the second line\n\nAnd add b.")
        .await
        .expect("commit");
    let s = r.status().await.expect("status");
    assert_eq!((s.ahead, s.behind), (1, 0));
    assert!(s.is_clean());
    ops::push(&cli, &r, false).await.expect("push");
    let s = r.status().await.expect("status");
    assert_eq!((s.ahead, s.behind), (0, 0));
    assert_eq!(git(&bare, &["rev-parse", "main"]), sha);
}

/// F-CD-20 AC3: a repository with no commits lists its files as added and
/// takes the first commit.
#[tokio::test]
async fn f_cd_20_3_a_repo_with_no_commits_lists_added_files_and_takes_the_first_commit() {
    let f = Fixture::new();
    let fresh = f.dir.path().join("fresh");
    std::fs::create_dir_all(fresh.join("src")).expect("mkdir");
    git(&fresh, &["-c", "init.defaultBranch=trunk", "init", "-q"]);
    git(&fresh, &["config", "user.name", "Test"]);
    git(&fresh, &["config", "user.email", "test@example.invalid"]);
    std::fs::write(fresh.join("README.md"), "# new\n").expect("write");
    std::fs::write(fresh.join("src/lib.rs"), "pub fn x() {}\n").expect("write");
    let r = repo(&fresh);
    let s = r.status().await.expect("status");
    assert!(s.no_commits);
    assert_eq!(s.branch.as_deref(), Some("trunk"));
    let mut paths: Vec<_> = s
        .unstaged
        .iter()
        .map(|f| (f.path.as_str(), f.change))
        .collect();
    paths.sort_by_key(|(path, _)| *path);
    assert_eq!(
        paths,
        vec![
            ("README.md", FileChange::Untracked),
            ("src/lib.rs", FileChange::Untracked)
        ]
    );
    let cli = f.git();
    ops::stage(&cli, &r, &["README.md".into(), "src/lib.rs".into()])
        .await
        .expect("stage");
    let s = r.status().await.expect("status");
    assert!(
        s.staged.iter().all(|f| f.change == FileChange::Added),
        "{s:?}"
    );
    let staged = r.diff_file("README.md", true).await.expect("diff");
    assert!(staged.patch.contains("+# new"), "{}", staged.patch);
    // Unstaging before the first commit forgets the file again.
    ops::unstage(&cli, &r, &["src/lib.rs".into()])
        .await
        .expect("unstage");
    assert_eq!(r.status().await.expect("status").staged.len(), 1);
    ops::stage(&cli, &r, &["src/lib.rs".into()])
        .await
        .expect("stage");
    ops::commit(&cli, &r, "First commit")
        .await
        .expect("first commit");
    let s = r.status().await.expect("status");
    assert!(!s.no_commits);
    assert!(s.is_clean());
    // No remote yet: publishing says so.
    let err = ops::push(&cli, &r, true).await.expect_err("no remote");
    assert!(matches!(err, GitError::Invalid(m) if m.contains("no remote")));
}

#[tokio::test]
async fn f_cd_20_unstage_and_discard_put_files_back() {
    let f = Fixture::new();
    let cli = f.git();
    let r = repo(&f.repo);
    std::fs::write(f.repo.join("a.txt"), "changed\n").expect("write");
    std::fs::write(f.repo.join("loose.txt"), "loose\n").expect("write");
    ops::stage(&cli, &r, &["a.txt".into()])
        .await
        .expect("stage");
    ops::unstage(&cli, &r, &["a.txt".into()])
        .await
        .expect("unstage");
    let s = r.status().await.expect("status");
    assert!(s.staged.is_empty());
    ops::discard(&cli, &r, &["a.txt".into(), "loose.txt".into()])
        .await
        .expect("discard");
    assert_eq!(
        std::fs::read_to_string(f.repo.join("a.txt")).expect("read"),
        "one\ntwo\nthree\n"
    );
    assert!(!f.repo.join("loose.txt").exists());
    assert!(r.status().await.expect("status").is_clean());
    // Nothing outside the repository, nothing in git's own data.
    for bad in ["../x", "/etc/hosts", ".git/config"] {
        assert!(matches!(
            ops::discard(&cli, &r, &[bad.into()]).await,
            Err(GitError::Invalid(_))
        ));
    }
}

#[tokio::test]
async fn f_cd_20_commit_needs_a_message_and_staged_changes_and_keeps_the_message() {
    let f = Fixture::new();
    let cli = f.git();
    let r = repo(&f.repo);
    assert!(matches!(
        ops::commit(&cli, &r, "  \n").await,
        Err(GitError::Invalid(_))
    ));
    assert!(matches!(
        ops::commit(&cli, &r, "nothing staged").await,
        Err(GitError::Invalid(m)) if m.contains("Stage")
    ));
    std::fs::write(f.repo.join("a.txt"), "x\n").expect("write");
    ops::stage(&cli, &r, &["a.txt".into()])
        .await
        .expect("stage");
    let message = "#42 keep the hash line\n\n- a list\n- with lines";
    ops::commit(&cli, &r, message).await.expect("commit");
    assert_eq!(git(&f.repo, &["log", "-1", "--format=%B"]), message);
}

#[tokio::test]
async fn f_cd_20_new_branch_switch_and_the_branch_list() {
    let f = Fixture::new();
    let bare = with_remote(&f);
    let cli = f.git();
    let r = repo(&f.repo);
    ops::create_branch(&cli, &r, "feature/x")
        .await
        .expect("new branch");
    assert_eq!(
        r.status().await.expect("status").branch.as_deref(),
        Some("feature/x")
    );
    assert!(matches!(
        ops::create_branch(&cli, &r, "feature/x").await,
        Err(GitError::Conflict(_))
    ));
    assert!(matches!(
        ops::create_branch(&cli, &r, "bad name..").await,
        Err(GitError::Invalid(_))
    ));
    // Not published: push says so; publish gives it an upstream.
    assert!(matches!(
        ops::push(&cli, &r, false).await,
        Err(GitError::Invalid(m)) if m.contains("isn't published")
    ));
    ops::push(&cli, &r, true).await.expect("publish");
    let s = r.status().await.expect("status");
    assert_eq!(s.upstream.as_deref(), Some("origin/feature/x"));
    assert_eq!((s.ahead, s.behind), (0, 0));
    // Another clone publishes a branch this one doesn't have.
    let other = f.dir.path().join("other");
    git(
        f.dir.path(),
        &[
            "clone",
            "-q",
            bare.to_str().expect("utf8"),
            other.to_str().expect("utf8"),
        ],
    );
    git(&other, &["switch", "-q", "-c", "theirs"]);
    git(&other, &["push", "-q", "-u", "origin", "theirs"]);
    git(&f.repo, &["fetch", "-q"]);
    let list = r.branches().await.expect("branches");
    let names: Vec<_> = list
        .iter()
        .map(|b| (b.name.as_str(), b.current, b.remote))
        .collect();
    assert_eq!(
        names,
        vec![
            ("feature/x", true, false),
            ("main", false, false),
            ("origin/theirs", false, true)
        ]
    );
    // A remote branch: switching makes its local tracking branch.
    ops::switch(&cli, &r, "origin/theirs")
        .await
        .expect("switch to remote");
    let s = r.status().await.expect("status");
    assert_eq!(s.branch.as_deref(), Some("theirs"));
    assert_eq!(s.upstream.as_deref(), Some("origin/theirs"));
    ops::switch(&cli, &r, "main").await.expect("switch");
    assert_eq!(
        r.status().await.expect("status").branch.as_deref(),
        Some("main")
    );
    assert!(matches!(
        ops::switch(&cli, &r, "nope").await,
        Err(GitError::Invalid(_))
    ));
}

#[tokio::test]
async fn f_cd_20_switching_over_uncommitted_changes_says_commit_first() {
    let f = Fixture::new();
    let cli = f.git();
    let r = repo(&f.repo);
    ops::create_branch(&cli, &r, "other").await.expect("branch");
    std::fs::write(f.repo.join("a.txt"), "on other\n").expect("write");
    f.commit_all(&f.repo, "other");
    ops::switch(&cli, &r, "main").await.expect("switch");
    std::fs::write(f.repo.join("a.txt"), "uncommitted\n").expect("write");
    let err = ops::switch(&cli, &r, "other").await.expect_err("conflict");
    assert!(matches!(err, GitError::Conflict(m) if m.contains("Commit or stash")));
}

#[tokio::test]
async fn f_cd_20_pull_brings_the_remote_in_and_a_stale_push_says_pull_first() {
    let f = Fixture::new();
    let bare = with_remote(&f);
    let cli = f.git();
    let r = repo(&f.repo);
    let other = f.dir.path().join("other");
    git(
        f.dir.path(),
        &[
            "clone",
            "-q",
            bare.to_str().expect("utf8"),
            other.to_str().expect("utf8"),
        ],
    );
    git(&other, &["config", "user.name", "Other"]);
    git(&other, &["config", "user.email", "other@example.invalid"]);
    std::fs::write(other.join("theirs.txt"), "theirs\n").expect("write");
    git(&other, &["add", "."]);
    git(&other, &["commit", "-qm", "theirs"]);
    git(&other, &["push", "-q"]);
    // Ours diverges: the push is refused with words, not git's.
    std::fs::write(f.repo.join("ours.txt"), "ours\n").expect("write");
    ops::stage(&cli, &r, &["ours.txt".into()])
        .await
        .expect("stage");
    ops::commit(&cli, &r, "ours").await.expect("commit");
    let err = ops::push(&cli, &r, false).await.expect_err("rejected");
    assert!(
        matches!(err, GitError::Conflict(ref m) if m.contains("Pull first")),
        "{err:?}"
    );
    git(&f.repo, &["config", "pull.rebase", "false"]);
    ops::pull(&cli, &r).await.expect("pull");
    assert!(f.repo.join("theirs.txt").exists());
    let s = r.status().await.expect("status");
    assert_eq!(s.behind, 0);
    ops::push(&cli, &r, false).await.expect("push after pull");
    assert_eq!(r.status().await.expect("status").ahead, 0);
}

#[tokio::test]
async fn f_cd_20_works_in_a_subfolder_workspace_and_in_a_lane() {
    let f = Fixture::new();
    let cli = f.git();
    let sub = f.repo.join("app");
    std::fs::create_dir_all(&sub).expect("mkdir");
    std::fs::write(sub.join("in.txt"), "in\n").expect("write");
    std::fs::write(f.repo.join("out.txt"), "out\n").expect("write");
    let r = repo(&sub);
    let s = r.status().await.expect("status");
    assert_eq!(
        s.unstaged
            .iter()
            .map(|x| x.path.as_str())
            .collect::<Vec<_>>(),
        vec!["app/in.txt"]
    );
    ops::stage(&cli, &r, &["app/in.txt".into()])
        .await
        .expect("stage");
    ops::commit(&cli, &r, "app file").await.expect("commit");
    assert!(r.status().await.expect("status").is_clean());

    let main = repo(&f.repo);
    let lane = LaneManager::create(&cli, &main, "panel work", &Default::default())
        .await
        .expect("lane");
    let lr = repo(Path::new(&lane.path));
    std::fs::write(Path::new(&lane.path).join("lane.txt"), "lane\n").expect("write");
    ops::stage(&cli, &lr, &["lane.txt".into()])
        .await
        .expect("stage");
    ops::commit(&cli, &lr, "lane work")
        .await
        .expect("commit in lane");
    let s = lr.status().await.expect("status");
    assert_eq!(s.branch.as_deref(), Some(lane.branch.as_str()));
    assert!(s.is_clean());
    assert!(
        !f.repo.join("lane.txt").exists(),
        "the main checkout is untouched"
    );
}

#[tokio::test]
async fn f_cd_20_unstaged_and_staged_diffs_of_one_file() {
    let f = Fixture::new();
    let cli = f.git();
    let r = repo(&f.repo);
    std::fs::write(f.repo.join("a.txt"), "one\nTWO\nthree\n").expect("write");
    ops::stage(&cli, &r, &["a.txt".into()])
        .await
        .expect("stage");
    std::fs::write(f.repo.join("a.txt"), "one\nTWO\nthree\nfour\n").expect("write");
    let staged = r.diff_file("a.txt", true).await.expect("staged");
    assert!(staged.patch.contains("-two\n+TWO\n"), "{}", staged.patch);
    assert!(!staged.patch.contains("four"));
    let unstaged = r.diff_file("a.txt", false).await.expect("unstaged");
    assert!(unstaged.patch.contains("+four\n"), "{}", unstaged.patch);
    assert!(!unstaged.patch.contains("-two"));
    std::fs::write(f.repo.join("brand-new.txt"), "hello\n").expect("write");
    let untracked = r
        .diff_file("brand-new.txt", false)
        .await
        .expect("untracked");
    assert_eq!(untracked.change, FileChange::Added);
    assert!(untracked.patch.contains("+hello"));
}
