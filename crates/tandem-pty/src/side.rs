//! The side channel (ARCHITECTURE §5.7): every output byte also runs through
//! the query responder and a `vt100` shadow screen with no scrollback. The
//! responder answers DA1/DA2/DSR/CPR/XTVERSION/OSC 10-12 for every pane,
//! visible or hidden; the shadow screen supplies the cursor for CPR and the
//! modes the UI needs for typing and paste (bracketed paste, mouse, alternate
//! screen, application cursor keys) plus the OSC 0/2 title.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

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

pub struct SideChannel {
    responder: QueryResponder,
    screen: vt100::Parser<TitleCallbacks>,
    published: PtyState,
}

impl SideChannel {
    pub fn new(pane_id: &str, rows: u16, cols: u16) -> Self {
        Self {
            responder: QueryResponder::new(),
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

    /// Feeds output; returns the bytes to write back to the program. The
    /// shadow screen sees everything up to a CPR query before it's answered.
    pub fn feed(&mut self, bytes: &[u8], colors: &TermColors) -> Vec<u8> {
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
            cwd: self.published.cwd.clone(),
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
    fn resize_moves_the_cpr_bounds() {
        let mut side = SideChannel::new("pn_1", 24, 80);
        side.resize(10, 40);
        let reply = side.feed(b"\x1b[99;99H\x1b[6n", &TermColors::default());
        assert_eq!(reply, b"\x1b[10;40R");
    }
}
