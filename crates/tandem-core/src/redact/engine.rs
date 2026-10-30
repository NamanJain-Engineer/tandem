//! The redactor's searches, on lazy DFAs whose caches it holds (the review
//! of e788fc6f, D-877).
//!
//! A lazy DFA gives up for good once its cache has been cleared three times
//! with little searched per state, and the `regex` crate keeps each cache
//! for later searches: it never resets one. So one crafted text left every
//! later search on its thread on the slow engine. 32 KB of `AIza` runs did
//! it to the prefilter, a megabyte then taking 33 times as long, and 64 KB
//! of `key` runs to the high-entropy rule, 25 times.
//!
//! Here a call takes its cache from a pool and, if it cleared it, resets it
//! before it goes back, however the call ends ([`Taken`]). So every call
//! starts with no clears but its own (the searches of one `find_iter` share
//! a cache), and keeps the states earlier ones built. A search whose lazy
//! DFA gives up finishes on an engine with none, and so does one that meets
//! a letter outside ASCII where a pattern has Unicode's `\b`.
//!
//! Each fresh cache costs 50 to 120 µs to warm (release build), against
//! under 2 µs for a warm one on short text, so a cache is kept, not made
//! per call. The redactor's other patterns can't be poisoned: the `regex`
//! crate runs its token pattern on a full DFA, which has no cache, and the
//! one-character `WORD` never clears one.
//!
//! What it costs: a crafted text's own call warms its cache until the lazy
//! DFA gives up, about twice the old steady state on 32 to 64 KB of runs
//! and less on longer text. The URL-password rule, which has no literal
//! prefix, is scanned forward from the text's start, where the `regex`
//! crate searched backwards from each `@`: about 2 ms more a megabyte, on
//! text that has such a URL (the review of 9ad7fe54).

use regex_automata::{
    hybrid, meta,
    nfa::thompson,
    util::{
        captures::Captures,
        iter::Searcher,
        pool::{Pool, PoolGuard},
        prefilter::Prefilter,
        syntax,
    },
    Anchored, Input, MatchErrorKind, MatchKind, PatternSet,
};
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::Arc;

/// The lazy DFA's cache and the NFA's size limit, as the `regex` crate sets
/// them.
const CACHE_CAPACITY: usize = 2 * (1 << 20);
const NFA_SIZE_LIMIT: usize = 10 * (1 << 20);

type Create<T> = Box<dyn Fn() -> T + Send + Sync + UnwindSafe + RefUnwindSafe>;

/// Why a pattern didn't build.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BuildError {
    #[error(transparent)]
    Lazy(Box<hybrid::BuildError>),
    #[error(transparent)]
    Slow(Box<meta::BuildError>),
}

impl From<hybrid::BuildError> for BuildError {
    fn from(err: hybrid::BuildError) -> Self {
        BuildError::Lazy(Box::new(err))
    }
}

impl From<meta::BuildError> for BuildError {
    fn from(err: meta::BuildError) -> Self {
        BuildError::Slow(Box::new(err))
    }
}

fn syntax_config() -> syntax::Config {
    syntax::Config::new().utf8(true)
}

fn thompson_config() -> thompson::Config {
    thompson::Config::new()
        .utf8(true)
        .shrink(false)
        .nfa_size_limit(Some(NFA_SIZE_LIMIT))
}

/// As the `regex` crate configures its lazy DFAs, with the literal
/// prefilter it would build for `patterns`.
fn lazy_config<P: AsRef<str>>(kind: MatchKind, patterns: &[P]) -> hybrid::dfa::Config {
    let prefilter = syntax::parse_many_with(patterns, &syntax_config())
        .ok()
        .and_then(|hirs| Prefilter::from_hirs_prefix(kind, &hirs));
    hybrid::dfa::Config::new()
        .match_kind(kind)
        .specialize_start_states(prefilter.is_some())
        .prefilter(prefilter)
        .unicode_word_boundary(true)
        .cache_capacity(CACHE_CAPACITY)
        .minimum_cache_clear_count(Some(3))
        .minimum_bytes_per_state(Some(10))
}

/// An engine with no lazy DFA, so nothing a search does lasts past it: the
/// one-pass DFA, the bounded backtracker or the PikeVM, as the `regex`
/// crate picks them.
fn slow_config(kind: MatchKind) -> meta::Config {
    meta::Config::new()
        .match_kind(kind)
        .utf8_empty(true)
        .nfa_size_limit(Some(NFA_SIZE_LIMIT))
        .hybrid(false)
        // No pattern fits a full DFA's state limit: building one only
        // costs time and memory (the review of 9ad7fe54).
        .dfa(false)
}

/// A cache from its pool, reset when its search cleared it before it goes
/// back, however the search ends: a panic too.
struct Taken<'a, C: Send, R: Fn(&mut C)> {
    cache: PoolGuard<'a, C, Create<C>>,
    reset: R,
}

impl<C: Send, R: Fn(&mut C)> Drop for Taken<'_, C, R> {
    fn drop(&mut self) {
        (self.reset)(&mut self.cache);
    }
}

impl<C: Send, R: Fn(&mut C)> std::ops::Deref for Taken<'_, C, R> {
    type Target = C;
    fn deref(&self) -> &C {
        &self.cache
    }
}

impl<C: Send, R: Fn(&mut C)> std::ops::DerefMut for Taken<'_, C, R> {
    fn deref_mut(&mut self) -> &mut C {
        &mut self.cache
    }
}

/// Patterns searched together, for which of them match.
pub(crate) struct Set {
    lazy: Arc<hybrid::dfa::DFA>,
    slow: meta::Regex,
    caches: Pool<hybrid::dfa::Cache, Create<hybrid::dfa::Cache>>,
}

impl Set {
    pub(crate) fn new<P: AsRef<str>>(patterns: &[P]) -> Result<Set, BuildError> {
        let lazy = Arc::new(
            hybrid::dfa::Builder::new()
                .configure(lazy_config(MatchKind::All, patterns))
                .syntax(syntax_config())
                .thompson(thompson_config())
                .build_many(patterns)?,
        );
        let slow = meta::Builder::new()
            .configure(slow_config(MatchKind::All).which_captures(thompson::WhichCaptures::None))
            .syntax(syntax_config())
            .build_many(patterns)?;
        let create = Arc::clone(&lazy);
        Ok(Set {
            lazy,
            slow,
            caches: Pool::new(Box::new(move || create.create_cache())),
        })
    }

    /// The patterns that match somewhere in `s`, by index, ascending.
    pub(crate) fn matches(&self, s: &str) -> Vec<usize> {
        let input = Input::new(s);
        let mut found = PatternSet::new(self.lazy.pattern_len());
        let mut cache = Taken {
            cache: self.caches.get(),
            reset: |c: &mut hybrid::dfa::Cache| {
                if c.clear_count() > 0 {
                    c.reset(&self.lazy);
                }
            },
        };
        if self
            .lazy
            .try_which_overlapping_matches(&mut cache, &input, &mut found)
            .is_err()
        {
            found.clear();
            self.slow.which_overlapping_matches(&input, &mut found);
        }
        found.iter().map(|id| id.as_usize()).collect()
    }
}

/// One pattern, read as `regex::Regex` reads it: its leftmost-first
/// matches and their groups. A match is found on the lazy DFA, and its
/// groups read on the slow engine over its own span, as the `regex` crate
/// reads them. Where the lazy DFA can't finish a search, the slow engine
/// finds the match and its groups in the one pass.
pub(crate) struct Pattern {
    #[cfg(test)]
    source: String,
    lazy: Arc<hybrid::regex::Regex>,
    slow: meta::Regex,
    caches: Pool<hybrid::regex::Cache, Create<hybrid::regex::Cache>>,
    /// Whether the pattern has groups of its own: without, a match is all
    /// its captures say.
    grouped: bool,
}

impl Pattern {
    pub(crate) fn new(pattern: &str) -> Result<Pattern, BuildError> {
        let kind = MatchKind::LeftmostFirst;
        let lazy = Arc::new(
            hybrid::regex::Builder::new()
                .syntax(syntax_config())
                .thompson(thompson_config())
                .dfa(lazy_config(kind, &[pattern]))
                .build(pattern)?,
        );
        let slow = meta::Builder::new()
            .configure(slow_config(kind))
            .syntax(syntax_config())
            .build(pattern)?;
        let create = Arc::clone(&lazy);
        Ok(Pattern {
            #[cfg(test)]
            source: pattern.to_string(),
            lazy,
            grouped: slow.captures_len() > 1,
            slow,
            caches: Pool::new(Box::new(move || create.create_cache())),
        })
    }

    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.source
    }

    /// The first match in `s`.
    #[cfg(test)]
    pub(crate) fn find<'h>(&self, s: &'h str) -> Option<Hit<'h>> {
        self.search_all(s, Input::new(s), 1, false)
            .pop()
            .map(|c| c.hit)
    }

    /// Every match in `s`, as `regex::Regex::find_iter` finds them.
    pub(crate) fn find_iter<'h>(&self, s: &'h str) -> impl Iterator<Item = Hit<'h>> {
        self.search_all(s, Input::new(s), usize::MAX, false)
            .into_iter()
            .map(|c| c.hit)
    }

    /// The first match at or after `at`, with its groups, as
    /// `regex::Regex::captures_at` reads it.
    pub(crate) fn captures_at<'h>(&self, s: &'h str, at: usize) -> Option<Caps<'h>> {
        self.search_all(s, Input::new(s).range(at..), 1, true).pop()
    }

    /// Every match in `s` with its groups, as
    /// `regex::Regex::captures_iter` reads them.
    pub(crate) fn captures_iter<'h>(&self, s: &'h str) -> std::vec::IntoIter<Caps<'h>> {
        self.search_all(s, Input::new(s), usize::MAX, true)
            .into_iter()
    }

    /// Up to `limit` matches in `input`, from its start on, with their
    /// groups if `grouped`. The lazy DFA finds them while it can: after it
    /// gives up, the rest of the text goes to the slow engine, and after a
    /// quit, that search alone.
    fn search_all<'h>(
        &self,
        s: &'h str,
        input: Input<'_>,
        limit: usize,
        grouped: bool,
    ) -> Vec<Caps<'h>> {
        let grouped = grouped && self.grouped;
        let mut cache = Taken {
            cache: self.caches.get(),
            reset: |c: &mut hybrid::regex::Cache| {
                if c.forward().clear_count() > 0 || c.reverse().clear_count() > 0 {
                    c.reset(&self.lazy);
                }
            },
        };
        let mut lazy = true;
        let mut searcher = Searcher::new(input);
        let mut found = Vec::new();
        while found.len() < limit {
            // Groups the slow engine read with the match it found.
            let mut read = None;
            let m = searcher.advance(|input| {
                if lazy {
                    match self.lazy.try_search(&mut cache, input) {
                        Ok(m) => return Ok(m),
                        Err(err) => lazy = !matches!(err.kind(), MatchErrorKind::GaveUp { .. }),
                    }
                }
                if !grouped {
                    return Ok(self.slow.search(input));
                }
                let mut caps = self.slow.create_captures();
                self.slow.search_captures(input, &mut caps);
                let m = caps.get_match();
                read = Some(caps);
                Ok(m)
            });
            let Some(m) = m else {
                break;
            };
            let hit = Hit {
                s,
                start: m.start(),
                end: m.end(),
            };
            let caps = match read {
                Some(caps) if caps.get_match() == Some(m) => Some(caps),
                _ if grouped => Some(self.groups(hit)),
                _ => None,
            };
            found.push(Caps { hit, caps });
        }
        found
    }

    /// The groups of `hit`, read on its span, anchored at its start.
    fn groups(&self, hit: Hit<'_>) -> Captures {
        let mut caps = self.slow.create_captures();
        let input = Input::new(hit.s)
            .span(hit.start..hit.end)
            .anchored(Anchored::Yes);
        self.slow.search_captures(&input, &mut caps);
        caps
    }
}

/// A match and its groups, read as `regex::Captures` reads them.
pub(crate) struct Caps<'h> {
    hit: Hit<'h>,
    /// The groups, for a pattern that has any.
    caps: Option<Captures>,
}

impl<'h> Caps<'h> {
    /// Group `index`, the whole match at 0.
    pub(crate) fn get(&self, index: usize) -> Option<Hit<'h>> {
        match &self.caps {
            None => (index == 0).then_some(self.hit),
            Some(caps) => caps
                .get_group(index)
                .map(|span| self.hit.at(span.start, span.end)),
        }
    }

    /// The group named `name`, if it took part in the match.
    pub(crate) fn name(&self, name: &str) -> Option<Hit<'h>> {
        let span = self.caps.as_ref()?.get_group_by_name(name)?;
        Some(self.hit.at(span.start, span.end))
    }
}

/// A match or group, read as `regex::Match` reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Hit<'h> {
    s: &'h str,
    start: usize,
    end: usize,
}

impl<'h> Hit<'h> {
    pub(crate) fn start(&self) -> usize {
        self.start
    }

    pub(crate) fn end(&self) -> usize {
        self.end
    }

    pub(crate) fn as_str(&self) -> &'h str {
        &self.s[self.start..self.end]
    }

    /// Another span in the same text.
    fn at(&self, start: usize, end: usize) -> Hit<'h> {
        Hit {
            s: self.s,
            start,
            end,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redact::sample::crafted;

    fn specs() -> Vec<String> {
        super::super::rule_specs()
            .into_iter()
            .map(|(pattern, _)| pattern)
            .collect()
    }

    /// The first rule whose pattern holds `needle`.
    fn spec(needle: &str) -> String {
        specs()
            .into_iter()
            .find(|p| p.contains(needle))
            .expect("a rule holds the needle")
    }

    fn spans(found: impl Iterator<Item = (usize, usize)>) -> Vec<(usize, usize)> {
        found.collect()
    }

    #[test]
    fn f_tk_08_1_a_set_resets_a_cache_it_cleared() {
        // The prefilter on 32 KB of `AIza` runs (the review of e788fc6f).
        let patterns: Vec<String> = specs()
            .iter()
            .map(|p| super::super::unanchored(p))
            .collect();
        let set = Set::new(&patterns).expect("the prefilter builds");
        let text = crafted("AIza", 33, 32 * 1024);
        // On a cache that keeps its clears, as the `regex` crate's do, the
        // lazy DFA gives up on it.
        let mut kept = set.lazy.create_cache();
        let mut found = PatternSet::new(set.lazy.pattern_len());
        let gave_up = set
            .lazy
            .try_which_overlapping_matches(&mut kept, &Input::new(&text), &mut found)
            .is_err();
        assert!(gave_up, "the text left {} clears", kept.clear_count());
        // Through the set, the slow engine finishes the search, and the
        // cache goes back with no clears for the next.
        let expected: Vec<usize> = regex::RegexSet::new(&patterns)
            .expect("the regex crate builds it")
            .matches(&text)
            .into_iter()
            .collect();
        assert_eq!(set.matches(&text), expected);
        assert_eq!(set.caches.get().clear_count(), 0);
    }

    #[test]
    fn f_tk_08_1_a_pattern_resets_a_cache_it_cleared() {
        // The high-entropy rule on 64 KB of `key` runs: on a kept cache, its
        // later scans took 25 times as long.
        let pattern = spec("signature");
        let rule = Pattern::new(&pattern).expect("the rule builds");
        let text = crafted("key", 19, 64 * 1024);
        let mut kept = rule.lazy.create_cache();
        let mut at = 0;
        let gave_up = loop {
            match rule
                .lazy
                .try_search(&mut kept, &Input::new(&text).range(at..))
            {
                Ok(Some(m)) => at = m.end().max(at + 1),
                Ok(None) => break false,
                Err(_) => break true,
            }
        };
        assert!(
            gave_up,
            "the text left {} clears",
            kept.forward().clear_count()
        );
        let expected = spans(
            regex::Regex::new(&pattern)
                .expect("the regex crate builds it")
                .find_iter(&text)
                .map(|m| (m.start(), m.end())),
        );
        assert_eq!(
            spans(rule.find_iter(&text).map(|m| (m.start(), m.end()))),
            expected
        );
        let mut cache = rule.caches.get();
        assert_eq!(cache.forward().clear_count(), 0);
        assert_eq!(cache.reverse().clear_count(), 0);
    }

    #[test]
    fn f_tk_08_1_a_rule_that_gives_up_midway_reads_as_the_regex_crate_does() {
        // Matches on both sides of 64 KB of `key` runs, where the
        // high-entropy rule's lazy DFA gives up midway: what the slow engine
        // finds after it, its groups and `captures_at` too, is what the
        // `regex` crate finds (the review of 9ad7fe54).
        let lines = |seed: u64| -> String {
            let mut seed = seed;
            (0..20)
                .map(|i| {
                    let token = crate::redact::sample::alnum(&mut seed, 32);
                    format!("{i}: session_token = {token} and auth ok\n")
                })
                .collect()
        };
        let (before, after) = (lines(7), lines(11));
        let text = format!("{before}{}{after}", crafted("key", 19, 64 * 1024));
        let plain = spec("signature");
        let written = super::super::tolerant(&plain).unwrap_or_else(|| plain.clone());
        for pattern in [plain, written] {
            let rule = Pattern::new(&pattern).expect("the rule builds");
            let mut kept = rule.lazy.create_cache();
            let mut at = 0;
            let gave_up = loop {
                match rule
                    .lazy
                    .try_search(&mut kept, &Input::new(&text).range(at..))
                {
                    Ok(Some(m)) => at = m.end().max(at + 1),
                    Ok(None) => break false,
                    Err(_) => break true,
                }
            };
            assert!(gave_up, "a cache that keeps its clears gives up: {pattern}");
            let reference = regex::Regex::new(&pattern).expect("the regex crate builds it");
            let groups = reference.captures_len();
            let theirs: Vec<Vec<Option<(usize, usize)>>> = reference
                .captures_iter(&text)
                .map(|c| {
                    (0..groups)
                        .map(|i| c.get(i).map(|m| (m.start(), m.end())))
                        .collect()
                })
                .collect();
            let ours: Vec<Vec<Option<(usize, usize)>>> = rule
                .captures_iter(&text)
                .map(|c| {
                    (0..groups)
                        .map(|i| c.get(i).map(|m| (m.start(), m.end())))
                        .collect()
                })
                .collect();
            let starts: Vec<usize> = theirs.iter().filter_map(|c| c[0].map(|m| m.0)).collect();
            assert!(
                starts.iter().any(|&s| s < before.len())
                    && starts.iter().any(|&s| s >= text.len() - after.len()),
                "matches on both sides: {pattern}"
            );
            assert_eq!(ours, theirs, "{pattern}");
            for at in starts {
                let read = |c: Option<regex::Captures<'_>>| {
                    c.map(|c| {
                        (0..groups)
                            .map(|i| c.get(i).map(|m| (m.start(), m.end())))
                            .collect::<Vec<_>>()
                    })
                };
                let mine = rule.captures_at(&text, at).map(|c| {
                    (0..groups)
                        .map(|i| c.get(i).map(|m| (m.start(), m.end())))
                        .collect::<Vec<_>>()
                });
                assert_eq!(mine, read(reference.captures_at(&text, at)), "at {at}");
            }
        }
    }

    #[test]
    fn f_tk_08_1_a_quit_finishes_on_the_slow_engine() {
        // A letter outside ASCII stops a lazy DFA where the pattern has
        // Unicode's `\b`; that search goes to the slow engine and finds
        // what the `regex` crate finds. Here `é` is a word character, so
        // the first token doesn't open on a boundary.
        let pattern = spec("gh[pousr]_");
        let rule = Pattern::new(&pattern).expect("the rule builds");
        let text = "jeton éghp_0123456789abcdefghijABCDEFGHIJ0123 und \
                    ghp_ABCDEFGHIJ0123456789abcdefghij0123é fin";
        let mut cache = rule.lazy.create_cache();
        let quit = rule.lazy.try_search(&mut cache, &Input::new(text));
        assert!(
            quit.as_ref()
                .is_err_and(|e| matches!(e.kind(), MatchErrorKind::Quit { .. })),
            "the lazy DFA read past é: {quit:?}"
        );
        let reference = regex::Regex::new(&pattern).expect("the regex crate builds it");
        let expected = spans(reference.find_iter(text).map(|m| (m.start(), m.end())));
        assert_eq!(expected.len(), 1);
        assert_eq!(
            spans(rule.find_iter(text).map(|m| (m.start(), m.end()))),
            expected
        );
    }

    #[test]
    fn f_tk_08_1_no_rule_matches_empty_text() {
        // So no match is ever empty: the guard against one stays a guard,
        // not a path the engine takes.
        for pattern in specs() {
            let written = super::super::tolerant(&pattern).unwrap_or_else(|| pattern.clone());
            for p in [&pattern, &written] {
                let rule = Pattern::new(p).expect("the rule builds");
                assert!(!rule.lazy.forward().get_nfa().has_empty(), "{p}");
            }
        }
    }
}
