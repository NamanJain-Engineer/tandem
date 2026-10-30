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
                !tandem_git::jj::jj_required(),
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

/// A person's confirmation, as Discard's native dialog gives it.
fn forced() -> Option<tandem_git::ForceConfirm> {
    let review =
        tandem_core::origin::LocalReview::check(&tandem_core::origin::Origin::main_window())
            .expect("main window");
    Some(tandem_git::ForceConfirm::new(review))
}

/// The commit a lane started from: its base's (its own first commit's)
/// parent.
fn start_of(jj: &dyn JjRunner, root: &Path, base: &str) -> String {
    commit_of(jj, root, &format!("\"{base}\"-"))
}

/// The commit id of `rev`.
fn commit_of(jj: &dyn JjRunner, root: &Path, rev: &str) -> String {
    let out = jj
        .run(
            root,
            &[
                "--ignore-working-copy",
                "log",
                "-r",
                rev,
                "--no-graph",
                "-T",
                "self.commit_id()",
            ],
        )
        .expect("jj");
    assert!(out.success(), "{}", out.stderr);
    out.stdout.trim().to_string()
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
    assert_eq!(jl.base.len(), 32, "a change id");
    assert_eq!(
        start_of(jj.as_ref(), &fx.repo, &jl.base),
        gl.base,
        "both start from the repository's HEAD"
    );
    change(&gl.path);
    change(&jl.path);
    let git_diff = Repo::discover(&gl.path)
        .expect("discover")
        .expect("lane repo")
        .diff_since(&gl.base)
        .await
        .expect("git diff");
    let jj_diff = jj::diff(jj.clone(), &fx.repo, &jl.path, &jl.base)
        .await
        .expect("jj diff");
    assert_eq!(seen(&jj_diff), seen(&git_diff));
    assert_eq!(seen(&jj_diff).len(), 2);
    // The overlap check's files match too.
    let git_files: BTreeSet<String> =
        tandem_git::overlap::changed_files(&git_cli, &gl.path, &gl.base)
            .await
            .expect("git files");
    let jj_files: BTreeSet<String> = jj::changed_files(jj.clone(), &fx.repo, &jl.path, &jl.base)
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
    // A file jj won't snapshot (over its size limit) would be lost with the
    // folder: Discard says so, as a git lane's uncommitted changes do.
    std::fs::write(lane.path.join("big.bin"), vec![b'x'; 2 * 1024 * 1024]).expect("write");
    let e = jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        None,
    )
    .await
    .expect_err("dirty");
    assert!(matches!(e, tandem_git::GitError::Dirty), "{e}");
    assert!(lane.path.join("big.bin").exists());
    assert!(jj::workspace_names(jj.as_ref(), &fx.repo)
        .expect("names")
        .contains("tidy"));
    // The person confirmed: forced.
    jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        forced(),
    )
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
    // The source keeps working: its edits don't move the new lane, which
    // isn't left stale and diffs only its own changes.
    std::fs::write(first.path.join("work.txt"), "w2\n").expect("write");
    std::fs::write(first.path.join("more.txt"), "m\n").expect("write");
    let st = jj.run(&first.path, &["status"]).expect("status");
    assert!(st.success(), "{}", st.stderr);
    std::fs::write(next.path.join("own.txt"), "o\n").expect("write");
    let files = jj::changed_files(jj.clone(), &fx.repo, &next.path, &next.base)
        .await
        .expect("the handoff lane isn't stale");
    assert_eq!(files, vec!["own.txt".to_string()]);
    assert_eq!(
        std::fs::read_to_string(next.path.join("work.txt")).expect("read"),
        "w\n"
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
    jj::diff(jj.clone(), &fx.repo, &lane.path, &lane.base)
        .await
        .expect("diff");
    jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        None,
    )
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
    let key = std::fs::canonicalize(&fx.repo).expect("canonical");
    // Markers naming tidy, in the old line format and in the record format
    // (written by Vibezeo's own writer, so they'd parse if they were read).
    for name in [".tidy.jj-adding", ".default.jj-adding"] {
        std::fs::write(dir.join(name), format!("{}\ntidy\n", key.display())).expect("write");
    }
    jj::write_pending(&dir, &key, "tidy").expect("committed record");
    git(&fx.repo, &["add", "-f", ".tandem/worktrees"]);
    git(&fx.repo, &["commit", "-qm", "ship markers"]);
    // A record naming `default`, even in Vibezeo's own folder.
    jj::write_pending(&pending, &key, "default").expect("record");
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
    assert!(jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &dir.join("default"),
        "default",
        None,
        forced()
    )
    .await
    .is_err());
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
    assert_eq!(
        start_of(jj.as_ref(), &fx.repo, &lane.base),
        head,
        "the base is the real parent"
    );
    let names = jj::workspace_names(jj.as_ref(), &fx.repo).expect("names");
    assert!(names.contains("tidy") && !names.contains("evil"));
    change(&lane.path);
    let files = jj::changed_files(jj.clone(), &fx.repo, &lane.path, &lane.base)
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
    assert_eq!(start_of(jj.as_ref(), &fx.repo, &lane.base), head);
    // A clone ships a tag named exactly like the base, pointing elsewhere.
    git(&fx.repo, &["tag", &lane.base, &first]);
    let _ = jj.run(&fx.repo, &["git", "import"]);
    change(&lane.path);
    let e = jj::changed_files(jj.clone(), &fx.repo, &lane.path, &lane.base)
        .await
        .expect_err("refused");
    assert!(e.to_string().contains(jj::SHADOWED_BASE), "{e}");
    assert!(jj::diff(jj.clone(), &fx.repo, &lane.path, &lane.base)
        .await
        .is_err());
    // Without it, the diff is from the real base.
    git(&fx.repo, &["tag", "-d", &lane.base]);
    let _ = jj.run(&fx.repo, &["git", "import"]);
    let files = jj::changed_files(jj.clone(), &fx.repo, &lane.path, &lane.base)
        .await
        .expect("files");
    assert_eq!(files, vec!["a.txt".to_string(), "new.txt".to_string()]);
}

#[tokio::test]
async fn f_cd_24_a_squash_in_the_main_checkout_keeps_the_lanes_base() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let lane = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    change(&lane.path);
    let _ = jj.run(&lane.path, &["status"]);
    let before = start_of(jj.as_ref(), &fx.repo, &lane.base);
    // The person squashes work into the commit the lane started from (its
    // first commit's parent) in the main checkout: that commit is rewritten.
    std::fs::write(fx.repo.join("main.txt"), "m\n").expect("write");
    let out = jj.run(&fx.repo, &["squash"]).expect("squash");
    assert!(out.success(), "{}", out.stderr);
    assert_ne!(start_of(jj.as_ref(), &fx.repo, &lane.base), before);
    // jj rebased the lane onto it, which leaves its folder stale: the diff
    // says what to do, and after that it's the lane's own changes again.
    let e = jj::diff(jj.clone(), &fx.repo, &lane.path, &lane.base)
        .await
        .expect_err("stale");
    assert_eq!(e.to_string(), jj::stale(&lane.path));
    // Discard says the same, and never as a Conflict a dialog would force
    // past: the folder stays.
    let e = jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        None,
    )
    .await
    .expect_err("stale");
    assert_eq!(e.to_string(), jj::stale(&lane.path));
    assert_eq!(e.code(), tandem_core::ErrorCode::Invalid);
    assert!(lane.path.join("new.txt").exists());
    let out = jj
        .run(&lane.path, &["workspace", "update-stale"])
        .expect("update-stale");
    assert!(out.success(), "{}", out.stderr);
    let files = jj::changed_files(jj.clone(), &fx.repo, &lane.path, &lane.base)
        .await
        .expect("files");
    assert_eq!(
        files,
        vec!["a.txt".to_string(), "new.txt".to_string()],
        "not main.txt: the base moved with the squash"
    );
}

#[tokio::test]
async fn f_cd_24_an_agents_squash_stays_in_the_lane_and_its_diff() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let person = commit_of(jj.as_ref(), &fx.repo, "@-");
    let lane = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    assert_eq!(start_of(jj.as_ref(), &fx.repo, &lane.base), person);
    // The agent squashes its work into the commit below its working copy
    // (the lane's own first commit), twice.
    change(&lane.path);
    let out = jj.run(&lane.path, &["squash"]).expect("squash");
    assert!(out.success(), "{}", out.stderr);
    std::fs::write(lane.path.join("more.txt"), "m\n").expect("write");
    let out = jj.run(&lane.path, &["squash"]).expect("squash");
    assert!(out.success(), "{}", out.stderr);
    let files = jj::changed_files(jj.clone(), &fx.repo, &lane.path, &lane.base)
        .await
        .expect("files");
    assert_eq!(
        files,
        vec!["a.txt".to_string(), "more.txt".into(), "new.txt".into()],
        "the squashed work still shows"
    );
    // The person's commit wasn't touched, and their checkout isn't stale.
    assert_eq!(commit_of(jj.as_ref(), &fx.repo, "@-"), person);
    let st = jj.run(&fx.repo, &["status"]).expect("status");
    assert!(st.success(), "{}", st.stderr);
}

#[tokio::test]
async fn f_cd_24_4_discard_keeps_work_jj_stopped_snapshotting_or_left_untracked() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    // The person forgets the workspace while the agent keeps editing: jj
    // in it no longer snapshots anything.
    let lane = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    let out = jj
        .run(&fx.repo, &["workspace", "forget", "tidy"])
        .expect("forget");
    assert!(out.success(), "{}", out.stderr);
    std::fs::write(lane.path.join("late.txt"), "late\n").expect("write");
    let e = jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        None,
    )
    .await
    .expect_err("not kept");
    assert!(matches!(e, tandem_git::GitError::Dirty), "{e}");
    assert!(lane.path.join("late.txt").exists());
    jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        forced(),
    )
    .await
    .expect("forced");
    assert!(!lane.path.exists());
    // A file outside `snapshot.auto-track` (the repository's setting).
    let other = jj::add_workspace(jj.clone(), &repo, "auto", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    let config = fx.repo.join(".jj/repo/config.toml");
    let mut text = std::fs::read_to_string(&config).unwrap_or_default();
    text.push_str("\n[snapshot]\nauto-track = \"none()\"\n");
    std::fs::write(&config, text).expect("config");
    std::fs::write(other.path.join("notes.txt"), "n\n").expect("write");
    let e = jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &other.path,
        &other.slug,
        Some(&other.base),
        None,
    )
    .await
    .expect_err("untracked");
    assert!(matches!(e, tandem_git::GitError::Dirty), "{e}");
    assert!(other.path.join("notes.txt").exists());
    // A nested clone in it.
    std::fs::write(&config, "").expect("config");
    let nested = other.path.join("vendor/lib");
    std::fs::create_dir_all(&nested).expect("mkdir");
    git(&nested, &["init", "-q"]);
    let e = jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &other.path,
        &other.slug,
        Some(&other.base),
        None,
    )
    .await
    .expect_err("nested");
    assert!(matches!(e, tandem_git::GitError::Dirty), "{e}");
    assert!(nested.join(".git").exists());
}

#[tokio::test]
async fn f_cd_24_a_quiet_or_templated_jj_still_hands_off() {
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
    // Config an agent in a lane can write: quiet, and a summary that
    // starts with a word jj's change ids are made of.
    let config = fx.repo.join(".jj/repo/config.toml");
    let mut text = std::fs::read_to_string(&config).unwrap_or_default();
    text.push_str("\n[ui]\nquiet = true\n[templates]\ncommit_summary = '\"lux \" ++ change_id'\n");
    std::fs::write(&config, text).expect("config");
    let next = jj::add_workspace(
        jj.clone(),
        &repo,
        "next",
        &HashSet::new(),
        Some(first.slug.clone()),
        None,
    )
    .await
    .expect("handoff");
    assert!(next.path.join("work.txt").exists());
}

#[tokio::test]
async fn f_cd_24_an_alias_named_like_the_base_cant_move_the_diff() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let lane = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    let config = fx.repo.join(".jj/repo/config.toml");
    let mut text = std::fs::read_to_string(&config).unwrap_or_default();
    text.push_str(&format!("\n[revset-aliases]\n'{}' = 'root()'\n", lane.base));
    std::fs::write(&config, text).expect("config");
    change(&lane.path);
    let files = jj::changed_files(jj.clone(), &fx.repo, &lane.path, &lane.base)
        .await
        .expect("files");
    assert_eq!(files, vec!["a.txt".to_string(), "new.txt".to_string()]);
}

/// Every commit in the repository's log: (change id, empty, description).
fn log_all(jj: &dyn JjRunner, root: &Path) -> Vec<String> {
    let out = jj
        .run(
            root,
            &[
                "--ignore-working-copy",
                "log",
                "-r",
                "all()",
                "--no-graph",
                "-T",
                "self.change_id() ++ \"\\n\"",
            ],
        )
        .expect("log");
    out.stdout.lines().map(str::to_string).collect()
}

#[tokio::test]
async fn f_cd_24_4_an_unused_lane_leaves_nothing_in_jj_log() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let before = log_all(jj.as_ref(), &fx.repo);
    let lane = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        None,
    )
    .await
    .expect("discard");
    assert_eq!(
        log_all(jj.as_ref(), &fx.repo),
        before,
        "its empty first commit went"
    );
    // A lane with work keeps it (and its first commit, which the work sits on).
    let used = jj::add_workspace(jj.clone(), &repo, "used", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    change(&used.path);
    jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &used.path,
        &used.slug,
        Some(&used.base),
        None,
    )
    .await
    .expect("discard");
    let after = log_all(jj.as_ref(), &fx.repo);
    assert!(
        after.contains(&used.base),
        "the first commit stays under the work"
    );
    assert_eq!(after.len(), before.len() + 2);
}

#[tokio::test]
async fn f_cd_24_a_lane_beside_a_merge_is_refused_and_leaves_nothing() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    // The person's working copy is a merge of main and another commit.
    let run = |args: &[&str]| {
        let out = jj.run(&fx.repo, args).expect("jj");
        assert!(out.success(), "{}", out.stderr);
    };
    run(&["new", "-m", "other", "root()"]);
    std::fs::write(fx.repo.join("other.txt"), "o\n").expect("write");
    run(&["new", "@", "main"]);
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    let before = log_all(jj.as_ref(), &fx.repo);
    let e = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect_err("a merge");
    assert_eq!(e.to_string(), jj::MERGE_BASE);
    assert!(!fx.repo.join(".tandem/worktrees/tidy").exists());
    assert!(!jj::workspace_names(jj.as_ref(), &fx.repo)
        .expect("names")
        .contains("tidy"));
    assert_eq!(
        log_all(jj.as_ref(), &fx.repo),
        before,
        "nothing left in jj log"
    );
}

#[tokio::test]
async fn f_cd_24_4_ignored_files_go_and_files_jj_skipped_dont() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    // Ignored content (by the lane's .gitignore, and by the repository's
    // info/exclude), even a nested clone in it, goes without force, as a
    // git lane's ignored files do.
    let lane = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    std::fs::write(lane.path.join(".gitignore"), "vendor/\n").expect("write");
    let nested = lane.path.join("vendor/lib");
    std::fs::create_dir_all(&nested).expect("mkdir");
    git(&nested, &["init", "-q"]);
    std::fs::write(nested.join("x.txt"), "x\n").expect("write");
    let exclude = fx.repo.join(".git/info/exclude");
    let mut text = std::fs::read_to_string(&exclude).unwrap_or_default();
    // Plain and anchored patterns, matched from the lane's root as jj does.
    text.push_str("\n*.scratch\n/build/\ndist/*.js\n.tandem/*\n");
    std::fs::write(&exclude, text).expect("exclude");
    std::fs::write(lane.path.join("notes.scratch"), "n\n").expect("write");
    for (dir, file) in [("build", "out.o"), ("dist", "app.js"), (".tandem", "state")] {
        std::fs::create_dir_all(lane.path.join(dir)).expect("mkdir");
        std::fs::write(lane.path.join(dir).join(file), "x\n").expect("write");
    }
    jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        None,
    )
    .await
    .expect("ignored content needs no force");
    assert!(!lane.path.exists());
    // A file outside the workspace's sparse patterns: jj never snapshots it.
    let sparse = jj::add_workspace(jj.clone(), &repo, "sparse", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    std::fs::create_dir_all(sparse.path.join("kept")).expect("mkdir");
    let out = jj
        .run(&sparse.path, &["sparse", "set", "--clear", "--add", "kept"])
        .expect("sparse");
    assert!(out.success(), "{}", out.stderr);
    std::fs::write(sparse.path.join("outside.txt"), "o\n").expect("write");
    let e = jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &sparse.path,
        &sparse.slug,
        Some(&sparse.base),
        None,
    )
    .await
    .expect_err("outside the sparse patterns");
    assert!(matches!(e, tandem_git::GitError::Dirty), "{e}");
    assert!(sparse.path.join("outside.txt").exists());
}

#[tokio::test]
async fn f_cd_24_4_a_tracked_file_rewritten_outside_the_sparse_patterns_is_kept() {
    let fx = Fixture::new();
    let Some(jj) = real_jj(fx.dir.path()) else {
        return;
    };
    // A repository with a root file and two folders.
    std::fs::create_dir_all(fx.repo.join("kept")).expect("mkdir");
    std::fs::create_dir_all(fx.repo.join("svc")).expect("mkdir");
    std::fs::write(fx.repo.join("kept/k.txt"), "k\n").expect("write");
    std::fs::write(fx.repo.join("svc/s.txt"), "s\n").expect("write");
    std::fs::write(fx.repo.join("package.json"), "{}\n").expect("write");
    std::fs::create_dir_all(fx.repo.join("docs")).expect("mkdir");
    std::fs::write(fx.repo.join("docs/guide.md"), "g\n").expect("write");
    git(&fx.repo, &["add", "."]);
    git(&fx.repo, &["commit", "-qm", "layout"]);
    colocated(&fx, jj.as_ref());
    let repo = Repo::discover(&fx.repo).expect("discover").expect("repo");
    // Sparse in the lane: an agent rewrites a tracked root file, which jj
    // neither snapshots nor warns about (checked on jj 0.34).
    let lane = jj::add_workspace(jj.clone(), &repo, "tidy", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    let out = jj
        .run(&lane.path, &["sparse", "set", "--clear", "--add", "kept"])
        .expect("sparse");
    assert!(out.success(), "{}", out.stderr);
    std::fs::write(lane.path.join("package.json"), "{\"rewritten\": true}\n").expect("write");
    let e = jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        None,
    )
    .await
    .expect_err("outside the sparse patterns");
    assert!(matches!(e, tandem_git::GitError::Dirty), "{e}");
    assert!(lane.path.join("package.json").exists());
    std::fs::remove_file(lane.path.join("package.json")).expect("rm");
    // An ignore rule covering a tracked path outside the patterns hides
    // nothing: rewritten there, it's still not kept.
    let exclude = fx.repo.join(".git/info/exclude");
    let mut text = std::fs::read_to_string(&exclude).unwrap_or_default();
    text.push_str("\ndocs/\n");
    std::fs::write(&exclude, text).expect("exclude");
    std::fs::create_dir_all(lane.path.join("docs")).expect("mkdir");
    std::fs::write(lane.path.join("docs/guide.md"), "rewritten\n").expect("write");
    let e = jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        None,
    )
    .await
    .expect_err("ignored, tracked, outside the patterns");
    assert!(matches!(e, tandem_git::GitError::Dirty), "{e}");
    assert!(lane.path.join("docs/guide.md").exists());
    std::fs::remove_dir_all(lane.path.join("docs")).expect("rm");
    // An untracked file nothing ignores, outside the patterns.
    std::fs::write(lane.path.join("notes.txt"), "n\n").expect("write");
    let e = jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        None,
    )
    .await
    .expect_err("untracked, outside the patterns");
    assert!(matches!(e, tandem_git::GitError::Dirty), "{e}");
    std::fs::remove_file(lane.path.join("notes.txt")).expect("rm");
    // Nothing outside the patterns: a clean sparse lane discards unforced.
    jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &lane.path,
        &lane.slug,
        Some(&lane.base),
        None,
    )
    .await
    .expect("a clean sparse lane");
    // A lane made beside a sparse main checkout copies its patterns
    // (`--sparse-patterns copy`, jj's default: checked on jj 0.34).
    let out = jj
        .run(&fx.repo, &["sparse", "set", "--clear", "--add", "svc"])
        .expect("sparse");
    assert!(out.success(), "{}", out.stderr);
    let inherited = jj::add_workspace(jj.clone(), &repo, "inherited", &HashSet::new(), None, None)
        .await
        .expect("jj workspace");
    assert!(!inherited.path.join("package.json").exists());
    std::fs::write(inherited.path.join("package.json"), "{\"pnpm\": 1}\n").expect("write");
    let e = jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &inherited.path,
        &inherited.slug,
        Some(&inherited.base),
        None,
    )
    .await
    .expect_err("inherited sparse patterns");
    assert!(matches!(e, tandem_git::GitError::Dirty), "{e}");
    // Inside the patterns, kept as usual.
    std::fs::remove_file(inherited.path.join("package.json")).expect("rm");
    std::fs::write(inherited.path.join("svc/s.txt"), "changed\n").expect("write");
    jj::forget_workspace(
        jj.clone(),
        &fx.repo,
        &inherited.path,
        &inherited.slug,
        Some(&inherited.base),
        None,
    )
    .await
    .expect("inside the patterns");
}
