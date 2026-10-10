//! F-RT-02: the scheduler's decisions against a fake clock.

use chrono::{DateTime, Utc};
use tandem_core::clock::{Clock, MockClock};
use tandem_routines::scheduler::*;
use tandem_routines::{Schedule, ScheduleSpec};

fn utc(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .expect("t")
        .with_timezone(&Utc)
}

/// Ticks every 30 s from `from` to `to`, firing like the app does; returns
/// the instants it fired at and whether each was late.
fn simulate(
    spec: &ScheduleSpec,
    next: Option<i64>,
    clock: &MockClock,
    to: DateTime<Utc>,
) -> Vec<(i64, bool)> {
    let mut next_run_at = next;
    let mut fired = Vec::new();
    while clock.now_utc() <= to {
        let now = clock.now_ms();
        match decide(
            TickState {
                next_run_at,
                paused: false,
                running: false,
            },
            now,
        ) {
            Decision::Schedule => {
                next_run_at = spec
                    .next_after(clock.now_utc())
                    .expect("ok")
                    .map(|t| t.timestamp_millis());
            }
            Decision::Fire { due_at, late } => {
                fired.push((now, late));
                let base = DateTime::from_timestamp_millis(after_firing(due_at, now)).expect("t");
                next_run_at = spec
                    .next_after(base)
                    .expect("ok")
                    .map(|t| t.timestamp_millis());
            }
            Decision::TooOld { due_at } => {
                let base = DateTime::from_timestamp_millis(after_firing(due_at, now)).expect("t");
                next_run_at = spec
                    .next_after(base)
                    .expect("ok")
                    .map(|t| t.timestamp_millis());
            }
            Decision::Skip { .. } | Decision::Wait => {}
        }
        clock.advance(TICK);
    }
    fired
}

fn nine_daily() -> ScheduleSpec {
    ScheduleSpec {
        schedule: Schedule::Daily {
            times: vec!["09:00".into()],
        },
        timezone: "UTC".into(),
    }
}

#[test]
fn f_rt_02_1_a_routine_due_at_t_fires_once_within_30_seconds() {
    let clock = MockClock::new(utc("2026-10-12T08:58:10Z"));
    let fired = simulate(&nine_daily(), None, &clock, utc("2026-10-12T09:10:00Z"));
    assert_eq!(fired.len(), 1, "{fired:?}");
    let t = utc("2026-10-12T09:00:00Z").timestamp_millis();
    assert!((fired[0].0 - t).abs() <= 30_000, "fired at T±30 s");
    assert!(!fired[0].1, "on time");
}

#[test]
fn f_rt_02_2_closed_across_the_due_time_runs_exactly_one_late_run() {
    // Closed at 08:00 with 09:00 stored; reopened at 11:30: three days of
    // ticks after it run once, late, then on time the next day.
    let spec = ScheduleSpec {
        schedule: Schedule::Interval { every_minutes: 30 },
        timezone: "UTC".into(),
    };
    let stored = utc("2026-10-12T09:00:00Z").timestamp_millis();
    let clock = MockClock::new(utc("2026-10-12T11:30:10Z"));
    let fired = simulate(&spec, Some(stored), &clock, utc("2026-10-12T11:45:00Z"));
    assert_eq!(fired.len(), 1, "one catch-up for five missed slots");
    assert!(fired[0].1, "marked late");
    // Daily: missed by two hours → one late run.
    let clock = MockClock::new(utc("2026-10-12T11:00:00Z"));
    let fired = simulate(
        &nine_daily(),
        Some(stored),
        &clock,
        utc("2026-10-13T09:01:00Z"),
    );
    assert_eq!(fired.len(), 2);
    assert!(fired[0].1);
    assert!(!fired[1].1);
    // Missed more than 24 h ago: logged only.
    let clock = MockClock::new(utc("2026-10-13T09:30:00Z"));
    let fired = simulate(
        &nine_daily(),
        Some(stored),
        &clock,
        utc("2026-10-13T10:00:00Z"),
    );
    assert!(fired.is_empty(), "{fired:?}");
}

#[test]
fn f_rt_04_2_a_paused_routine_does_not_fire_and_overlap_is_skipped() {
    let t = utc("2026-10-12T09:00:00Z").timestamp_millis();
    assert_eq!(
        decide(
            TickState {
                next_run_at: Some(t),
                paused: true,
                running: false
            },
            t + 1
        ),
        Decision::Wait
    );
    assert_eq!(
        decide(
            TickState {
                next_run_at: Some(t),
                paused: false,
                running: true
            },
            t + 1
        ),
        Decision::Skip { due_at: t }
    );
    assert_eq!(
        decide(
            TickState {
                next_run_at: Some(t),
                paused: false,
                running: false
            },
            t - 1
        ),
        Decision::Wait
    );
    assert_eq!(
        decide(
            TickState {
                next_run_at: None,
                paused: false,
                running: false
            },
            t
        ),
        Decision::Schedule
    );
    assert_eq!(OVERLAP_REASON, "previous run still in progress");
}
