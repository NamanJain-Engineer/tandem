mod support;

use std::collections::HashSet;

use support::{git, Fixture};
use tandem_git::queue::{
    ahead, contained_in, current_branch, first_parents, land, merge_base_into, remotes_containing,
    reset_to, resolve, Merged,
};
use tandem_git::{LaneManager, Repo};

fn parents(root: &std::path::Path, rev: &str) -> usize {
    git(root, &["rev-list", "--parents", "-n", "1", rev])
        .split_whitespace()
        .count()
        - 1
}

#[tokio::test]
async fn f_wt_07_1_a_worktree_lands_with_a_no_ff_merge_after_taking_the_base() {
    let f = Fixture::new();
    let (cli, repo) = (
        f.git(),
        Repo::discover(&f.repo).expect("discover").expect("repo"),
    );
    let lane = LaneManager::create(&cli, &repo, "one", &HashSet::new())
        .await
        .expect("lane");
    std::fs::write(lane.path.join("one.txt"), "one\n").expect("write");
    git(&lane.path, &["add", "."]);
    git(&lane.path, &["commit", "-qm", "one"]);
    // The base moves on meanwhile.
    std::fs::write(f.repo.join("base.txt"), "base\n").expect("write");
    git(&f.repo, &["add", "."]);
    git(&f.repo, &["commit", "-qm", "base"]);
    assert_eq!(
        current_branch(&cli, &f.repo)
            .await
            .expect("branch")
            .as_deref(),
        Some("main")
    );
    assert_eq!(
        ahead(&cli, &f.repo, "main", &lane.branch)
            .await
            .expect("ahead"),
        1
    );

    let into_lane = merge_base_into(&cli, &lane.path, "main", "main", &lane.branch)
        .await
        .expect("base in");
    assert!(matches!(into_lane, Merged::Done { .. }));
    assert!(
        lane.path.join("base.txt").exists(),
        "the worktree has the base"
    );

    let before = resolve(&cli, &f.repo, "HEAD")
        .await
        .expect("resolve")
        .expect("head");
    let Merged::Done { head } = land(&cli, &f.repo, &lane.branch, &lane.branch)
        .await
        .expect("land")
    else {
        panic!("conflict")
    };
    assert_ne!(head, before);
    assert_eq!(parents(&f.repo, "HEAD"), 2, "a --no-ff merge commit");
    assert!(f.repo.join("one.txt").exists());
    assert_eq!(
        ahead(&cli, &f.repo, "main", &lane.branch)
            .await
            .expect("ahead"),
        0
    );
}

#[tokio::test]
async fn f_wt_07_3_a_conflict_is_aborted_and_leaves_the_worktree_as_it_was() {
    let f = Fixture::new();
    let (cli, repo) = (
        f.git(),
        Repo::discover(&f.repo).expect("discover").expect("repo"),
    );
    let lane = LaneManager::create(&cli, &repo, "clash", &HashSet::new())
        .await
        .expect("lane");
    std::fs::write(lane.path.join("a.txt"), "one\nfrom the worktree\nthree\n").expect("write");
    git(&lane.path, &["commit", "-qam", "lane"]);
    std::fs::write(f.repo.join("a.txt"), "one\nfrom main\nthree\n").expect("write");
    git(&f.repo, &["commit", "-qam", "main"]);
    let lane_head = git(&lane.path, &["rev-parse", "HEAD"]);
    let base_head = git(&f.repo, &["rev-parse", "HEAD"]);

    let out = merge_base_into(&cli, &lane.path, "main", "main", &lane.branch)
        .await
        .expect("merge");
    assert_eq!(
        out,
        Merged::Conflicts {
            files: vec!["a.txt".into()]
        }
    );
    assert_eq!(git(&lane.path, &["rev-parse", "HEAD"]), lane_head);
    assert_eq!(git(&lane.path, &["status", "--porcelain"]), "");
    assert!(std::process::Command::new("git")
        .current_dir(&lane.path)
        .args(["rev-parse", "-q", "--verify", "MERGE_HEAD"])
        .status()
        .map(|s| !s.success())
        .unwrap_or(false));

    // Landing the conflicting branch straight into the base aborts too.
    let out = land(&cli, &f.repo, &lane.branch, &lane.branch)
        .await
        .expect("land");
    assert!(matches!(out, Merged::Conflicts { .. }));
    assert_eq!(git(&f.repo, &["rev-parse", "HEAD"]), base_head);
    assert_eq!(git(&f.repo, &["status", "--porcelain"]), "");
    assert!(!f.repo.join(".git/MERGE_HEAD").exists());
}

#[tokio::test]
async fn f_wt_07_5_uncommitted_tracked_changes_refuse_a_merge() {
    let f = Fixture::new();
    let (cli, repo) = (
        f.git(),
        Repo::discover(&f.repo).expect("discover").expect("repo"),
    );
    let lane = LaneManager::create(&cli, &repo, "dirty", &HashSet::new())
        .await
        .expect("lane");
    std::fs::write(f.repo.join("a.txt"), "edited\n").expect("write");
    let err = land(&cli, &f.repo, &lane.branch, &lane.branch)
        .await
        .expect_err("dirty");
    assert!(matches!(err, tandem_git::GitError::Dirty), "{err:?}");
}

#[tokio::test]
async fn f_wt_07_4_reset_moves_the_base_back_and_origin_containment_is_seen() {
    let f = Fixture::new();
    let (cli, repo) = (
        f.git(),
        Repo::discover(&f.repo).expect("discover").expect("repo"),
    );
    let lane = LaneManager::create(&cli, &repo, "undo", &HashSet::new())
        .await
        .expect("lane");
    std::fs::write(lane.path.join("u.txt"), "u\n").expect("write");
    git(&lane.path, &["add", "."]);
    git(&lane.path, &["commit", "-qm", "u"]);
    let before = git(&f.repo, &["rev-parse", "HEAD"]);
    let Merged::Done { head } = land(&cli, &f.repo, &lane.branch, &lane.branch)
        .await
        .expect("land")
    else {
        panic!("conflict")
    };
    // No origin yet: nothing is contained in it.
    assert!(!contained_in(&cli, &f.repo, &head, "origin/main")
        .await
        .expect("contained"));
    // A bare "origin" that has the landed merge.
    let remote = f.dir.path().join("origin.git");
    git(
        f.dir.path(),
        &["init", "-q", "--bare", remote.to_str().expect("utf8")],
    );
    git(
        &f.repo,
        &["remote", "add", "origin", remote.to_str().expect("utf8")],
    );
    git(&f.repo, &["push", "-q", "origin", "main"]);
    assert!(contained_in(&cli, &f.repo, &head, "origin/main")
        .await
        .expect("contained"));
    assert!(contained_in(&cli, &f.repo, &before, "main")
        .await
        .expect("ancestor"));

    reset_to(&cli, &f.repo, &before).await.expect("reset");
    assert_eq!(git(&f.repo, &["rev-parse", "HEAD"]), before);
    assert!(!f.repo.join("u.txt").exists());
    assert_eq!(git(&f.repo, &["status", "--porcelain"]), "");
}

#[tokio::test]
async fn f_wt_07_8_a_merge_that_would_replace_an_ignored_file_is_refused() {
    let f = Fixture::new();
    let (cli, repo) = (
        f.git(),
        Repo::discover(&f.repo).expect("discover").expect("repo"),
    );
    std::fs::write(f.repo.join(".gitignore"), ".env\n").expect("write");
    git(&f.repo, &["add", "."]);
    git(&f.repo, &["commit", "-qm", "ignore .env"]);
    std::fs::write(f.repo.join(".env"), "SECRET=real\n").expect("write");
    let lane = LaneManager::create(&cli, &repo, "env", &HashSet::new())
        .await
        .expect("lane");
    std::fs::write(lane.path.join(".env"), "SECRET=placeholder\n").expect("write");
    git(&lane.path, &["add", "-f", ".env"]);
    git(&lane.path, &["commit", "-qm", "placeholder"]);
    let before = git(&f.repo, &["rev-parse", "HEAD"]);
    let out = land(&cli, &f.repo, &lane.branch, &lane.branch)
        .await
        .expect("land");
    assert_eq!(
        out,
        Merged::WouldReplace {
            files: vec![".env".into()]
        }
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join(".env")).expect("read"),
        "SECRET=real\n"
    );
    assert_eq!(
        git(&f.repo, &["rev-parse", "HEAD"]),
        before,
        "nothing merged"
    );
}

#[tokio::test]
async fn f_wt_07_1_step_one_works_with_merge_ff_only_and_lands_by_commit() {
    let f = Fixture::new();
    let (cli, repo) = (
        f.git(),
        Repo::discover(&f.repo).expect("discover").expect("repo"),
    );
    git(&f.repo, &["config", "merge.ff", "only"]);
    let lane = LaneManager::create(&cli, &repo, "ff", &HashSet::new())
        .await
        .expect("lane");
    std::fs::write(lane.path.join("ff.txt"), "ff\n").expect("write");
    git(&lane.path, &["add", "."]);
    git(&lane.path, &["commit", "-qm", "ff"]);
    std::fs::write(f.repo.join("base.txt"), "base\n").expect("write");
    git(&f.repo, &["add", "."]);
    git(&f.repo, &["commit", "-qm", "base"]);
    let base = git(&f.repo, &["rev-parse", "main"]);
    let Merged::Done { head } = merge_base_into(&cli, &lane.path, "main", &base, &lane.branch)
        .await
        .expect("base in")
    else {
        panic!("conflict")
    };
    assert_eq!(
        git(&lane.path, &["log", "-1", "--format=%s"]),
        format!("Merge branch 'main' into {}", lane.branch)
    );
    // A commit after the check isn't landed: the noted one is.
    std::fs::write(lane.path.join("late.txt"), "late\n").expect("write");
    git(&lane.path, &["add", "."]);
    git(&lane.path, &["commit", "-qm", "late"]);
    let Merged::Done { head: merged } = land(&cli, &f.repo, &lane.branch, &head)
        .await
        .expect("land")
    else {
        panic!("conflict")
    };
    assert_eq!(
        git(&f.repo, &["log", "-1", "--format=%s"]),
        format!("Merge branch '{}'", lane.branch)
    );
    assert!(f.repo.join("ff.txt").exists() && !f.repo.join("late.txt").exists());
    assert_eq!(
        first_parents(&cli, &f.repo, &base, "HEAD")
            .await
            .expect("chain"),
        std::slice::from_ref(&merged)
    );
    assert!(remotes_containing(&cli, &f.repo, &merged)
        .await
        .expect("remotes")
        .is_empty());
}
