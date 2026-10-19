//! Testing a command as an ACP agent (F-EN-10): start it, send
//! `initialize`, and read what it says about itself. Nothing else is sent:
//! no session is opened and no prompt, so the test can't run anything.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

use super::PROTOCOL_VERSION;
use crate::chat::process::{ChatProcess, ProcessLine, ProcessSpec};

/// How long an agent has to answer `initialize`.
pub const PROBE_WAIT: Duration = Duration::from_secs(10);

/// What an agent says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentInfo {
    pub name: Option<String>,
    pub version: Option<String>,
    #[ts(type = "number")]
    pub protocol_version: u64,
    /// The names of its sign-in methods.
    pub auth_methods: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    #[error("it couldn't start: {0}")]
    Spawn(String),
    #[error("it didn't answer within {0} seconds")]
    Timeout(u64),
    #[error("it stopped before answering{0}")]
    Exited(String),
    #[error("its answer isn't the Agent Client Protocol")]
    NotAcp,
    #[error("it answered with an error: {0}")]
    Refused(String),
    #[error(
        "it speaks version {0} of the Agent Client Protocol; Vibezeo speaks {PROTOCOL_VERSION}"
    )]
    Version(u64),
}

/// Starts `program` and asks it to `initialize`, waiting up to `wait`. The
/// process is stopped either way.
pub async fn probe(
    program: PathBuf,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: PathBuf,
    wait: Duration,
) -> Result<AgentInfo, ProbeError> {
    let spec = ProcessSpec::new(
        program,
        args.into_iter().map(Into::into).collect(),
        env,
        cwd,
    );
    let (process, mut lines) =
        ChatProcess::spawn(spec).map_err(|e| ProbeError::Spawn(e.to_string()))?;
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL_VERSION,
            "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
            "clientInfo": {"name": "vibezeo", "title": "Vibezeo", "version": env!("CARGO_PKG_VERSION")},
        },
    });
    let answer = async {
        process
            .write_line(&request.to_string())
            .await
            .map_err(|e| ProbeError::Spawn(e.to_string()))?;
        while let Some(line) = lines.recv().await {
            let ProcessLine::Line(line) = line else {
                return Err(ProbeError::NotAcp);
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // An agent may log to stdout before it answers; anything that
            // isn't JSON-RPC at all is the answer of something else.
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                return Err(ProbeError::NotAcp);
            };
            if v.get("id") == Some(&json!(1)) && v.get("method").is_none() {
                return read_answer(&v);
            }
            if v.get("jsonrpc").is_none() {
                return Err(ProbeError::NotAcp);
            }
        }
        let code = process.exit_code().flatten();
        Err(ProbeError::Exited(
            code.map(|c| format!(" (exit {c})")).unwrap_or_default(),
        ))
    };
    let out = match tokio::time::timeout(wait, answer).await {
        Ok(out) => out,
        Err(_) => Err(ProbeError::Timeout(wait.as_secs())),
    };
    process.kill_tree().await;
    out
}

/// The `initialize` result, or why it isn't one.
pub fn read_answer(v: &Value) -> Result<AgentInfo, ProbeError> {
    if let Some(err) = v.get("error") {
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("no message");
        return Err(ProbeError::Refused(message.chars().take(200).collect()));
    }
    let result = v.get("result").ok_or(ProbeError::NotAcp)?;
    let version = result
        .get("protocolVersion")
        .and_then(Value::as_u64)
        .ok_or(ProbeError::NotAcp)?;
    if version != PROTOCOL_VERSION {
        return Err(ProbeError::Version(version));
    }
    let text = |path: &str| {
        result
            .pointer(path)
            .and_then(Value::as_str)
            .map(|s| s.chars().take(80).collect::<String>())
    };
    let auth_methods = result
        .get("authMethods")
        .and_then(Value::as_array)
        .map(|methods| {
            methods
                .iter()
                .filter_map(|m| {
                    m.get("name")
                        .or_else(|| m.get("id"))
                        .and_then(Value::as_str)
                        .map(|s| s.chars().take(80).collect())
                })
                .take(8)
                .collect()
        })
        .unwrap_or_default();
    Ok(AgentInfo {
        name: text("/agentInfo/title").or_else(|| text("/agentInfo/name")),
        version: text("/agentInfo/version"),
        protocol_version: version,
        auth_methods,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_en_10_1_an_initialize_answer_gives_the_agents_name_version_and_sign_in() {
        let v = json!({"jsonrpc": "2.0", "id": 1, "result": {
            "protocolVersion": 1,
            "agentInfo": {"name": "goose", "title": "Goose", "version": "1.9.0"},
            "authMethods": [{"id": "oauth", "name": "Sign in with Block"}, {"id": "key"}],
        }});
        let info = read_answer(&v).expect("info");
        assert_eq!(info.name.as_deref(), Some("Goose"));
        assert_eq!(info.version.as_deref(), Some("1.9.0"));
        assert_eq!(info.auth_methods, vec!["Sign in with Block", "key"]);
    }

    #[test]
    fn f_en_10_2_errors_other_versions_and_non_answers_arent_agents() {
        assert_eq!(
            read_answer(
                &json!({"id": 1, "error": {"code": -32601, "message": "Method not found"}})
            ),
            Err(ProbeError::Refused("Method not found".into()))
        );
        assert_eq!(
            read_answer(&json!({"id": 1, "result": {"protocolVersion": 2}})),
            Err(ProbeError::Version(2))
        );
        assert_eq!(
            read_answer(&json!({"id": 1, "result": {}})),
            Err(ProbeError::NotAcp)
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_en_10_2_a_command_that_never_answers_times_out_and_is_stopped() {
        let dir = tempfile::tempdir().expect("dir");
        let started = std::time::Instant::now();
        let out = probe(
            PathBuf::from("/bin/sleep"),
            vec!["30".into()],
            vec![("PATH".into(), "/usr/bin:/bin".into())],
            dir.path().to_path_buf(),
            Duration::from_millis(500),
        )
        .await;
        assert_eq!(out, Err(ProbeError::Timeout(0)));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn f_en_10_2_a_command_that_echoes_or_exits_isnt_an_agent() {
        let dir = tempfile::tempdir().expect("dir");
        let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
        // `cat` sends the request back: a request, not an answer.
        let cat = probe(
            PathBuf::from("/bin/cat"),
            vec![],
            env.clone(),
            dir.path().to_path_buf(),
            Duration::from_secs(1),
        )
        .await;
        assert!(
            matches!(
                cat,
                Err(ProbeError::Exited(_)) | Err(ProbeError::Timeout(_)) | Err(ProbeError::NotAcp)
            ),
            "{cat:?}"
        );
        let echo = probe(
            PathBuf::from("/bin/echo"),
            vec!["hello".into()],
            env.clone(),
            dir.path().to_path_buf(),
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(echo, Err(ProbeError::NotAcp));
        let gone = probe(
            PathBuf::from("/nonexistent/agent"),
            vec![],
            env,
            dir.path().to_path_buf(),
            Duration::from_secs(5),
        )
        .await;
        assert!(matches!(gone, Err(ProbeError::Spawn(_))), "{gone:?}");
    }
}
