//! Bounded raw-output ring: at most `lines` newlines and `lines × 200` bytes (§5.4).

use std::collections::VecDeque;

pub struct ScrollbackRing {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
    newlines: usize,
    max_lines: usize,
}

impl ScrollbackRing {
    pub fn new(max_lines: usize) -> Self {
        Self {
            chunks: VecDeque::new(),
            bytes: 0,
            newlines: 0,
            max_lines: max_lines.max(1),
        }
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.bytes += chunk.len();
        self.newlines += chunk.iter().filter(|b| **b == b'\n').count();
        self.chunks.push_back(chunk.to_vec());
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
}
