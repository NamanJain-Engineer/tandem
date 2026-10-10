//! What a thread may do (F-TH-09): the access level and the Plan/Act mode,
//! stored on `threads.access` / `threads.mode`, and [`resolve`], the one
//! place that turns them into the access an engine session gets.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// "Read only", "Edits" or "Full access".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Access {
    Read,
    Edits,
    Full,
}

/// The composer's "Plan | Act".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ThreadMode {
    Plan,
    Act,
}

impl Access {
    pub fn as_str(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Edits => "edits",
            Access::Full => "full",
        }
    }

    pub fn parse(s: &str) -> Option<Access> {
        match s {
            "read" => Some(Access::Read),
            "edits" => Some(Access::Edits),
            "full" => Some(Access::Full),
            _ => None,
        }
    }
}

impl ThreadMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ThreadMode::Plan => "plan",
            ThreadMode::Act => "act",
        }
    }

    pub fn parse(s: &str) -> Option<ThreadMode> {
        match s {
            "plan" => Some(ThreadMode::Plan),
            "act" => Some(ThreadMode::Act),
            _ => None,
        }
    }
}

/// Who started a run (F-TH-09 "unattended rule").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A live user action.
    Interactive,
    /// A routine run; `grants_full` is the routine's own explicit setting.
    Routine { grants_full: bool },
    /// A task run; `grants_full` is the task's own explicit setting.
    Task { grants_full: bool },
    /// A teammate chat started by mail.
    Mail,
    /// An Auto (Conductor) run.
    Auto,
}

impl Origin {
    pub fn attended(self) -> bool {
        self == Origin::Interactive
    }
}

/// The access a session runs with: Plan mode reads only; an unattended run
/// never inherits Full, unless its routine or task explicitly grants it
/// (`invariant_rule_4_unattended_never_full`).
pub fn resolve(level: Access, mode: ThreadMode, origin: Origin) -> Access {
    if mode == ThreadMode::Plan {
        return Access::Read;
    }
    match (level, origin) {
        (Access::Full, Origin::Interactive)
        | (Access::Full, Origin::Routine { grants_full: true })
        | (Access::Full, Origin::Task { grants_full: true }) => Access::Full,
        (Access::Full, _) => Access::Edits,
        (level, _) => level,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_th_09_access_and_mode_round_trip_their_column_values() {
        for a in [Access::Read, Access::Edits, Access::Full] {
            assert_eq!(Access::parse(a.as_str()), Some(a));
            assert_eq!(serde_json::to_value(a).expect("json"), a.as_str());
        }
        for m in [ThreadMode::Plan, ThreadMode::Act] {
            assert_eq!(ThreadMode::parse(m.as_str()), Some(m));
        }
        assert_eq!(Access::parse("admin"), None);
    }

    // F-TL-06 rule 4 is tests/invariants/tests/rule_4.rs.

    #[test]
    fn f_th_09_2_plan_mode_reads_only_at_any_level() {
        for level in [Access::Read, Access::Edits, Access::Full] {
            assert_eq!(
                resolve(level, ThreadMode::Plan, Origin::Interactive),
                Access::Read
            );
        }
    }
}
