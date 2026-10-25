//! Masks known secret values in a pane's output (F-TL-12): each exact value
//! becomes `[REDACTED]`, also when it arrives split across reads. Matching is
//! leftmost-longest, so a value wins over one that's its prefix. A tail that
//! could still become a value, a longer one included, is held until the next
//! read, or until [`LiteralMasker::flush`] (the reader's flusher, after
//! [`HOLD`] of quiet).

use std::time::{Duration, Instant};

use aho_corasick::{AhoCorasick, MatchKind};
use zeroize::Zeroizing;

/// What a masked value reads as.
pub const MASK: &[u8] = b"[REDACTED]";

/// Shorter values aren't masked: they'd match ordinary output.
pub const MIN_LEN: usize = 4;

/// A held tail is released after this much quiet.
pub const HOLD: Duration = Duration::from_millis(50);

pub struct LiteralMasker {
    matcher: AhoCorasick,
    values: Vec<Zeroizing<Vec<u8>>>,
    longest: usize,
    held: Zeroizing<Vec<u8>>,
    held_since: Option<Instant>,
}

impl LiteralMasker {
    /// `None` when no value is long enough to mask.
    pub fn new<I, S>(values: I) -> Option<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<[u8]>,
    {
        let mut values: Vec<Zeroizing<Vec<u8>>> = values
            .into_iter()
            .map(|v| Zeroizing::new(v.as_ref().to_vec()))
            .filter(|v| v.len() >= MIN_LEN)
            .collect();
        values.sort_by(|a, b| a.as_slice().cmp(b.as_slice()));
        values.dedup_by(|a, b| a.as_slice() == b.as_slice());
        let longest = values.iter().map(|v| v.len()).max()?;
        let matcher = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostLongest)
            .build(values.iter().map(|v| v.as_slice()))
            .ok()?;
        Some(Self {
            matcher,
            values,
            longest,
            held: Zeroizing::new(Vec::new()),
            held_since: None,
        })
    }

    /// Where, at or after `from`, the rest of `data` could still grow into
    /// a value: the start of the tail to hold, or `data.len()`.
    fn hold_from(&self, data: &[u8], from: usize) -> usize {
        let first = from.max(data.len().saturating_sub(self.longest - 1));
        (first..data.len())
            .find(|&i| {
                let tail = &data[i..];
                self.values
                    .iter()
                    .any(|v| v.len() > tail.len() && v.starts_with(tail))
            })
            .unwrap_or(data.len())
    }

    /// The masked output for `input`, after anything held before it.
    pub fn push(&mut self, input: &[u8]) -> Vec<u8> {
        let mut data = Zeroizing::new(std::mem::take(&mut *self.held));
        data.extend_from_slice(input);
        let mut out = Vec::with_capacity(data.len());
        let mut pos = 0;
        loop {
            let hold = self.hold_from(&data, pos);
            // A whole value that starts before the held tail is masked; one
            // that starts in it could still be the start of a longer one.
            match self.matcher.find(&data[pos..]) {
                Some(m) if pos + m.start() < hold => {
                    out.extend_from_slice(&data[pos..pos + m.start()]);
                    out.extend_from_slice(MASK);
                    pos += m.end();
                }
                _ => {
                    out.extend_from_slice(&data[pos..hold]);
                    self.held.extend_from_slice(&data[hold..]);
                    break;
                }
            }
        }
        self.held_since = (!self.held.is_empty()).then(Instant::now);
        out
    }

    /// How long until the held tail is due; `None` with nothing held.
    pub fn due_in(&self) -> Option<Duration> {
        self.held_since.map(|t| HOLD.saturating_sub(t.elapsed()))
    }

    /// True when a held tail has waited [`HOLD`].
    pub fn due(&self) -> bool {
        self.due_in().is_some_and(|d| d.is_zero())
    }

    /// Releases the held tail: it didn't grow into the value it might have
    /// been, but a shorter value inside it is still masked.
    pub fn flush(&mut self) -> Vec<u8> {
        self.held_since = None;
        let held = Zeroizing::new(std::mem::take(&mut *self.held));
        let mut out = Vec::with_capacity(held.len());
        self.matcher
            .replace_all_with_bytes(&held, &mut out, |_, _, dst| {
                dst.extend_from_slice(MASK);
                true
            });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masked(values: &[&str], reads: &[&str]) -> String {
        let mut m = LiteralMasker::new(values.iter().map(|v| v.as_bytes())).expect("masker");
        let mut out = Vec::new();
        for r in reads {
            out.extend(m.push(r.as_bytes()));
        }
        out.extend(m.flush());
        String::from_utf8(out).expect("utf8")
    }

    #[test]
    fn f_tl_12_3_a_printed_value_is_masked() {
        assert_eq!(
            masked(&["sk_test_42"], &["key=sk_test_42\r\n"]),
            "key=[REDACTED]\r\n"
        );
        assert_eq!(
            masked(&["p@ss w0rd!"], &["a p@ss w0rd! b"]),
            "a [REDACTED] b"
        );
    }

    #[test]
    fn f_tl_12_3_a_value_split_across_reads_is_masked_at_every_offset() {
        let line = "token: sk_test_4242 done";
        for cut in 0..=line.len() {
            assert_eq!(
                masked(&["sk_test_4242"], &[&line[..cut], &line[cut..]]),
                "token: [REDACTED] done",
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn f_tl_12_3_a_held_prefix_is_released_as_it_was() {
        let mut m = LiteralMasker::new(["sk_test_42"]).expect("masker");
        assert_eq!(m.push(b"$ sk_te"), b"$ ".to_vec());
        assert!(!m.due());
        assert_eq!(m.flush(), b"sk_te".to_vec());
        assert_eq!(m.push(b"st"), b"st".to_vec());
    }

    #[test]
    fn f_tl_12_3_overlapping_values_take_the_longest() {
        assert_eq!(
            masked(&["abcd", "abcdefgh"], &["xabcdefghy abcdz"]),
            "x[REDACTED]y [REDACTED]z"
        );
    }

    #[test]
    fn short_values_are_not_masked() {
        assert!(LiteralMasker::new(["abc", ""]).is_none());
    }

    #[test]
    fn f_tl_12_3_a_longer_value_split_after_a_shorter_one_is_masked_whole() {
        let line = "a abcdefgh b abcd c";
        for cut in 0..=line.len() {
            assert_eq!(
                masked(&["abcd", "abcdefgh"], &[&line[..cut], &line[cut..]]),
                "a [REDACTED] b [REDACTED] c",
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn f_tl_12_3_a_whole_value_isnt_held_back_for_one_that_overlaps_it() {
        // "wxyz" is whole; the "yzab" after it may become "yzabcd".
        let mut m = LiteralMasker::new(["wxyz", "yzabcd"]).expect("masker");
        assert_eq!(m.push(b"wxyzab"), b"[REDACTED]ab".to_vec());
        assert_eq!(m.push(b"q"), b"q".to_vec());
    }

    #[test]
    fn f_tl_12_3_a_released_tail_still_masks_a_shorter_value_inside_it() {
        let mut m = LiteralMasker::new(["abcdefgh", "cdef"]).expect("masker");
        assert_eq!(m.push(b"> abcdef"), b"> ".to_vec());
        assert_eq!(m.flush(), b"ab[REDACTED]".to_vec());
    }

    #[test]
    fn f_tl_12_3_three_values_split_at_every_pair_of_offsets() {
        let values = ["sk_live_1", "sk_live_12345", "tok-999"];
        let line = "x sk_live_12345 y sk_live_1 z tok-999 sk_live_";
        let want = "x [REDACTED] y [REDACTED] z [REDACTED] sk_live_";
        for a in 0..=line.len() {
            for b in a..=line.len() {
                assert_eq!(
                    masked(&values, &[&line[..a], &line[a..b], &line[b..]]),
                    want,
                    "cuts {a} {b}"
                );
            }
        }
    }

    #[test]
    fn due_in_counts_down_only_while_something_is_held() {
        let mut m = LiteralMasker::new(["sk_test_42"]).expect("masker");
        assert_eq!(m.due_in(), None);
        m.push(b"sk_");
        assert!(m.due_in().is_some_and(|d| d <= HOLD));
        m.flush();
        assert_eq!(m.due_in(), None);
    }
}
