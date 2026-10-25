//! `vibezeo run` (F-TL-12 slice 2): an agent asks the app to run a command
//! with the folder's secrets, over the pane's status channel. The body is
//! signed with the pane's per-launch secret, like a status report; the app
//! holds the request until the run ends and replies with its masked output.

use serde::{Deserialize, Serialize};

use crate::shellquote::ShellDialect;
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
    /// What followed `vibezeo run [--]`: one argument is a command line for
    /// the shell, several are a program and its arguments.
    pub argv: Vec<String>,
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
    if req.argv.iter().all(|a| a.trim().is_empty()) || req.nonce.len() < 16 || req.nonce.len() > 64
    {
        return Err(ReportError::Invalid);
    }
    if req.ts < now_ms - MAX_AGE_MS || req.ts > now_ms + MAX_SKEW_MS {
        return Err(ReportError::BadTimestamp);
    }
    Ok(req)
}

/// Why an argv can't become a command line.
pub const NO_LINE_BREAKS: &str = "vibezeo run takes one line: no line breaks or control characters";

/// The command line a run executes and its approval shows, exactly:
/// one argument as it is (a command line for `sh -c` or `cmd /C`), several
/// joined with each one that needs it quoted for `dialect`. Control
/// characters are refused; so is, on cmd.exe, an argument cmd can't quote.
pub fn command_line(argv: &[String], dialect: ShellDialect) -> Result<String, &'static str> {
    if argv
        .iter()
        .any(|a| a.chars().any(|c| c.is_control() && c != '\t'))
    {
        return Err(NO_LINE_BREAKS);
    }
    if let [one] = argv {
        return Ok(one.trim().to_string());
    }
    argv.iter()
        .map(|a| word(a, dialect))
        .collect::<Option<Vec<_>>>()
        .map(|w| w.join(" "))
        .ok_or("An argument can't be passed safely: quote the whole command as one argument")
}

fn word(a: &str, dialect: ShellDialect) -> Option<String> {
    if a.is_empty() {
        return Some(
            if dialect == ShellDialect::Cmd {
                "\"\""
            } else {
                "''"
            }
            .into(),
        );
    }
    let plain = !a.is_empty()
        && a.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    // cmd expands %VAR% even in a plain word.
    if plain && !(dialect == ShellDialect::Cmd && a.contains('%')) {
        return Some(a.to_string());
    }
    crate::shellquote::quote_path(a, dialect).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status_report::sign;

    fn body(ts: i64, command: &str) -> Vec<u8> {
        serde_json::to_vec(&RunRequest {
            pane_id: "pn_1".into(),
            cwd: "/w".into(),
            argv: vec![command.into()],
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
            accept("s3cret", &b, Some(&sig), now).map(|r| r.argv),
            Ok(vec!["pnpm deploy".to_string()])
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

    fn line(argv: &[&str], d: ShellDialect) -> Result<String, &'static str> {
        command_line(&argv.iter().map(|a| a.to_string()).collect::<Vec<_>>(), d)
    }

    #[test]
    fn f_tl_12_7_one_argument_is_a_command_line_several_are_quoted_words() {
        use ShellDialect::*;
        assert_eq!(
            line(&["pnpm deploy && echo ok"], Posix),
            Ok("pnpm deploy && echo ok".into())
        );
        assert_eq!(
            line(&["pnpm", "deploy", "--prod"], Posix),
            Ok("pnpm deploy --prod".into())
        );
        assert_eq!(
            line(&["printf", "%s\\n", "a b", "it's"], Posix),
            Ok("printf '%s\\n' 'a b' 'it'\\''s'".into())
        );
        assert_eq!(
            line(&["echo", "$HOME", ""], Posix),
            Ok("echo '$HOME' ''".into())
        );
        assert_eq!(
            line(&["echo", "a;b"], Posix).map(|l| l.contains("'a;b'")),
            Ok(true)
        );
        assert_eq!(line(&["echo", "x\ny"], Posix), Err(NO_LINE_BREAKS));
        assert_eq!(line(&["echo x\rrm -rf y"], Posix), Err(NO_LINE_BREAKS));
    }

    #[test]
    fn f_tl_12_7_cmd_quotes_with_double_quotes_and_refuses_what_it_cant() {
        use ShellDialect::Cmd;
        assert_eq!(
            line(&["npm", "run", "deploy"], Cmd),
            Ok("npm run deploy".into())
        );
        assert_eq!(
            line(&["dir", "C:/Program Files"], Cmd),
            Ok("dir \"C:/Program Files\"".into())
        );
        assert_eq!(line(&["echo", "a&b"], Cmd), Ok("echo \"a&b\"".into()));
        assert!(line(&["echo", "%PATH%"], Cmd).is_err());
        assert!(line(&["echo", "say \"hi\""], Cmd).is_err());
        assert!(line(&["echo", "wow!"], Cmd).is_err());
        // One argument is the whole command line, as typed.
        assert_eq!(
            line(&["npm run deploy & echo %PATH%"], Cmd),
            Ok("npm run deploy & echo %PATH%".into())
        );
    }
}
