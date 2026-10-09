mod support;

use std::ffi::OsStr;

use support::{git, Fixture};
use tandem_git::{FileChange, Git, GitError, Mode, Repo};

#[tokio::test]
async fn f_cd_20_status_lists_staged_unstaged_and_untracked() {
    let f = Fixture::new();
    std::fs::write(f.repo.join("a.txt"), "changed\n").expect("write");
    std::fs::write(f.repo.join("staged.txt"), "s\n").expect("write");
    git(&f.repo, &["add", "staged.txt"]);
    std::fs::write(f.repo.join("loose.txt"), "u\n").expect("write");
    let repo = Repo::discover(&f.repo).expect("discover").expect("repo");
    let s = repo.status().await.expect("status");
    assert_eq!(s.branch.as_deref(), Some("main"));
    assert!(!s.no_commits);
    assert_eq!(s.staged.len(), 1);
    assert_eq!(
        (s.staged[0].path.as_str(), s.staged[0].change),
        ("staged.txt", FileChange::Added)
    );
    let unstaged: Vec<_> = s
        .unstaged
        .iter()
        .map(|f| (f.path.as_str(), f.change))
        .collect();
    assert!(unstaged.contains(&("a.txt", FileChange::Modified)));
    assert!(unstaged.contains(&("loose.txt", FileChange::Untracked)));
    assert!(s.has_tracked_changes());
}

#[tokio::test]
async fn f_cd_20_subfolder_workspace_sees_only_its_folder() {
    let f = Fixture::new();
    let sub = f.repo.join("app");
    std::fs::create_dir_all(&sub).expect("mkdir");
    std::fs::write(sub.join("in.txt"), "x\n").expect("write");
    std::fs::write(f.repo.join("out.txt"), "y\n").expect("write");
    let repo = Repo::discover(&sub).expect("discover").expect("repo");
    assert_eq!(repo.root(), f.repo);
    assert_eq!(repo.prefix(), std::path::Path::new("app"));
    let paths: Vec<_> = repo
        .status()
        .await
        .expect("status")
        .unstaged
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert_eq!(paths, vec!["app/in.txt".to_string()]);
}

/// F-TH-13 AC1: two files changed since the thread's start commit (one
/// edited and committed during the thread, one new) are both listed with
/// their diffs.
#[tokio::test]
async fn f_th_13_1_changes_since_the_start_commit_list_each_file_with_its_diff() {
    let f = Fixture::new();
    let repo = Repo::discover(&f.repo).expect("discover").expect("repo");
    let start = repo.head().await.expect("head").expect("a commit");
    std::fs::write(f.repo.join("a.txt"), "one\nTWO\nthree\n").expect("write");
    f.commit_all(&f.repo, "during the thread");
    std::fs::write(f.repo.join("b.txt"), "new\n").expect("write");
    let files = repo.diff_from(Some(start)).await.expect("diff");
    let summary: Vec<_> = files
        .iter()
        .map(|d| (d.path.as_str(), d.change, d.additions, d.deletions))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("a.txt", FileChange::Modified, 1, 1),
            ("b.txt", FileChange::Added, 1, 0)
        ]
    );
    assert!(
        files[0].patch.contains("-two\n+TWO\n"),
        "{}",
        files[0].patch
    );
    assert!(files[1].patch.contains("+new\n"), "{}", files[1].patch);
}

/// F-TH-13 AC2: in a subfolder workspace only that subfolder's changes show.
#[tokio::test]
async fn f_th_13_2_a_subfolder_workspace_lists_only_its_changes() {
    let f = Fixture::new();
    let sub = f.repo.join("app");
    std::fs::create_dir_all(&sub).expect("mkdir");
    std::fs::write(sub.join("keep.txt"), "k\n").expect("write");
    f.commit_all(&f.repo, "app");
    let repo = Repo::discover(&sub).expect("discover").expect("repo");
    let start = repo.head().await.expect("head").expect("a commit");
    std::fs::write(sub.join("keep.txt"), "changed\n").expect("write");
    std::fs::write(f.repo.join("a.txt"), "outside\n").expect("write");
    let paths: Vec<_> = repo
        .diff_from(Some(start))
        .await
        .expect("diff")
        .into_iter()
        .map(|d| d.path)
        .collect();
    assert_eq!(paths, vec!["app/keep.txt".to_string()]);
}

#[tokio::test]
async fn f_th_13_a_repo_with_no_commits_diffs_from_nothing() {
    let f = Fixture::new();
    let fresh = f.dir.path().join("fresh");
    std::fs::create_dir_all(&fresh).expect("mkdir");
    git(&fresh, &["-c", "init.defaultBranch=trunk", "init", "-q"]);
    std::fs::write(fresh.join("first.txt"), "hello\n").expect("write");
    let repo = Repo::discover(&fresh).expect("discover").expect("repo");
    let files = repo.diff_from(None).await.expect("diff");
    assert_eq!(files.len(), 1);
    assert_eq!(
        (files[0].path.as_str(), files[0].change),
        ("first.txt", FileChange::Added)
    );
}

#[tokio::test]
async fn f_cd_20_repo_without_commits_lists_added_files() {
    let f = Fixture::new();
    let fresh = f.dir.path().join("fresh");
    std::fs::create_dir_all(&fresh).expect("mkdir");
    git(&fresh, &["-c", "init.defaultBranch=trunk", "init", "-q"]);
    std::fs::write(fresh.join("first.txt"), "hello\n").expect("write");
    let repo = Repo::discover(&fresh).expect("discover").expect("repo");
    assert_eq!(repo.head().await.expect("head"), None);
    let s = repo.status().await.expect("status");
    assert!(s.no_commits);
    assert_eq!(s.branch.as_deref(), Some("trunk"));
    assert_eq!(s.unstaged[0].path, "first.txt");
}

#[test]
fn outside_a_repository_there_is_none() {
    let dir = tempfile::tempdir().expect("tmp");
    // A ceiling keeps discovery from finding a repository above the temp dir.
    std::env::set_var(
        "GIT_CEILING_DIRECTORIES",
        dir.path().parent().expect("parent"),
    );
    assert_eq!(Repo::discover(dir.path()).expect("discover"), None);
}

#[tokio::test]
async fn missing_git_is_reported_as_not_installed() {
    let dir = tempfile::tempdir().expect("tmp");
    let err =
        Git::locate(Some(dir.path().as_os_str()), dir.path().join("hooks")).expect_err("no git");
    assert!(matches!(err, GitError::NotInstalled));
}

/// A repository whose `.git/config` runs a command from each helper git
/// supports: fsmonitor, a clean filter, a textconv driver and hooks.
#[cfg(unix)]
mod rule_6 {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    pub struct Trap {
        pub f: Fixture,
        pub marks: PathBuf,
    }

    fn script(path: &Path, body: &str) {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("script");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    pub fn trapped_repo() -> Trap {
        let f = Fixture::new();
        let marks = f.dir.path().join("marks");
        std::fs::create_dir_all(&marks).expect("mkdir");
        let bin = f.dir.path().join("bin");
        std::fs::create_dir_all(&bin).expect("mkdir");
        let m = marks.display();
        script(&bin.join("fsmonitor"), &format!("touch {m}/fsmonitor"));
        script(&bin.join("clean"), &format!("touch {m}/clean\ncat"));
        script(
            &bin.join("textconv"),
            &format!("touch {m}/textconv\ncat \"$1\""),
        );
        let hooks = f.dir.path().join("repo-hooks");
        std::fs::create_dir_all(&hooks).expect("mkdir");
        for hook in [
            "pre-commit",
            "post-checkout",
            "post-index-change",
            "reference-transaction",
        ] {
            script(&hooks.join(hook), &format!("touch {m}/hook-{hook}"));
        }
        std::fs::write(
            f.repo.join(".gitattributes"),
            "*.txt filter=trap diff=trap\n",
        )
        .expect("attrs");
        f.commit_all(&f.repo, "attributes");
        let b = bin.display();
        git(
            &f.repo,
            &["config", "core.fsmonitor", &format!("{b}/fsmonitor")],
        );
        git(
            &f.repo,
            &["config", "filter.trap.clean", &format!("{b}/clean")],
        );
        git(
            &f.repo,
            &["config", "diff.trap.textconv", &format!("{b}/textconv")],
        );
        git(
            &f.repo,
            &["config", "core.hooksPath", &hooks.display().to_string()],
        );
        git(
            &f.repo,
            &["config", "credential.helper", &format!("!{b}/fsmonitor")],
        );
        // A worktree change, so status and diff have content to look at.
        std::fs::write(f.repo.join("a.txt"), "one\ntwo\nthree\nfour\n").expect("write");
        Trap { f, marks }
    }

    pub fn fired(marks: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(marks)
            .expect("marks")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

/// TL-06 rule 6 (F-CD-20 AC1): no git read runs a repository-configured
/// helper. The trap is proven live first with plain `git`.
#[cfg(unix)]
#[tokio::test]
async fn invariant_rule_6_read_ops_no_repo_helpers() {
    let trap = rule_6::trapped_repo();
    let repo_dir = trap.f.repo.clone();

    // Unhardened git does run them: the test can't pass vacuously.
    std::process::Command::new("git")
        .current_dir(&repo_dir)
        .args(["status", "--porcelain"])
        .output()
        .expect("git status");
    std::process::Command::new("git")
        .current_dir(&repo_dir)
        .args(["diff"])
        .output()
        .expect("git diff");
    let live = rule_6::fired(&trap.marks);
    for helper in ["clean", "fsmonitor", "textconv"] {
        assert!(
            live.iter().any(|m| m == helper),
            "trap {helper} didn't fire: {live:?}"
        );
    }
    for mark in &live {
        std::fs::remove_file(trap.marks.join(mark)).expect("reset");
    }

    // Every read Tandem does.
    let repo = Repo::discover(&repo_dir).expect("discover").expect("repo");
    let head = repo.head().await.expect("head").expect("a commit");
    let status = repo.status().await.expect("status");
    assert!(status.unstaged.iter().any(|f| f.path == "a.txt"));
    let diff = repo.diff_since(&head).await.expect("diff");
    assert!(diff.iter().any(|d| d.path == "a.txt"));
    repo.local_branches().await.expect("branches");
    repo.is_merged("main", &head).await.expect("merged");
    repo.conflicts().await.expect("conflicts");

    let cli = trap.f.git();
    for args in [
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ][..],
        &["diff", "--no-color"][..],
        &["diff", "--no-color", "--cached"][..],
        &["log", "-p", "-n", "1", "--no-color"][..],
        &["worktree", "list", "--porcelain"][..],
    ] {
        let argv: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        cli.ok(Mode::Read, &repo_dir, &argv)
            .await
            .expect("hardened read");
    }

    assert_eq!(
        rule_6::fired(&trap.marks),
        Vec::<String>::new(),
        "a repo helper ran during a read"
    );
}

/// Writes are the user's own action: their hooks run, fsmonitor never does.
#[cfg(unix)]
#[tokio::test]
async fn writes_run_hooks_but_never_fsmonitor() {
    let trap = rule_6::trapped_repo();
    let cli = trap.f.git();
    let argv: Vec<&OsStr> = ["commit", "-qam", "user commit"]
        .iter()
        .map(OsStr::new)
        .collect();
    cli.ok(Mode::Write, &trap.f.repo, &argv)
        .await
        .expect("commit");
    let fired = rule_6::fired(&trap.marks);
    assert!(fired.iter().any(|m| m == "hook-pre-commit"), "{fired:?}");
    assert!(!fired.iter().any(|m| m == "fsmonitor"), "{fired:?}");
}

/// The editor's Compare (F-CD-19): the file on disk against unsaved text.
#[test]
fn f_cd_19_compare_diffs_two_texts_without_a_repository() {
    let d = tandem_git::diff_texts("src/a.rs", b"one\ntwo\nthree\n", b"one\nTWO\nthree\nfour\n")
        .expect("diff");
    assert_eq!((d.additions, d.deletions, d.binary), (2, 1, false));
    assert!(d.patch.contains("-two\n+TWO\n"), "{}", d.patch);
    assert!(d.patch.contains("+four\n"), "{}", d.patch);
    assert!(d.patch.contains("src/a.rs"), "{}", d.patch);
    let same = tandem_git::diff_texts("a", b"x\n", b"x\n").expect("diff");
    assert_eq!((same.additions, same.deletions), (0, 0));
    let bin = tandem_git::diff_texts("b.bin", b"\0\x01", b"\0\x02").expect("diff");
    assert!(bin.binary);
}
