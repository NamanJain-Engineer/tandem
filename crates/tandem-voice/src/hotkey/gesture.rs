//! Turns key events into take actions (F-DI-01, ARCHITECTURE §12.2 "Mode
//! logic"). Pure: time comes in with each event, so every rule is tested
//! without a keyboard.
//!
//! - **Hold:** the mic starts on the key-down (`Arm`, nothing shown yet); at
//!   150 ms still held the take becomes real (`Start`), and the release stops
//!   it. A shorter tap is dropped (`Discard`): no recording.
//! - **Latch:** two taps within 350 ms start a hands-free take; the next tap
//!   stops it.
//! - **Toggle:** its own binding; a press starts, the next press stops.
//! - A stop needs a native key-up for a key-down that was seen. Esc cancels.

/// Which binding an event is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Hold,
    Toggle,
    Esc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub which: Which,
    pub phase: Phase,
    /// Seen from the OS (not something Tandem or another app synthesised).
    pub native: bool,
    /// Milliseconds on any monotonic clock.
    pub at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Open the mic now, show nothing yet.
    Arm,
    /// The take is real: show it.
    Start { hands_free: bool },
    /// Stop recording and transcribe.
    Stop,
    /// Drop an armed tap silently: it never was a take.
    Discard,
    /// Esc: drop the take, no transcription, nothing inserted.
    Cancel,
}

pub const TAP_MS: u64 = 150;
pub const DOUBLE_TAP_MS: u64 = 350;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    /// Hold key down at `since`, not a take yet.
    Pressed {
        since: u64,
    },
    /// A held take.
    Holding,
    /// A short tap ended at `at`; a second one latches.
    TapGap {
        at: u64,
    },
    /// The latching press is still down.
    LatchedHeld,
    /// Hands-free.
    Latched,
    /// Toggled on with the toggle key.
    Toggled,
    /// A stop came on a key-down; its key-up is swallowed.
    StopPending,
}

#[derive(Debug, Clone)]
pub struct Gesture {
    state: State,
    latch: bool,
    toggle_down: bool,
}

impl Gesture {
    pub fn new(latch: bool) -> Self {
        Self {
            state: State::Idle,
            latch,
            toggle_down: false,
        }
    }

    pub fn set_latch(&mut self, latch: bool) {
        self.latch = latch;
    }

    /// True while a take is recording (shown or not).
    pub fn active(&self) -> bool {
        matches!(
            self.state,
            State::Pressed { .. }
                | State::Holding
                | State::LatchedHeld
                | State::Latched
                | State::Toggled
        )
    }

    /// The service ended the take itself (the 10-minute cap, an error).
    pub fn reset(&mut self) {
        self.state = match self.state {
            State::Pressed { .. } | State::Holding | State::LatchedHeld => State::StopPending,
            _ => State::Idle,
        };
    }

    /// Timers: a hold becomes a take at 150 ms, a tap gap expires at 350 ms.
    pub fn tick(&mut self, now: u64) -> Vec<Action> {
        match self.state {
            State::Pressed { since } if now.saturating_sub(since) >= TAP_MS => {
                self.state = State::Holding;
                vec![Action::Start { hands_free: false }]
            }
            State::TapGap { at } if now.saturating_sub(at) > DOUBLE_TAP_MS => {
                self.state = State::Idle;
                vec![]
            }
            _ => vec![],
        }
    }

    pub fn on(&mut self, e: KeyEvent) -> Vec<Action> {
        let mut out = self.tick(e.at_ms);
        out.extend(self.handle(e));
        out
    }

    fn handle(&mut self, e: KeyEvent) -> Vec<Action> {
        use Phase::*;
        use State::*;
        match (e.which, e.phase) {
            (Which::Esc, Down) => match self.state {
                Pressed { .. } => {
                    self.state = StopPending;
                    vec![Action::Discard]
                }
                Holding | LatchedHeld => {
                    self.state = StopPending;
                    vec![Action::Cancel]
                }
                Latched | Toggled => {
                    self.state = Idle;
                    vec![Action::Cancel]
                }
                _ => vec![],
            },
            (Which::Esc, Up) => vec![],
            (Which::Toggle, Down) => {
                if self.toggle_down {
                    return vec![]; // auto-repeat
                }
                self.toggle_down = true;
                match self.state {
                    Idle | TapGap { .. } | StopPending => {
                        self.state = Toggled;
                        vec![Action::Arm, Action::Start { hands_free: true }]
                    }
                    Toggled | Latched => {
                        self.state = Idle;
                        vec![Action::Stop]
                    }
                    _ => vec![],
                }
            }
            (Which::Toggle, Up) => {
                if e.native {
                    self.toggle_down = false;
                }
                vec![]
            }
            (Which::Hold, Down) => match self.state {
                Idle | StopPending => {
                    self.state = Pressed { since: e.at_ms };
                    vec![Action::Arm]
                }
                TapGap { .. } => {
                    self.state = LatchedHeld;
                    vec![Action::Arm, Action::Start { hands_free: true }]
                }
                Latched | Toggled => {
                    self.state = StopPending;
                    vec![Action::Stop]
                }
                // Auto-repeat while held.
                Pressed { .. } | Holding | LatchedHeld => vec![],
            },
            (Which::Hold, Up) => {
                // A phantom release (synthesised, or for a press never seen)
                // never stops anything.
                if !e.native {
                    return vec![];
                }
                match self.state {
                    Pressed { since } => {
                        let held = e.at_ms.saturating_sub(since);
                        if held >= TAP_MS {
                            // The release beat the timer (a slow mic start):
                            // still a full hold.
                            self.state = Idle;
                            vec![Action::Start { hands_free: false }, Action::Stop]
                        } else if self.latch {
                            self.state = TapGap { at: e.at_ms };
                            vec![Action::Discard]
                        } else {
                            self.state = Idle;
                            vec![Action::Discard]
                        }
                    }
                    Holding => {
                        self.state = Idle;
                        vec![Action::Stop]
                    }
                    LatchedHeld => {
                        self.state = Latched;
                        vec![]
                    }
                    StopPending => {
                        self.state = Idle;
                        vec![]
                    }
                    _ => vec![],
                }
            }
        }
    }
}

/// "Press your shortcut now" (F-DI-11 AC2): passes only after a native
/// key-down and then a native key-up.
#[derive(Debug, Default, Clone)]
pub struct Verifier {
    down: bool,
    passed: bool,
}

impl Verifier {
    /// Feeds an event; true once verified.
    pub fn on(&mut self, e: &KeyEvent) -> bool {
        if e.which != Which::Hold || !e.native {
            return self.passed;
        }
        match e.phase {
            Phase::Down => self.down = true,
            Phase::Up if self.down => self.passed = true,
            Phase::Up => {}
        }
        self.passed
    }

    pub fn passed(&self) -> bool {
        self.passed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(which: Which, phase: Phase, at_ms: u64) -> KeyEvent {
        KeyEvent {
            which,
            phase,
            native: true,
            at_ms,
        }
    }
    fn down(at: u64) -> KeyEvent {
        ev(Which::Hold, Phase::Down, at)
    }
    fn up(at: u64) -> KeyEvent {
        ev(Which::Hold, Phase::Up, at)
    }

    #[test]
    fn f_di_01_hold_records_while_held() {
        let mut g = Gesture::new(true);
        assert_eq!(g.on(down(0)), vec![Action::Arm]);
        assert_eq!(g.tick(149), vec![]);
        assert_eq!(g.tick(150), vec![Action::Start { hands_free: false }]);
        assert_eq!(g.on(down(400)), vec![], "auto-repeat");
        assert_eq!(g.on(up(2000)), vec![Action::Stop]);
        assert!(!g.active());
    }

    #[test]
    fn f_di_01_2_a_100ms_tap_produces_no_recording() {
        let mut g = Gesture::new(false);
        let mut all = g.on(down(0));
        all.extend(g.on(up(100)));
        all.extend(g.tick(1000));
        assert_eq!(all, vec![Action::Arm, Action::Discard]);
        assert!(!all
            .iter()
            .any(|a| matches!(a, Action::Start { .. } | Action::Stop)));
    }

    #[test]
    fn f_di_01_quick_release_during_mic_start_is_a_full_hold() {
        let mut g = Gesture::new(true);
        g.on(down(0));
        // No tick ran (the mic was starting); the release comes at 200 ms.
        assert_eq!(
            g.on(up(200)),
            vec![Action::Start { hands_free: false }, Action::Stop]
        );
    }

    #[test]
    fn f_di_01_double_tap_latches_and_one_tap_stops() {
        let mut g = Gesture::new(true);
        assert_eq!(g.on(down(0)), vec![Action::Arm]);
        assert_eq!(g.on(up(80)), vec![Action::Discard]);
        assert_eq!(
            g.on(down(250)),
            vec![Action::Arm, Action::Start { hands_free: true }]
        );
        assert_eq!(
            g.on(up(320)),
            vec![],
            "releasing doesn't stop a latched take"
        );
        assert!(g.active());
        assert_eq!(g.on(down(5000)), vec![Action::Stop]);
        assert_eq!(g.on(up(5060)), vec![]);
        assert!(!g.active());
        // Too slow a second tap is a new hold.
        let mut g = Gesture::new(true);
        g.on(down(0));
        g.on(up(80));
        assert_eq!(g.on(down(600)), vec![Action::Arm]);
    }

    #[test]
    fn f_di_01_latch_off_means_taps_never_latch() {
        let mut g = Gesture::new(false);
        g.on(down(0));
        g.on(up(80));
        assert_eq!(g.on(down(200)), vec![Action::Arm]);
    }

    #[test]
    fn f_di_01_3_esc_cancels_without_transcribing() {
        let mut g = Gesture::new(true);
        g.on(down(0));
        g.tick(200);
        assert_eq!(g.on(ev(Which::Esc, Phase::Down, 500)), vec![Action::Cancel]);
        assert_eq!(
            g.on(up(900)),
            vec![],
            "the hold key's release stops nothing"
        );
        // Esc while only armed is silent.
        g.on(down(1000));
        assert_eq!(
            g.on(ev(Which::Esc, Phase::Down, 1050)),
            vec![Action::Discard]
        );
        // Esc when idle does nothing.
        let mut idle = Gesture::new(true);
        assert_eq!(idle.on(ev(Which::Esc, Phase::Down, 0)), vec![]);
    }

    #[test]
    fn f_di_01_stops_need_a_real_key_up() {
        let mut g = Gesture::new(true);
        g.on(down(0));
        g.tick(200);
        let phantom = KeyEvent {
            native: false,
            ..up(300)
        };
        assert_eq!(g.on(phantom), vec![]);
        assert!(g.active());
        assert_eq!(g.on(up(400)), vec![Action::Stop]);
        // A release with no press seen (Windows watchdog) does nothing.
        let mut fresh = Gesture::new(true);
        assert_eq!(fresh.on(up(10)), vec![]);
    }

    #[test]
    fn f_di_01_toggle_press_start_press_stop() {
        let mut g = Gesture::new(true);
        let t = |p, at| ev(Which::Toggle, p, at);
        assert_eq!(
            g.on(t(Phase::Down, 0)),
            vec![Action::Arm, Action::Start { hands_free: true }]
        );
        assert_eq!(g.on(t(Phase::Down, 30)), vec![], "auto-repeat");
        g.on(t(Phase::Up, 60));
        assert_eq!(g.on(t(Phase::Down, 3000)), vec![Action::Stop]);
    }

    #[test]
    fn f_di_11_2_verification_needs_press_and_release() {
        let mut v = Verifier::default();
        assert!(!v.on(&up(0)), "a release alone isn't enough");
        assert!(!v.on(&down(10)));
        assert!(!v.on(&KeyEvent {
            native: false,
            ..up(20)
        }));
        assert!(v.on(&up(30)));
        assert!(v.passed());
    }
}
