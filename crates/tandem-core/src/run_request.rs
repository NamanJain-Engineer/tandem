//! `vibezeo run` (F-TL-12 slice 2): an agent asks the app to run a command
//! with the folder's secrets, over the pane's status channel. The body is
//! signed with the pane's per-launch secret, like a status report; the app
//! holds the request until the run ends and replies with its masked output.

use serde::{Deserialize, Serialize};

use crate::status_report::{verify, ReportError, MAX_AGE_MS, MAX_SKEW_MS};

/// Larger requests are refused unread.
pub const MAX_RUN_BYTES: usize = 8 * 1024;

/// The last this much output comes back.
pub const OUTPUT_CAP: usize = 64 * 1024;

/// The route on the status server.
pub const RUN_PATH: &str = "/v1/run";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRequest {
    pub pane_id: String,
    /// Where the agent ran `vibezeo run`.
    pub cwd: String,
    pub command: String,
    pub ts: i64,
    /// Random per request: a replayed body is refused.
    pub nonce: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReply {
    /// The command's exit code; `None` when it didn't run or was killed.
    pub exit_code: Option<i32>,
    /// stdout and stderr together, secret values masked.
    pub output: String,
    /// Earlier output was dropped (only the last [`OUTPUT_CAP`] bytes came back).
    pub cut: bool,
    /// Why nothing ran, or why it stopped.
    pub error: Option<String>,
}

impl RunReply {
    pub fn refused(why: impl Into<String>) -> Self {
        Self {
            error: Some(why.into()),
            ..Self::default()
        }
    }
}

/// Checks the signature first, then the body, then the timestamp window.
pub fn accept(
    secret: &str,
    body: &[u8],
    header: Option<&str>,
    now_ms: i64,
) -> Result<RunRequest, ReportError> {
    if body.len() > MAX_RUN_BYTES {
        return Err(ReportError::TooLarge);
    }
    if !verify(secret, body, header) {
        return Err(ReportError::Unauthorized);
    }
    let req: RunRequest = serde_json::from_slice(body).map_err(|_| ReportError::Invalid)?;
    if req.command.trim().is_empty() || req.nonce.len() < 16 || req.nonce.len() > 64 {
        return Err(ReportError::Invalid);
    }
    if req.ts < now_ms - MAX_AGE_MS || req.ts > now_ms + MAX_SKEW_MS {
        return Err(ReportError::BadTimestamp);
    }
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status_report::sign;

    fn body(ts: i64, command: &str) -> Vec<u8> {
        serde_json::to_vec(&RunRequest {
            pane_id: "pn_1".into(),
            cwd: "/w".into(),
            command: command.into(),
            ts,
            nonce: "0123456789abcdef".into(),
        })
        .expect("json")
    }

    #[test]
    fn f_tl_12_10_only_a_signed_recent_request_is_accepted() {
        let now = 1_000_000_000;
        let b = body(now, "pnpm deploy");
        let sig = sign("s3cret", &b);
        assert_eq!(
            accept("s3cret", &b, Some(&sig), now).map(|r| r.command),
            Ok("pnpm deploy".into())
        );
        assert_eq!(
            accept("other", &b, Some(&sig), now),
            Err(ReportError::Unauthorized)
        );
        assert_eq!(
            accept("s3cret", &b, None, now),
            Err(ReportError::Unauthorized)
        );
        let old = body(now - MAX_AGE_MS - 1, "x");
        assert_eq!(
            accept("s3cret", &old, Some(&sign("s3cret", &old)), now),
            Err(ReportError::BadTimestamp)
        );
        let empty = body(now, "  ");
        assert_eq!(
            accept("s3cret", &empty, Some(&sign("s3cret", &empty)), now),
            Err(ReportError::Invalid)
        );
        let big = vec![b' '; MAX_RUN_BYTES + 1];
        assert_eq!(
            accept("s3cret", &big, None, now),
            Err(ReportError::TooLarge)
        );
    }
}
