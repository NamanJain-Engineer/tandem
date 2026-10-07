//! Status reports from engine hooks (ARCHITECTURE §6.6, F-EN-05).
//!
//! `tandem-hook` posts a small JSON body signed with the pane's per-launch
//! secret; the status server checks the signature before anything else, then
//! the timestamp window, then turns the report into a [`StatusUpdate`].

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use ts_rs::TS;

use crate::status::{AgentStatus, StatusUpdate, WaitingReason};

pub const SIGNATURE_HEADER: &str = "x-tandem-signature";
/// Larger bodies are refused unread (§6.6: ≤ 4 KB).
pub const MAX_REPORT_BYTES: usize = 4 * 1024;
/// Reports older than this are refused (replays).
pub const MAX_AGE_MS: i64 = 10 * 60 * 1000;
/// Reports this far in the future are refused (clock games).
pub const MAX_SKEW_MS: i64 = 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ReportEvent {
    Working,
    Waiting,
    Done,
    Idle,
    Failed,
    /// The engine named the work (title only, status unchanged).
    Title,
    /// The engine's own session id, for resume.
    Session,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReportPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<WaitingReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
}

fn v1() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StatusReport {
    #[serde(default = "v1")]
    pub v: u32,
    pub pane_id: String,
    pub event: ReportEvent,
    /// Milliseconds since the epoch, from the reporter's clock.
    pub ts: i64,
    #[serde(default)]
    pub payload: ReportPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReportError {
    #[error("report too large")]
    TooLarge,
    #[error("report isn't valid")]
    Invalid,
    #[error("signature missing or wrong")]
    Unauthorized,
    #[error("report timestamp outside the accepted window")]
    BadTimestamp,
}

impl ReportError {
    /// The HTTP status the server answers with.
    pub fn http_status(self) -> u16 {
        match self {
            ReportError::TooLarge => 413,
            ReportError::Invalid | ReportError::BadTimestamp => 400,
            ReportError::Unauthorized => 401,
        }
    }
}

/// `sha256=<hex(HMAC-SHA256(secret, body))>`.
pub fn sign(secret: &str, body: &[u8]) -> String {
    // HMAC takes keys of any length, so this never fails.
    let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()) else {
        return String::new();
    };
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

/// Constant-time check of a signature header against the body.
pub fn verify(secret: &str, body: &[u8], header: Option<&str>) -> bool {
    let Some(sig) = header.and_then(|h| h.trim().strip_prefix("sha256=")) else {
        return false;
    };
    let Ok(expected) = hex::decode(sig) else {
        return false;
    };
    let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}

/// The pane a body claims to be for, read before the signature is checked
/// (to find the secret). Nothing else in the body is trusted yet.
pub fn claimed_pane(body: &[u8]) -> Result<String, ReportError> {
    #[derive(Deserialize)]
    struct Claim {
        pane_id: String,
    }
    if body.len() > MAX_REPORT_BYTES {
        return Err(ReportError::TooLarge);
    }
    serde_json::from_slice::<Claim>(body)
        .map(|c| c.pane_id)
        .map_err(|_| ReportError::Invalid)
}

/// Verifies and parses a report: signature first (401), then shape (400),
/// then the timestamp window (400).
pub fn accept(
    secret: &str,
    body: &[u8],
    header: Option<&str>,
    now_ms: i64,
) -> Result<StatusReport, ReportError> {
    if body.len() > MAX_REPORT_BYTES {
        return Err(ReportError::TooLarge);
    }
    if !verify(secret, body, header) {
        return Err(ReportError::Unauthorized);
    }
    let report: StatusReport = serde_json::from_slice(body).map_err(|_| ReportError::Invalid)?;
    if report.v != 1 {
        return Err(ReportError::Invalid);
    }
    if report.ts < now_ms - MAX_AGE_MS || report.ts > now_ms + MAX_SKEW_MS {
        return Err(ReportError::BadTimestamp);
    }
    Ok(report)
}

impl StatusReport {
    /// The status change this report makes; `None` for title and session
    /// reports, which don't change the status.
    pub fn update(&self) -> Option<StatusUpdate> {
        let status = match self.event {
            ReportEvent::Working => AgentStatus::Working,
            ReportEvent::Waiting => AgentStatus::Waiting,
            ReportEvent::Done => AgentStatus::Done,
            ReportEvent::Idle => AgentStatus::Idle,
            ReportEvent::Failed => AgentStatus::Failed,
            ReportEvent::Title | ReportEvent::Session => return None,
        };
        Some(StatusUpdate {
            status,
            reason: match status {
                AgentStatus::Waiting => Some(self.payload.reason.unwrap_or(WaitingReason::Input)),
                _ => None,
            },
            ts: self.ts,
            tool: self.payload.tool.clone(),
            title: self.payload.title.clone(),
            preview: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const NOW: i64 = 1_800_000_000_000;

    fn body(ts: i64) -> Vec<u8> {
        format!(r#"{{"v":1,"pane_id":"pn_1","event":"waiting","ts":{ts},"payload":{{"reason":"approval"}}}}"#).into_bytes()
    }

    #[test]
    fn f_en_05_1_a_missing_or_wrong_signature_is_refused() {
        let b = body(NOW);
        assert_eq!(
            accept(SECRET, &b, None, NOW),
            Err(ReportError::Unauthorized)
        );
        assert_eq!(
            accept(SECRET, &b, Some("deadbeef"), NOW),
            Err(ReportError::Unauthorized)
        );
        assert_eq!(
            accept(SECRET, &b, Some("sha256=deadbeef"), NOW),
            Err(ReportError::Unauthorized)
        );
        // Signed with another pane's secret.
        let other = sign(&"f".repeat(64), &b);
        assert_eq!(
            accept(SECRET, &b, Some(&other), NOW),
            Err(ReportError::Unauthorized)
        );
        // A tampered body.
        let sig = sign(SECRET, &b);
        let mut tampered = b.clone();
        tampered.extend_from_slice(b" ");
        assert_eq!(
            accept(SECRET, &tampered, Some(&sig), NOW),
            Err(ReportError::Unauthorized)
        );
        assert_eq!(ReportError::Unauthorized.http_status(), 401);
    }

    #[test]
    fn f_en_05_1_a_signed_report_becomes_an_update() {
        let b = body(NOW);
        let report = accept(SECRET, &b, Some(&sign(SECRET, &b)), NOW).expect("accepted");
        let update = report.update().expect("status");
        assert_eq!(update.status, AgentStatus::Waiting);
        assert_eq!(update.reason, Some(WaitingReason::Approval));
        assert_eq!(claimed_pane(&b).expect("pane"), "pn_1");
    }

    #[test]
    fn f_en_05_2_replayed_and_future_reports_are_refused() {
        for ts in [NOW - MAX_AGE_MS - 1, NOW + MAX_SKEW_MS + 1] {
            let b = body(ts);
            assert_eq!(
                accept(SECRET, &b, Some(&sign(SECRET, &b)), NOW),
                Err(ReportError::BadTimestamp)
            );
        }
        let big = vec![b' '; MAX_REPORT_BYTES + 1];
        assert_eq!(
            accept(SECRET, &big, Some(&sign(SECRET, &big)), NOW),
            Err(ReportError::TooLarge)
        );
    }

    #[test]
    fn title_and_session_reports_change_no_status() {
        let b = format!(
            r#"{{"pane_id":"pn_1","event":"session","ts":{NOW},"payload":{{"session_id":"s-1"}}}}"#
        )
        .into_bytes();
        let report = accept(SECRET, &b, Some(&sign(SECRET, &b)), NOW).expect("accepted");
        assert!(report.update().is_none());
        assert_eq!(report.payload.session_id.as_deref(), Some("s-1"));
    }
}
