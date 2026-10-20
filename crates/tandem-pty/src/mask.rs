//! Masks known secret values in a pane's output (F-TL-12): each exact value
//! becomes `[REDACTED]`, also when it arrives split across reads. A tail that
//! could be the start of a value is held until the next read, or until
//! [`LiteralMasker::flush`] (the reader's flusher, after 50 ms of quiet).

use std::time::{Duration, Instant};

use zeroize::Zeroizing;

/// What a masked value reads as.
pub const MASK: &[u8] = b"[REDACTED]";

/// Shorter values aren't masked: they'd match ordinary output.
pub const MIN_LEN: usize = 4;

/// A held tail is released after this much quiet.
pub const HOLD: Duration = Duration::from_millis(50);

pub struct LiteralMasker {
    /// Longest first, so a value wins over one that's its prefix.
    values: Vec<Zeroizing<Vec<u8>>>,
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
        values.sort_by_key(|v| std::cmp::Reverse(v.len()));
        values.dedup_by(|a, b| a.as_slice() == b.as_slice());
        (!values.is_empty()).then(|| Self {
            values,
            held: Zeroizing::new(Vec::new()),
            held_since: None,
        })
    }

    /// The masked output for `input`, after anything held before it.
    pub fn push(&mut self, input: &[u8]) -> Vec<u8> {
        let mut data = Zeroizing::new(std::mem::take(&mut *self.held));
        data.extend_from_slice(input);
        let mut out = Vec::with_capacity(data.len());
        let mut i = 0;
        'scan: while i < data.len() {
            let rest = &data[i..];
            for v in &self.values {
                if rest.starts_with(v) {
                    out.extend_from_slice(MASK);
                    i += v.len();
                    continue 'scan;
                }
            }
            if self
                .values
                .iter()
                .any(|v| rest.len() < v.len() && v.starts_with(rest))
            {
                self.held.extend_from_slice(rest);
                break;
            }
            out.push(data[i]);
            i += 1;
        }
        self.held_since = (!self.held.is_empty()).then(Instant::now);
        out
    }

    /// True when a held tail has waited [`HOLD`].
    pub fn due(&self) -> bool {
        self.held_since.is_some_and(|t| t.elapsed() >= HOLD)
    }

    /// Releases the held tail as it is: it wasn't a whole value.
    pub fn flush(&mut self) -> Vec<u8> {
        self.held_since = None;
        std::mem::take(&mut *self.held).to_vec()
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
}
