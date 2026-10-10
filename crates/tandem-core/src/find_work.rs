//! Find work by meaning, on this machine (F-CN-05, ARCHITECTURE §12.6).
//!
//! Ranks panes, threads and teammates for a description of the work, like
//! "the login tests". The hosted ranking understands meaning better; this
//! one runs when that's off or can't be reached. Words meet at a light
//! stem ("testing" and "tests" are both "test"), a small table joins terms
//! that mean the same thing in software work ("login" and "auth"), and a
//! typo is forgiven (D-814).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// Something to find: its id, the name people call it by, and what else
/// it's known by (its title, recent output, last message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub label: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub id: String,
    pub score: f32,
}

/// What a search comes to (F-CN-05 "Result handling").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// One clear match: go to it.
    One(String),
    /// Close matches, best first: ask which.
    Several(Vec<String>),
    None,
}

/// A best score at or above this can be gone to without asking: the name
/// matches (a word exactly, a word of the same meaning, a prefix or a
/// typo), not just the output.
pub const STRONG: f32 = 1.5;
/// Another match is close when the best is under this many times its score.
const CLEAR: f32 = 1.5;
/// At most this many close matches are offered.
const OFFER: usize = 3;

/// What a query word earns: (in the name, in the rest of the text).
const EXACT: (f32, f32) = (3.0, 1.0);
const SAME_MEANING: (f32, f32) = (2.0, 0.6);
const PREFIX: (f32, f32) = (2.0, 0.6);
const TYPO: (f32, f32) = (1.5, 0.5);

/// Words that say how to look, not what for ("where's the agent doing…").
const FILLER: &[&str] = &[
    "a", "about", "agent", "agents", "an", "and", "any", "are", "at", "be", "by", "can", "could",
    "doing", "find", "for", "from", "get", "go", "here", "how", "i", "in", "is", "it", "its", "me",
    "my", "of", "on", "one", "open", "or", "our", "pane", "panes", "please", "running", "show",
    "some", "that", "the", "there", "these", "this", "those", "to", "was", "we", "were", "what",
    "whats", "when", "where", "wheres", "which", "who", "with", "working", "you", "your",
];

/// Terms that mean the same thing in software work. Each word is stemmed
/// before it's looked up, like the words it's compared with.
const SAME: &[&[&str]] = &[
    &[
        "auth",
        "authentication",
        "authenticate",
        "authorization",
        "authorize",
        "login",
        "logon",
        "signin",
        "signon",
        "signup",
        "oauth",
        "sso",
        "password",
        "credential",
        "session",
    ],
    &[
        "test",
        "spec",
        "e2e",
        "unittest",
        "jest",
        "vitest",
        "pytest",
        "playwright",
        "assert",
        "coverage",
    ],
    &[
        "ui",
        "ux",
        "frontend",
        "interface",
        "css",
        "style",
        "styling",
        "layout",
        "component",
        "design",
        "react",
        "screen",
    ],
    &[
        "doc",
        "documentation",
        "readme",
        "guide",
        "manual",
        "wiki",
        "changelog",
    ],
    &[
        "billing",
        "payment",
        "pay",
        "invoice",
        "checkout",
        "stripe",
        "subscription",
        "pricing",
        "price",
        "charge",
    ],
    &[
        "bug",
        "fix",
        "error",
        "crash",
        "broken",
        "failing",
        "failure",
        "exception",
        "panic",
        "regression",
    ],
    &[
        "deploy",
        "deployment",
        "release",
        "ship",
        "publish",
        "rollout",
        "ci",
        "pipeline",
    ],
    &[
        "db",
        "database",
        "sql",
        "postgres",
        "sqlite",
        "mysql",
        "migration",
        "schema",
        "query",
    ],
    &[
        "api", "endpoint", "route", "server", "backend", "rest", "graphql", "handler",
    ],
    &[
        "perf",
        "performance",
        "slow",
        "speed",
        "latency",
        "optimize",
        "optimization",
        "benchmark",
        "profiling",
    ],
    &["refactor", "refactoring", "cleanup", "restructure", "tidy"],
    &["dependency", "deps", "upgrade", "bump", "package"],
    &["security", "vulnerability", "cve", "secret", "xss", "csrf"],
    &["config", "configuration", "setting", "env", "environment"],
    &[
        "log",
        "logging",
        "trace",
        "tracing",
        "telemetry",
        "monitoring",
    ],
    &["email", "mail", "smtp", "notification"],
];

fn meanings() -> &'static HashMap<String, usize> {
    static MAP: OnceLock<HashMap<String, usize>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::new();
        for (group, words) in SAME.iter().enumerate() {
            for w in *words {
                map.insert(stem(w), group);
            }
        }
        map
    })
}

/// The group of terms `word` means the same as, if any. A typo of one of
/// those terms counts too ("documntation").
fn meaning_of(word: &str) -> Option<usize> {
    let map = meanings();
    if let Some(group) = map.get(word) {
        return Some(*group);
    }
    let mut near: Vec<(&String, usize)> = map
        .iter()
        .filter(|(term, _)| typo(word, term))
        .map(|(term, group)| (term, *group))
        .collect();
    near.sort();
    near.first().map(|(_, group)| *group)
}

/// Lower-case words, split at anything that isn't a letter or digit and
/// at camelCase humps ("authTests" is "auth" and "tests").
fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut after_lower = false;
    for ch in s.chars() {
        if !ch.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            after_lower = false;
            continue;
        }
        if ch.is_uppercase() && after_lower {
            out.push(std::mem::take(&mut cur));
        }
        after_lower = ch.is_lowercase() || ch.is_ascii_digit();
        cur.extend(ch.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A light English stem: plurals, and -ing and -ed endings when what's left
/// still has a vowel, so "tests", "testing" and "tested" meet at "test"
/// while "string" stays whole.
pub fn stem(word: &str) -> String {
    let mut s = word.to_string();
    if s.len() > 4 && s.ends_with("ies") {
        s.truncate(s.len() - 3);
        s.push('y');
    } else if s.ends_with("sses") {
        s.truncate(s.len() - 2);
    } else if s.len() > 3
        && s.ends_with('s')
        && !s.ends_with("ss")
        && !s.ends_with("us")
        && !s.ends_with("is")
    {
        s.pop();
    }
    for suffix in ["ing", "ed"] {
        if s.len() < suffix.len() + 3 || !s.ends_with(suffix) {
            continue;
        }
        let root = &s[..s.len() - suffix.len()];
        if !root.chars().any(|c| "aeiouy".contains(c)) {
            break;
        }
        let mut root = root.to_string();
        // "running" → "run", but "billing" keeps "bill".
        let b = root.as_bytes();
        let n = b.len();
        if n > 3
            && b[n - 1] == b[n - 2]
            && b[n - 1].is_ascii_lowercase()
            && !b"aeioulsz".contains(&b[n - 1])
        {
            root.pop();
        }
        s = root;
        break;
    }
    s
}

/// The stems of a text's words, one-letter words left out.
fn stems(s: &str) -> HashSet<String> {
    words(s)
        .into_iter()
        .filter(|w| w.chars().count() > 1)
        .map(|w| stem(&w))
        .collect()
}

/// Edit distance with adjacent swaps (optimal string alignment), or `None`
/// once it's over `max`.
fn distance(a: &str, b: &str, max: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in rows[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut d = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d = d.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = d;
        }
    }
    let d = rows[a.len()][b.len()];
    (d <= max).then_some(d)
}

/// One typo in a word of 4–7 letters, two from 8.
fn typo(q: &str, t: &str) -> bool {
    let shorter = q.chars().count().min(t.chars().count());
    let max = match shorter {
        0..=3 => return false,
        4..=7 => 1,
        _ => 2,
    };
    distance(q, t, max).is_some()
}

/// The most a query word earns among a text's stems.
fn earn(q: &str, meaning: Option<usize>, terms: &HashSet<String>, in_name: bool) -> f32 {
    let pick = |w: (f32, f32)| if in_name { w.0 } else { w.1 };
    if terms.contains(q) {
        return pick(EXACT);
    }
    let mut best = 0.0f32;
    for t in terms {
        let earned = if meaning.is_some() && meanings().get(t).copied() == meaning {
            pick(SAME_MEANING)
        } else if q.chars().count() >= 3 && t.starts_with(q) {
            pick(PREFIX)
        } else if typo(q, t) {
            pick(TYPO)
        } else {
            0.0
        };
        best = best.max(earned);
    }
    best
}

/// The candidates that match `query`, best first. Each query word earns the
/// most it can in the name or the text; the score is their average, so a
/// name that matches every word scores 3.
pub fn rank(query: &str, candidates: &[Candidate]) -> Vec<Ranked> {
    let mut asked: Vec<(String, Option<usize>)> = Vec::new();
    for word in words(query) {
        if FILLER.contains(&word.as_str()) || word.chars().count() < 2 {
            continue;
        }
        let word = stem(&word);
        if !asked.iter().any(|(seen, _)| *seen == word) {
            let meaning = meaning_of(&word);
            asked.push((word, meaning));
        }
    }
    if asked.is_empty() {
        return Vec::new();
    }
    let mut ranked: Vec<(Ranked, String)> = candidates
        .iter()
        .filter_map(|c| {
            let name = stems(&c.label);
            let text = stems(&c.text);
            let total: f32 = asked
                .iter()
                .map(|(q, meaning)| {
                    earn(q, *meaning, &name, true).max(earn(q, *meaning, &text, false))
                })
                .sum();
            let score = total / asked.len() as f32;
            (score > 0.0).then(|| {
                (
                    Ranked {
                        id: c.id.clone(),
                        score,
                    },
                    c.label.to_lowercase(),
                )
            })
        })
        .collect();
    ranked.sort_by(|(a, a_label), (b, b_label)| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a_label.cmp(b_label))
            .then_with(|| a.id.cmp(&b.id))
    });
    ranked.into_iter().map(|(r, _)| r).collect()
}

/// One clear, strong match is gone to; otherwise the close ones are offered.
pub fn decide(ranked: &[Ranked]) -> Decision {
    let Some(best) = ranked.first() else {
        return Decision::None;
    };
    let close: Vec<String> = ranked
        .iter()
        .take_while(|r| r.score * CLEAR > best.score)
        .take(OFFER)
        .map(|r| r.id.clone())
        .collect();
    if best.score >= STRONG && close.len() == 1 {
        Decision::One(best.id.clone())
    } else {
        Decision::Several(close)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: &str, label: &str, text: &str) -> Candidate {
        Candidate {
            id: id.into(),
            label: label.into(),
            text: text.into(),
        }
    }

    fn panes() -> Vec<Candidate> {
        vec![
            pane("p_auth", "auth tests", "npm test -- auth\n12 passing"),
            pane("p_billing", "billing UI", "vite dev server ready on :5173"),
            pane("p_docs", "docs", "editing README.md"),
        ]
    }

    fn ids(ranked: &[Ranked]) -> Vec<&str> {
        ranked.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn f_cn_05_1_login_testing_resolves_to_auth_tests_locally() {
        let ranked = rank("login testing", &panes());
        assert_eq!(ids(&ranked), ["p_auth"]);
        assert_eq!(decide(&ranked), Decision::One("p_auth".into()));
        // As Conductor hears it.
        let spoken = rank("Where's the agent doing the login tests?", &panes());
        assert_eq!(decide(&spoken), Decision::One("p_auth".into()));
    }

    #[test]
    fn f_cn_05_words_meet_at_their_stem() {
        for (word, expected) in [
            ("tests", "test"),
            ("testing", "test"),
            ("tested", "test"),
            ("running", "run"),
            ("billing", "bill"),
            ("logging", "log"),
            ("dependencies", "dependency"),
            ("docs", "doc"),
            ("string", "string"),
            ("status", "status"),
            ("class", "class"),
            ("added", "add"),
        ] {
            assert_eq!(stem(word), expected, "{word}");
        }
    }

    #[test]
    fn f_cn_05_a_typo_or_a_prefix_still_finds_it() {
        assert_eq!(ids(&rank("biling", &panes()))[0], "p_billing");
        assert_eq!(ids(&rank("documntation", &panes()))[0], "p_docs");
        assert_eq!(ids(&rank("bil", &panes()))[0], "p_billing");
        // Short words aren't stretched into others.
        assert!(rank("cat", &panes()).is_empty());
    }

    #[test]
    fn f_cn_05_the_name_counts_more_than_the_output() {
        let candidates = vec![
            pane("p_out", "server", "running the auth tests now"),
            pane("p_name", "auth tests", ""),
        ];
        let ranked = rank("auth", &candidates);
        assert_eq!(ids(&ranked), ["p_name", "p_out"]);
        assert!(ranked[0].score > ranked[1].score * CLEAR);
    }

    #[test]
    fn f_cn_05_close_matches_are_offered_to_choose_from() {
        let candidates = vec![
            pane("p_tests", "auth tests", ""),
            pane("p_page", "auth page", ""),
            pane("p_docs", "docs", ""),
        ];
        let ranked = rank("auth", &candidates);
        assert_eq!(
            decide(&ranked),
            Decision::Several(vec!["p_page".into(), "p_tests".into()])
        );
        // The words of a request break the tie.
        assert_eq!(
            decide(&rank("login tests", &candidates)),
            Decision::One("p_tests".into())
        );
    }

    #[test]
    fn f_cn_05_camel_case_and_punctuation_split_into_words() {
        let candidates = vec![pane("p", "authTests", ""), pane("q", "billing-ui", "")];
        assert_eq!(ids(&rank("auth tests", &candidates)), ["p"]);
        assert_eq!(ids(&rank("billing ui", &candidates)), ["q"]);
    }

    #[test]
    fn f_cn_05_only_filler_or_no_match_finds_nothing() {
        assert!(rank("where is the agent", &panes()).is_empty());
        assert!(rank("", &panes()).is_empty());
        let ranked = rank("kubernetes", &panes());
        assert!(ranked.is_empty());
        assert_eq!(decide(&ranked), Decision::None);
    }

    #[test]
    fn f_cn_05_a_weak_lone_match_is_offered_not_gone_to() {
        let candidates = vec![pane("p", "server", "one auth line in the output")];
        let ranked = rank("auth", &candidates);
        assert_eq!(ids(&ranked), ["p"]);
        assert!(ranked[0].score < STRONG);
        assert_eq!(decide(&ranked), Decision::Several(vec!["p".into()]));
    }
}
