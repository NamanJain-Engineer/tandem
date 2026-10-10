//! The side channel (ARCHITECTURE §5.7): every output byte also runs through
//! the query responder and a `vt100` shadow screen with no scrollback. The
//! responder answers DA1/DA2/DSR/CPR/XTVERSION/OSC 10-12 for every pane,
//! visible or hidden; the shadow screen supplies the cursor for CPR and the
//! modes the UI needs for typing and paste (bracketed paste, mouse, alternate
//! screen, application cursor keys) plus the OSC 0/2 title. Before any of
//! that, the shell-integration marks are taken out of the stream and checked
//! against the launch token (§5.8, `marks.rs`): they give the cwd, command
//! boundaries and the last command's output.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::marks::{Mark, MarkFilter, OutputCapture, Ran};
use crate::responder::{QueryResponder, TermColors};

/// What `pty:state` carries for a pane.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PtyState {
    pub pane_id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub alt_screen: bool,
    pub bracketed_paste: bool,
    /// The program reads the mouse (any of ?1000/1002/1003).
    pub mouse: bool,
    pub app_cursor: bool,
}

#[derive(Default)]
struct TitleCallbacks {
    title: Option<String>,
}

impl vt100::Callbacks for TitleCallbacks {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        let text = String::from_utf8_lossy(title).trim().to_string();
        self.title = (!text.is_empty()).then_some(text);
    }
}

/// `ESC [ 6 n`: answered with the shadow cursor as of the query.
const CPR: &[u8] = b"\x1b[6n";

/// What one read produced.
#[derive(Debug, Default)]
pub struct Fed {
    /// The output with marks removed (what the ring and the view get).
    pub output: Vec<u8>,
    /// Answers to terminal queries, for the program.
    pub replies: Vec<u8>,
    /// Marks with this launch's token, in stream order.
    pub marks: Vec<Mark>,
}

pub struct SideChannel {
    responder: QueryResponder,
    screen: vt100::Parser<TitleCallbacks>,
    published: PtyState,
    filter: MarkFilter,
    capture: OutputCapture,
    cwd: Option<String>,
    /// The command running now, from its `C` mark (F-EX-02).
    running: Option<(Option<String>, i64, std::time::Instant)>,
    /// A prompt is up and no command started since (F-EX-02 Run).
    at_prompt: bool,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl SideChannel {
    pub fn new(pane_id: &str, rows: u16, cols: u16) -> Self {
        Self::with_token(pane_id, rows, cols, None)
    }

    /// `token` is the per-launch secret shell-integration marks carry.
    pub fn with_token(pane_id: &str, rows: u16, cols: u16, token: Option<String>) -> Self {
        Self {
            responder: QueryResponder::new(),
            filter: MarkFilter::new(token),
            capture: OutputCapture::default(),
            cwd: None,
            running: None,
            at_prompt: false,
            screen: vt100::Parser::new_with_callbacks(
                rows.max(2),
                cols.max(2),
                0,
                TitleCallbacks::default(),
            ),
            published: PtyState {
                pane_id: pane_id.to_string(),
                ..PtyState::default()
            },
        }
    }

    /// After a panic while feeding: a fresh screen of the same size and a
    /// fresh responder and capture, keeping the mark token, so the pane
    /// carries on (§5.2 watchdog).
    pub fn recover(&mut self) {
        let (rows, cols) = self.screen.screen().size();
        self.screen = vt100::Parser::new_with_callbacks(rows, cols, 0, TitleCallbacks::default());
        self.responder = QueryResponder::new();
        self.capture = OutputCapture::default();
    }

    /// Feeds raw output: marks come out first, then the screen, the query
    /// responder and the output capture see the rest, with each mark applied
    /// where it sat in the stream.
    pub fn feed_raw(&mut self, raw: &[u8], colors: &TermColors) -> Fed {
        let mut fed = Fed::default();
        let found = self.filter.filter(raw, &mut fed.output);
        let mut at = 0;
        for (offset, mark) in found {
            let segment = fed.output[at..offset].to_vec();
            fed.replies.extend(self.feed(&segment, colors));
            at = offset;
            let mut ran = None;
            match &mark {
                Mark::CommandStart {
                    skip_newline,
                    command,
                } => {
                    self.capture.start(*skip_newline);
                    self.running = Some((command.clone(), now_ms(), std::time::Instant::now()));
                }
                Mark::CommandEnd { exit, command } => {
                    self.capture.end(*exit);
                    let started = self.running.take();
                    let (text, started_at, duration_ms) = match started {
                        Some((text, at, clock)) => (
                            text.or_else(|| command.clone()),
                            at,
                            Some(clock.elapsed().as_millis() as i64),
                        ),
                        None => (command.clone(), now_ms(), None),
                    };
                    if let Some(command) = text {
                        ran = Some(Mark::Ran(Ran {
                            command,
                            cwd: self.cwd.clone(),
                            exit: *exit,
                            started_at,
                            duration_ms,
                        }));
                    }
                }
                Mark::Cwd(path) => self.cwd = Some(path.clone()),
                _ => {}
            }
            match &mark {
                Mark::PromptStart | Mark::PromptEnd => self.at_prompt = true,
                Mark::CommandStart { .. } | Mark::Handback(_) => self.at_prompt = false,
                _ => {}
            }
            fed.marks.push(mark);
            fed.marks.extend(ran);
        }
        let rest = fed.output[at..].to_vec();
        fed.replies.extend(self.feed(&rest, colors));
        fed
    }

    /// Feeds output that has no marks; returns the bytes to write back to
    /// the program. The shadow screen sees everything up to a CPR query
    /// before it's answered.
    pub fn feed(&mut self, bytes: &[u8], colors: &TermColors) -> Vec<u8> {
        self.capture.feed(bytes);
        let mut replies = Vec::new();
        let mut start = 0;
        while start < bytes.len() {
            let end = find(&bytes[start..], CPR)
                .map(|i| start + i + CPR.len())
                .unwrap_or(bytes.len());
            let part = &bytes[start..end];
            self.screen.process(part);
            let (row, col) = self.screen.screen().cursor_position();
            replies.extend(self.responder.feed_at(part, colors, (row + 1, col + 1)));
            start = end;
        }
        replies
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.screen.screen_mut().set_size(rows.max(2), cols.max(2));
    }

    pub fn state(&self) -> PtyState {
        let s = self.screen.screen();
        PtyState {
            pane_id: self.published.pane_id.clone(),
            title: self.screen.callbacks().title.clone(),
            cwd: self.cwd.clone(),
            alt_screen: s.alternate_screen(),
            bracketed_paste: s.bracketed_paste(),
            mouse: s.mouse_protocol_mode() != vt100::MouseProtocolMode::None,
            app_cursor: s.application_cursor(),
        }
    }

    /// What `take_change` last reported.
    pub fn published(&self) -> PtyState {
        self.published.clone()
    }

    /// The state if it changed since the last call (for `pty:state`).
    pub fn take_change(&mut self) -> Option<PtyState> {
        let now = self.state();
        if now == self.published {
            return None;
        }
        self.published = now.clone();
        Some(now)
    }

    /// The last finished command's plain-text output and exit code
    /// ("Copy last command output", F-CD-10 AC3).
    pub fn last_output(&self) -> Option<(String, Option<i32>)> {
        self.capture.last()
    }

    /// A command is running (between its C and D marks).
    /// The shell showed a prompt and started nothing since: typing a
    /// command and Enter runs it (F-EX-02).
    pub fn at_prompt(&self) -> bool {
        self.at_prompt && !self.capture.running()
    }

    pub fn command_running(&self) -> bool {
        self.capture.running()
    }

    /// The screen's width in columns.
    pub fn cols(&self) -> u16 {
        self.screen.screen().size().1
    }

    /// Plain text of the visible screen (TUI goldens, readiness patterns).
    pub fn screen_text(&self) -> String {
        self.screen.screen().contents()
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cd_03_5_cpr_answers_with_the_cursor_at_the_query() {
        let mut side = SideChannel::new("pn_1", 24, 80);
        let c = TermColors::default();
        let reply = side.feed(b"\x1b[2J\x1b[5;10Hab\x1b[6nlater text", &c);
        assert_eq!(reply, b"\x1b[5;12R");
        // A hidden pane still gets DA1 answered (with the CPR rules unchanged).
        assert_eq!(side.feed(b"\x1b[c", &c), b"\x1b[?62;22c");
    }

    #[test]
    fn tracks_modes_and_title_for_pty_state() {
        let mut side = SideChannel::new("pn_1", 24, 80);
        let c = TermColors::default();
        assert!(side.take_change().is_none());
        side.feed(
            b"\x1b]0;vim main.rs\x07\x1b[?2004h\x1b[?1049h\x1b[?1000h\x1b[?1h",
            &c,
        );
        let state = side.take_change().expect("changed");
        assert_eq!(state.title.as_deref(), Some("vim main.rs"));
        assert!(state.bracketed_paste && state.alt_screen && state.mouse && state.app_cursor);
        assert!(side.take_change().is_none(), "reported once");
        side.feed(b"\x1b[?1049l\x1b[?2004l", &c);
        let state = side.take_change().expect("changed");
        assert!(!state.alt_screen && !state.bracketed_paste);
    }

    #[test]
    fn f_cd_10_marks_set_the_cwd_and_capture_the_last_output() {
        let mut side = SideChannel::with_token("pn_1", 24, 80, Some("tk".into()));
        let c = TermColors::default();
        let fed = side.feed_raw(
            b"\x1b]7;file://h/tmp;tandem=tk\x07\x1b]133;A;tandem=tk\x07$ ls\r\n\x1b]133;C;tandem=tk\x07a.txt  b.txt\r\n\x1b]133;D;0;tandem=tk\x07\x1b]133;A;tandem=tk\x07$ ",
            &c,
        );
        assert_eq!(fed.marks.len(), 5);
        assert_eq!(
            side.take_change().and_then(|s| s.cwd).as_deref(),
            Some("/tmp")
        );
        assert_eq!(
            side.last_output(),
            Some(("a.txt  b.txt".to_string(), Some(0)))
        );
        assert!(!String::from_utf8_lossy(&fed.output).contains("tandem="));
        // The shadow screen saw the text, not the marks.
        assert!(side.screen_text().contains("a.txt  b.txt"));
        // A spoofed D can't end anything or change the output.
        side.feed_raw(b"\x1b]133;C\x07fake\r\n\x1b]133;D;1\x07", &c);
        assert_eq!(
            side.last_output(),
            Some(("a.txt  b.txt".to_string(), Some(0)))
        );
    }

    #[test]
    fn f_ex_02_1_a_finished_command_is_reported_with_its_folder_exit_and_time() {
        let mut side = SideChannel::with_token("pn_1", 24, 80, Some("tk".into()));
        let c = TermColors::default();
        let fed = side.feed_raw(
            b"\x1b]7;file://h/srv/app;tandem=tk\x07\x1b]133;C;cmdline_url=false;tandem=tk\x07\x1b]133;D;1;tandem=tk\x07",
            &c,
        );
        let ran: Vec<&Ran> = fed
            .marks
            .iter()
            .filter_map(|m| if let Mark::Ran(r) = m { Some(r) } else { None })
            .collect();
        assert_eq!(ran.len(), 1);
        assert_eq!(ran[0].command, "false");
        assert_eq!(ran[0].cwd.as_deref(), Some("/srv/app"));
        assert_eq!(ran[0].exit, Some(1));
        assert!(ran[0].duration_ms.is_some());
        assert!(ran[0].started_at > 0);
        // Without a command line (a space-prefixed one) nothing is reported.
        let fed = side.feed_raw(b"\x1b]133;C;tandem=tk\x07\x1b]133;D;0;tandem=tk\x07", &c);
        assert!(!fed.marks.iter().any(|m| matches!(m, Mark::Ran(_))));
    }

    #[test]
    fn a_query_after_a_mark_is_answered_from_the_cursor_at_the_query() {
        let mut side = SideChannel::with_token("pn_1", 24, 80, Some("tk".into()));
        let fed = side.feed_raw(
            b"\x1b[2J\x1b[3;4H\x1b]133;A;tandem=tk\x07ab\x1b[6n",
            &TermColors::default(),
        );
        assert_eq!(fed.replies, b"\x1b[3;6R");
    }

    #[test]
    fn resize_moves_the_cpr_bounds() {
        let mut side = SideChannel::new("pn_1", 24, 80);
        side.resize(10, 40);
        let reply = side.feed(b"\x1b[99;99H\x1b[6n", &TermColors::default());
        assert_eq!(reply, b"\x1b[10;40R");
    }
}
