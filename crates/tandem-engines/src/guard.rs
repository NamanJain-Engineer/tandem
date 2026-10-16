//! Runaway guard (F-AC-09): per-agent limits on time spent working, share
//! of the account's window, subagents started and turns that end the same
//! way. Pure state over the status reports the app already receives; it
//! keeps hashes and counts only, never screen text.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The limits, each `None` when turned off (setting `runaway_guard`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GuardLimits {
    pub working_minutes: Option<u32>,
    pub window_points: Option<f64>,
    pub subagents: Option<u32>,
    pub circles: bool,
}

impl Default for GuardLimits {
    fn default() -> Self {
        Self {
            working_minutes: Some(90),
            window_points: Some(30.0),
            subagents: Some(20),
            circles: true,
        }
    }
}

impl GuardLimits {
    /// Reads the setting's JSON; missing or invalid fields keep their
    /// defaults, `null` turns a limit off, and values are clamped to range.
    pub fn from_json(v: &serde_json::Value) -> Self {
        let d = Self::default();
        let field = |k: &str| v.get(k);
        let num = |k: &str, lo: f64, hi: f64, def: Option<f64>| match field(k) {
            Some(serde_json::Value::Null) => None,
            Some(x) => x.as_f64().map(|n| n.clamp(lo, hi)).or(def),
            None => def,
        };
        Self {
            working_minutes: num(
                "workingMinutes",
                10.0,
                1_440.0,
                d.working_minutes.map(f64::from),
            )
            .map(|n| n.round() as u32),
            window_points: num("windowPoints", 5.0, 100.0, d.window_points),
            subagents: num("subagents", 1.0, 500.0, d.subagents.map(f64::from))
                .map(|n| n.round() as u32),
            circles: field("circles")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(d.circles),
        }
    }
}

/// Which limit tripped, with what it measured.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Trip {
    Working { minutes: u32 },
    Window { points: f64 },
    Subagents { count: u32 },
    Circles { turns: u32 },
}

impl Trip {
    fn key(&self) -> &'static str {
        match self {
            Trip::Working { .. } => "working",
            Trip::Window { .. } => "window",
            Trip::Subagents { .. } => "subagents",
            Trip::Circles { .. } => "circles",
        }
    }
}

/// Turns compared for "going in circles".
const CIRCLE_TURNS: usize = 3;

/// One agent's guard state.
#[derive(Debug, Default, Clone)]
pub struct PaneGuard {
    working_since: Option<i64>,
    subagents: u32,
    ends: Vec<u64>,
    /// Limits that tripped (or were dismissed) this turn.
    quiet: Vec<&'static str>,
}

impl PaneGuard {
    /// The user (or Vibezeo for them) sent a prompt: a new turn.
    pub fn prompt(&mut self, now: i64) {
        self.working_since = Some(now);
        self.subagents = 0;
        self.quiet.clear();
    }

    /// The agent reported Working.
    pub fn working(&mut self, now: i64) {
        if self.working_since.is_none() {
            self.working_since = Some(now);
        }
    }

    /// The agent stopped working without a turn ending (waiting, idle).
    pub fn stopped(&mut self) {
        self.working_since = None;
    }

    /// A tool report: `subagent` when the tool starts a subagent.
    pub fn tool(&mut self, subagent: bool, limits: &GuardLimits) -> Option<Trip> {
        if !subagent {
            return None;
        }
        self.subagents += 1;
        let limit = limits.subagents?;
        (self.subagents >= limit)
            .then_some(Trip::Subagents {
                count: self.subagents,
            })
            .and_then(|t| self.fire(t))
    }

    /// The turn ended, with a hash of how the screen ended.
    pub fn turn_end(&mut self, screen_hash: Option<u64>, limits: &GuardLimits) -> Option<Trip> {
        self.working_since = None;
        let Some(hash) = screen_hash else {
            self.ends.clear();
            return None;
        };
        self.ends.push(hash);
        if self.ends.len() > CIRCLE_TURNS {
            self.ends.remove(0);
        }
        let circling = limits.circles
            && self.ends.len() == CIRCLE_TURNS
            && self.ends.iter().all(|h| *h == hash);
        // The next turn starts fresh for the per-turn limits, but a circle
        // is about the turns themselves: report it now.
        self.quiet.retain(|k| *k == "circles");
        if circling {
            self.ends.clear();
            return self.fire(Trip::Circles {
                turns: CIRCLE_TURNS as u32,
            });
        }
        None
    }

    /// The periodic check: time working, and this session's points of its
    /// account's window in the last hour (when known).
    pub fn tick(
        &mut self,
        now: i64,
        window_points: Option<f64>,
        limits: &GuardLimits,
    ) -> Vec<Trip> {
        let mut out = Vec::new();
        if let (Some(since), Some(limit)) = (self.working_since, limits.working_minutes) {
            let minutes = ((now - since).max(0) / 60_000) as u32;
            if minutes >= limit {
                out.extend(self.fire(Trip::Working { minutes: limit }));
            }
        }
        if let (Some(points), Some(limit)) = (window_points, limits.window_points) {
            if points >= limit {
                out.extend(self.fire(Trip::Window { points }));
            }
        }
        out
    }

    /// Keeps a limit quiet until the next prompt.
    pub fn dismiss(&mut self, kind: &str) {
        for k in ["working", "window", "subagents", "circles"] {
            if k == kind && !self.quiet.contains(&k) {
                self.quiet.push(k);
            }
        }
    }

    fn fire(&mut self, trip: Trip) -> Option<Trip> {
        let key = trip.key();
        if self.quiet.contains(&key) {
            return None;
        }
        self.quiet.push(key);
        Some(trip)
    }
}

/// A turn's ending, as a hash of its last 12 non-empty screen lines.
pub fn screen_hash(screen: &str) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let lines: Vec<&str> = screen
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let tail = &lines[lines.len().saturating_sub(12)..];
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tail.hash(&mut h);
    Some(h.finish())
}

/// This session's points of its account's window: its share of the
/// account's tokens in the last hour times the window's rise over it.
pub fn window_points(share: f64, rise: f64) -> f64 {
    (share.clamp(0.0, 1.0) * rise.max(0.0) * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;

    #[test]
    fn f_ac_09_1_ninety_minutes_working_trips_once_per_turn() {
        let limits = GuardLimits::default();
        let mut g = PaneGuard::default();
        g.prompt(0);
        assert!(g.tick(89 * MIN, None, &limits).is_empty());
        assert_eq!(
            g.tick(91 * MIN, None, &limits),
            vec![Trip::Working { minutes: 90 }]
        );
        assert!(g.tick(120 * MIN, None, &limits).is_empty(), "once per turn");
        // The next prompt is a new turn.
        g.prompt(200 * MIN);
        assert!(g.tick(250 * MIN, None, &limits).is_empty());
        assert_eq!(g.tick(291 * MIN, None, &limits).len(), 1);
        // Waiting for the user stops the clock.
        let mut g = PaneGuard::default();
        g.working(0);
        g.stopped();
        assert!(g.tick(100 * MIN, None, &limits).is_empty());
    }

    #[test]
    fn f_ac_09_2_the_twentieth_subagent_trips() {
        let limits = GuardLimits::default();
        let mut g = PaneGuard::default();
        g.prompt(0);
        for _ in 0..19 {
            assert_eq!(g.tool(true, &limits), None);
        }
        assert_eq!(g.tool(false, &limits), None, "other tools don't count");
        assert_eq!(g.tool(true, &limits), Some(Trip::Subagents { count: 20 }));
        assert_eq!(g.tool(true, &limits), None);
        g.prompt(1);
        assert_eq!(g.tool(true, &limits), None, "the count starts again");
    }

    #[test]
    fn f_ac_09_3_three_turns_ending_the_same_way_are_circles() {
        let limits = GuardLimits::default();
        let same = screen_hash("> I'll try again\n\nError: test failed\n");
        let other = screen_hash("All tests pass");
        let mut g = PaneGuard::default();
        assert_eq!(g.turn_end(same, &limits), None);
        assert_eq!(g.turn_end(same, &limits), None);
        assert_eq!(g.turn_end(same, &limits), Some(Trip::Circles { turns: 3 }));
        let mut g = PaneGuard::default();
        assert_eq!(g.turn_end(same, &limits), None);
        assert_eq!(g.turn_end(other, &limits), None);
        assert_eq!(g.turn_end(same, &limits), None);
        // Trailing blanks and spaces don't make turns differ.
        assert_eq!(screen_hash("a\nb  \n\n"), screen_hash("a\nb"));
        assert_eq!(screen_hash("  \n\n"), None);
    }

    #[test]
    fn f_ac_09_4_window_points_are_share_times_rise() {
        let limits = GuardLimits::default();
        assert_eq!(window_points(0.75, 40.0), 30.0);
        let mut g = PaneGuard::default();
        g.prompt(0);
        assert!(g.tick(MIN, Some(29.9), &limits).is_empty());
        assert_eq!(
            g.tick(2 * MIN, Some(30.0), &limits),
            vec![Trip::Window { points: 30.0 }]
        );
        assert!(
            g.tick(3 * MIN, None, &limits).is_empty(),
            "no windows: never"
        );
    }

    #[test]
    fn f_ac_09_5_a_dismissed_limit_stays_quiet_until_the_next_prompt() {
        let limits = GuardLimits::default();
        let mut g = PaneGuard::default();
        g.prompt(0);
        g.dismiss("working");
        assert!(g.tick(100 * MIN, None, &limits).is_empty());
        g.prompt(200 * MIN);
        assert_eq!(g.tick(300 * MIN, None, &limits).len(), 1);
    }

    #[test]
    fn f_ac_09_6_limits_turned_off_never_trip_and_settings_are_clamped() {
        let off = GuardLimits::from_json(&serde_json::json!({
            "workingMinutes": null, "windowPoints": null, "subagents": null, "circles": false
        }));
        let mut g = PaneGuard::default();
        g.prompt(0);
        assert!(g.tick(10_000 * MIN, Some(100.0), &off).is_empty());
        for _ in 0..600 {
            assert_eq!(g.tool(true, &off), None);
        }
        let h = screen_hash("x");
        for _ in 0..5 {
            assert_eq!(g.turn_end(h, &off), None);
        }
        let clamped =
            GuardLimits::from_json(&serde_json::json!({"workingMinutes": 1, "subagents": 9999}));
        assert_eq!(clamped.working_minutes, Some(10));
        assert_eq!(clamped.subagents, Some(500));
        assert_eq!(clamped.window_points, Some(30.0));
        assert_eq!(
            GuardLimits::from_json(&serde_json::Value::Null),
            GuardLimits::default()
        );
    }
}
