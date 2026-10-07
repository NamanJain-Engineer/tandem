//! Agent pane first names (F-CD-12): an original list of 200, shared with
//! the webview (`src/data/names.json`).

use std::sync::OnceLock;

const NAMES_JSON: &str = include_str!("../../../src/data/names.json");

pub fn names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| serde_json::from_str(NAMES_JSON).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn f_cd_12_the_list_has_200_unique_first_names() {
        let n = names();
        assert_eq!(n.len(), 200);
        assert_eq!(n.iter().collect::<HashSet<_>>().len(), 200);
        assert!(n
            .iter()
            .all(|x| !x.is_empty() && x.chars().all(char::is_alphabetic)));
    }
}
