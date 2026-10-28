//! F-CD-24 against the real `jj` (installed from its release; see
//! OPERATING). Without jj on PATH or in the usual places these say so and
//! return, rather than pass silently: `JJ_REQUIRED=1` turns that into a
//! failure.

mod support;

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use support::{git, Fixture};
use tandem_git::jj::{self, Jj, JjRunner};
use tandem_git::{FileDiff, LaneManager, Repo};

/// jj with an identity of its own (never the developer's config).
fn real_jj(dir: &Path) -> Option<Arc<dyn JjRunner>> {
    let config = dir.join("jj-config.toml");
    std::fs::write(
        &config,
        "[user]\nname = \"Test\"\nemail = \"test@example.invalid\"\n",
    )
    .expect("config");
    let env = vec![
        (
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        ),
        ("HOME".to_string(), dir.to_string_lossy().into_owned()),
        (
            "JJ_CONFIG".to_string(),
            config.to_string_lossy().into_owned(),
        ),
    ];
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match Jj::locate(
        &std::env::var("PATH").unwrap_or_default(),
        home.as_deref(),
        env,
    ) {
        Some(j) => Some(Arc::new(j)),
        None => {
            assert!(
                std::env::var("JJ_REQUIRED").is_err(),
                "JJ_REQUIRED is set but jj isn't installed"
            );
            eprintln!("F-CD-24: jj isn't installed here; the real-jj checks didn't run");
            None
        }
    }
}

/// The fixture repository, co-located with jj.
fn colocated(fx: &Fixture, jj: &dyn JjRunner) {
    let out = jj
        .run(&fx.repo, &["git", "init", "--colocate"])
        .expect("jj");
    assert!(out.success(), "jj git init: {}", out.stderr);
    assert!(jj::colocated(&fx.repo));
}

/// What a reviewer sees of a diff: each file, its counts, and its hunk
/// lines (headers left out: jj and git number them alike but spell them
/// differently).
fn seen(files: &[FileDiff]) -> Vec<(String, String, u32, u32, Vec<String>)> {
    let mut out: Vec<_> = files
        .iter()
        .map(|f| {
            let lines = f
                .patch
                .lines()
                .skip_while(|l| !l.starts_with("@@"))
                .filter(|l| l.starts_with('+') || l.starts_with('-') || l.starts_with(' '))
                .map(str::to_string)
                .collect();
            (
                f.path.clone(),
                format!("{:?}", f.change),
                f.additions,
                f.deletions,
                lines,
            )
        })
        .collect();
    out.sort();
    out
}

fn change(root: &Path) {
    std::fs::write(root.join("a.txt"), "one\nTWO\nthree\nfour\n").expect("write");
    std::fs::write(root.join("new.txt"), "fresh\n").expect("write");
}

#[tokio::test]
async fn f_cd_24_1_2_a_jj_workspace_diffs_and_overlaps_like_a_git_worktree() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let git_cli = fx.git();
    // A git worktree and a jj workspace, the same changes in each.
    let gl = LaneManager::create(&git_cli, &repo, "git lane", &HashSet::new())
        .await
        .expect("git lane");
    let jl = jj::add_workspace(jj.clone(), &repo, "jj lane", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    assert_eq!(jl.path, fx.repo.join(".tandem/worktrees/jj-lane"));
    assert!(
        jj::jj_without_git(&jl.path),
        "a secondary workspace has no .git"
    );
    assert_eq!(jl.base, gl.base, "both start from the repository's HEAD");
    change(&gl.path);
    change(&jl.path);
    let git_diff = Repo::discover(&gl.path)
        .expect("discover")
        .expect("lane repo")
        .diff_since(&gl.base)
        .await
        .expect("git diff");
    let jj_diff = jj::diff(jj.clone(), &jl.path, &jl.base)
        .await
        .expect("jj diff");
    assert_eq!(seen(&jj_diff), seen(&git_diff));
    assert_eq!(seen(&jj_diff).len(), 2);
    // The overlap check's files match too.
    let git_files: BTreeSet<String> =
        tandem_git::overlap::changed_files(&git_cli, &gl.path, &gl.base)
            .await
            .expect("git files");
    let jj_files: BTreeSet<String> = jj::changed_files(jj.clone(), &jl.path, &jl.base)
        .await
        .expect("jj files")
        .into_iter()
        .collect();
    assert_eq!(jj_files, git_files);
    // The main workspace never picks up the lanes under .tandem/.
    let st = jj.run(&fx.repo, &["status"]).expect("status");
    assert!(!st.stdout.contains(".tandem"), "{}", st.stdout);
}

#[tokio::test]
async fn f_cd_24_4_discard_forgets_the_workspace_and_keeps_its_commit() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let lane = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    // Uncommitted work: the snapshot keeps it in the workspace's commit.
    std::fs::write(lane.path.join("kept.txt"), "kept\n").expect("write");
    jj::forget_workspace(jj.clone(), &fx.repo, &lane.path, &lane.slug)
        .await
        .expect("forget");
    assert!(!lane.path.exists());
    let names = jj::workspace_names(jj.as_ref(), &fx.repo).expect("names");
    assert!(!names.contains("tidy"));
    // Its commit, with the file, is still in the repository.
    let log = jj
        .run(
            &fx.repo,
            &[
                "log",
                "-r",
                "all()",
                "--no-graph",
                "-T",
                "commit_id ++ \"\\n\"",
            ],
        )
        .expect("log");
    let mut found = false;
    for id in log.stdout.lines().filter(|l| !l.is_empty()) {
        let files = jj
            .run(&fx.repo, &["diff", "--name-only", "-r", id])
            .expect("files");
        if files.stdout.lines().any(|l| l.trim() == "kept.txt") {
            found = true;
        }
    }
    assert!(found, "the discarded workspace's commit keeps its file");
    // git still sees the repository as it was: no branch was deleted.
    assert_eq!(git(&fx.repo, &["branch", "--show-current"]), "main");
}

#[tokio::test]
async fn f_cd_24_1_a_workspace_made_from_another_starts_at_its_working_copy() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let first = jj::add_workspace(jj.clone(), &repo, "first", &HashSet::new(), None, None)
        .await
        .expect("first");
    std::fs::write(first.path.join("work.txt"), "w\n").expect("write");
    let _ = jj.run(&first.path, &["status"]);
    let next = jj::add_workspace(
        jj.clone(),
        &repo,
        "next",
        &HashSet::new(),
        Some(first.slug.clone()),
        None,
    )
    .await
    .expect("from the first");
    assert!(
        next.path.join("work.txt").exists(),
        "it starts from that work"
    );
    // And a second workspace of one name gets the next slug.
    let again = jj::add_workspace(jj.clone(), &repo, "first", &HashSet::new(), None, None)
        .await
        .expect("again");
    assert_eq!(again.slug, "first-2");
}

#[test]
fn f_cd_24_every_jj_call_carries_the_fixed_overrides_first() {
    let argv = jj::argv(&["status"]);
    assert_eq!(argv[..2], ["--no-pager", "--color=never"]);
    for o in jj::HARDENED {
        assert!(argv.contains(&o.to_string()), "{o}");
    }
    assert!(argv
        .iter()
        .any(|a| a == "--config=fsmonitor.backend=\"none\""));
    assert!(argv
        .iter()
        .any(|a| a == "--config=signing.behavior=\"drop\""));
    assert_eq!(argv.last().map(String::as_str), Some("status"));
}

#[tokio::test]
async fn f_cd_24_a_repositorys_own_config_runs_no_program() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    // A hostile repository config: a signing program, and watchman.
    let marker = fx.dir.path().join("ran");
    let program = fx.dir.path().join("sign.sh");
    std::fs::write(
        &program,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let config = fx.repo.join(".jj/repo/config.toml");
    let mut text = std::fs::read_to_string(&config).unwrap_or_default();
    text.push_str(&format!(
        "\n[signing]\nbehavior = \"own\"\nbackend = \"gpg\"\n[signing.backends.gpg]\nprogram = \"{}\"\n[fsmonitor]\nbackend = \"watchman\"\n",
        program.display()
    ));
    std::fs::write(&config, text).expect("config");
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let lane = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    change(&lane.path);
    jj::diff(jj.clone(), &lane.path, &lane.base)
        .await
        .expect("diff");
    jj::forget_workspace(jj.clone(), &fx.repo, &lane.path, &lane.slug)
        .await
        .expect("forget");
    assert!(!marker.exists(), "the repository's signing program ran");
}

#[tokio::test]
async fn f_cd_24_a_committed_or_foreign_jj_folder_is_refused() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    jj::check_repo(&fx.repo).expect("jj's own .jj");
    // A store that points at another repository's .git.
    let target = fx.repo.join(".jj/repo/store/git_target");
    let was = std::fs::read_to_string(&target).expect("git_target");
    let elsewhere = fx.dir.path().join("other");
    std::fs::create_dir_all(elsewhere.join(".git")).expect("mkdir");
    std::fs::write(&target, elsewhere.join(".git").to_string_lossy().as_bytes()).expect("write");
    let e = jj::check_repo(&fx.repo).expect_err("foreign");
    assert!(e.to_string().contains(jj::FOREIGN_STORE), "{e}");
    std::fs::write(&target, was).expect("restore");
    // A .jj committed to git, as a cloned repository could ship it.
    std::fs::write(fx.repo.join(".jj/repo/config.toml"), "").expect("config");
    git(&fx.repo, &["add", "-f", ".jj/repo/config.toml"]);
    git(&fx.repo, &["commit", "-qm", "ship a jj config"]);
    let e = jj::check_repo(&fx.repo).expect_err("committed");
    assert!(e.to_string().contains(jj::COMMITTED_JJ), "{e}");
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    assert!(
        jj::add_workspace(jj.clone(), &repo, "x", &HashSet::new(), None, None)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn f_cd_24_a_workspace_whose_making_was_cut_short_is_forgotten_by_the_sweep() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    // Vibezeo's own folder, outside the repository.
    let pending = fx.dir.path().join("data/jj-pending");
    // Made, but its row never written (the record stays).
    let lane = jj::add_workspace(
        jj.clone(),
        &repo,
        "cut",
        &HashSet::new(),
        None,
        Some(pending.clone()),
    )
    .await
    .expect("jj workspace");
    assert!(lane
        .pending
        .as_ref()
        .is_some_and(|p| p.starts_with(&pending)));
    // A finished one isn't touched.
    let kept = jj::add_workspace(
        jj.clone(),
        &repo,
        "kept",
        &HashSet::new(),
        None,
        Some(pending.clone()),
    )
    .await
    .expect("jj workspace");
    jj::done_adding(&kept);
    // One being made now isn't either.
    let busy_one = jj::add_workspace(
        jj.clone(),
        &repo,
        "busy",
        &HashSet::new(),
        None,
        Some(pending.clone()),
    )
    .await
    .expect("jj workspace");
    let busy = HashSet::from(["busy".to_string()]);
    let n = jj::forget_orphans(
        jj.clone(),
        &pending,
        &fx.repo,
        std::time::Duration::ZERO,
        &busy,
    )
    .await
    .expect("sweep");
    assert_eq!(n, 1);
    assert!(!lane.path.exists());
    let names = jj::workspace_names(jj.as_ref(), &fx.repo).expect("names");
    assert!(!names.contains("cut"));
    assert!(names.contains("kept") && kept.path.exists());
    assert!(names.contains("busy") && busy_one.path.exists());
}

#[tokio::test]
async fn f_cd_24_what_a_repository_commits_never_names_what_the_sweep_forgets() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    let pending = fx.dir.path().join("data/jj-pending");
    std::fs::create_dir_all(&pending).expect("mkdir");
    // Markers a repository could commit, in its own worktrees folder,
    // under the old scheme's name and the new one's.
    let dir = fx.repo.join(".tandem/worktrees");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let tidy = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect("a workspace with no row and no record of Vibezeo's");
    for name in [
        ".default.jj-adding",
        "default-1.pending",
        ".x.jj-adding",
        "tidy-1.pending",
    ] {
        std::fs::write(dir.join(name), format!("{}\ndefault\n", fx.repo.display())).expect("write");
    }
    git(&fx.repo, &["add", "-f", ".tandem/worktrees"]);
    git(&fx.repo, &["commit", "-qm", "ship markers"]);
    // A record naming `default`, even in Vibezeo's own folder.
    std::fs::write(
        pending.join("default-1.pending"),
        format!(
            "{}\ndefault\n",
            std::fs::canonicalize(&fx.repo)
                .expect("canonical")
                .display()
        ),
    )
    .expect("write");
    let n = jj::forget_orphans(
        jj.clone(),
        &pending,
        &fx.repo,
        std::time::Duration::ZERO,
        &HashSet::new(),
    )
    .await
    .expect("sweep");
    assert_eq!(n, 0);
    let names = jj::workspace_names(jj.as_ref(), &fx.repo).expect("names");
    assert!(
        names.contains("default"),
        "the main workspace is still there"
    );
    assert!(
        names.contains("tidy") && tidy.path.exists(),
        "a committed record naming tidy is never read"
    );
    // And Discard itself never forgets `default`.
    assert!(
        jj::forget_workspace(jj.clone(), &fx.repo, &dir.join("default"), "default")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn f_cd_24_a_repositorys_aliases_cant_change_the_base_or_the_diff() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    // A base that isn't root's tree: a diff from root() would show base.txt.
    std::fs::write(fx.repo.join("base.txt"), "base\n").expect("write");
    git(&fx.repo, &["add", "."]);
    git(&fx.repo, &["commit", "-qm", "second"]);
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let head = git(&fx.repo, &["rev-parse", "HEAD"]);
    // Aliases that would shadow the template keywords and the base's id.
    let config = fx.repo.join(".jj/repo/config.toml");
    let mut text = std::fs::read_to_string(&config).unwrap_or_default();
    text.push_str(&format!(
        "\n[revset-aliases]\n'{head}' = 'root()'\n'commit_id(x)' = 'root()'\n[template-aliases]\n'commit_id' = '\"{zero}\"'\n'name' = '\"evil\"'\n",
        zero = "0".repeat(40)
    ));
    std::fs::write(&config, text).expect("config");
    let lane = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    assert_eq!(lane.base, head, "the base is the real parent");
    let names = jj::workspace_names(jj.as_ref(), &fx.repo).expect("names");
    assert!(names.contains("tidy") && !names.contains("evil"));
    change(&lane.path);
    let files = jj::changed_files(jj.clone(), &lane.path, &lane.base)
        .await
        .expect("files");
    assert_eq!(
        files,
        vec!["a.txt".to_string(), "new.txt".to_string()],
        "not the whole tree from root (which has base.txt too)"
    );
    let next = jj::add_workspace(
        jj.clone(),
        &repo,
        "next",
        &HashSet::new(),
        Some("tidy".into()),
        None,
    )
    .await
    .expect("from tidy");
    assert!(
        next.path.join("new.txt").exists(),
        "it starts from tidy's working copy"
    );
}

#[tokio::test]
async fn f_cd_24_a_stale_record_never_forgets_a_live_or_someone_elses_workspace() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let pending = fx.dir.path().join("data/jj-pending");
    std::fs::create_dir_all(&pending).expect("mkdir");
    let key = std::fs::canonicalize(&fx.repo).expect("canonical");
    // The app stopped after writing a record for "fix", before jj made it;
    // later the lane was made again under the same name, with its row.
    jj::write_pending(&pending, &key, "fix").expect("stale record");
    let live = jj::add_workspace(
        jj.clone(),
        &repo,
        "fix",
        &HashSet::new(),
        None,
        Some(pending.clone()),
    )
    .await
    .expect("live lane");
    jj::done_adding(&live);
    // The person's own workspace of a name Vibezeo might use, elsewhere.
    let theirs = fx.dir.path().join("mine");
    let out = jj
        .run(
            &fx.repo,
            &[
                "workspace",
                "add",
                &theirs.to_string_lossy(),
                "--name",
                "mine",
            ],
        )
        .expect("jj");
    assert!(out.success(), "{}", out.stderr);
    jj::write_pending(&pending, &key, "mine").expect("stale record");
    let rows = HashSet::from(["fix".to_string()]);
    let n = jj::forget_orphans(
        jj.clone(),
        &pending,
        &fx.repo,
        std::time::Duration::ZERO,
        &rows,
    )
    .await
    .expect("sweep");
    assert_eq!(n, 0);
    let names = jj::workspace_names(jj.as_ref(), &fx.repo).expect("names");
    assert!(
        names.contains("fix") && live.path.exists(),
        "the live lane survives"
    );
    assert!(
        names.contains("mine") && theirs.exists(),
        "the person's own workspace survives"
    );
    assert!(
        std::fs::read_dir(&pending).expect("dir").next().is_none(),
        "the stale records went"
    );
}

#[tokio::test]
async fn f_cd_24_a_ref_named_like_the_base_is_refused_not_followed() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    let first = git(&fx.repo, &["rev-parse", "HEAD"]);
    std::fs::write(fx.repo.join("base.txt"), "base\n").expect("write");
    git(&fx.repo, &["add", "."]);
    git(&fx.repo, &["commit", "-qm", "second"]);
    let head = git(&fx.repo, &["rev-parse", "HEAD"]);
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let lane = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    assert_eq!(lane.base, head);
    // A clone ships a tag named exactly like the base, pointing elsewhere.
    git(&fx.repo, &["tag", &head, &first]);
    let _ = jj.run(&fx.repo, &["git", "import"]);
    change(&lane.path);
    let e = jj::changed_files(jj.clone(), &lane.path, &lane.base)
        .await
        .expect_err("refused");
    assert!(e.to_string().contains(jj::SHADOWED_BASE), "{e}");
    assert!(jj::diff(jj.clone(), &lane.path, &lane.base).await.is_err());
    // Without it, the diff is from the real base.
    git(&fx.repo, &["tag", "-d", &head]);
    let _ = jj.run(&fx.repo, &["git", "import"]);
    let files = jj::changed_files(jj.clone(), &lane.path, &lane.base)
        .await
        .expect("files");
    assert_eq!(files, vec!["a.txt".to_string(), "new.txt".to_string()]);
}
