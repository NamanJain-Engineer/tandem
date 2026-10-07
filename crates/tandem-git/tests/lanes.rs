mod support;

use std::collections::HashSet;

use chrono::{Duration, Utc};
use support::{git, Fixture};
use tandem_core::origin::{LocalReview, Origin};
use tandem_git::{FileChange, ForceConfirm, GitError, LaneManager, Repo};

fn repo(f: &Fixture) -> Repo {
    Repo::discover(&f.repo).expect("discover").expect("a repo")
}

#[tokio::test]
async fn f_cd_15_two_lanes_for_the_same_task_dedupe() {
    let f = Fixture::new();
    let (git_cli, repo) = (f.git(), repo(&f));
    let a = LaneManager::create(&git_cli, &repo, "fix auth", &HashSet::new())
        .await
        .expect("first lane");
    let b = LaneManager::create(&git_cli, &repo, "fix auth", &HashSet::new())
        .await
        .expect("second lane");
    assert_eq!(a.branch, "tandem/fix-auth");
    assert_eq!(b.branch, "tandem/fix-auth-2");
    assert_eq!(a.path, f.repo.join(".tandem/worktrees/fix-auth"));
    assert!(b.path.join("a.txt").is_file(), "the lane is a checkout");
    let head = git(&f.repo, &["rev-parse", "HEAD"]);
    assert_eq!(a.base, head);
    assert_eq!(
        git(&a.path, &["rev-parse", "HEAD"]),
        head,
        "lanes start at the parent HEAD"
    );
    let exclude = std::fs::read_to_string(f.repo.join(".git/info/exclude")).expect("exclude");
    assert_eq!(exclude.lines().filter(|l| *l == ".tandem/").count(), 1);
    // The worktrees don't show up as changes in the parent.
    assert!(repo.status().await.expect("status").is_clean());
    // Slugs reserved by the same launch are skipped too.
    let reserved = HashSet::from(["fix-auth-3".to_string()]);
    let c = LaneManager::create(&git_cli, &repo, "Fix auth!", &reserved)
        .await
        .expect("third lane");
    assert_eq!(c.branch, "tandem/fix-auth-4");
}

#[tokio::test]
async fn f_cd_15_lane_needs_a_first_commit() {
    let f = Fixture::new();
    let empty = f.dir.path().join("empty");
    std::fs::create_dir_all(&empty).expect("mkdir");
    git(&empty, &["-c", "init.defaultBranch=main", "init", "-q"]);
    let repo = Repo::discover(&empty).expect("discover").expect("repo");
    let err = LaneManager::create(&f.git(), &repo, "x", &HashSet::new())
        .await
        .expect_err("no commit yet");
    assert!(matches!(err, GitError::Invalid(_)), "{err:?}");
}

#[tokio::test]
async fn f_cd_15_dirty_lane_removal_needs_a_local_force() {
    let f = Fixture::new();
    let (git_cli, repo) = (f.git(), repo(&f));
    let lane = LaneManager::create(&git_cli, &repo, "remove me", &HashSet::new())
        .await
        .expect("lane");
    std::fs::write(lane.path.join("scratch.txt"), "work in progress").expect("write");

    let err = LaneManager::remove(&git_cli, &repo, &lane.path, None)
        .await
        .expect_err("dirty");
    assert!(matches!(err, GitError::Dirty), "{err:?}");
    assert!(lane.path.exists());

    // Voice and the phone can't produce the confirmation (TL-06 rule 5).
    assert!(LocalReview::check(&Origin::Conductor).is_none());
    assert!(LocalReview::check(&Origin::Phone).is_none());

    let review = LocalReview::check(&Origin::main_window()).expect("main window");
    LaneManager::remove(&git_cli, &repo, &lane.path, Some(ForceConfirm::new(review)))
        .await
        .expect("forced remove");
    assert!(!lane.path.exists());
    // The branch, and any commits on it, stay.
    assert!(git(&f.repo, &["branch", "--list", "tandem/remove-me"]).contains("tandem/remove-me"));
}

#[tokio::test]
async fn f_cd_15_clean_lane_removes_without_force() {
    let f = Fixture::new();
    let (git_cli, repo) = (f.git(), repo(&f));
    let lane = LaneManager::create(&git_cli, &repo, "tidy", &HashSet::new())
        .await
        .expect("lane");
    LaneManager::remove(&git_cli, &repo, &lane.path, None)
        .await
        .expect("remove");
    assert!(!lane.path.exists());
    // Already gone: prunes and succeeds.
    LaneManager::remove(&git_cli, &repo, &lane.path, None)
        .await
        .expect("idempotent");
}

#[tokio::test]
async fn f_cd_15_merge_conflict_is_reported_and_nothing_is_committed() {
    let f = Fixture::new();
    let (git_cli, repo) = (f.git(), repo(&f));
    let lane = LaneManager::create(&git_cli, &repo, "conflict", &HashSet::new())
        .await
        .expect("lane");
    std::fs::write(lane.path.join("a.txt"), "one\nLANE\nthree\n").expect("write");
    f.commit_all(&lane.path, "lane edit");
    std::fs::write(f.repo.join("a.txt"), "one\nPARENT\nthree\n").expect("write");
    f.commit_all(&f.repo, "parent edit");
    let before = git(&f.repo, &["rev-parse", "HEAD"]);

    let report = LaneManager::merge_into_base(&git_cli, &repo, &lane.branch)
        .await
        .expect("merge runs");
    assert!(!report.merged);
    assert_eq!(report.conflicts, vec!["a.txt".to_string()]);
    assert_eq!(report.commit, None);
    assert_eq!(git(&f.repo, &["rev-parse", "HEAD"]), before, "no commit");
    assert!(
        f.repo.join(".git/MERGE_HEAD").exists(),
        "left conflicted for the user"
    );
    let status = repo.status().await.expect("status");
    assert!(status
        .unstaged
        .iter()
        .any(|s| s.path == "a.txt" && s.change == FileChange::Conflicted));
}

#[tokio::test]
async fn f_cd_15_merge_into_base_makes_a_merge_commit() {
    let f = Fixture::new();
    let (git_cli, repo) = (f.git(), repo(&f));
    let lane = LaneManager::create(&git_cli, &repo, "feature", &HashSet::new())
        .await
        .expect("lane");
    std::fs::write(lane.path.join("b.txt"), "new\n").expect("write");
    f.commit_all(&lane.path, "add b");

    let report = LaneManager::merge_into_base(&git_cli, &repo, &lane.branch)
        .await
        .expect("merge");
    assert!(report.merged);
    assert!(report.conflicts.is_empty());
    let head = git(&f.repo, &["rev-parse", "HEAD"]);
    assert_eq!(report.commit.as_deref(), Some(head.as_str()));
    assert_eq!(
        git(&f.repo, &["rev-list", "--count", "--merges", "HEAD"]),
        "1",
        "--no-ff"
    );
    assert!(f.repo.join("b.txt").is_file());

    // Merging again changes nothing and makes no commit.
    let again = LaneManager::merge_into_base(&git_cli, &repo, &lane.branch)
        .await
        .expect("again");
    assert!(again.merged);
    assert_eq!(again.commit, None);
}

#[tokio::test]
async fn f_cd_15_merge_refuses_a_dirty_parent() {
    let f = Fixture::new();
    let (git_cli, repo) = (f.git(), repo(&f));
    let lane = LaneManager::create(&git_cli, &repo, "x", &HashSet::new())
        .await
        .expect("lane");
    std::fs::write(f.repo.join("a.txt"), "edited, not committed\n").expect("write");
    let err = LaneManager::merge_into_base(&git_cli, &repo, &lane.branch)
        .await
        .expect_err("dirty parent");
    assert!(matches!(err, GitError::Dirty), "{err:?}");
}

#[tokio::test]
async fn f_cd_15_diff_vs_base_lists_the_lanes_work() {
    let f = Fixture::new();
    let (git_cli, repo) = (f.git(), repo(&f));
    let lane = LaneManager::create(&git_cli, &repo, "diff", &HashSet::new())
        .await
        .expect("lane");
    std::fs::write(lane.path.join("a.txt"), "one\ntwo\nthree\nfour\n").expect("write");
    f.commit_all(&lane.path, "committed");
    std::fs::write(lane.path.join("new.txt"), "untracked\n").expect("write");
    std::fs::write(lane.path.join("bin.dat"), [0u8, 1, 2, 0, 255]).expect("write");

    let lane_repo = Repo::discover(&lane.path)
        .expect("discover")
        .expect("lane repo");
    assert!(lane_repo.is_linked_worktree());
    assert_eq!(lane_repo.main_root(), f.repo);
    let diff = lane_repo.diff_since(&lane.base).await.expect("diff");
    let by_path = |p: &str| diff.iter().find(|d| d.path == p).cloned().expect(p);
    let a = by_path("a.txt");
    assert_eq!(
        (a.change, a.additions, a.deletions),
        (FileChange::Modified, 1, 0)
    );
    assert!(a.patch.contains("+four"));
    let new = by_path("new.txt");
    assert_eq!((new.change, new.additions), (FileChange::Added, 1));
    let bin = by_path("bin.dat");
    assert!(bin.binary && bin.patch.is_empty());
}

#[tokio::test]
async fn f_cd_15_sweep_prunes_and_lists_merged_idle_lanes() {
    let f = Fixture::new();
    let (git_cli, repo) = (f.git(), repo(&f));
    let merged = LaneManager::create(&git_cli, &repo, "merged", &HashSet::new())
        .await
        .expect("lane");
    let busy = LaneManager::create(&git_cli, &repo, "busy", &HashSet::new())
        .await
        .expect("lane");
    std::fs::write(busy.path.join("c.txt"), "unmerged work\n").expect("write");
    f.commit_all(&busy.path, "unmerged");
    let gone = LaneManager::create(&git_cli, &repo, "gone", &HashSet::new())
        .await
        .expect("lane");
    std::fs::remove_dir_all(&gone.path).expect("rm");

    // Fresh lanes aren't idle.
    assert!(LaneManager::sweep(&git_cli, &repo, Utc::now())
        .await
        .expect("sweep")
        .is_empty());
    assert!(
        !git(&f.repo, &["worktree", "list"]).contains("gone"),
        "pruned"
    );

    let later = Utc::now() + Duration::days(10);
    let stale = LaneManager::sweep(&git_cli, &repo, later)
        .await
        .expect("sweep");
    assert_eq!(stale.len(), 1, "{stale:?}");
    assert_eq!(stale[0].branch, merged.branch);
    assert!(stale[0].idle_days >= 9);
    // Nothing was deleted.
    assert!(merged.path.exists() && busy.path.exists());
}
