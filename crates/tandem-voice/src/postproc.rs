//! After the transcript: dictionary replacements, then clean-up (F-DI-06
//! step 3, ARCHITECTURE §12.5), and what a terminal target may receive.

use crate::dictionary::Dictionary;

/// Filler words dropped where they open or close the take, or a sentence.
const FILLERS: &[&str] = &["um", "uh", "erm", "er", "uhm", "umm", "hmm", "mm"];

/// The whole pipeline for one take: dictionary first, then clean-up.
pub fn apply(text: &str, dict: &Dictionary) -> String {
    cleanup(&dict.apply(text))
}

/// Strips filler at the edges of sentences and trailing ellipses, collapses
/// spaces and keeps at most one final period.
pub fn cleanup(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut sentences: Vec<String> = Vec::new();
    for raw in split_sentences(&collapsed) {
        let s = strip_fillers(raw);
        if !s.is_empty() {
            sentences.push(s);
        }
    }
    let mut out = sentences.join(" ");
    // Trailing "..." / "…" become nothing, then one final period at most.
    loop {
        let trimmed = out
            .trim_end()
            .trim_end_matches('…')
            .trim_end_matches("...")
            .trim_end();
        if trimmed.len() == out.len() {
            break;
        }
        out = trimmed.to_string();
    }
    while out.ends_with("..") {
        out.pop();
    }
    out
}

/// Splits after `.`, `?` or `!` followed by a space, keeping the mark.
fn split_sentences(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if matches!(b, b'.' | b'?' | b'!') && bytes.get(i + 1) == Some(&b' ') {
            out.push(&s[start..=i]);
            start = i + 2;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

fn is_filler(word: &str) -> bool {
    let w = word
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    FILLERS.contains(&w.as_str())
}

fn strip_fillers(sentence: &str) -> String {
    let mut words: Vec<&str> = sentence.split(' ').filter(|w| !w.is_empty()).collect();
    let end_mark: String = words
        .last()
        .map(|w| {
            w.chars()
                .rev()
                .take_while(|c| matches!(c, '.' | '?' | '!'))
                .collect()
        })
        .unwrap_or_default();
    while words.first().is_some_and(|w| is_filler(w)) {
        words.remove(0);
    }
    let mut dropped_tail = false;
    while words.last().is_some_and(|w| is_filler(w)) {
        words.pop();
        dropped_tail = true;
    }
    if words.is_empty() {
        return String::new();
    }
    let mut s = words.join(" ");
    // Keep the sentence's own mark if the filler carried it ("…, um.").
    if dropped_tail && !end_mark.is_empty() {
        s = s.trim_end_matches(',').to_string();
        s.extend(end_mark.chars().rev());
    }
    // Capitalise a sentence whose opening filler was removed.
    let mut chars = s.chars();
    match chars.next() {
        Some(c)
            if c.is_lowercase() && sentence.chars().next().is_some_and(|f| f.is_uppercase()) =>
        {
            c.to_uppercase().chain(chars).collect()
        }
        _ => s,
    }
}

/// Text for a terminal: control characters (ESC sequences included) are
/// removed, newlines become spaces and nothing that submits is ever added
/// (F-DI-06 AC4).
pub fn for_terminal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Drop a CSI/OSC introducer with it.
            if matches!(chars.peek(), Some('[') | Some(']') | Some('(') | Some(')')) {
                chars.next();
            }
            continue;
        }
        if c == '\n' || c == '\r' || c == '\t' {
            out.push(' ');
            continue;
        }
        if c.is_control() || ('\u{80}'..='\u{9f}').contains(&c) {
            continue;
        }
        out.push(c);
    }
    out.trim_end().to_string()
}

/// Words for the history and stats.
pub fn word_count(text: &str) -> u32 {
    text.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::Rule;

    #[test]
    fn f_di_06_cleanup_strips_filler_and_trailing_ellipses() {
        assert_eq!(cleanup("Um, so we ship it..."), "So we ship it");
        assert_eq!(cleanup("we ship it, uh."), "we ship it.");
        assert_eq!(cleanup("Done.. "), "Done.");
        assert_eq!(cleanup("wait…"), "wait");
        assert_eq!(cleanup("Uh. Hello world. Um."), "Hello world.");
        assert_eq!(cleanup("the  umbrella   is here"), "the umbrella is here");
        assert_eq!(cleanup("um"), "");
    }

    #[test]
    fn f_di_06_dictionary_runs_before_cleanup() {
        let d = Dictionary::compile(&[Rule::correction("r", "tandem", "Tandem")], None, None);
        assert_eq!(apply("um use tandem...", &d), "use Tandem");
    }

    #[test]
    fn f_di_06_4_terminal_text_has_no_control_chars_and_no_cr() {
        let out = for_terminal("ls \u{1b}[31mred\u{1b}[0m\r\nrm -rf\u{7}\u{9b}x\n");
        assert_eq!(out, "ls 31mred0m  rm -rfx");
        assert!(!out.contains('\r') && !out.contains('\n') && !out.contains('\u{1b}'));
    }

    #[test]
    fn word_counts() {
        assert_eq!(word_count("hello, world — ok"), 3);
        assert_eq!(word_count(""), 0);
    }
}
