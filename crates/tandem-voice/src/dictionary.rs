//! The Dictate dictionary (F-DI-09, ARCHITECTURE §12.5): preferred spellings
//! and correction rules applied after transcription with Aho-Corasick, scoped
//! by language and app profile, skipping links and code when asked to.

use std::ops::Range;

use aho_corasick::{AhoCorasick, MatchKind};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RuleKind {
    /// A preferred spelling and the ways it gets misheard.
    Word,
    /// Phrases → a replacement.
    Correction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Spacing {
    #[default]
    Exact,
    /// Matches across spaces and hyphens ("type script" = "type-script").
    Flexible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Rule {
    pub id: String,
    pub kind: RuleKind,
    /// Word: the spelling to write. Correction: what to write.
    pub replacement: String,
    /// Word: misheard variants. Correction: "When I say" phrases.
    pub phrases: Vec<String>,
    pub match_case: bool,
    /// Skip URLs and backtick code.
    pub protect: bool,
    pub spacing: Spacing,
    pub enabled: bool,
    /// `*` or a language code (`en`, `de`, …).
    pub lang: String,
    /// Empty = everywhere; else app identifiers (bundle ids, exe names, WM classes).
    pub app_profiles: Vec<String>,
    #[ts(type = "number")]
    pub updated_at: i64,
    #[ts(type = "number | null")]
    pub deleted_at: Option<i64>,
}

impl Rule {
    pub fn correction(id: &str, phrase: &str, replacement: &str) -> Self {
        Self {
            id: id.into(),
            kind: RuleKind::Correction,
            replacement: replacement.into(),
            phrases: vec![phrase.into()],
            match_case: false,
            protect: true,
            spacing: Spacing::Exact,
            enabled: true,
            lang: "*".into(),
            app_profiles: Vec::new(),
            updated_at: 0,
            deleted_at: None,
        }
    }

    pub fn word(id: &str, preferred: &str, variants: &[&str]) -> Self {
        Self {
            kind: RuleKind::Word,
            phrases: variants.iter().map(|v| v.to_string()).collect(),
            ..Self::correction(id, "", preferred)
        }
    }

    /// Whether this rule applies for a take in `lang` going to `app`.
    pub fn in_scope(&self, lang: Option<&str>, app: Option<&str>) -> bool {
        if !self.enabled || self.deleted_at.is_some() {
            return false;
        }
        let lang_ok = self.lang == "*"
            || lang.is_some_and(|l| {
                let l = l.to_lowercase();
                let want = self.lang.to_lowercase();
                l == want || l.starts_with(&format!("{want}-"))
            });
        let app_ok = self.app_profiles.is_empty()
            || app.is_some_and(|a| {
                self.app_profiles
                    .iter()
                    .any(|p| p.trim().eq_ignore_ascii_case(a.trim()))
            });
        lang_ok && app_ok
    }

    /// What this rule looks for.
    fn patterns(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .phrases
            .iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        // A word also fixes its own capitalisation ("tauri" → "Tauri").
        if self.kind == RuleKind::Word && !self.replacement.trim().is_empty() {
            out.push(self.replacement.trim().to_string());
        }
        out
    }
}

struct Entry {
    replacement: String,
    protect: bool,
}

struct Matcher {
    ac: AhoCorasick,
    entries: Vec<usize>,
    flexible: bool,
    case_sensitive: bool,
}

/// Rules compiled for one take's language and target app.
pub struct Dictionary {
    entries: Vec<Entry>,
    matchers: Vec<Matcher>,
}

impl Dictionary {
    pub fn compile(rules: &[Rule], lang: Option<&str>, app: Option<&str>) -> Self {
        let mut entries = Vec::new();
        // Four buckets: (flexible, case sensitive).
        let mut buckets: [(Vec<String>, Vec<usize>); 4] = Default::default();
        for rule in rules.iter().filter(|r| r.in_scope(lang, app)) {
            let idx = entries.len();
            entries.push(Entry {
                replacement: rule.replacement.clone(),
                protect: rule.protect,
            });
            let flexible = rule.spacing == Spacing::Flexible;
            let b = &mut buckets[(flexible as usize) * 2 + rule.match_case as usize];
            for p in rule.patterns() {
                let p = if flexible { normalize(&p).0 } else { p };
                let p = if rule.match_case { p } else { fold(&p) };
                if p.is_empty() || p == rule.replacement && rule.match_case {
                    continue;
                }
                b.0.push(p);
                b.1.push(idx);
            }
        }
        let matchers = buckets
            .into_iter()
            .enumerate()
            .filter(|(_, (p, _))| !p.is_empty())
            .filter_map(|(i, (patterns, owners))| {
                let ac = AhoCorasick::builder()
                    .match_kind(MatchKind::LeftmostLongest)
                    .build(&patterns)
                    .ok()?;
                Some(Matcher {
                    ac,
                    entries: owners,
                    flexible: i >= 2,
                    case_sensitive: i % 2 == 1,
                })
            })
            .collect();
        Self { entries, matchers }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Applies every rule in one pass. Matches must be whole words; where two
    /// overlap, the leftmost (then longest) wins.
    pub fn apply(&self, text: &str) -> String {
        if self.entries.is_empty() || text.is_empty() {
            return text.to_string();
        }
        let protected = protected_spans(text);
        let folded = fold(text);
        let (norm, map) = normalize(text);
        let norm_folded = fold(&norm);
        let mut found: Vec<(Range<usize>, usize)> = Vec::new();
        for m in &self.matchers {
            let hay: &str = match (m.flexible, m.case_sensitive) {
                (false, true) => text,
                (false, false) => &folded,
                (true, true) => &norm,
                (true, false) => &norm_folded,
            };
            for hit in m.ac.find_overlapping_iter_safe(hay) {
                let range = if m.flexible {
                    let start = map[hit.0.start];
                    let end = map[hit.0.end - 1] + char_len(text, map[hit.0.end - 1]);
                    start..end
                } else {
                    hit.0
                };
                let entry = m.entries[hit.1];
                if !whole_word(text, &range) {
                    continue;
                }
                if self.entries[entry].protect && protected.iter().any(|p| overlaps(p, &range)) {
                    continue;
                }
                found.push((range, entry));
            }
        }
        found.sort_by(|a, b| {
            a.0.start
                .cmp(&b.0.start)
                .then((b.0.end - b.0.start).cmp(&(a.0.end - a.0.start)))
        });
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for (range, entry) in found {
            if range.start < at {
                continue;
            }
            out.push_str(&text[at..range.start]);
            out.push_str(&self.entries[entry].replacement);
            at = range.end;
        }
        out.push_str(&text[at..]);
        out
    }
}

/// Every match, not only non-overlapping ones, so a pattern in one bucket
/// can't hide a longer one from the same bucket that starts later.
trait FindAll {
    fn find_overlapping_iter_safe(&self, hay: &str) -> Vec<(Range<usize>, usize)>;
}

impl FindAll for AhoCorasick {
    fn find_overlapping_iter_safe(&self, hay: &str) -> Vec<(Range<usize>, usize)> {
        // LeftmostLongest can't iterate overlapping matches; re-search from
        // each match start + 1 char instead.
        let mut out = Vec::new();
        let mut from = 0;
        while from <= hay.len() {
            let input = aho_corasick::Input::new(hay).range(from..);
            let Some(m) = self.find(input) else { break };
            out.push((m.start()..m.end(), m.pattern().as_usize()));
            from = m.start() + char_len(hay, m.start()).max(1);
        }
        out
    }
}

fn char_len(s: &str, at: usize) -> usize {
    s[at..].chars().next().map(char::len_utf8).unwrap_or(1)
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn whole_word(text: &str, r: &Range<usize>) -> bool {
    let before = text[..r.start].chars().next_back();
    let after = text[r.end..].chars().next();
    let first = text[r.start..].chars().next();
    let last = text[..r.end].chars().next_back();
    let edge_ok = |outside: Option<char>, inside: Option<char>| match (outside, inside) {
        (Some(o), Some(i)) => !(is_word_char(o) && is_word_char(i)),
        _ => true,
    };
    edge_ok(before, first) && edge_ok(after, last)
}

/// Lower-cases without moving byte offsets: characters whose lower case has
/// a different UTF-8 length stay as they are.
fn fold(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mut l = c.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) if x.len_utf8() == c.len_utf8() => x,
                _ => c,
            }
        })
        .collect()
}

/// Runs of spaces and hyphens become one space. Returns the text and, for
/// each output byte, the byte it came from in the input.
fn normalize(s: &str) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(s.len());
    let mut map = Vec::with_capacity(s.len());
    let mut gap = false;
    for (i, c) in s.char_indices() {
        if c.is_whitespace() || c == '-' || c == '‐' || c == '–' {
            if !gap {
                out.push(' ');
                map.push(i);
            }
            gap = true;
            continue;
        }
        gap = false;
        let start = out.len();
        out.push(c);
        for _ in start..out.len() {
            map.push(i);
        }
    }
    (out, map)
}

/// URLs, e-mail addresses, file paths with a scheme and backtick code.
fn protected_spans(text: &str) -> Vec<Range<usize>> {
    static PATTERN: &str = r"(?x)
        ```[\s\S]*?```                         # fenced code
      | `[^`\n]*`                              # inline code
      | \b[a-zA-Z][a-zA-Z0-9+.-]*://\S+        # scheme://…
      | \bwww\.\S+
      | \b[\w.+-]+@[\w-]+\.[\w.-]+             # e-mail
    ";
    let Ok(re) = regex::Regex::new(PATTERN) else {
        return Vec::new();
    };
    re.find_iter(text).map(|m| m.range()).collect()
}

// ── Protected spellings around a rewrite (Enhance, Polish: M13) ──────────

pub const OPEN: char = '⟦';
pub const CLOSE: char = '⟧';

/// The spellings a rewrite must keep: every preferred word and replacement
/// of the rules in scope.
pub fn protected_terms(rules: &[Rule], lang: Option<&str>, app: Option<&str>) -> Vec<String> {
    let mut terms: Vec<String> = rules
        .iter()
        .filter(|r| r.in_scope(lang, app))
        .map(|r| r.replacement.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    terms.sort_by_key(|t| std::cmp::Reverse(t.len()));
    terms.dedup();
    terms
}

/// Wraps each protected term that occurs in `text` as `⟦term⟧`.
pub fn wrap_protected(text: &str, terms: &[String]) -> String {
    if terms.is_empty() {
        return text.to_string();
    }
    let Ok(ac) = AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostLongest)
        .build(terms)
    else {
        return text.to_string();
    };
    let mut out = String::with_capacity(text.len() + 16);
    let mut at = 0;
    for m in ac.find_iter(text) {
        if !whole_word(text, &(m.start()..m.end())) {
            continue;
        }
        out.push_str(&text[at..m.start()]);
        out.push(OPEN);
        out.push_str(&text[m.start()..m.end()]);
        out.push(CLOSE);
        at = m.end();
    }
    out.push_str(&text[at..]);
    out
}

/// Takes a rewrite of `wrapped` back: if every protected term survived
/// verbatim, the rewrite without markers; otherwise the corrected
/// un-enhanced text (F-DI-09 AC2).
pub fn accept_rewrite(corrected: &str, wrapped: &str, rewritten: &str) -> String {
    let kept = |s: &str| -> Vec<String> {
        let mut v = Vec::new();
        let mut rest = s;
        while let Some(i) = rest.find(OPEN) {
            let after = &rest[i + OPEN.len_utf8()..];
            let Some(j) = after.find(CLOSE) else { break };
            v.push(after[..j].to_string());
            rest = &after[j + CLOSE.len_utf8()..];
        }
        v
    };
    let mut want = kept(wrapped);
    let mut got = kept(rewritten);
    want.sort();
    want.dedup();
    got.sort();
    got.dedup();
    let stray = rewritten.matches(OPEN).count() != rewritten.matches(CLOSE).count();
    if want.iter().any(|t| !got.contains(t)) || got.iter().any(|t| !want.contains(t)) || stray {
        return corrected.to_string();
    }
    rewritten.replace([OPEN, CLOSE], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(rules: &[Rule], text: &str) -> String {
        Dictionary::compile(rules, Some("en"), None).apply(text)
    }

    #[test]
    fn f_di_09_1_tandem_rule_skips_links() {
        let rules = [Rule::correction("r1", "tandem", "Tandem")];
        assert_eq!(apply(&rules, "use tandem"), "use Tandem");
        assert_eq!(
            apply(&rules, "use tandem, https://tandem.dev"),
            "use Tandem, https://tandem.dev"
        );
        assert_eq!(
            apply(&rules, "see `tandem run` and TANDEM"),
            "see `tandem run` and Tandem"
        );
        // Protect off: links change too.
        let mut open = rules[0].clone();
        open.protect = false;
        assert_eq!(apply(&[open], "https://tandem.dev"), "https://Tandem.dev");
    }

    #[test]
    fn f_di_09_whole_words_only_and_match_case() {
        let rules = [Rule::correction("r1", "cat", "Cat")];
        assert_eq!(apply(&rules, "concat cat category"), "concat Cat category");
        let mut cased = Rule::correction("r2", "Go", "Golang");
        cased.match_case = true;
        assert_eq!(apply(&[cased], "go Go GO"), "go Golang GO");
    }

    #[test]
    fn f_di_09_words_fix_variants_and_capitalisation() {
        let rules = [Rule::word("w1", "Tauri", &["tory", "tawri"])];
        assert_eq!(
            apply(&rules, "tory and tawri and tauri"),
            "Tauri and Tauri and Tauri"
        );
    }

    #[test]
    fn f_di_09_flexible_spacing() {
        let mut r = Rule::correction("r1", "type script", "TypeScript");
        r.spacing = Spacing::Flexible;
        assert_eq!(
            apply(&[r], "type-script, type   script and typescript"),
            "TypeScript, TypeScript and typescript"
        );
    }

    #[test]
    fn f_di_09_longest_match_wins_and_one_pass() {
        let rules = [
            Rule::correction("a", "new york", "New York"),
            Rule::correction("b", "new york city", "NYC"),
            Rule::correction("c", "nyc", "should not chain"),
        ];
        assert_eq!(apply(&rules, "in new york city now"), "in NYC now");
        assert_eq!(apply(&rules, "new york"), "New York");
    }

    #[test]
    fn f_di_09_3_app_profile_scoping() {
        let mut r = Rule::correction("r1", "pr", "pull request");
        r.app_profiles = vec!["com.tinyspeck.slackmacgap".into()];
        let in_slack = Dictionary::compile(&[r.clone()], None, Some("com.tinyspeck.slackmacgap"));
        let elsewhere = Dictionary::compile(&[r.clone()], None, Some("org.gnome.gedit"));
        let unknown = Dictionary::compile(&[r], None, None);
        assert_eq!(in_slack.apply("open a pr"), "open a pull request");
        assert_eq!(elsewhere.apply("open a pr"), "open a pr");
        assert_eq!(unknown.apply("open a pr"), "open a pr");
    }

    #[test]
    fn f_di_09_language_scope_disabled_and_deleted() {
        let mut de = Rule::correction("r1", "ja", "Ja");
        de.lang = "de".into();
        assert!(de.in_scope(Some("de-AT"), None));
        assert!(!de.in_scope(Some("en"), None));
        assert!(!de.in_scope(None, None));
        let mut off = Rule::correction("r2", "x", "y");
        off.enabled = false;
        assert!(!off.in_scope(None, None));
        let mut gone = Rule::correction("r3", "x", "y");
        gone.deleted_at = Some(1);
        assert!(!gone.in_scope(None, None));
    }

    #[test]
    fn f_di_09_unicode_offsets_stay_aligned() {
        let rules = [Rule::correction("r1", "café", "Café")];
        assert_eq!(apply(&rules, "ÄÖ café İstanbul"), "ÄÖ Café İstanbul");
    }

    #[test]
    fn f_di_09_2_rewrite_that_changes_a_protected_term_falls_back() {
        let rules = [Rule::word("w1", "Tauri", &["tory"])];
        let terms = protected_terms(&rules, None, None);
        let corrected = "ship the Tauri build";
        let wrapped = wrap_protected(corrected, &terms);
        assert_eq!(wrapped, "ship the ⟦Tauri⟧ build");
        assert_eq!(
            accept_rewrite(corrected, &wrapped, "Ship the ⟦Tauri⟧ build."),
            "Ship the Tauri build."
        );
        assert_eq!(
            accept_rewrite(corrected, &wrapped, "Ship the ⟦Tory⟧ build."),
            corrected
        );
        assert_eq!(
            accept_rewrite(corrected, &wrapped, "Ship the build."),
            corrected
        );
    }
}
