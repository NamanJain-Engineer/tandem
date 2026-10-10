//! Bounded raw-output ring: at most `lines` newlines and `lines × 200` bytes (§5.4).
//!
//! Positions: `end()` counts every byte ever pushed, so a view that knows how
//! much output it has parsed can say exactly where it stands. A parked view
//! (§5.1, D-139) hands back its state as a base that replaces everything
//! before that position; the bytes after it stay as they are.

use std::collections::VecDeque;

pub struct ScrollbackRing {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
    newlines: usize,
    max_lines: usize,
    /// Bytes ever pushed (a base doesn't count: it stands for bytes already pushed).
    end: u64,
}

impl ScrollbackRing {
    pub fn new(max_lines: usize) -> Self {
        Self {
            chunks: VecDeque::new(),
            bytes: 0,
            newlines: 0,
            max_lines: max_lines.max(1),
            end: 0,
        }
    }

    /// The position just past the last byte pushed.
    pub fn end(&self) -> u64 {
        self.end
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.end += chunk.len() as u64;
        self.add(chunk.to_vec());
    }

    /// Replaces what came before position `at` with `base` (one chunk per
    /// line, so trimming drops whole lines) and keeps the bytes from `at` on.
    /// False, and nothing changes, when the ring no longer holds all of them.
    pub fn rebase(&mut self, at: u64, base: Vec<Vec<u8>>) -> bool {
        if at > self.end || (self.end - at) as usize > self.bytes {
            return false;
        }
        let mut keep = (self.end - at) as usize;
        let mut tail: VecDeque<Vec<u8>> = VecDeque::new();
        while keep > 0 {
            let Some(mut chunk) = self.chunks.pop_back() else {
                break;
            };
            if chunk.len() > keep {
                chunk = chunk.split_off(chunk.len() - keep);
            }
            keep -= chunk.len();
            tail.push_front(chunk);
        }
        self.chunks.clear();
        self.bytes = 0;
        self.newlines = 0;
        for chunk in base.into_iter().chain(tail) {
            self.add(chunk);
        }
        true
    }

    fn add(&mut self, chunk: Vec<u8>) {
        if chunk.is_empty() {
            return;
        }
        self.bytes += chunk.len();
        self.newlines += chunk.iter().filter(|b| **b == b'\n').count();
        self.chunks.push_back(chunk);
        while self.chunks.len() > 1
            && (self.newlines > self.max_lines || self.bytes > self.max_lines * 200)
        {
            if let Some(old) = self.chunks.pop_front() {
                self.bytes -= old.len();
                self.newlines -= old.iter().filter(|b| **b == b'\n').count();
            }
        }
    }

    pub fn contents(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes);
        for c in &self.chunks {
            out.extend_from_slice(c);
        }
        out
    }

    /// The last `lines` lines (raw bytes, escapes included).
    pub fn tail(&self, lines: usize) -> Vec<u8> {
        let all = self.contents();
        let mut seen = 0;
        // A trailing newline ends the last line rather than starting one.
        let body = all.strip_suffix(b"\n").unwrap_or(&all);
        for (i, b) in body.iter().enumerate().rev() {
            if *b == b'\n' {
                seen += 1;
                if seen == lines {
                    return all[i + 1..].to_vec();
                }
            }
        }
        all
    }

    pub fn len_bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_within_line_and_byte_bounds() {
        let mut ring = ScrollbackRing::new(100);
        for i in 0..1000 {
            ring.push(format!("line {i}\n").as_bytes());
        }
        let text = String::from_utf8(ring.contents()).expect("utf8");
        assert!(text.lines().count() <= 100);
        assert!(text.ends_with("line 999\n"));
        assert!(ring.len_bytes() <= 100 * 200);
    }

    #[test]
    fn tail_keeps_the_last_lines() {
        let mut ring = ScrollbackRing::new(100);
        ring.push(b"one\ntwo\nthree\n");
        assert_eq!(ring.tail(2), b"two\nthree\n");
        assert_eq!(ring.tail(10), b"one\ntwo\nthree\n");
        ring.push(b"four");
        assert_eq!(ring.tail(2), b"three\nfour");
    }

    #[test]
    fn nfr_5_1_positions_count_every_byte_pushed_even_after_trimming() {
        let mut ring = ScrollbackRing::new(2);
        ring.push(b"a\n");
        ring.push(b"bb\n");
        ring.push(b"ccc\n");
        assert_eq!(ring.end(), 9);
        assert_eq!(ring.contents(), b"bb\nccc\n");
    }

    #[test]
    fn nfr_5_1_a_rebase_replaces_what_the_view_had_and_keeps_what_came_after() {
        let mut ring = ScrollbackRing::new(100);
        ring.push(b"one\ntw");
        ring.push(b"o\nthree\n");
        // The view had parsed "one\ntwo\n" (8 bytes) when it parked.
        assert!(ring.rebase(
            8,
            vec![b"ONE\n".to_vec(), b"TWO\n".to_vec(), b"SCREEN".to_vec()]
        ));
        assert_eq!(ring.contents(), b"ONE\nTWO\nSCREENthree\n");
        assert_eq!(ring.end(), 14, "positions don't move");
        ring.push(b"four\n");
        assert_eq!(ring.contents(), b"ONE\nTWO\nSCREENthree\nfour\n");
        // A second park after a later attach.
        assert!(ring.rebase(19, vec![b"ALL\n".to_vec()]));
        assert_eq!(ring.contents(), b"ALL\n");
    }

    #[test]
    fn nfr_5_1_a_rebase_the_ring_can_no_longer_honour_changes_nothing() {
        let mut ring = ScrollbackRing::new(2);
        for i in 0..10 {
            ring.push(format!("line {i}\n").as_bytes());
        }
        let before = ring.contents();
        // Position 0 was trimmed long ago; a position past the end never existed.
        assert!(!ring.rebase(0, vec![b"base\n".to_vec()]));
        assert!(!ring.rebase(ring.end() + 1, vec![b"base\n".to_vec()]));
        assert_eq!(ring.contents(), before);
    }

    #[test]
    fn nfr_5_1_a_parked_base_is_trimmed_a_line_at_a_time() {
        let mut ring = ScrollbackRing::new(3);
        ring.push(b"x");
        let base = (0..3).map(|i| format!("base {i}\n").into_bytes()).collect();
        assert!(ring.rebase(1, base));
        ring.push(b"new\n");
        let text = String::from_utf8(ring.contents()).expect("utf8");
        assert_eq!(text, "base 1\nbase 2\nnew\n");
    }
}
