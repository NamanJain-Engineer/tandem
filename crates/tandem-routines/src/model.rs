//! The routine model's rules (F-RT-01).

use serde::{Deserialize, Serialize};
use tandem_core::access::Access;
use ts_rs::TS;

use crate::schedule::ScheduleSpec;
use crate::ScheduleError;

/// An instruction is at most this many characters.
pub const INSTRUCTION_MAX: usize = 8_000;
pub const NAME_MAX: usize = 80;
/// What the form says when Full access is picked.
pub const FULL_WARNING: &str = "This routine will act without asking.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RunMode {
    /// Runs while the app is open.
    InApp,
    /// Also runs from the OS launcher when the app is closed (F-RT-03).
    Background,
}

impl RunMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RunMode::InApp => "in_app",
            RunMode::Background => "background",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "in_app" => Some(RunMode::InApp),
            "background" => Some(RunMode::Background),
            _ => None,
        }
    }
}

/// What the New routine form (or an agent's schedule tool) sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoutineInput {
    /// Set to edit an existing routine.
    #[ts(optional)]
    pub id: Option<String>,
    pub teammate_id: String,
    pub name: String,
    pub instruction: String,
    pub schedule: ScheduleSpec,
    pub access: Access,
    /// Full access only with this set: the user saw "This routine will act
    /// without asking." and chose it for this routine.
    #[serde(default)]
    pub full_confirmed: bool,
    pub run_mode: RunMode,
}

impl RoutineInput {
    /// Checks the fields; returns the trimmed name.
    pub fn validate(&self) -> Result<String, ScheduleError> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > NAME_MAX {
            return Err(ScheduleError::Invalid(format!(
                "A name is 1–{NAME_MAX} characters"
            )));
        }
        if self.instruction.trim().is_empty() {
            return Err(ScheduleError::Invalid(
                "Say what the routine should do".into(),
            ));
        }
        if self.instruction.chars().count() > INSTRUCTION_MAX {
            return Err(ScheduleError::Invalid(format!(
                "An instruction is at most {} characters",
                "8,000"
            )));
        }
        if self.teammate_id.trim().is_empty() {
            return Err(ScheduleError::Invalid("Pick a teammate".into()));
        }
        if self.access == Access::Full && !self.full_confirmed {
            return Err(ScheduleError::Invalid(format!(
                "Full access must be chosen for this routine: {FULL_WARNING}"
            )));
        }
        self.schedule.resolve()?;
        Ok(name.to_string())
    }
}
