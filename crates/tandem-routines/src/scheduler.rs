//! What the scheduler does with a routine on a tick (F-RT-02, ARCHITECTURE
//! §10.2). Pure, so the clock and the store stay with the caller: the app
//! ticks every [`TICK`] against its `Clock` and acts on the decision.

use std::time::Duration;

/// How often the scheduler looks for due routines.
pub const TICK: Duration = Duration::from_secs(30);
/// A run that starts more than this after its slot is "late" (the app was
/// closed or the machine asleep across it).
pub const LATE_AFTER_MS: i64 = 2 * 60 * 1000;
/// A missed slot older than this is only logged, never run.
pub const CATCH_UP_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;
/// What an overlapping slot is skipped with.
pub const OVERLAP_REASON: &str = "previous run still in progress";

/// A routine as the scheduler sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickState {
    pub next_run_at: Option<i64>,
    /// Paused by the user or by the plan.
    pub paused: bool,
    /// A run of it is still going.
    pub running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Not due (or paused): nothing to do.
    Wait,
    /// No next run stored yet: compute one from now.
    Schedule,
    /// Run the slot `due_at`; `late` when it's a catch-up.
    Fire { due_at: i64, late: bool },
    /// The previous run is still going: record the slot as skipped.
    Skip { due_at: i64 },
    /// Missed more than a day ago: log it and move on.
    TooOld { due_at: i64 },
}

pub fn decide(r: TickState, now: i64) -> Decision {
    if r.paused {
        return Decision::Wait;
    }
    let Some(due_at) = r.next_run_at else {
        return Decision::Schedule;
    };
    if due_at > now {
        return Decision::Wait;
    }
    if now - due_at > CATCH_UP_WINDOW_MS {
        return Decision::TooOld { due_at };
    }
    if r.running {
        return Decision::Skip { due_at };
    }
    Decision::Fire {
        due_at,
        late: now - due_at > LATE_AFTER_MS,
    }
}

/// The slot after one fired: the next run after both the slot and now, so
/// any number of missed slots catch up at most once.
pub fn after_firing(due_at: i64, now: i64) -> i64 {
    due_at.max(now)
}
