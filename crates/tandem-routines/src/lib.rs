//! Routines (ARCHITECTURE §2.9, §10): schedules (cron, interval, daily,
//! weekly) with DST-correct next runs, the routine model's rules, the
//! scheduler and the OS launchers for background runs.

pub mod cron;
pub mod launcher;
pub mod model;
pub mod schedule;
pub mod scheduler;

pub use model::*;
pub use schedule::{Schedule, ScheduleSpec, ScheduleWords};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    /// A message for the form, in words.
    #[error("{0}")]
    Invalid(String),
}
