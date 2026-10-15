//! Usage forecast (F-AC-08): when a usage window will reach 100% at the
//! current pace, from the readings the usage probe already takes (F-AC-04).
//! Pure arithmetic over in-memory samples; nothing is read or sent.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::usage::UsageWindow;

/// Samples older than this are dropped.
pub const KEEP_MS: i64 = 2 * 60 * 60_000;
/// At most this many samples per window.
pub const MAX_SAMPLES: usize = 120;
/// The forecast reads the readings of this span.
pub const BASIS_MS: i64 = 60 * 60_000;
/// Less than this between the first and the last reading is too little.
pub const MIN_SPAN_MS: i64 = 10 * 60_000;
/// A fall of this many points means the window reset.
const RESET_DROP: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub at: i64,
    pub used_pct: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ForecastState {
    NotEnoughData,
    OnTrack,
    RunsOut,
}

/// One window's forecast, beside the window in `UsageView`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WindowForecast {
    pub kind: String,
    pub state: ForecastState,
    /// Unix ms when the window reaches 100% (runs out only).
    pub runs_out_at: Option<i64>,
    /// How long before the reset it runs out (runs out with a reset only).
    pub before_reset_ms: Option<i64>,
    /// Points an hour, when there's enough data.
    pub rate_per_hour: Option<f64>,
    /// The minutes the forecast is based on.
    pub basis_minutes: u32,
}

impl WindowForecast {
    /// Runs out before the window resets (or with no reset known).
    pub fn runs_out_early(&self) -> bool {
        self.state == ForecastState::RunsOut
    }
}

/// The forecast from `samples` (oldest first) for a window resetting at
/// `resets_at`, as of `now`.
pub fn forecast(
    kind: &str,
    samples: &[Sample],
    resets_at: Option<i64>,
    now: i64,
) -> WindowForecast {
    let recent: Vec<&Sample> = samples
        .iter()
        .filter(|s| s.at >= now - BASIS_MS && s.at <= now)
        .collect();
    let mut out = WindowForecast {
        kind: kind.to_string(),
        state: ForecastState::NotEnoughData,
        runs_out_at: None,
        before_reset_ms: None,
        rate_per_hour: None,
        basis_minutes: 0,
    };
    let (Some(first), Some(last)) = (recent.first(), recent.last()) else {
        return out;
    };
    let span = last.at - first.at;
    out.basis_minutes = u32::try_from(span / 60_000).unwrap_or(u32::MAX);
    if recent.len() < 2 || span < MIN_SPAN_MS {
        return out;
    }
    let rate = (last.used_pct - first.used_pct) / (span as f64 / 3_600_000.0);
    out.rate_per_hour = Some(rate);
    if rate <= 0.0 {
        out.state = ForecastState::OnTrack;
        return out;
    }
    let left = (100.0 - last.used_pct).max(0.0);
    let at = last.at + (left / rate * 3_600_000.0).round() as i64;
    match resets_at {
        Some(reset) if at >= reset => out.state = ForecastState::OnTrack,
        Some(reset) => {
            out.state = ForecastState::RunsOut;
            out.runs_out_at = Some(at);
            out.before_reset_ms = Some(reset - at);
        }
        None => {
            out.state = ForecastState::RunsOut;
            out.runs_out_at = Some(at);
        }
    }
    out
}

#[derive(Debug, Default)]
struct Track {
    resets_at: Option<i64>,
    samples: VecDeque<Sample>,
}

/// Samples per `(account, window kind)`.
#[derive(Debug, Default)]
pub struct UsageHistory {
    tracks: HashMap<(String, String), Track>,
}

impl UsageHistory {
    /// Records a reading of `windows` for `account` at `now`, and returns
    /// each window's forecast.
    pub fn record(
        &mut self,
        account: &str,
        windows: &[UsageWindow],
        now: i64,
    ) -> Vec<WindowForecast> {
        windows
            .iter()
            .map(|w| {
                let track = self
                    .tracks
                    .entry((account.to_string(), w.kind.clone()))
                    .or_default();
                let moved = match (track.resets_at, w.resets_at) {
                    (Some(old), Some(new)) => new > old + 60_000,
                    _ => false,
                };
                let fell = track
                    .samples
                    .back()
                    .is_some_and(|s| s.used_pct - w.used_pct >= RESET_DROP);
                if moved || fell {
                    track.samples.clear();
                }
                track.resets_at = w.resets_at;
                track.samples.push_back(Sample {
                    at: now,
                    used_pct: w.used_pct,
                });
                while track.samples.front().is_some_and(|s| s.at < now - KEEP_MS)
                    || track.samples.len() > MAX_SAMPLES
                {
                    track.samples.pop_front();
                }
                let samples: Vec<Sample> = track.samples.iter().copied().collect();
                forecast(&w.kind, &samples, w.resets_at, now)
            })
            .collect()
    }

    /// Moves every sample `ms` into the past (the test harness, to stand in
    /// for time passing between two readings).
    pub fn backdate(&mut self, ms: i64) {
        for track in self.tracks.values_mut() {
            for s in &mut track.samples {
                s.at -= ms;
            }
        }
    }

    /// Forgets an account (removed).
    pub fn forget(&mut self, account: &str) {
        self.tracks.retain(|(a, _), _| a != account);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;
    const T0: i64 = 1_790_000_000_000;

    fn win(kind: &str, used: f64, resets_at: Option<i64>) -> UsageWindow {
        UsageWindow {
            kind: kind.into(),
            used_pct: used,
            resets_at,
        }
    }

    #[test]
    fn f_ac_08_1_forty_then_sixty_in_half_an_hour_runs_out_an_hour_later() {
        let mut h = UsageHistory::default();
        let reset = Some(T0 + 120 * MIN);
        let first = h.record("acc", &[win("5h", 40.0, reset)], T0);
        assert_eq!(first[0].state, ForecastState::NotEnoughData);
        let f = h.record("acc", &[win("5h", 60.0, reset)], T0 + 30 * MIN);
        assert_eq!(f[0].state, ForecastState::RunsOut);
        assert_eq!(f[0].runs_out_at, Some(T0 + 90 * MIN));
        assert_eq!(f[0].before_reset_ms, Some(30 * MIN));
        assert_eq!(f[0].rate_per_hour, Some(40.0));
        assert_eq!(f[0].basis_minutes, 30);
        assert!(f[0].runs_out_early());
    }

    #[test]
    fn f_ac_08_2_too_little_data_flat_use_and_a_late_run_out_are_not_warnings() {
        let one = [Sample {
            at: T0,
            used_pct: 10.0,
        }];
        assert_eq!(
            forecast("5h", &one, None, T0).state,
            ForecastState::NotEnoughData
        );
        let close = [
            Sample {
                at: T0,
                used_pct: 10.0,
            },
            Sample {
                at: T0 + 9 * MIN,
                used_pct: 30.0,
            },
        ];
        assert_eq!(
            forecast("5h", &close, None, T0 + 9 * MIN).state,
            ForecastState::NotEnoughData
        );
        let flat = [
            Sample {
                at: T0,
                used_pct: 30.0,
            },
            Sample {
                at: T0 + 20 * MIN,
                used_pct: 30.0,
            },
        ];
        assert_eq!(
            forecast("5h", &flat, Some(T0 + 60 * MIN), T0 + 20 * MIN).state,
            ForecastState::OnTrack
        );
        // 10 points an hour from 50%: 100% five hours on, after the reset.
        let slow = [
            Sample {
                at: T0,
                used_pct: 45.0,
            },
            Sample {
                at: T0 + 30 * MIN,
                used_pct: 50.0,
            },
        ];
        let f = forecast("5h", &slow, Some(T0 + 120 * MIN), T0 + 30 * MIN);
        assert_eq!(f.state, ForecastState::OnTrack);
        assert_eq!(f.runs_out_at, None);
        // Readings older than the basis don't count.
        let old = [
            Sample {
                at: T0,
                used_pct: 0.0,
            },
            Sample {
                at: T0 + 90 * MIN,
                used_pct: 50.0,
            },
        ];
        assert_eq!(
            forecast("5h", &old, None, T0 + 90 * MIN).state,
            ForecastState::NotEnoughData
        );
        // With no reset time, a rise runs out.
        let f = forecast("week", &slow, None, T0 + 30 * MIN);
        assert_eq!(f.state, ForecastState::RunsOut);
        assert_eq!(f.before_reset_ms, None);
    }

    #[test]
    fn f_ac_08_3_a_reset_that_moved_later_or_a_fall_starts_a_new_window() {
        let mut h = UsageHistory::default();
        h.record("acc", &[win("5h", 80.0, Some(T0 + 10 * MIN))], T0);
        let f = h.record(
            "acc",
            &[win("5h", 90.0, Some(T0 + 300 * MIN))],
            T0 + 20 * MIN,
        );
        assert_eq!(f[0].state, ForecastState::NotEnoughData);
        assert_eq!(f[0].basis_minutes, 0);
        // A fall of 5 points or more, same reset time.
        let mut h = UsageHistory::default();
        h.record("acc", &[win("week", 60.0, None)], T0);
        let f = h.record("acc", &[win("week", 2.0, None)], T0 + 20 * MIN);
        assert_eq!(f[0].state, ForecastState::NotEnoughData);
        // Accounts and windows are tracked apart.
        let mut h = UsageHistory::default();
        h.record("a", &[win("5h", 10.0, None), win("week", 10.0, None)], T0);
        let f = h.record("b", &[win("5h", 50.0, None)], T0 + 20 * MIN);
        assert_eq!(f[0].state, ForecastState::NotEnoughData);
        h.forget("a");
        assert!(h.tracks.keys().all(|(a, _)| a != "a"));
    }

    #[test]
    fn f_ac_08_samples_are_capped_by_age_and_count() {
        let mut h = UsageHistory::default();
        for i in 0..200 {
            h.record("acc", &[win("5h", (i as f64) * 0.1, None)], T0 + i * MIN);
        }
        let track = &h.tracks[&("acc".to_string(), "5h".to_string())];
        assert!(track.samples.len() <= MAX_SAMPLES);
        assert!(track
            .samples
            .front()
            .is_some_and(|s| s.at >= T0 + 199 * MIN - KEEP_MS));
    }
}
