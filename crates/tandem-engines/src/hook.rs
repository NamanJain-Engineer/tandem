//! `tandem-hook`'s logic (ARCHITECTURE §6.6, F-EN-05 AC4): engine hook input
//! in, a signed status report out. Only the event, tool, notification type,
//! session id, transcript path and an edit tool's file path are read;
//! prompts, other tool inputs, edit contents and transcript text never
//! leave the hook.

use std::io::{self, Read, Write};
use std::time::Duration;

use serde_json::Value;
use tandem_core::status::WaitingReason;
use tandem_core::status_report::{
    sign, ReportEvent, ReportPayload, StatusReport, SIGNATURE_HEADER,
};

/// Hook stdin beyond this is discarded unread.
pub const MAX_STDIN: usize = 1024 * 1024;
pub const POST_TIMEOUT: Duration = Duration::from_millis(500);
pub const PANE_VAR: &str = "TANDEM_PANE_ID";
/// Claude's tools that change a file, and the input field naming it
/// (F-TK-05 inferred claims).
const EDIT_TOOLS: [(&str, &str); 4] = [
    ("Edit", "file_path"),
    ("MultiEdit", "file_path"),
    ("Write", "file_path"),
    ("NotebookEdit", "notebook_path"),
];
const PATH_MAX: usize = 1024;

/// The report for one Claude hook call (`tandem-hook claude <Event>`).
pub fn claude_report(
    event_arg: &str,
    stdin: &[u8],
    pane_id: &str,
    now_ms: i64,
) -> Option<StatusReport> {
    let input: Value = serde_json::from_slice(stdin).unwrap_or(Value::Null);
    let field = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);
    let event_name = field("hook_event_name").unwrap_or_else(|| event_arg.to_string());
    let mut payload = ReportPayload {
        session_id: field("session_id").filter(|s| short(s)),
        transcript_path: field("transcript_path").filter(|s| s.len() <= 1024),
        ..ReportPayload::default()
    };
    let event = match event_name.as_str() {
        "UserPromptSubmit" | "SubagentStop" => ReportEvent::Working,
        "PreToolUse" => {
            payload.tool = field("tool_name").filter(|s| short(s));
            let edited = payload
                .tool
                .as_deref()
                .and_then(|t| EDIT_TOOLS.iter().find(|(name, _)| *name == t))
                .and_then(|(_, key)| input.get("tool_input")?.get(*key)?.as_str())
                .filter(|p| !p.is_empty() && p.len() <= PATH_MAX);
            payload.paths = edited.map(str::to_string).into_iter().collect();
            ReportEvent::Working
        }
        "Notification" => {
            let kind = field("notification_type").unwrap_or_default();
            payload.reason = Some(if kind.contains("permission") {
                WaitingReason::Approval
            } else if kind.contains("elicitation") || kind.contains("question") {
                WaitingReason::Question
            } else {
                WaitingReason::Input
            });
            ReportEvent::Waiting
        }
        "Stop" => ReportEvent::Done,
        "SessionEnd" => ReportEvent::Idle,
        "SessionStart" => ReportEvent::Session,
        _ => return None,
    };
    Some(StatusReport {
        v: 1,
        pane_id: pane_id.to_string(),
        event,
        ts: now_ms,
        payload,
    })
}

/// The report for Codex's `notify` (`tandem-hook codex '<json>'`).
pub fn codex_report(json_arg: &str, pane_id: &str, now_ms: i64) -> Option<StatusReport> {
    let input: Value = serde_json::from_str(json_arg).ok()?;
    let kind = input.get("type").and_then(Value::as_str)?;
    let event = match kind {
        "agent-turn-complete" => ReportEvent::Done,
        _ => return None,
    };
    Some(StatusReport {
        v: 1,
        pane_id: pane_id.to_string(),
        event,
        ts: now_ms,
        payload: ReportPayload {
            session_id: input
                .get("thread-id")
                .and_then(Value::as_str)
                .filter(|s| short(s))
                .map(str::to_string),
            ..ReportPayload::default()
        },
    })
}

fn short(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200
}

/// Reads at most [`MAX_STDIN`] bytes; the rest is drained and dropped.
pub fn read_limited(mut input: impl Read) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = (&mut input).take(MAX_STDIN as u64).read_to_end(&mut buf);
    let _ = io::copy(&mut input, &mut io::sink());
    buf
}

/// The request bytes for a signed report.
pub fn request(report: &StatusReport, secret: &str) -> Option<Vec<u8>> {
    let body = serde_json::to_vec(report).ok()?;
    let sig = sign(secret, &body);
    let mut req = format!(
        "POST /v1/status HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n{SIGNATURE_HEADER}: {sig}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    req.extend_from_slice(&body);
    Some(req)
}

/// Sends a request to `TANDEM_STATUS_URL` (`unix:///…` or `http://127.0.0.1:…`)
/// and returns the HTTP status.
pub fn post(url: &str, request: &[u8], timeout: Duration) -> io::Result<u16> {
    exchange(url, request, timeout, 4096).map(|(code, _)| code)
}

/// The signed request bytes for `vibezeo run` (F-TL-12).
pub fn run_request(req: &tandem_core::run_request::RunRequest, secret: &str) -> Option<Vec<u8>> {
    let body = serde_json::to_vec(req).ok()?;
    let sig = sign(secret, &body);
    let mut out = format!(
        "POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n{SIGNATURE_HEADER}: {sig}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        tandem_core::run_request::RUN_PATH,
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(&body);
    Some(out)
}

/// Sends `request` and reads at most `max` bytes back: the HTTP status and
/// the body.
pub fn exchange(
    url: &str,
    request: &[u8],
    timeout: Duration,
    max: u64,
) -> io::Result<(u16, Vec<u8>)> {
    let mut response = Vec::new();
    if let Some(path) = url.strip_prefix("unix://") {
        #[cfg(unix)]
        {
            let mut s = std::os::unix::net::UnixStream::connect(path)?;
            s.set_read_timeout(Some(timeout))?;
            s.set_write_timeout(Some(timeout))?;
            s.write_all(request)?;
            let _ = s.take(max).read_to_end(&mut response);
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            return Err(io::Error::new(io::ErrorKind::Unsupported, "unix sockets"));
        }
    } else if let Some(rest) = url.strip_prefix("http://") {
        let host = rest.split('/').next().unwrap_or_default();
        let addr: std::net::SocketAddr = host
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "status url"))?;
        if !addr.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "status url must be loopback",
            ));
        }
        let mut s = std::net::TcpStream::connect_timeout(&addr, timeout)?;
        s.set_read_timeout(Some(timeout))?;
        s.set_write_timeout(Some(timeout))?;
        s.write_all(request)?;
        let _ = s.take(max).read_to_end(&mut response);
    } else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "status url"));
    }
    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map_or(response.len(), |i| i + 4);
    let head = String::from_utf8_lossy(&response[..split]);
    let code = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no status line"))?;
    Ok((code, response[split..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_en_05_4_a_stop_payload_with_transcript_text_produces_a_body_without_it() {
        let stdin = br#"{"session_id":"abc-123","transcript_path":"/h/.claude/projects/x/abc.jsonl","hook_event_name":"Stop","stop_hook_active":false,
            "prompt":"SECRET PROMPT TEXT","message":"the assistant said SECRET REPLY","transcript":[{"role":"user","content":"SECRET TRANSCRIPT"}]}"#;
        let report = claude_report("Stop", stdin, "pn_1", 1).expect("report");
        let body = String::from_utf8(serde_json::to_vec(&report).expect("json")).expect("utf8");
        assert!(!body.contains("SECRET"), "{body}");
        assert_eq!(report.event, ReportEvent::Done);
        assert_eq!(report.payload.session_id.as_deref(), Some("abc-123"));
        assert!(body.contains("abc.jsonl"));
    }

    #[test]
    fn claude_events_map_to_statuses() {
        let r = |event: &str, extra: &str| {
            let stdin = format!(r#"{{"hook_event_name":"{event}"{extra}}}"#);
            claude_report(event, stdin.as_bytes(), "pn_1", 1)
        };
        assert_eq!(
            r("UserPromptSubmit", r#","prompt":"x""#).map(|r| r.event),
            Some(ReportEvent::Working)
        );
        let tool = r(
            "PreToolUse",
            r#","tool_name":"Bash","tool_input":{"command":"rm -rf secret"}"#,
        )
        .expect("tool");
        assert_eq!(tool.payload.tool.as_deref(), Some("Bash"));
        assert!(!serde_json::to_string(&tool)
            .expect("json")
            .contains("rm -rf"));
        let wait = r(
            "Notification",
            r#","notification_type":"permission_prompt""#,
        )
        .expect("wait");
        assert_eq!(
            (wait.event, wait.payload.reason),
            (ReportEvent::Waiting, Some(WaitingReason::Approval))
        );
        let idle = r("Notification", r#","notification_type":"idle_prompt""#).expect("idle");
        assert_eq!(idle.payload.reason, Some(WaitingReason::Input));
        assert_eq!(r("Stop", "").map(|r| r.event), Some(ReportEvent::Done));
        assert_eq!(
            r("SubagentStop", "").map(|r| r.event),
            Some(ReportEvent::Working)
        );
        assert_eq!(
            r("SessionEnd", "").map(|r| r.event),
            Some(ReportEvent::Idle)
        );
        assert_eq!(
            r("SessionStart", r#","session_id":"s""#).map(|r| r.event),
            Some(ReportEvent::Session)
        );
        assert_eq!(r("SomethingNew", ""), None);
        // Garbage stdin still reports the event from argv.
        assert_eq!(
            claude_report("Stop", b"not json", "pn_1", 1).map(|r| r.event),
            Some(ReportEvent::Done)
        );
    }

    #[test]
    fn f_tk_05_2_an_edit_reports_its_file_path_but_not_the_edit() {
        let stdin = br#"{"hook_event_name":"PreToolUse","tool_name":"Edit","tool_input":{"file_path":"/r/app/a.ts","old_string":"SECRET OLD","new_string":"SECRET NEW"}}"#;
        let report = claude_report("PreToolUse", stdin, "pn_1", 1).expect("report");
        assert_eq!(report.payload.paths, vec!["/r/app/a.ts".to_string()]);
        assert!(!serde_json::to_string(&report)
            .expect("json")
            .contains("SECRET"));
        let note = br#"{"hook_event_name":"PreToolUse","tool_name":"NotebookEdit","tool_input":{"notebook_path":"/r/n.ipynb"}}"#;
        assert_eq!(
            claude_report("PreToolUse", note, "pn_1", 1)
                .expect("note")
                .payload
                .paths,
            vec!["/r/n.ipynb".to_string()]
        );
        let read = br#"{"hook_event_name":"PreToolUse","tool_name":"Read","tool_input":{"file_path":"/r/app/a.ts"}}"#;
        assert!(claude_report("PreToolUse", read, "pn_1", 1)
            .expect("read")
            .payload
            .paths
            .is_empty());
    }

    #[test]
    fn codex_turn_complete_is_done_without_messages() {
        let arg = r#"{"type":"agent-turn-complete","thread-id":"t-1","turn-id":"u","input-messages":["SECRET"],"last-assistant-message":"SECRET"}"#;
        let report = codex_report(arg, "pn_1", 1).expect("report");
        assert_eq!(report.event, ReportEvent::Done);
        assert!(!serde_json::to_string(&report)
            .expect("json")
            .contains("SECRET"));
        assert_eq!(codex_report(r#"{"type":"other"}"#, "pn_1", 1), None);
    }

    #[test]
    fn stdin_is_capped() {
        let big = vec![b'x'; MAX_STDIN + 10];
        assert_eq!(read_limited(&big[..]).len(), MAX_STDIN);
    }

    #[test]
    fn only_loopback_http_urls_are_used() {
        let err = post("http://10.0.0.1:80", b"", Duration::from_millis(10)).expect_err("refused");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(post("ftp://x", b"", Duration::from_millis(10)).is_err());
    }
}
