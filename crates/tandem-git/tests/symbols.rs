mod support;

use std::collections::HashSet;

use support::{git, Fixture};
use tandem_git::symbols::{index_cached, ParseCache, SymbolChange};
use tandem_git::{LaneManager, Repo};

#[tokio::test]
async fn f_wt_10_a_refresh_parses_only_the_files_that_changed_since() {
    let f = Fixture::new();
    let (cli, repo) = (
        f.git(),
        Repo::discover(&f.repo).expect("discover").expect("repo"),
    );
    let lane = LaneManager::create(&cli, &repo, "radar", &HashSet::new())
        .await
        .expect("lane");
    let base = git(&lane.path, &["rev-parse", "HEAD"]);
    std::fs::write(lane.path.join("a.ts"), "export function alpha() {}\n").expect("write");
    std::fs::write(lane.path.join("b.py"), "def beta():\n    pass\n").expect("write");
    std::fs::write(lane.path.join("notes.md"), "words\n").expect("write");
    let mut cache = ParseCache::default();

    let first = index_cached(&cli, &lane.path, &base, &mut cache)
        .await
        .expect("index");
    assert_eq!(first.parsed, 2);
    assert_eq!(first.index.unparsed, 1, "the Markdown file is counted");
    let names = |r: &tandem_git::symbols::Refreshed| {
        let mut n: Vec<(String, SymbolChange)> = r
            .index
            .symbols
            .iter()
            .map(|s| (s.qualified.clone(), s.change))
            .collect();
        n.sort();
        n
    };
    assert_eq!(
        names(&first),
        [
            ("alpha".to_string(), SymbolChange::Added),
            ("beta".to_string(), SymbolChange::Added)
        ]
    );

    // Nothing touched: nothing parsed, the same index.
    let again = index_cached(&cli, &lane.path, &base, &mut cache)
        .await
        .expect("index");
    assert_eq!(again.parsed, 0);
    assert_eq!(again.index, first.index);

    // One file touched: only it is parsed again.
    std::fs::write(lane.path.join("a.ts"), "export function gamma() {}\n").expect("write");
    let third = index_cached(&cli, &lane.path, &base, &mut cache)
        .await
        .expect("index");
    assert_eq!(third.parsed, 1);
    assert_eq!(
        names(&third),
        [
            ("beta".to_string(), SymbolChange::Added),
            ("gamma".to_string(), SymbolChange::Added)
        ]
    );

    // A file reverted to the base leaves the index.
    std::fs::remove_file(lane.path.join("b.py")).expect("rm");
    let fourth = index_cached(&cli, &lane.path, &base, &mut cache)
        .await
        .expect("index");
    assert_eq!(fourth.parsed, 0);
    assert_eq!(names(&fourth), [("gamma".to_string(), SymbolChange::Added)]);
}

#[cfg(unix)]
#[tokio::test]
async fn f_wt_10_a_link_to_a_pipe_or_out_of_the_worktree_is_not_read() {
    // `metadata` followed a link to a FIFO, whose read then waited for good
    // (the review of 9a4e93ab, M2). Only a regular file is read: a link is
    // counted, not followed, so no pipe is waited on and no file out of the
    // worktree is read.
    let f = Fixture::new();
    let (cli, repo) = (
        f.git(),
        Repo::discover(&f.repo).expect("discover").expect("repo"),
    );
    let lane = LaneManager::create(&cli, &repo, "radar", &HashSet::new())
        .await
        .expect("lane");
    let base = git(&lane.path, &["rev-parse", "HEAD"]);
    let outside = tempfile::tempdir().expect("dir");
    let pipe = outside.path().join("pipe");
    let made = std::process::Command::new("mkfifo")
        .arg(&pipe)
        .status()
        .expect("mkfifo");
    assert!(made.success());
    // Its writer stays open, so a read would wait for data that never comes.
    let _writer = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pipe)
        .expect("open the pipe");
    std::os::unix::fs::symlink(&pipe, lane.path.join("evil.ts")).expect("link");
    let away = outside.path().join("secret.ts");
    std::fs::write(&away, "export function hidden() {}\n").expect("write");
    std::os::unix::fs::symlink(&away, lane.path.join("away.ts")).expect("link");
    std::fs::write(lane.path.join("ok.ts"), "export function fine() {}\n").expect("write");
    let mut cache = ParseCache::default();

    let done = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        index_cached(&cli, &lane.path, &base, &mut cache),
    )
    .await
    .expect("the index doesn't wait on the pipe")
    .expect("index");
    let names: Vec<&str> = done
        .index
        .symbols
        .iter()
        .map(|s| s.qualified.as_str())
        .collect();
    assert_eq!(names, ["fine"]);
    assert_eq!(
        done.index.unparsed, 2,
        "the two links are counted, not read"
    );
}

#[tokio::test]
async fn f_wt_10_a_big_base_file_and_text_that_isnt_utf8_are_unparsed() {
    let f = Fixture::new();
    // In the base: TypeScript past the 512 KiB limit, and a small file.
    let big: String = (0..30_000)
        .map(|i| format!("export const v{i} = {i};\n"))
        .collect();
    assert!(big.len() > 512 * 1024);
    std::fs::write(f.repo.join("big.ts"), &big).expect("write");
    std::fs::write(f.repo.join("small.ts"), "export function keep() {}\n").expect("write");
    git(&f.repo, &["add", "-A"]);
    git(&f.repo, &["commit", "-qm", "base files"]);
    let (cli, repo) = (
        f.git(),
        Repo::discover(&f.repo).expect("discover").expect("repo"),
    );
    let lane = LaneManager::create(&cli, &repo, "radar-big", &HashSet::new())
        .await
        .expect("lane");
    let base = git(&lane.path, &["rev-parse", "HEAD"]);
    // Deleted here: the base's copy is too big to read, so its definitions
    // aren't listed as removed.
    std::fs::remove_file(lane.path.join("big.ts")).expect("rm");
    // Not UTF-8: unparsed, not every definition in it removed.
    std::fs::write(
        lane.path.join("small.ts"),
        b"export function keep() {}\n\xff\n",
    )
    .expect("write");
    // Added: parsed with no base to read.
    std::fs::write(lane.path.join("fresh.ts"), "export function fresh() {}\n").expect("write");
    let got = index_cached(&cli, &lane.path, &base, &mut ParseCache::default())
        .await
        .expect("index");
    assert_eq!(got.index.unparsed, 2, "{:?}", got.index);
    let names: Vec<(&str, SymbolChange)> = got
        .index
        .symbols
        .iter()
        .map(|s| (s.qualified.as_str(), s.change))
        .collect();
    assert_eq!(names, [("fresh", SymbolChange::Added)]);
}

#[tokio::test]
async fn f_wt_10_many_long_new_paths_dont_stall_the_base_check() {
    // 300 new files with paths of about 470 characters: `cat-file
    // --batch-check` names each missing one back, and with its input
    // written before its output was read, both pipes filled and the
    // refresh never returned (the review of 30d5bcad, M-2).
    let f = Fixture::new();
    let (cli, repo) = (
        f.git(),
        Repo::discover(&f.repo).expect("discover").expect("repo"),
    );
    let lane = LaneManager::create(&cli, &repo, "radar-long", &HashSet::new())
        .await
        .expect("lane");
    let base = git(&lane.path, &["rev-parse", "HEAD"]);
    let folder = lane.path.join("a".repeat(200)).join("b".repeat(200));
    std::fs::create_dir_all(&folder).expect("mkdir");
    for n in 0..300 {
        std::fs::write(
            folder.join(format!("{}_{n:03}.ts", "c".repeat(60))),
            format!("export function made{n}() {{}}\n"),
        )
        .expect("write");
    }
    let got = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        index_cached(&cli, &lane.path, &base, &mut ParseCache::default()),
    )
    .await
    .expect("the refresh returns")
    .expect("index");
    assert_eq!(got.parsed, 300);
    assert_eq!(got.index.symbols.len(), 300);
    assert!(got
        .index
        .symbols
        .iter()
        .all(|s| s.change == SymbolChange::Added));
}

#[tokio::test]
async fn f_wt_10_a_base_path_with_a_line_break_is_read_from_the_base() {
    // Left out of the batch, it read as a new file: every definition in it
    // added rather than the one changed (the review of 30d5bcad).
    let f = Fixture::new();
    let odd = f.repo.join("we\nird");
    std::fs::create_dir_all(&odd).expect("mkdir");
    std::fs::write(
        odd.join("a.ts"),
        "export function alpha() { return 1; }\nexport function beta() { return 2; }\n",
    )
    .expect("write");
    git(&f.repo, &["add", "-A"]);
    git(&f.repo, &["commit", "-qm", "odd path"]);
    let (cli, repo) = (
        f.git(),
        Repo::discover(&f.repo).expect("discover").expect("repo"),
    );
    let lane = LaneManager::create(&cli, &repo, "radar-odd", &HashSet::new())
        .await
        .expect("lane");
    let base = git(&lane.path, &["rev-parse", "HEAD"]);
    std::fs::write(
        lane.path.join("we\nird").join("a.ts"),
        "export function alpha() { return 10; }\nexport function beta() { return 2; }\n",
    )
    .expect("write");
    let got = index_cached(&cli, &lane.path, &base, &mut ParseCache::default())
        .await
        .expect("index");
    let names: Vec<(&str, SymbolChange)> = got
        .index
        .symbols
        .iter()
        .map(|s| (s.qualified.as_str(), s.change))
        .collect();
    assert_eq!(names, [("alpha", SymbolChange::Changed)]);
}

#[cfg(unix)]
#[tokio::test]
async fn f_wt_10_a_file_that_cant_be_read_is_unparsed_not_deleted() {
    // A link loop, a permission or a full file table read as a deleted
    // file: every definition in the base's copy removed (the review of
    // 30d5bcad). Here its folder became a link to itself, so looking the
    // file up fails with ELOOP.
    let f = Fixture::new();
    std::fs::create_dir_all(f.repo.join("dir")).expect("mkdir");
    std::fs::write(f.repo.join("dir/keep.ts"), "export function keep() {}\n").expect("write");
    git(&f.repo, &["add", "-A"]);
    git(&f.repo, &["commit", "-qm", "base file"]);
    let (cli, repo) = (
        f.git(),
        Repo::discover(&f.repo).expect("discover").expect("repo"),
    );
    let lane = LaneManager::create(&cli, &repo, "radar-unreadable", &HashSet::new())
        .await
        .expect("lane");
    let base = git(&lane.path, &["rev-parse", "HEAD"]);
    std::fs::remove_dir_all(lane.path.join("dir")).expect("rm");
    std::os::unix::fs::symlink("dir", lane.path.join("dir")).expect("link");
    let got = index_cached(&cli, &lane.path, &base, &mut ParseCache::default())
        .await
        .expect("index");
    assert!(got.index.symbols.is_empty(), "{:?}", got.index.symbols);
    // The link itself, and the file under it.
    assert_eq!(got.index.unparsed, 2, "{:?}", got.index);
}
