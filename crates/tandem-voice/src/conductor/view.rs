//! What the HUD, the title-bar ring and the call log show (F-CN-04, UI-61,
//! UI-66): `conductor:state` carries a [`CallView`] to every window.

use serde::{Deserialize, Serialize};
use tandem_core::voice_tools::{Review, VoiceToolGroup};
use ts_rs::TS;

/// The HUD's state label (UI-61 "States").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CallPhase {
    /// No call: the HUD is hidden and the ring idle.
    Idle,
    Connecting,
    Listening,
    /// The doer is planning.
    Thinking,
    /// A tool is running.
    Acting,
    Speaking,
    /// Ended: the receipt shows for 3 s.
    Ended,
    /// Startup or call failure; the reason is in `error`.
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ChipState {
    Running,
    Done,
    Failed,
    /// Waiting on the user (a question).
    Waiting,
}

/// One executed action (F-CN-02: every action shows as a chip).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ChipView {
    pub id: String,
    pub tool: String,
    pub group: VoiceToolGroup,
    /// A short label, middle-truncated to 40 characters.
    pub label: String,
    pub state: ChipState,
    /// Why it failed; never secrets (redacted).
    pub reason: Option<String>,
}

/// A question Conductor asks (UI-61 "Question row"), bound to its id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QuestionView {
    pub request_id: String,
    pub tool: String,
    /// "Run `pnpm test` in Fix auth guard?"
    pub summary: String,
    /// The Yes button's verb: "run it", "close it".
    pub verb: String,
    /// `screen_only`: only a click in the main window answers.
    pub review: Review,
}

/// Why a call couldn't start or had to end (UI-61 "Errors").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CallErrorCode {
    ConnectFailed,
    Offline,
    MicNotDelivering,
    TimeCap,
    OutOfCredits,
    SignedOut,
    NeedsPro,
    TurnedOff,
    RateLimited,
    /// The voice service can't take calls from the user's country (D-1018).
    NotInRegion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CallError {
    pub code: CallErrorCode,
    pub message: String,
}

/// The 3 s receipt that replaces the HUD: "Call ended · 4:12 · 520
/// credits · 6 actions".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Receipt {
    pub call_id: String,
    pub seconds: u32,
    pub credits: u32,
    pub actions: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CallView {
    pub call_id: Option<String>,
    pub phase: CallPhase,
    pub muted: bool,
    /// The last thing the user said.
    pub utterance: Option<String>,
    /// What Conductor is saying.
    pub caption: Option<String>,
    /// Every action this call, oldest first (the HUD shows the newest 3).
    pub chips: Vec<ChipView>,
    pub question: Option<QuestionView>,
    /// Connected time so far.
    pub elapsed_ms: i64,
    pub max_seconds: u32,
    pub error: Option<CallError>,
    /// "2 minutes left in this call", "This call reached its 30-minute limit."
    pub notice: Option<String>,
    pub receipt: Option<Receipt>,
}

impl Default for CallView {
    fn default() -> Self {
        Self {
            call_id: None,
            phase: CallPhase::Idle,
            muted: false,
            utterance: None,
            caption: None,
            chips: Vec::new(),
            question: None,
            elapsed_ms: 0,
            max_seconds: 0,
            error: None,
            notice: None,
            receipt: None,
        }
    }
}

/// `conductor:ui-action`: a navigation-only tool the UI performs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UiAction {
    pub action: String,
    #[ts(type = "Record<string, unknown>")]
    pub args: serde_json::Value,
}

/// Credits a call of `seconds` costs: 20 per started 10 s (PRODUCT_SPEC
/// §3.3, F-CN-01 AC1).
pub fn credits_for(seconds: u32) -> u32 {
    seconds.div_ceil(10) * PRICE_PER_10S
}

pub const PRICE_PER_10S: u32 = 20;

/// "Fix the auth guard in the billing module" → "Fix the auth gu…ling module".
pub fn middle_truncate(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max || max < 2 {
        return s.to_string();
    }
    let head = (max - 1) / 2 + (max - 1) % 2;
    let tail = max - 1 - head;
    let chars: Vec<char> = s.chars().collect();
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[n - tail..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cn_01_1_a_65_second_call_costs_140_credits() {
        assert_eq!(credits_for(65), 140);
        assert_eq!(credits_for(0), 0);
        assert_eq!(credits_for(1), 20);
        assert_eq!(credits_for(10), 20);
        assert_eq!(credits_for(11), 40);
        // The 30-minute cap: 3,600.
        assert_eq!(credits_for(1800), 3600);
    }

    #[test]
    fn f_cn_04_long_labels_are_middle_truncated() {
        let s = "Prompting Fix auth guard: run the unit tests and report failures";
        let t = middle_truncate(s, 40);
        assert_eq!(t.chars().count(), 40);
        assert!(t.starts_with("Prompting Fix auth"));
        assert!(t.ends_with("report failures"));
        assert!(t.contains('…'));
        assert_eq!(middle_truncate("short", 40), "short");
    }
}
