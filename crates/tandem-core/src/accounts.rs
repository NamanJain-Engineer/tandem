//! Which engine account a new session uses (F-AC-03).

/// The account for a new session: an explicit pick in the launcher or
/// composer, then the teammate's preferred account, then the workspace's
/// override, then the engine's default. Live sessions never change account.
pub fn resolve<T: Clone>(
    explicit: Option<&T>,
    teammate: Option<&T>,
    workspace: Option<&T>,
    default: Option<&T>,
) -> Option<T> {
    explicit.or(teammate).or(workspace).or(default).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_ac_03_1_override_b_default_a_gives_b_and_a_teammate_preferring_c_gives_c() {
        let (a, b, c) = ("A", "B", "C");
        assert_eq!(resolve(None, None, Some(&b), Some(&a)), Some("B"));
        assert_eq!(resolve(None, Some(&c), Some(&b), Some(&a)), Some("C"));
        assert_eq!(resolve(Some(&a), Some(&c), Some(&b), None), Some("A"));
        assert_eq!(resolve(None, None, None, Some(&a)), Some("A"));
        assert_eq!(resolve::<&str>(None, None, None, None), None);
    }
}
