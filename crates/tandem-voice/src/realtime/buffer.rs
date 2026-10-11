//! The pre-connect buffer (F-CN-01, ARCHITECTURE §12.6): the mic starts the
//! moment a call does, and until the socket opens its audio waits here. It
//! holds the latest 3 s; anything older falls off the front. Mute and
//! hang-up clear it, so nothing said then is ever sent.

use std::collections::VecDeque;

use super::SAMPLE_RATE;

/// How much early speech is kept.
pub const SECONDS: u32 = 3;

#[derive(Debug)]
pub struct PreConnect {
    buf: VecDeque<i16>,
    cap: usize,
}

impl Default for PreConnect {
    fn default() -> Self {
        Self::new(SECONDS)
    }
}

impl PreConnect {
    pub fn new(seconds: u32) -> Self {
        let cap = (SAMPLE_RATE * seconds) as usize;
        Self {
            buf: VecDeque::with_capacity(cap),
            cap,
        }
    }

    pub fn push(&mut self, samples: &[i16]) {
        let samples = if samples.len() > self.cap {
            &samples[samples.len() - self.cap..]
        } else {
            samples
        };
        let over = (self.buf.len() + samples.len()).saturating_sub(self.cap);
        self.buf.drain(..over);
        self.buf.extend(samples);
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Everything held, oldest first, leaving the buffer empty.
    pub fn take(&mut self) -> Vec<i16> {
        self.buf.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cn_01_4_two_seconds_before_connect_are_all_kept() {
        let mut b = PreConnect::default();
        let second: Vec<i16> = (0..SAMPLE_RATE as i16 / 2).chain(0..12_000).collect();
        b.push(&second);
        b.push(&second);
        assert_eq!(b.len(), 2 * SAMPLE_RATE as usize);
        let out = b.take();
        assert_eq!(out.len(), 2 * SAMPLE_RATE as usize);
        assert_eq!(&out[..second.len()], &second[..]);
        assert!(b.is_empty());
    }

    #[test]
    fn f_cn_01_4_only_the_latest_three_seconds_are_kept() {
        let mut b = PreConnect::default();
        for i in 0..5i16 {
            b.push(&vec![i; SAMPLE_RATE as usize]);
        }
        let out = b.take();
        assert_eq!(out.len(), 3 * SAMPLE_RATE as usize);
        assert_eq!(out[0], 2);
        assert_eq!(out[out.len() - 1], 4);
        // One block longer than the buffer keeps its own end.
        b.push(&vec![7; 4 * SAMPLE_RATE as usize]);
        assert_eq!(b.len(), 3 * SAMPLE_RATE as usize);
    }

    #[test]
    fn f_cn_01_mute_or_hang_up_clears_it() {
        let mut b = PreConnect::default();
        b.push(&[1, 2, 3]);
        b.clear();
        assert!(b.take().is_empty());
    }
}
