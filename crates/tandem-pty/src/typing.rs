//! Typing a prompt into an engine's TUI (ARCHITECTURE §6.5, F-EN-06 AC1).
//!
//! The text goes in as one write — wrapped in a bracketed paste when the
//! program turned that mode on — and Enter follows as its own write 50 ms
//! later, so TUIs that treat a paste specially still see a separate submit.

use std::time::Duration;

/// The pause between the text and Enter.
pub const ENTER_DELAY: Duration = Duration::from_millis(50);

const PASTE_START: &str = "\x1b[200~";
const PASTE_END: &str = "\x1b[201~";

/// Strips C0 and C1 control characters (and DEL) except tab. Newlines are
/// kept only when they can travel inside a bracketed paste; otherwise each
/// becomes the engine's newline sequence (newline without submit).
pub fn sanitize_prompt(text: &str, bracketed: bool, newline_sequence: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\t' => out.push('\t'),
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if bracketed {
                    out.push('\n');
                } else {
                    out.push_str(newline_sequence);
                }
            }
            c if (c as u32) < 0x20 || c == '\u{7f}' || ('\u{80}'..='\u{9f}').contains(&c) => {}
            c => out.push(c),
        }
    }
    out
}

/// The two writes for a prompt: the text, then Enter.
pub fn prompt_writes(text: &str, bracketed: bool, newline_sequence: &str) -> (Vec<u8>, Vec<u8>) {
    let clean = sanitize_prompt(text, bracketed, newline_sequence);
    let body = if bracketed {
        format!("{PASTE_START}{clean}{PASTE_END}")
    } else {
        clean
    };
    (body.into_bytes(), b"\r".to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_en_06_1_control_characters_are_stripped_and_enter_is_its_own_write() {
        let (text, enter) = prompt_writes("hello\x03world", false, "\x1b\r");
        assert_eq!(text, b"helloworld");
        assert_eq!(enter, b"\r");
        let (text, _) = prompt_writes("hello\x03world", true, "\x1b\r");
        assert_eq!(text, b"\x1b[200~helloworld\x1b[201~");
    }

    #[test]
    fn tabs_stay_and_newlines_follow_the_paste_mode() {
        assert_eq!(
            sanitize_prompt("a\tb\u{9b}c\u{7f}", false, "\x1b\r"),
            "a\tbc"
        );
        assert_eq!(
            sanitize_prompt("one\r\ntwo\nthree", true, "\x1b\r"),
            "one\ntwo\nthree"
        );
        assert_eq!(sanitize_prompt("one\ntwo", false, "\x1b\r"), "one\x1b\rtwo");
        // A paste can't be ended early from inside the text.
        assert_eq!(sanitize_prompt("x\x1b[201~y", true, "\x1b\r"), "x[201~y");
    }
}
