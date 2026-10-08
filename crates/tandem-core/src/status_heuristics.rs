//! Status from a pane's output, for engines without hooks (ARCHITECTURE
//! §6.6, F-EN-05 AC3).
//!
//! Output means working; a waiting pattern on the screen means waiting;
//! 30 s of quiet after work means done; done becomes idle after 10 minutes
//! unseen, or 2 s with the pane focused. Output that is only the echo of
//! the user's typing, or a repaint after a resize, is never work. Once an
//! engine reports through its hooks, the hooks are the truth and this stays
//! quiet for that pane.

use regex::Regex;

use crate::status::{AgentStatus, StatusUpdate, WaitingReason};

/// Output this soon after a resize or keystroke is a repaint or an echo.
pub const ECHO_MS: i64 = 500;
pub const DONE_AFTER_QUIET_MS: i64 = 30_000;
pub const IDLE_AFTER_UNSEEN_MS: i64 = 10 * 60_000;
pub const IDLE_AFTER_FOCUSED_MS: i64 = 2_000;

#[derive(Debug)]
pub struct Heuristics {
    waiting: Vec<Regex>,
    status: Option<AgentStatus>,
    last_output: Option<i64>,
    quiet_from: Option<i64>,
    done_at: Option<i64>,
    focused_since: Option<i64>,
    /// Hooks reported for this pane: they win from here on.
    hooked: bool,
}

impl Heuristics {
    /// `waiting_patterns` from the registry; invalid ones are skipped.
    pub fn new(waiting_patterns: &[String]) -> Self {
        Self {
            waiting: waiting_patterns
                .iter()
                .filter_map(|p| Regex::new(p).ok())
                .collect(),
            status: None,
            last_output: None,
            quiet_from: None,
            done_at: None,
            focused_since: None,
            hooked: false,
        }
    }

    pub fn status(&self) -> Option<AgentStatus> {
        self.status
    }

    /// A signed hook report arrived for this pane. Returns whether one had
    /// arrived before.
    pub fn hooked(&mut self) -> bool {
        std::mem::replace(&mut self.hooked, true)
    }

    /// The user typed, or the pane was resized or repainted: the output that
    /// follows within [`ECHO_MS`] isn't the engine working.
    pub fn quiet_window(&mut self, now: i64) {
        self.quiet_from = Some(now);
    }

    /// The engine wrote output; `screen` is the visible text when the caller
    /// has it (checked for waiting patterns).
    pub fn output(&mut self, now: i64, screen: Option<&str>) -> Option<StatusUpdate> {
        if self.hooked {
            return None;
        }
        if let Some(screen) = screen {
            if self.waiting.iter().any(|re| re.is_match(screen)) {
                self.last_output = Some(now);
                return self.set(AgentStatus::Waiting, Some(WaitingReason::Approval), now);
            }
        }
        if self.quiet_from.is_some_and(|q| now - q < ECHO_MS) {
            return None;
        }
        self.last_output = Some(now);
        self.set(AgentStatus::Working, None, now)
    }

    pub fn focus(&mut self, now: i64, focused: bool) -> Option<StatusUpdate> {
        self.focused_since = focused.then_some(now);
        self.tick(now)
    }

    /// Time passed: working → done after quiet, done → idle.
    pub fn tick(&mut self, now: i64) -> Option<StatusUpdate> {
        if self.hooked {
            return None;
        }
        match self.status {
            Some(AgentStatus::Working)
                if self
                    .last_output
                    .is_some_and(|t| now - t >= DONE_AFTER_QUIET_MS) =>
            {
                self.set(AgentStatus::Done, None, now)
            }
            Some(AgentStatus::Done) => {
                let done_at = self.done_at.unwrap_or(now);
                let seen = self
                    .focused_since
                    .is_some_and(|f| now - f.max(done_at) >= IDLE_AFTER_FOCUSED_MS);
                if seen || now - done_at >= IDLE_AFTER_UNSEEN_MS {
                    self.set(AgentStatus::Idle, None, now)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn set(
        &mut self,
        status: AgentStatus,
        reason: Option<WaitingReason>,
        now: i64,
    ) -> Option<StatusUpdate> {
        if self.status == Some(status) {
            return None;
        }
        self.status = Some(status);
        self.done_at = (status == AgentStatus::Done).then_some(now);
        Some(StatusUpdate {
            status,
            reason,
            ts: now,
            tool: None,
            title: None,
            preview: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h() -> Heuristics {
        Heuristics::new(&["Do you want to proceed\\?".into()])
    }

    fn status(u: Option<StatusUpdate>) -> Option<AgentStatus> {
        u.map(|u| u.status)
    }

    #[test]
    fn f_en_05_3_output_is_work_then_quiet_is_done_then_idle() {
        let mut s = h();
        assert_eq!(status(s.output(1_000, None)), Some(AgentStatus::Working));
        assert_eq!(s.output(1_500, None), None, "no repeat updates");
        assert_eq!(s.tick(30_000), None);
        assert_eq!(status(s.tick(31_500)), Some(AgentStatus::Done));
        assert_eq!(s.tick(31_500 + IDLE_AFTER_UNSEEN_MS - 1), None);
        assert_eq!(
            status(s.tick(31_500 + IDLE_AFTER_UNSEEN_MS)),
            Some(AgentStatus::Idle)
        );
    }

    #[test]
    fn f_en_05_3_done_becomes_idle_after_two_seconds_focused() {
        let mut s = h();
        s.output(0, None);
        s.tick(DONE_AFTER_QUIET_MS);
        assert_eq!(s.focus(DONE_AFTER_QUIET_MS + 100, true), None);
        assert_eq!(
            status(s.tick(DONE_AFTER_QUIET_MS + 2_100)),
            Some(AgentStatus::Idle)
        );
    }

    #[test]
    fn f_en_05_3_resizes_repaints_and_typing_are_never_work() {
        let mut s = h();
        for i in 0..20 {
            let at = 10_000 + i * 1_000;
            s.quiet_window(at);
            assert_eq!(s.output(at + 100, None), None, "resize {i}");
        }
        assert_eq!(s.status(), None);
        // Output well after a keystroke is work.
        assert_eq!(status(s.output(40_000, None)), Some(AgentStatus::Working));
    }

    #[test]
    fn a_waiting_pattern_on_screen_is_waiting_for_approval() {
        let mut s = h();
        s.output(0, None);
        let u = s
            .output(500, Some("Edit main.rs\nDo you want to proceed?\n❯ 1. Yes"))
            .expect("waiting");
        assert_eq!(u.status, AgentStatus::Waiting);
        assert_eq!(u.reason, Some(WaitingReason::Approval));
        // Waiting doesn't time out into done.
        assert_eq!(s.tick(100_000), None);
    }

    #[test]
    fn hooks_win_once_they_report() {
        let mut s = h();
        s.hooked();
        assert_eq!(s.output(0, None), None);
        assert_eq!(s.tick(100_000), None);
    }
}
