mod support;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use support::{git, isolate};
use tandem_git::{Git, LaneManager, Repo};

/// A folder holding a repository per name (each with one commit), and the
/// hardened runner.
fn folder(names: &[&str], empty: &[&str]) -> (tempfile::TempDir, PathBuf, Git) {
    isolate();
    let dir = tempfile::tempdir().expect("tmp");
    let root = std::fs::canonicalize(dir.path())
        .expect("canonical")
        .join("shop");
    for name in names.iter().chain(empty) {
        let repo = root.join(name);
        std::fs::create_dir_all(&repo).expect("mkdir");
        git(&repo, &["-c", "init.defaultBranch=main", "init", "-q"]);
        git(&repo, &["config", "user.name", "Test"]);
        git(&repo, &["config", "user.email", "test@example.invalid"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        if !empty.contains(name) {
            std::fs::write(repo.join("README.md"), format!("{name}\n")).expect("write");
            git(&repo, &["add", "."]);
            git(&repo, &["commit", "-qm", "first"]);
        }
    }
    let hooks = dir.path().join("empty-hooks");
    let cli = Git::locate(None, &hooks).expect("git");
    (dir, root, cli)
}

fn repos(root: &Path, names: &[&str]) -> Vec<(String, Repo)> {
    names
        .iter()
        .map(|n| {
            (
                (*n).to_string(),
                Repo::discover(&root.join(n))
                    .expect("discover")
                    .expect("repo"),
            )
        })
        .collect()
}

#[tokio::test]
async fn f_cd_22_1_one_worktree_per_repository_on_one_branch() {
    let (_d, root, cli) = folder(&["api", "web"], &[]);
    // The slug is taken in web only: both get the next free one.
    git(&root.join("web"), &["branch", "tandem/tidy"]);
    let lane = LaneManager::create_multi(
        &cli,
        &root,
        &repos(&root, &["api", "web"]),
        "tidy",
        &HashSet::new(),
    )
    .await
    .expect("create");
    assert_eq!(lane.slug, "tidy-2");
    assert_eq!(lane.branch, "tandem/tidy-2");
    assert_eq!(lane.path, root.join(".tandem/worktrees/tidy-2"));
    for (name, wt) in &lane.repos {
        assert_eq!(wt.path, lane.path.join(name));
        assert_eq!(
            git(&wt.path, &["branch", "--show-current"]),
            "tandem/tidy-2"
        );
        assert_eq!(git(&root.join(name), &["rev-parse", "HEAD"]), wt.base);
        assert!(wt.path.join("README.md").exists());
    }
    assert_eq!(
        lane.repos
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        ["api", "web"]
    );
}

#[tokio::test]
async fn f_cd_22_1_a_failing_repository_leaves_nothing_behind() {
    let (_d, root, cli) = folder(&["api"], &["web"]);
    let e = LaneManager::create_multi(
        &cli,
        &root,
        &repos(&root, &["api", "web"]),
        "tidy",
        &HashSet::new(),
    )
    .await
    .expect_err("web has no commits");
    assert!(e.to_string().contains("In web"), "{e}");
    assert!(!root.join(".tandem/worktrees/tidy").exists());
    assert_eq!(
        git(&root.join("api"), &["branch", "--list", "tandem/*"]),
        ""
    );
    assert_eq!(
        git(&root.join("api"), &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|l| l.starts_with("worktree "))
            .count(),
        1,
        "api's worktree was removed"
    );
}

#[tokio::test]
async fn f_cd_22_review_concurrent_creates_with_one_slug_get_their_own_folders() {
    let (_d, root, cli) = folder(&["api", "web"], &[]);
    let rs = repos(&root, &["api", "web"]);
    let none = HashSet::new();
    let (a, b) = tokio::join!(
        LaneManager::create_multi(&cli, &root, &rs, "tidy", &none),
        LaneManager::create_multi(&cli, &root, &rs, "tidy", &none),
    );
    // Both may not succeed (git refuses the second branch of one name), but
    // neither removes the other's checkouts.
    let made: Vec<_> = [a, b].into_iter().filter_map(Result::ok).collect();
    assert!(!made.is_empty());
    let slugs: HashSet<String> = made.iter().map(|l| l.slug.clone()).collect();
    assert_eq!(slugs.len(), made.len(), "each has its own slug");
    for lane in &made {
        for (_, wt) in &lane.repos {
            assert!(
                wt.path.join("README.md").exists(),
                "{} intact",
                wt.path.display()
            );
        }
    }
}
