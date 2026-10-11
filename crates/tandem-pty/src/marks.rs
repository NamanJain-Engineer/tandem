//! Shell-integration marks (F-CD-10, ARCHITECTURE §5.8).
//!
//! The integration scripts print `OSC 7` (cwd) and `OSC 133 A/B/C/D` (prompt
//! start, prompt end, command start, command end with exit code), and agent
//! wrappers print `OSC 7770;handback`, each carrying this launch's token.
//! [`MarkFilter`] takes every such sequence out of the output stream before
//! anything else sees it:
//! - with the right token it becomes a [`Mark`]; OSC 133 goes back into the
//!   stream without the token (the terminal view decorates commands from it);
//! - without it (a program, a remote host, pasted text) it is dropped, so it
//!   can't spoof a mark and never reaches the terminal view.
//!
//! The token never reaches the webview, the scrollback ring or snapshots.
//! [`OutputCapture`] turns the bytes between C and D into the command's
//! plain-text output ("Copy last command output").

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use vte::{Params, Parser, Perform};

/// `pty:mark` (ARCHITECTURE §3.3): a prompt or command boundary in a pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PtyMark {
    pub pane_id: String,
    /// "A" prompt start, "B" prompt end, "C" command start, "D" command end.
    pub kind: String,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// `133;A`: a prompt starts.
    PromptStart,
    /// `133;B`: the prompt ends (input starts).
    PromptEnd,
    /// `133;C`: a command starts; its output follows. `skip_newline`: the
    /// shell wrote the mark before echoing the line break (PowerShell).
    /// `command`: the line the shell runs (`cmdline_url=`, F-EX-02).
    CommandStart {
        skip_newline: bool,
        command: Option<String>,
    },
    /// `133;D;<exit>`: the command ended. PowerShell sends the command line
    /// here instead of on `C`.
    CommandEnd {
        exit: Option<i32>,
        command: Option<String>,
    },
    /// A command finished (from `C` and `D`, with the shell's cwd): what
    /// command recall records (F-EX-02). Never in the stream.
    Ran(Ran),
    /// `OSC 7`: the shell's working directory.
    Cwd(String),
    /// `OSC 7770;handback`: an agent exited and its shell took the pane back.
    Handback(i32),
}

/// One finished command, as command recall stores it (F-EX-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub command: String,
    pub cwd: Option<String>,
    pub exit: Option<i32>,
    /// Unix ms.
    pub started_at: i64,
    pub duration_ms: Option<i64>,
}

/// The longest command line kept (percent-encoded it's at most three times
/// longer, inside [`MAX_MARK`]).
pub const MAX_COMMAND: usize = 4096;

/// `cmdline_url=<percent-encoded>` from a mark's parameters: one line or
/// several, never starting with a space (the shells' "keep out of history").
fn command_of(params: &[&str]) -> Option<String> {
    let raw = params.iter().find_map(|p| p.strip_prefix("cmdline_url="))?;
    let text = String::from_utf8(percent_decode(raw)?).ok()?;
    let text = text.trim_end_matches(['\n', '\r']).to_string();
    if text.is_empty() || text.starts_with(' ') || text.len() > MAX_COMMAND {
        return None;
    }
    Some(text)
}

/// `term_last_output`: the last finished command's output (F-CD-10 AC3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LastOutput {
    pub text: String,
    pub exit_code: Option<i32>,
}

impl Mark {
    /// The `pty:mark` event for prompt and command marks.
    pub fn event(&self, pane_id: &str) -> Option<PtyMark> {
        let (kind, exit_code) = match self {
            Mark::PromptStart => ("A", None),
            Mark::PromptEnd => ("B", None),
            Mark::CommandStart { .. } => ("C", None),
            Mark::CommandEnd { exit, .. } => ("D", *exit),
            Mark::Cwd(_) | Mark::Handback(_) | Mark::Ran(_) => return None,
        };
        Some(PtyMark {
            pane_id: pane_id.to_string(),
            kind: kind.to_string(),
            exit_code,
        })
    }
}

/// Longest mark kept for inspection; anything longer passes through untouched
/// (marks are short; a cwd is at most a few KB even percent-encoded).
const MAX_MARK: usize = 16 * 1024;

const OSC_PREFIXES: [&[u8]; 3] = [b"133;", b"7;", b"7770;"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    /// Saw ESC.
    Esc,
    /// Inside `ESC ]`, deciding whether it's one of ours.
    Osc,
    /// Inside one of ours; `true` after an ESC (a possible ST).
    Mark(bool),
}

/// Streaming filter; keeps its state across reads, so a mark split between
/// two chunks is still recognised.
pub struct MarkFilter {
    token: Option<String>,
    state: State,
    /// The pending `ESC ] …` bytes.
    pending: Vec<u8>,
}

impl MarkFilter {
    pub fn new(token: Option<String>) -> Self {
        Self {
            token,
            state: State::Ground,
            pending: Vec::new(),
        }
    }

    /// Appends the filtered bytes to `out` and returns the marks found, each
    /// with its offset in `out` (where it sat in the stream).
    pub fn filter(&mut self, input: &[u8], out: &mut Vec<u8>) -> Vec<(usize, Mark)> {
        let mut marks = Vec::new();
        let mut i = 0;
        while i < input.len() {
            let b = input[i];
            match self.state {
                State::Ground => {
                    // Copy everything up to the next ESC in one go.
                    let next = input[i..]
                        .iter()
                        .position(|&c| c == 0x1b)
                        .map(|p| i + p)
                        .unwrap_or(input.len());
                    out.extend_from_slice(&input[i..next]);
                    i = next;
                    if i < input.len() {
                        self.state = State::Esc;
                        i += 1;
                    }
                    continue;
                }
                State::Esc => {
                    if b == b']' {
                        self.pending.clear();
                        self.pending.extend_from_slice(b"\x1b]");
                        self.state = State::Osc;
                    } else {
                        out.push(0x1b);
                        if b == 0x1b {
                            // Still an ESC pending.
                            i += 1;
                            continue;
                        }
                        out.push(b);
                        self.state = State::Ground;
                    }
                }
                State::Osc => {
                    self.pending.push(b);
                    let body = &self.pending[2..];
                    let could_be_ours = OSC_PREFIXES
                        .iter()
                        .any(|p| p.starts_with(body) || body.starts_with(p));
                    if !could_be_ours {
                        // Not ours: pass it on as is, the rest streams through.
                        out.append(&mut self.pending);
                        self.state = State::Ground;
                    } else if OSC_PREFIXES.iter().any(|p| body.starts_with(p)) {
                        self.state = State::Mark(false);
                    }
                }
                State::Mark(after_esc) => {
                    if after_esc {
                        if b == b'\\' {
                            self.finish(out, &mut marks, 1);
                        } else {
                            // ESC without `\` cancels the OSC: pass what we
                            // held (less that ESC) and start over at the ESC.
                            self.pending.pop();
                            out.append(&mut self.pending);
                            self.state = State::Esc;
                            continue;
                        }
                    } else if b == 0x07 {
                        self.finish(out, &mut marks, 0);
                    } else if b == 0x1b {
                        self.pending.push(b);
                        self.state = State::Mark(true);
                    } else if self.pending.len() >= MAX_MARK {
                        self.pending.push(b);
                        out.append(&mut self.pending);
                        self.state = State::Ground;
                    } else {
                        self.pending.push(b);
                    }
                }
            }
            i += 1;
        }
        marks
    }

    /// A complete mark: `pending` holds `ESC ] body` (plus a trailing ESC when
    /// `st_len` is 1).
    fn finish(&mut self, out: &mut Vec<u8>, marks: &mut Vec<(usize, Mark)>, st_len: usize) {
        let end = self.pending.len() - st_len;
        let body = self.pending[2..end].to_vec();
        self.pending.clear();
        self.state = State::Ground;
        let Ok(body) = std::str::from_utf8(&body) else {
            return;
        };
        let mut parts = body.split(';');
        let code = parts.next().unwrap_or_default();
        let rest: Vec<&str> = parts.collect();
        let token_ok = |params: &[&str]| {
            let Some(token) = self.token.as_deref() else {
                return false;
            };
            params
                .iter()
                .any(|p| p.strip_prefix("tandem=") == Some(token))
        };
        match code {
            "133" => {
                if !token_ok(&rest) {
                    return;
                }
                let kind = rest.first().copied().unwrap_or_default();
                let mark = match kind {
                    "A" => Mark::PromptStart,
                    "B" => Mark::PromptEnd,
                    "C" => Mark::CommandStart {
                        skip_newline: rest.contains(&"nl=1"),
                        command: command_of(&rest),
                    },
                    "D" => Mark::CommandEnd {
                        exit: rest
                            .get(1)
                            .filter(|p| !p.contains('='))
                            .and_then(|p| p.parse().ok()),
                        command: command_of(&rest),
                    },
                    _ => return,
                };
                // Back into the stream without the token, for the view.
                out.extend_from_slice(b"\x1b]133;");
                out.extend_from_slice(kind.as_bytes());
                if let Mark::CommandEnd {
                    exit: Some(code), ..
                } = &mark
                {
                    out.extend_from_slice(format!(";{code}").as_bytes());
                }
                out.push(0x07);
                marks.push((out.len(), mark));
            }
            "7" => {
                if !token_ok(&rest) {
                    return;
                }
                if let Some(path) = rest.first().and_then(|url| file_url_path(url)) {
                    marks.push((out.len(), Mark::Cwd(path)));
                }
            }
            "7770" => {
                // `7770;handback;<token>;<exit>`
                if rest.first() != Some(&"handback") {
                    return;
                }
                if self
                    .token
                    .as_deref()
                    .is_some_and(|t| rest.get(1) == Some(&t))
                {
                    let exit = rest
                        .get(2)
                        .and_then(|c| c.trim().parse().ok())
                        .unwrap_or(-1);
                    marks.push((out.len(), Mark::Handback(exit)));
                }
            }
            _ => {}
        }
    }
}

/// `file://host/a%20b` → `/a b` (Windows: `file://host/C:/x` → `C:/x`).
pub fn file_url_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    let path = &rest[rest.find('/')?..];
    let bytes = percent_decode(path)?;
    let mut path = String::from_utf8(bytes).ok()?;
    // `/C:/Users/a` is a Windows drive path.
    if path.len() >= 3 && path.as_bytes()[2] == b':' && path.as_bytes()[1].is_ascii_alphabetic() {
        path.remove(0);
    }
    Some(path)
}

fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

/// Most text kept for one command's output; older lines go first.
const CAPTURE_LIMIT: usize = 4 * 1024 * 1024;

/// The plain text a command printed: escape sequences dropped, `\r`
/// overwrites, backspace and erase-in-line applied, line by line.
#[derive(Default)]
struct Lines {
    lines: std::collections::VecDeque<String>,
    line: Vec<char>,
    col: usize,
    size: usize,
    skip_newline: bool,
}

impl Lines {
    fn commit(&mut self) {
        let text: String = self.line.drain(..).collect();
        let text = text.trim_end().to_string();
        self.size += text.len() + 1;
        self.lines.push_back(text);
        self.col = 0;
        while self.size > CAPTURE_LIMIT {
            match self.lines.pop_front() {
                Some(old) => self.size -= old.len() + 1,
                None => break,
            }
        }
    }
}

impl Perform for Lines {
    fn print(&mut self, c: char) {
        self.skip_newline = false;
        if self.col < self.line.len() {
            self.line[self.col] = c;
        } else {
            self.line.resize(self.col, ' ');
            self.line.push(c);
        }
        self.col += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                if std::mem::take(&mut self.skip_newline) && self.line.is_empty() {
                    self.col = 0;
                    return;
                }
                self.commit();
            }
            b'\r' => self.col = 0,
            0x08 => self.col = self.col.saturating_sub(1),
            b'\t' => {
                let to = (self.col / 8 + 1) * 8;
                while self.col < to {
                    self.print(' ');
                }
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _: &[u8], _: bool, action: char) {
        let first = params
            .iter()
            .next()
            .and_then(|p| p.first().copied())
            .unwrap_or(0);
        if action == 'K' {
            match first {
                0 => self.line.truncate(self.col),
                1 => {
                    for c in self.line.iter_mut().take(self.col + 1) {
                        *c = ' ';
                    }
                }
                _ => self.line.clear(),
            }
        }
    }
}

/// Captures the output between `C` and `D` marks.
#[derive(Default)]
pub struct OutputCapture {
    parser: Option<Parser>,
    current: Lines,
    last: Option<String>,
    last_exit: Option<i32>,
}

impl OutputCapture {
    pub fn start(&mut self, skip_newline: bool) {
        self.parser = Some(Parser::new());
        self.current = Lines {
            skip_newline,
            ..Lines::default()
        };
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        if let Some(parser) = &mut self.parser {
            parser.advance(&mut self.current, bytes);
        }
    }

    pub fn end(&mut self, exit: Option<i32>) {
        if self.parser.take().is_none() {
            return;
        }
        let mut lines = std::mem::take(&mut self.current);
        if !lines.line.is_empty() {
            lines.commit();
        }
        while lines.lines.back().is_some_and(|l| l.is_empty()) {
            lines.lines.pop_back();
        }
        self.last = Some(lines.lines.into_iter().collect::<Vec<_>>().join("\n"));
        self.last_exit = exit;
    }

    pub fn running(&self) -> bool {
        self.parser.is_some()
    }

    /// The last finished command's output and exit code.
    pub fn last(&self) -> Option<(String, Option<i32>)> {
        self.last.clone().map(|text| (text, self.last_exit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(filter: &mut MarkFilter, chunks: &[&[u8]]) -> (Vec<u8>, Vec<Mark>) {
        let mut out = Vec::new();
        let mut marks = Vec::new();
        for c in chunks {
            marks.extend(filter.filter(c, &mut out).into_iter().map(|(_, m)| m));
        }
        (out, marks)
    }

    // F-TL-06 rule 2 (shell marks) is tests/invariants/tests/rule_2.rs.

    #[test]
    fn f_cd_10_2_marks_without_the_token_are_dropped() {
        let mut f = MarkFilter::new(Some("t0k".into()));
        let (out, marks) = run(
            &mut f,
            &[b"a\x1b]133;A\x07b\x1b]133;A;tandem=nope\x07c\x1b]7;file://h/etc\x07d"],
        );
        assert!(marks.is_empty(), "a program can't create a prompt mark");
        assert_eq!(out, b"abcd");
    }

    #[test]
    fn f_cd_10_tokened_marks_become_marks_and_lose_the_token() {
        let mut f = MarkFilter::new(Some("t0k".into()));
        let (out, marks) = run(
            &mut f,
            &[
                b"\x1b]7;file://box/tmp/a%20b;tandem=t0k\x07\x1b]133;A;tandem=t0k\x07$ ",
                b"\x1b]133;B;tandem=t0k\x1b\\ls\r\n\x1b]133;C;tandem=t0k\x07x\r\n",
                b"\x1b]133;D;2;tandem=t0k\x07",
            ],
        );
        assert_eq!(
            marks,
            vec![
                Mark::Cwd("/tmp/a b".into()),
                Mark::PromptStart,
                Mark::PromptEnd,
                Mark::CommandStart {
                    skip_newline: false,
                    command: None
                },
                Mark::CommandEnd {
                    exit: Some(2),
                    command: None
                },
            ]
        );
        assert_eq!(
            out,
            b"\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07x\r\n\x1b]133;D;2\x07".to_vec()
        );
        assert!(!String::from_utf8_lossy(&out).contains("t0k"));
    }

    #[test]
    fn f_ex_02_3_command_lines_come_only_with_the_token_and_never_reach_the_view() {
        let mut f = MarkFilter::new(Some("t0k".into()));
        let (out, marks) = run(
            &mut f,
            &[
                b"\x1b]133;C;cmdline_url=echo%20%22a%22%3Bls;tandem=t0k\x07x\r\n",
                b"\x1b]133;D;0;cmdline_url=pwsh%20line;tandem=t0k\x07",
                // A program's own mark with a command: dropped.
                b"\x1b]133;C;cmdline_url=rm%20-rf;tandem=nope\x07",
                // A leading space is never recorded.
                b"\x1b]133;C;cmdline_url=%20secret;tandem=t0k\x07",
            ],
        );
        assert_eq!(
            marks,
            vec![
                Mark::CommandStart {
                    skip_newline: false,
                    command: Some("echo \"a\";ls".into())
                },
                Mark::CommandEnd {
                    exit: Some(0),
                    command: Some("pwsh line".into())
                },
                Mark::CommandStart {
                    skip_newline: false,
                    command: None
                },
            ]
        );
        let shown = String::from_utf8_lossy(&out);
        assert!(!shown.contains("cmdline"), "{shown}");
        assert!(!shown.contains("echo"), "{shown}");
        assert!(!shown.contains("t0k"), "{shown}");
    }

    #[test]
    fn a_mark_split_across_reads_is_still_found() {
        let mut f = MarkFilter::new(Some("t0k".into()));
        let whole: &[u8] = b"x\x1b]133;D;0;tandem=t0k\x1b\\y";
        for cut in 1..whole.len() {
            let mut f2 = MarkFilter::new(Some("t0k".into()));
            let (out, marks) = run(&mut f2, &[&whole[..cut], &whole[cut..]]);
            assert_eq!(
                marks,
                vec![Mark::CommandEnd {
                    exit: Some(0),
                    command: None
                }],
                "cut at {cut}"
            );
            assert_eq!(out, b"x\x1b]133;D;0\x07y".to_vec(), "cut at {cut}");
        }
        // Other sequences pass through byte for byte, split or not.
        let other: &[u8] =
            b"\x1b]0;title\x07\x1b[31mred\x1b[0m\x1b]8;;http://x\x1b\\l\x1b]8;;\x1b\\";
        for cut in 0..other.len() {
            let (out, marks) = run(&mut f, &[&other[..cut], &other[cut..]]);
            assert!(marks.is_empty());
            assert_eq!(out, other.to_vec(), "cut at {cut}");
        }
    }

    #[test]
    fn an_esc_inside_a_mark_cancels_it_and_starts_a_new_sequence() {
        let mut f = MarkFilter::new(Some("t0k".into()));
        let (out, marks) = run(&mut f, &[b"\x1b]133;A;tand\x1b[1mbold"]);
        assert!(marks.is_empty());
        assert_eq!(out, b"\x1b]133;A;tand\x1b[1mbold".to_vec());
    }

    #[test]
    fn handback_marks_need_the_launch_token() {
        let mut f = MarkFilter::new(Some("tok123".into()));
        let (out, marks) = run(
            &mut f,
            &[b"\x1b]7770;handback;forged;0\x07bye\r\n\x1b]7770;handback;tok123;3\x07$ "],
        );
        assert_eq!(marks, vec![Mark::Handback(3)]);
        assert_eq!(out, b"bye\r\n$ ".to_vec());
        let mut none = MarkFilter::new(None);
        let (_, marks) = run(&mut none, &[b"\x1b]7770;handback;;0\x07"]);
        assert!(marks.is_empty());
    }

    #[test]
    fn file_urls_decode_to_paths() {
        assert_eq!(
            file_url_path("file://h/tmp/x%3By").as_deref(),
            Some("/tmp/x;y")
        );
        assert_eq!(
            file_url_path("file:///home/%C3%A9").as_deref(),
            Some("/home/é")
        );
        assert_eq!(
            file_url_path("file://PC/C:/Users/a").as_deref(),
            Some("C:/Users/a")
        );
        assert_eq!(file_url_path("http://h/x"), None);
    }

    #[test]
    fn f_cd_10_3_capture_is_the_plain_text_between_c_and_d() {
        let mut cap = OutputCapture::default();
        cap.feed(b"not captured\r\n");
        cap.start(false);
        cap.feed(b"\x1b[1;34mdir\x1b[0m  file.txt\r\n");
        cap.feed(b"50%\r100%\r\n");
        cap.feed(b"abc\x08\x08X\x1b[K\r\n\r\n");
        cap.end(Some(0));
        assert_eq!(
            cap.last(),
            Some(("dir  file.txt\n100%\naX".to_string(), Some(0)))
        );
        // PowerShell writes C before the line break of the command it accepted.
        cap.start(true);
        cap.feed(b"\r\nout\r\n");
        cap.end(Some(1));
        assert_eq!(cap.last(), Some(("out".to_string(), Some(1))));
        // A D without a C changes nothing.
        cap.end(Some(5));
        assert_eq!(cap.last().map(|l| l.1), Some(Some(1)));
    }
}
