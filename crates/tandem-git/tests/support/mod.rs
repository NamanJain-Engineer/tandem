#![allow(dead_code)]

pub mod trap;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

use tandem_git::Git;

/// Tests never read the developer's global or system git config.
pub fn isolate() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let empty = std::env::temp_dir().join("tandem-git-tests-empty-gitconfig");
        std::fs::write(&empty, "").expect("empty gitconfig");
        std::env::set_var("GIT_CONFIG_GLOBAL", &empty);
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        std::env::set_var("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z");
        std::env::set_var("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z");
    });
}

/// Plain git, for setting fixtures up (not the hardened runner).
pub fn git(cwd: &Path, args: &[&str]) -> String {
    isolate();
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub repo: PathBuf,
    pub hooks: PathBuf,
}

impl Fixture {
    /// A repository on `main` with one commit (`a.txt`).
    pub fn new() -> Self {
        isolate();
        let dir = tempfile::tempdir().expect("tmp");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).expect("mkdir");
        let repo = std::fs::canonicalize(&repo).expect("canonical");
        git(&repo, &["-c", "init.defaultBranch=main", "init", "-q"]);
        git(&repo, &["config", "user.name", "Test"]);
        git(&repo, &["config", "user.email", "test@example.invalid"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\nthree\n").expect("write");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "first"]);
        let hooks = dir.path().join("empty-hooks");
        Fixture { dir, repo, hooks }
    }

    pub fn git(&self) -> Git {
        Git::locate(None, &self.hooks).expect("git on PATH")
    }

    pub fn commit_all(&self, cwd: &Path, msg: &str) {
        git(cwd, &["add", "-A"]);
        git(cwd, &["commit", "-qm", msg]);
    }
}
