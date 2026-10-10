//! What a teammate is, apart from where it's stored (F-AG-01, F-AG-05): the
//! name rules, the settings and their defaults, the Lead's brief, and the
//! avatar's hue.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use unicode_normalization::UnicodeNormalization;

use crate::access::Access;

/// Names are 1–40 characters (F-AG-01).
pub const NAME_MAX: usize = 40;
/// Briefs are at most 8,000 characters.
pub const BRIEF_MAX: usize = 8_000;
/// `ABOUT_YOU.md`'s own cap (F-AG-05).
pub const ABOUT_CAP: u32 = 2_048;
/// The memory caps Settings offers; 4k is the default.
pub const MEMORY_CAPS: [u32; 4] = [2_048, 4_096, 8_192, 16_384];
pub const DEFAULT_MEMORY_CAP: u32 = 4_096;
/// The avatar palette's size (F-AG-01: 12 hues).
pub const HUES: u8 = 12;

pub const LEAD_NAME: &str = "Lead";
pub const LEAD_BRIEF: &str = "You coordinate the other teammates. Break work down, delegate with clear briefs via teammate mail, track progress, and report back concisely.";

/// "Learn after a turn" (F-AG-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LearnMode {
    Ask,
    Auto,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TeammateSettings {
    pub memory_cap: u32,
    pub learn_mode: LearnMode,
    pub may_message: bool,
    pub may_schedule: bool,
    pub default_access: Access,
}

impl Default for TeammateSettings {
    fn default() -> Self {
        Self {
            memory_cap: DEFAULT_MEMORY_CAP,
            learn_mode: LearnMode::Ask,
            may_message: true,
            may_schedule: false,
            default_access: Access::Edits,
        }
    }
}

impl TeammateSettings {
    /// A cap Settings doesn't offer falls back to the default.
    pub fn normalized(mut self) -> Self {
        if !MEMORY_CAPS.contains(&self.memory_cap) {
            self.memory_cap = DEFAULT_MEMORY_CAP;
        }
        self
    }
}

/// The trimmed name, or why it can't be one.
pub fn validate_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    let n = name.chars().count();
    if n == 0 {
        return Err("Give your teammate a name");
    }
    if n > NAME_MAX {
        return Err("Names are at most 40 characters");
    }
    if name.chars().any(char::is_control) {
        return Err("Names can't hold control characters");
    }
    Ok(name.to_string())
}

/// What uniqueness compares: NFKC, then lower case ("Scout" = "scout").
pub fn fold_name(name: &str) -> String {
    name.trim().nfkc().collect::<String>().to_lowercase()
}

/// FNV-1a over UTF-16 code units, as `@tandem/ui` hashes (ids are ASCII).
pub fn fnv1a32(text: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for unit in text.encode_utf16() {
        h ^= u32::from(unit);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// The palette index an avatar uses: the user's pick, else one from the id.
pub fn hue_index(id: &str, picked: Option<u8>) -> u8 {
    picked
        .filter(|h| *h < HUES)
        .unwrap_or_else(|| (fnv1a32(id) % u32::from(HUES)) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_ag_01_1_names_fold_case_and_width_and_keep_to_40_characters() {
        assert_eq!(fold_name("Scout"), fold_name("scout"));
        assert_eq!(fold_name(" SCOUT "), "scout");
        // Fullwidth letters are the same name after NFKC.
        assert_eq!(fold_name("Ｓｃｏｕｔ"), "scout");
        assert_eq!(validate_name("  Scout "), Ok("Scout".into()));
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(40)).is_ok());
        assert!(validate_name(&"x".repeat(41)).is_err());
        assert!(
            validate_name(&"é".repeat(40)).is_ok(),
            "characters, not bytes"
        );
    }

    #[test]
    fn f_ag_01_2_the_hue_comes_from_the_id_unless_one_was_picked() {
        // The same as `@tandem/ui`'s fnv1a32("tm_abc").
        assert_eq!(fnv1a32(""), 0x811c_9dc5);
        assert_eq!(fnv1a32("a"), 0xe40c_292c);
        let id = "tm_01HZX4Q8VJ8Q0W5N2Y3B6C7D8E";
        assert_eq!(hue_index(id, None), hue_index(id, None));
        assert!(hue_index(id, None) < HUES);
        assert_eq!(hue_index(id, Some(3)), 3);
        assert_eq!(hue_index(id, Some(12)), hue_index(id, None));
    }

    #[test]
    fn f_ag_05_settings_default_to_ask_may_message_and_edits() {
        let s = TeammateSettings::default();
        assert_eq!(s.memory_cap, 4_096);
        assert_eq!(s.learn_mode, LearnMode::Ask);
        assert!(s.may_message && !s.may_schedule);
        assert_eq!(s.default_access, Access::Edits);
        let odd = TeammateSettings { memory_cap: 5, ..s };
        assert_eq!(odd.normalized().memory_cap, 4_096);
    }
}
