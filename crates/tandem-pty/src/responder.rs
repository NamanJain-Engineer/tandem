//! Answers terminal queries from Rust so each is answered exactly once, even
//! for hidden panes (§5.7): DA1, DA2, DSR, XTVERSION and OSC 10/11/12.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use vte::{Params, Parser, Perform};

/// Theme colours as `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TermColors {
    pub fg: String,
    pub bg: String,
    pub cursor: String,
}

impl Default for TermColors {
    /// "Tandem Night" (UI_SPEC §1.9).
    fn default() -> Self {
        Self {
            fg: "#dbe4ef".into(),
            bg: "#0b111c".into(),
            cursor: "#2dd4bf".into(),
        }
    }
}

/// `#0b111c` → `rgb:0b0b/1111/1c1c` (xterm's 16-bit-per-channel form).
pub fn xparse_color(hex: &str) -> String {
    let h = hex.trim_start_matches('#');
    let channel = |i: usize| h.get(i..i + 2).unwrap_or("00").to_ascii_lowercase();
    let (r, g, b) = (channel(0), channel(2), channel(4));
    format!("rgb:{r}{r}/{g}{g}/{b}{b}")
}

#[derive(Default)]
struct Collector {
    replies: Vec<u8>,
    colors: TermColors,
}

impl Perform for Collector {
    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        let first = params
            .iter()
            .next()
            .and_then(|p| p.first().copied())
            .unwrap_or(0);
        match (intermediates, action) {
            ([], 'c') if first == 0 => self.replies.extend_from_slice(b"\x1b[?62;22c"),
            ([b'>'], 'c') => self.replies.extend_from_slice(b"\x1b[>0;10;1c"),
            ([], 'n') if first == 5 => self.replies.extend_from_slice(b"\x1b[0n"),
            ([b'>'], 'q') => {
                self.replies.extend_from_slice(
                    format!("\x1bP>|Tandem {}\x1b\\", env!("CARGO_PKG_VERSION")).as_bytes(),
                );
            }
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        let (Some(code), Some(arg)) = (params.first(), params.get(1)) else {
            return;
        };
        if *arg != b"?" {
            return;
        }
        let color = match *code {
            b"10" => &self.colors.fg,
            b"11" => &self.colors.bg,
            b"12" => &self.colors.cursor,
            _ => return,
        };
        let end: &[u8] = if bell_terminated { b"\x07" } else { b"\x1b\\" };
        self.replies.extend_from_slice(b"\x1b]");
        self.replies.extend_from_slice(code);
        self.replies.push(b';');
        self.replies
            .extend_from_slice(xparse_color(color).as_bytes());
        self.replies.extend_from_slice(end);
    }
}

/// Streaming parser: keeps state across chunks, returns bytes to write back.
pub struct QueryResponder {
    parser: Parser,
    collector: Collector,
}

impl Default for QueryResponder {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryResponder {
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
            collector: Collector::default(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8], colors: &TermColors) -> Vec<u8> {
        self.collector.colors = colors.clone();
        self.parser.advance(&mut self.collector, bytes);
        std::mem::take(&mut self.collector.replies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_osc_queries_with_theme_colours() {
        let mut r = QueryResponder::new();
        let colors = TermColors {
            fg: "#1e293b".into(),
            bg: "#ffffff".into(),
            cursor: "#0d9488".into(),
        };
        let reply = r.feed(b"before\x1b]11;?\x1b\\after", &colors);
        assert_eq!(reply, b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\");
        let reply = r.feed(b"\x1b]10;?\x07", &colors);
        assert_eq!(reply, b"\x1b]10;rgb:1e1e/2929/3b3b\x07");
    }

    #[test]
    fn answers_split_queries_and_device_attributes() {
        let mut r = QueryResponder::new();
        let c = TermColors::default();
        assert!(r.feed(b"\x1b]11", &c).is_empty());
        assert_eq!(r.feed(b";?\x1b\\", &c), b"\x1b]11;rgb:0b0b/1111/1c1c\x1b\\");
        assert_eq!(r.feed(b"\x1b[c", &c), b"\x1b[?62;22c");
        assert_eq!(r.feed(b"\x1b[5n", &c), b"\x1b[0n");
        assert!(
            r.feed(b"\x1b]11;#000000\x07", &c).is_empty(),
            "setting a colour isn't a query"
        );
    }
}
