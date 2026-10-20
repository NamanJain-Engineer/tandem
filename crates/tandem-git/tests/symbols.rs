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
