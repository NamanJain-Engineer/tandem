//! Context fill from an engine's own session transcript (F-AC-06,
//! ARCHITECTURE §6.6): the last `usage` object's token counts, read from the
//! file's tail. Only numbers and the model id are taken; message content is
//! never kept (F-AC-01 allows reading the engine's session logs for usage).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How much of the context window the session uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContextFill {
    /// Tokens the next turn starts with (input + cache reads + cache writes).
    pub used: u64,
    /// The model's window.
    pub window: u64,
}

/// The tail read from a transcript: enough for the last turns.
const TAIL: u64 = 256 * 1024;
/// Claude models' window, unless the model id says 1M.
const WINDOW: u64 = 200_000;
const WINDOW_1M: u64 = 1_000_000;

/// The context fill in `transcript`, which must be inside `home` (a pane's
/// provider home): a path elsewhere is never read.
pub fn context_fill(transcript: &Path, home: &Path) -> Option<ContextFill> {
    let path = transcript.canonicalize().ok()?;
    let home = home.canonicalize().ok()?;
    if !path.starts_with(&home) || path == home {
        return None;
    }
    let mut file = std::fs::File::open(&path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL))).ok()?;
    let mut tail = Vec::new();
    file.take(TAIL).read_to_end(&mut tail).ok()?;
    let text = String::from_utf8_lossy(&tail);
    text.lines()
        .rev()
        .filter(|l| l.contains("\"usage\""))
        .find_map(fill_from_line)
}

fn fill_from_line(line: &str) -> Option<ContextFill> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let message = v.get("message").unwrap_or(&v);
    let usage = message.get("usage").or_else(|| v.get("usage"))?;
    let n = |k: &str| {
        usage
            .get(k)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    let used = n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens");
    if used == 0 {
        return None;
    }
    let model = message
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    Some(ContextFill {
        used,
        window: window_for(model),
    })
}

/// A Claude model's context window: 1M when the model id says so.
pub fn window_for(model: &str) -> u64 {
    if model.contains("[1m]") || model.ends_with("-1m") {
        WINDOW_1M
    } else {
        WINDOW
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(input: u64, read: u64, model: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "message": {
                "model": model,
                "content": [{"type": "text", "text": "SECRET ANSWER"}],
                "usage": {"input_tokens": input, "cache_read_input_tokens": read, "cache_creation_input_tokens": 0, "output_tokens": 9}
            }
        })
        .to_string()
    }

    #[test]
    fn f_ac_06_1_the_last_usage_in_the_transcript_is_the_fill() {
        let dir = tempfile::tempdir().expect("tmp");
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("projects/p")).expect("dirs");
        let t = home.join("projects/p/s.jsonl");
        let body = [
            line(1_000, 500, "claude-sonnet-4-5"),
            r#"{"type":"user","message":{"content":"no usage here"}}"#.to_string(),
            line(140_000, 10_000, "claude-sonnet-4-5"),
            "not json".to_string(),
        ]
        .join("\n");
        std::fs::write(&t, body).expect("write");
        assert_eq!(
            context_fill(&t, &home),
            Some(ContextFill {
                used: 150_000,
                window: 200_000
            })
        );
        std::fs::write(&t, line(170_000, 0, "claude-sonnet-4-5[1m]")).expect("write");
        assert_eq!(
            context_fill(&t, &home),
            Some(ContextFill {
                used: 170_000,
                window: 1_000_000
            })
        );
    }

    #[test]
    fn f_ac_01_a_transcript_outside_the_provider_home_is_never_read() {
        let dir = tempfile::tempdir().expect("tmp");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        let outside = dir.path().join("elsewhere.jsonl");
        std::fs::write(&outside, line(150_000, 0, "m")).expect("write");
        assert_eq!(context_fill(&outside, &home), None);
        // Nor through a link out of it.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, home.join("link.jsonl")).expect("link");
            assert_eq!(context_fill(&home.join("link.jsonl"), &home), None);
        }
        assert_eq!(context_fill(&home.join("missing.jsonl"), &home), None);
    }
}
