//! F-TL-06 rule 6: git reads never run repository-configured helpers
//! (F-CD-20): fsmonitor, clean filters, textconv drivers, hooks and
//! credential helpers. The trap repository is `tandem-git`'s own fixture.

#[cfg(unix)]
#[path = "../../../crates/tandem-git/tests/support/mod.rs"]
mod support;

#[cfg(unix)]
use std::ffi::OsStr;
#[cfg(unix)]
use tandem_git::{Mode, Repo};

/// F-TL-06 rule 6 (F-CD-20 AC1): no git read runs a repository-configured
/// helper. The trap is proven live first with plain `git`.
#[cfg(unix)]
#[tokio::test]
async fn invariant_rule_6_read_ops_no_repo_helpers() {
    let trap = support::trap::trapped_repo();
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
    let live = support::trap::fired(&trap.marks);
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
    // The git panel's reads (F-CD-20), and its status polling: the Changes
    // view and the status bar read the status again on every watch batch,
    // agent turn and window focus.
    for _ in 0..3 {
        let polled = repo.status().await.expect("status poll");
        assert!(polled.unstaged.iter().any(|f| f.path == "a.txt"));
    }
    let unstaged = repo.diff_file("a.txt", false).await.expect("unstaged diff");
    assert!(unstaged.patch.contains("+four"), "{}", unstaged.patch);
    repo.diff_file("a.txt", true).await.expect("staged diff");
    repo.branches().await.expect("branch list");

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
        support::trap::fired(&trap.marks),
        Vec::<String>::new(),
        "a repo helper ran during a read"
    );
}

/// On an SSH host (F-RM-01) the git panel's reads run the host's git
/// through `tandem-host`, hardened the same way: the trap repository's
/// helpers stay silent.
#[cfg(unix)]
#[test]
fn invariant_rule_6_remote_reads_no_repo_helpers() {
    let trap = support::trap::trapped_repo();
    let repo_dir = trap.f.repo.clone();
    std::process::Command::new("git")
        .current_dir(&repo_dir)
        .args(["status", "--porcelain"])
        .output()
        .expect("git status");
    let live = support::trap::fired(&trap.marks);
    assert!(!live.is_empty(), "the trap didn't fire for plain git");
    for mark in &live {
        std::fs::remove_file(trap.marks.join(mark)).expect("reset");
    }
    for _ in 0..3 {
        let state = tandem_host::gitops::status(&repo_dir)
            .expect("status")
            .expect("repo");
        assert!(state.status.unstaged.iter().any(|f| f.path == "a.txt"));
    }
    let diff = tandem_host::gitops::diff(&repo_dir, "a.txt", false).expect("diff");
    assert!(diff.patch.contains("+four"), "{}", diff.patch);
    tandem_host::gitops::diff(&repo_dir, "a.txt", true).expect("staged diff");
    assert_eq!(
        support::trap::fired(&trap.marks),
        Vec::<String>::new(),
        "a repo helper ran during a remote read"
    );
}
