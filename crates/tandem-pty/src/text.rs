//! Plain text from terminal output: what a person would have read, for
//! handoff briefs (F-CD-16).

/// Plays `bytes` on a `rows`×`cols` screen and returns its text, with
/// escape sequences applied rather than shown and trailing spaces and blank
/// rows at either end dropped. Output taller than `rows` keeps its end.
pub fn render_plain(bytes: &[u8], rows: u16, cols: u16) -> String {
    let cols = cols.max(2);
    let mut parser = vt100::Parser::new(rows.max(2), cols, 0);
    parser.process(bytes);
    let lines: Vec<String> = parser
        .screen()
        .rows(0, cols)
        .map(|r| r.trim_end().to_string())
        .collect();
    let start = lines
        .iter()
        .position(|l| !l.is_empty())
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(start, |i| i + 1);
    lines[start..end].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cd_16_scrollback_text_applies_escapes_instead_of_showing_them() {
        let raw = b"\x1b[1;32mok\x1b[0m done\r\nworking\x1b[2K\rfinished\r\n\x1b]2;title\x07> ";
        assert_eq!(render_plain(raw, 10, 40), "ok done\nfinished\n>");
    }

    #[test]
    fn f_cd_16_taller_output_keeps_its_last_rows() {
        let raw: String = (1..=30).map(|i| format!("line {i}\r\n")).collect();
        let text = render_plain(raw.as_bytes(), 5, 40);
        assert_eq!(text, "line 27\nline 28\nline 29\nline 30");
    }
}
