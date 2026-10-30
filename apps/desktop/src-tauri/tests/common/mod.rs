//! What the fixture tests share: when a run rewrites a fixture, and how, so
//! a rewrite never passes as a test.

/// Whether this run rewrites the fixtures: only with `UPDATE_FIXTURES=1`,
/// and never on CI. Any other value is a mistake, so it fails.
pub fn updating() -> bool {
    match std::env::var("UPDATE_FIXTURES") {
        Err(std::env::VarError::NotPresent) => false,
        Ok(v) if v == "1" => {
            assert!(
                std::env::var_os("CI").is_none(),
                "UPDATE_FIXTURES is refused on CI: rewrite the fixtures locally and commit them"
            );
            true
        }
        Ok(v) => panic!("UPDATE_FIXTURES must be 1 to rewrite the fixtures, not {v:?}"),
        Err(e) => panic!("UPDATE_FIXTURES: {e}"),
    }
}

/// Writes `json` over the fixture at `path`, and fails when that changed
/// it: check its diff, then run again without UPDATE_FIXTURES.
pub fn rewrite(path: &str, json: &str) {
    let before = std::fs::read_to_string(path).expect("the fixture");
    std::fs::write(path, json).expect("the fixture written");
    assert!(
        before == json,
        "{path} was rewritten: check its diff, then run again without UPDATE_FIXTURES"
    );
}
