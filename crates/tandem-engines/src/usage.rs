//! Usage windows per engine account (F-AC-04, ARCHITECTURE §6.7): Codex
//! reports its rate limits through `codex app-server`
//! (`account/rateLimits/read`); engines without such a source are
//! "Usage unavailable", with the reason.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use ts_rs::TS;

/// One usage window ("5h", "week").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UsageWindow {
    pub kind: String,
    /// 0–100.
    pub used_pct: f64,
    /// Unix ms.
    pub resets_at: Option<i64>,
}

/// The window closest to exhaustion.
pub fn fullest(windows: &[UsageWindow]) -> Option<&UsageWindow> {
    windows.iter().max_by(|a, b| {
        a.used_pct
            .partial_cmp(&b.used_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn kind_for(minutes: Option<u64>, fallback: &str) -> String {
    match minutes {
        Some(300) => "5h".into(),
        Some(10_080) => "week".into(),
        Some(m) if m % 1_440 == 0 => format!("{}d", m / 1_440),
        Some(m) if m % 60 == 0 => format!("{}h", m / 60),
        Some(m) => format!("{m}m"),
        None => fallback.into(),
    }
}

fn ms(v: Option<&serde_json::Value>) -> Option<i64> {
    let n = v?.as_i64()?;
    // Seconds or milliseconds.
    Some(if n < 100_000_000_000 { n * 1_000 } else { n })
}

/// The windows in an `account/rateLimits/read` result: `primary` and
/// `secondary` objects with `usedPercent`, `windowMinutes` (or
/// `windowDurationMins`) and `resetsAt`, at the top or under `rateLimits`.
pub fn parse_rate_limits(result: &serde_json::Value) -> Vec<UsageWindow> {
    let limits = result.get("rateLimits").unwrap_or(result);
    ["primary", "secondary"]
        .iter()
        .filter_map(|key| {
            let w = limits.get(*key)?;
            let used = w.get("usedPercent").and_then(serde_json::Value::as_f64)?;
            let minutes = w
                .get("windowMinutes")
                .or_else(|| w.get("windowDurationMins"))
                .and_then(serde_json::Value::as_u64);
            Some(UsageWindow {
                kind: kind_for(minutes, key),
                used_pct: used.clamp(0.0, 100.0),
                resets_at: ms(w.get("resetsAt")),
            })
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum UsageError {
    #[error("couldn't start: {0}")]
    Spawn(std::io::Error),
    #[error("timed out")]
    Timeout,
    #[error("the engine answered with an error")]
    Refused,
    #[error("i/o: {0}")]
    Io(std::io::Error),
}

/// Asks `codex app-server` (started in the account's home) for its rate
/// limits: `initialize`, then `account/rateLimits/read`, over JSON-RPC lines
/// on stdio. The process is stopped afterwards.
pub async fn codex_rate_limits(
    program: &Path,
    env: &[(String, String)],
    cwd: &Path,
    timeout: Duration,
) -> Result<Vec<UsageWindow>, UsageError> {
    let mut child = tokio::process::Command::new(program)
        .arg("app-server")
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(UsageError::Spawn)?;
    let mut stdin = child.stdin.take().ok_or(UsageError::Refused)?;
    let stdout = child.stdout.take().ok_or(UsageError::Refused)?;
    let ask = async move {
        let requests = [
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"clientInfo": {"name": "vibezeo", "version": env!("CARGO_PKG_VERSION")}}}),
            serde_json::json!({"jsonrpc": "2.0", "method": "initialized"}),
            serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "account/rateLimits/read", "params": {}}),
        ];
        for r in requests {
            stdin
                .write_all(format!("{r}\n").as_bytes())
                .await
                .map_err(UsageError::Io)?;
        }
        stdin.flush().await.map_err(UsageError::Io)?;
        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await.map_err(UsageError::Io)? {
            let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            if msg.get("id").and_then(serde_json::Value::as_i64) != Some(2) {
                continue;
            }
            return match msg.get("result") {
                Some(result) => Ok(parse_rate_limits(result)),
                None => Err(UsageError::Refused),
            };
        }
        Err(UsageError::Refused)
    };
    let out = tokio::time::timeout(timeout, ask)
        .await
        .map_err(|_| UsageError::Timeout)?;
    let _ = child.kill().await;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_ac_04_rate_limit_windows_are_read_from_the_app_server_answer() {
        let result = serde_json::json!({
            "rateLimits": {
                "primary": {"usedPercent": 40.0, "windowMinutes": 300, "resetsAt": 1_791_418_440},
                "secondary": {"usedPercent": 85.5, "windowDurationMins": 10080, "resetsAt": 1_791_900_000_000_i64}
            }
        });
        let w = parse_rate_limits(&result);
        assert_eq!(w.len(), 2);
        assert_eq!(
            (w[0].kind.as_str(), w[0].used_pct, w[0].resets_at),
            ("5h", 40.0, Some(1_791_418_440_000))
        );
        assert_eq!(
            (w[1].kind.as_str(), w[1].resets_at),
            ("week", Some(1_791_900_000_000))
        );
        assert_eq!(fullest(&w).map(|w| w.used_pct), Some(85.5));
        assert!(parse_rate_limits(&serde_json::json!({"other": 1})).is_empty());
        assert_eq!(kind_for(Some(1_440), "x"), "1d");
        assert_eq!(kind_for(None, "primary"), "primary");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_ac_04_the_probe_speaks_json_rpc_on_stdio_and_reads_the_answer() {
        // A stand-in app-server: skip two lines, answer request 2.
        let dir = tempfile::tempdir().expect("tmp");
        let script = dir.path().join("codex");
        std::fs::write(
            &script,
            "#!/bin/sh\nread a\nread b\nread c\necho '{\"id\":1,\"result\":{}}'\necho '{\"id\":2,\"result\":{\"rateLimits\":{\"primary\":{\"usedPercent\":12,\"windowMinutes\":300}}}}'\nsleep 5\n",
        )
        .expect("script");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let env = [("PATH".to_string(), "/usr/bin:/bin".to_string())];
        let w = codex_rate_limits(&script, &env, dir.path(), Duration::from_secs(5))
            .await
            .expect("answered");
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].used_pct, 12.0);
    }
}
