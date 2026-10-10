//! The take state machine (F-DI-06 fail-closed, ARCHITECTURE §12.3).
//!
//! Every asynchronous result carries the take id it belongs to. Anything for
//! a take that isn't current any more (a newer take started, it was
//! cancelled, it already finished) is refused, so a late transcript or a
//! stale timer can never paste.

use serde::{Deserialize, Serialize};
use tandem_core::ids::TakeId;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TakeState {
    Idle,
    Arming,
    Recording,
    Transcribing,
    PostProcessing,
    Inserting,
    Done,
    Cancelled,
    Failed,
}

impl TakeState {
    fn rank(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Arming => 1,
            Self::Recording => 2,
            Self::Transcribing => 3,
            Self::PostProcessing => 4,
            Self::Inserting => 5,
            Self::Done | Self::Cancelled | Self::Failed => 6,
        }
    }

    pub fn is_final(self) -> bool {
        self.rank() == 6
    }
}

/// Where a take's text goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export, rename = "TakeOrigin")]
pub enum Origin {
    /// The focused app, captured at Arming.
    Global,
    /// A composer in Tandem's main window: inserted at its caret, no clipboard.
    Composer {
        #[serde(rename = "targetId")]
        target_id: String,
    },
}

#[derive(Debug, Default)]
pub struct TakeMachine {
    current: Option<(TakeId, Origin, TakeState)>,
}

impl TakeMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a take. Whatever the previous take was doing is cancelled, so
    /// its pending insertion can never land.
    pub fn begin(&mut self, origin: Origin) -> (TakeId, Option<TakeId>) {
        let replaced = self
            .current
            .take()
            .filter(|(_, _, s)| !s.is_final())
            .map(|(id, _, _)| id);
        let id = TakeId::new();
        self.current = Some((id.clone(), origin, TakeState::Arming));
        (id, replaced)
    }

    pub fn current(&self) -> Option<&TakeId> {
        self.current
            .as_ref()
            .filter(|(_, _, s)| !s.is_final())
            .map(|(id, _, _)| id)
    }

    pub fn origin(&self, id: &TakeId) -> Option<&Origin> {
        self.current
            .as_ref()
            .filter(|(cur, _, _)| cur == id)
            .map(|(_, o, _)| o)
    }

    pub fn state(&self) -> TakeState {
        self.current
            .as_ref()
            .map(|(_, _, s)| *s)
            .unwrap_or(TakeState::Idle)
    }

    pub fn is_current(&self, id: &TakeId) -> bool {
        self.current() == Some(id)
    }

    /// Moves `id` forward. Refused (false) for a take that isn't current, a
    /// finished take, or a step backwards.
    pub fn advance(&mut self, id: &TakeId, to: TakeState) -> bool {
        match &mut self.current {
            Some((cur, _, state)) if cur == id && !state.is_final() && to.rank() > state.rank() => {
                *state = to;
                true
            }
            _ => false,
        }
    }

    /// Cancels the current take (or only `id`, when given). True if one was.
    pub fn cancel(&mut self, id: Option<&TakeId>) -> bool {
        match &mut self.current {
            Some((cur, _, state)) if !state.is_final() && id.is_none_or(|i| i == cur) => {
                *state = TakeState::Cancelled;
                true
            }
            _ => false,
        }
    }

    pub fn fail(&mut self, id: &TakeId) -> bool {
        self.advance(id, TakeState::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_di_06_1_a_late_transcript_from_take_n_is_discarded() {
        let mut m = TakeMachine::new();
        let (n, _) = m.begin(Origin::Global);
        assert!(m.advance(&n, TakeState::Recording));
        assert!(m.advance(&n, TakeState::Transcribing));
        let (n1, replaced) = m.begin(Origin::Global);
        assert_eq!(replaced.as_ref(), Some(&n));
        // Take N's transcript arrives now: refused at every later step.
        assert!(!m.advance(&n, TakeState::PostProcessing));
        assert!(!m.advance(&n, TakeState::Inserting));
        assert!(!m.is_current(&n));
        assert!(m.is_current(&n1));
        assert_eq!(m.state(), TakeState::Arming);
    }

    #[test]
    fn f_di_06_cancelled_and_finished_takes_never_insert() {
        let mut m = TakeMachine::new();
        let (a, _) = m.begin(Origin::Global);
        assert!(m.advance(&a, TakeState::Recording));
        assert!(m.cancel(None));
        assert!(!m.advance(&a, TakeState::Inserting));
        assert_eq!(m.current(), None);
        let (b, replaced) = m.begin(Origin::Composer {
            target_id: "thread:1".into(),
        });
        assert_eq!(replaced, None, "a finished take isn't 'replaced'");
        assert!(m.advance(&b, TakeState::Inserting));
        assert!(m.advance(&b, TakeState::Done));
        assert!(!m.advance(&b, TakeState::Inserting), "no step backwards");
        assert!(!m.cancel(Some(&b)));
        assert_eq!(m.state(), TakeState::Done);
    }

    #[test]
    fn f_di_06_cancel_by_id_only_hits_that_take() {
        let mut m = TakeMachine::new();
        let (a, _) = m.begin(Origin::Global);
        let (b, _) = m.begin(Origin::Global);
        assert!(!m.cancel(Some(&a)));
        assert!(m.is_current(&b));
        assert!(matches!(m.origin(&b), Some(Origin::Global)));
        assert!(m.origin(&a).is_none());
    }
}
