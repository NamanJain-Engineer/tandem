mod support;

use std::collections::HashSet;

use support::{git, Fixture};
use tandem_git::overlap::{changed_files, forecast, shared, Forecast};
use tandem_git::{LaneManager, Repo};

#[tokio::test]
async fn f_wt_03_1_worktrees_that_touch_the_same_file_list_each_other() {
    let f = Fixture::new();
    let (cli, repo) = (f.git(), Repo::discover(&f.repo).expect("discover").expect("repo"));
    let a = LaneManager::create(&cli, &repo, "a", &HashSet::new()).await.expect("a");
    let b = LaneManager::create(&cli, &repo, "b", &HashSet::new()).await.expect("b");
    let c = LaneManager::create(&cli, &repo, "c", &HashSet::new()).await.expect("c");
    // a commits a change to a.txt; b only edits it (uncommitted); c adds b.ts.
    std::fs::write(a.path.join("a.txt"), "one\nTWO\nthree\n").expect("write");
    git(&a.path, &["commit", "-qam", "a"]);
    std::fs::write(b.path.join("a.txt"), "one\ntwo\nTHREE\n").expect("write");
    std::fs::write(c.path.join("b.ts"), "new\n").expect("write");
    let fa = changed_files(&cli, &a.path, &a.base).await.expect("a files");
    let fb = changed_files(&cli, &b.path, &b.base).await.expect("b files");
    let fc = changed_files(&cli, &c.path, &c.base).await.expect("c files");
    assert_eq!(shared(&fa, &fb), vec!["a.txt".to_string()]);
    assert!(shared(&fa, &fc).is_empty());
    assert!(fc.contains("b.ts"), "untracked files count: {fc:?}");
}

#[tokio::test]
async fn f_wt_03_2_a_conflicting_branch_is_forecast_and_the_checkout_is_untouched() {
    let f = Fixture::new();
    let (cli, repo) = (f.git(), Repo::discover(&f.repo).expect("discover").expect("repo"));
    let lane = LaneManager::create(&cli, &repo, "fix", &HashSet::new()).await.expect("lane");
    std::fs::write(lane.path.join("a.txt"), "one\nfrom the worktree\nthree\n").expect("write");
    git(&lane.path, &["commit", "-qam", "lane"]);
    std::fs::write(f.repo.join("a.txt"), "one\nfrom main\nthree\n").expect("write");
    git(&f.repo, &["commit", "-qam", "main"]);
    let head = git(&f.repo, &["rev-parse", "HEAD"]);
    let out = forecast(&cli, &f.repo, "HEAD", &lane.branch).await.expect("forecast");
    assert_eq!(out, Forecast::Conflicts { files: vec!["a.txt".into()] });
    assert_eq!(git(&f.repo, &["rev-parse", "HEAD"]), head);
    assert!(!f.repo.join(".git/MERGE_HEAD").exists());
    assert_eq!(git(&f.repo, &["status", "--porcelain"]), "");
    assert_eq!(std::fs::read_to_string(f.repo.join("a.txt")).expect("read"), "one\nfrom main\nthree\n");
}

#[tokio::test]
async fn f_wt_03_3_a_separate_change_forecasts_clean() {
    let f = Fixture::new();
    let (cli, repo) = (f.git(), Repo::discover(&f.repo).expect("discover").expect("repo"));
    let lane = LaneManager::create(&cli, &repo, "docs", &HashSet::new()).await.expect("lane");
    std::fs::write(lane.path.join("docs.md"), "docs\n").expect("write");
    git(&lane.path, &["add", "."]);
    git(&lane.path, &["commit", "-qm", "docs"]);
    std::fs::write(f.repo.join("a.txt"), "one\ntwo\nthree\nfour\n").expect("write");
    git(&f.repo, &["commit", "-qam", "main"]);
    let out = forecast(&cli, &f.repo, "HEAD", &lane.branch).await.expect("forecast");
    assert_eq!(out, Forecast::Clean);
}
