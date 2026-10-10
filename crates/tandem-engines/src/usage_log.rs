//! Token usage from engines' own local logs (F-AC-07, ARCHITECTURE §6.7):
//! Claude Code's `projects/**/*.jsonl` and Codex's `sessions/**/*.jsonl`,
//! found through the registry's `usage_log_globs` inside each account's
//! provider home. Parsing streams line by line and is tolerant: a line that
//! isn't JSON, has no usable date or no token counts is skipped and counted.
//! Only numbers, dates, model ids and the working folder are read; message
//! content is never kept (F-AC-01 allows reading session logs for usage).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// A line longer than this is skipped unread (it's content, not usage).
const MAX_LINE: usize = 4 * 1024 * 1024;

/// One account's logs to read.
#[derive(Debug, Clone)]
pub struct LogSource {
    pub engine_id: String,
    pub account_id: String,
    /// The provider home the globs are relative to; nothing outside it is read.
    pub home: PathBuf,
    pub globs: Vec<String>,
}

/// Tokens one engine account used on one day, in one folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    pub input: u64,
    pub output: u64,
    /// Cache reads and writes (Claude), cached input (Codex).
    pub cache: u64,
}

impl Tally {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache
    }

    fn add(&mut self, other: &Tally) {
        self.input += other.input;
        self.output += other.output;
        self.cache += other.cache;
    }
}

/// Everything read: `(day, engine, account, folder)` → tokens, the sessions
/// seen per account, and what was skipped.
#[derive(Debug, Default)]
pub struct UsageScan {
    pub cells: BTreeMap<(NaiveDate, String, String, String), Tally>,
    pub sessions: HashMap<(String, String), HashSet<String>>,
    pub skipped_lines: u64,
    pub files_read: u64,
}

/// A line's usage once parsed.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    at: DateTime<FixedOffset>,
    tally: Tally,
    /// Claude repeats a message's usage once per content block: keep one.
    dedupe: Option<String>,
}

/// The files a glob like `projects/**/*.jsonl` or `sessions/*.jsonl` names
/// under `home`. Only `<dir>/**/<*.ext>` and `<dir>/<*.ext>` forms are
/// supported (what the registry uses); symlinks are never followed.
pub fn files_for(home: &Path, glob: &str) -> Vec<PathBuf> {
    let (dir, recursive, pattern) = match glob.split_once("/**/") {
        Some((dir, pat)) => (dir, true, pat),
        None => match glob.rsplit_once('/') {
            Some((dir, pat)) => (dir, false, pat),
            None => ("", false, glob),
        },
    };
    let Some(ext) = pattern.strip_prefix('*') else {
        return Vec::new();
    };
    if dir.split('/').any(|p| p == ".." || p.contains('*')) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut stack = vec![home.join(dir)];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() && recursive {
                stack.push(path);
            } else if kind.is_file()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(ext))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn num(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

fn parse_time(v: &Value) -> Option<DateTime<FixedOffset>> {
    let s = v.get("timestamp").and_then(Value::as_str)?;
    DateTime::parse_from_rfc3339(s).ok()
}

/// Claude Code: assistant lines carry `message.usage`, `message.id`, `cwd`.
fn claude_line(v: &Value) -> Option<(Entry, Option<String>, Option<String>)> {
    let message = v.get("message")?;
    let usage = message.get("usage")?;
    let tally = Tally {
        input: num(usage, "input_tokens"),
        output: num(usage, "output_tokens"),
        cache: num(usage, "cache_read_input_tokens") + num(usage, "cache_creation_input_tokens"),
    };
    let at = parse_time(v)?;
    let dedupe = message.get("id").and_then(Value::as_str).map(|id| {
        format!(
            "{id}:{}",
            v.get("requestId").and_then(Value::as_str).unwrap_or("")
        )
    });
    let cwd = v.get("cwd").and_then(Value::as_str).map(str::to_string);
    let session = v
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some((Entry { at, tally, dedupe }, cwd, session))
}

/// Codex: `event_msg` lines of type `token_count` carry `last_token_usage`;
/// the session's `session_meta` (or `turn_context`) line names its `cwd`.
fn codex_line(v: &Value) -> Option<Entry> {
    let payload = v.get("payload")?;
    if payload.get("type").and_then(Value::as_str) != Some("token_count") {
        return None;
    }
    let last = payload.get("info")?.get("last_token_usage")?;
    let cached = num(last, "cached_input_tokens");
    let tally = Tally {
        input: num(last, "input_tokens").saturating_sub(cached),
        output: num(last, "output_tokens") + num(last, "reasoning_output_tokens"),
        cache: cached,
    };
    Some(Entry {
        at: parse_time(v)?,
        tally,
        dedupe: None,
    })
}

fn codex_meta(v: &Value) -> Option<(Option<String>, Option<String>)> {
    let kind = v.get("type").and_then(Value::as_str)?;
    if kind != "session_meta" && kind != "turn_context" {
        return None;
    }
    let payload = v.get("payload")?;
    Some((
        payload
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::to_string),
        payload
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string),
    ))
}

/// Reads every source's logs into `scan`, keeping days from `since` on
/// (in `tz`, the user's zone). Files untouched since then aren't opened.
pub fn scan(sources: &[LogSource], since: NaiveDate, tz: FixedOffset) -> UsageScan {
    let mut out = UsageScan::default();
    let since_time = since
        .and_hms_opt(0, 0, 0)
        .and_then(|t| t.and_local_timezone(tz).single());
    for src in sources {
        let Ok(home) = src.home.canonicalize() else {
            continue;
        };
        let mut seen: HashSet<String> = HashSet::new();
        for glob in &src.globs {
            for file in files_for(&home, glob) {
                // Only files inside the home (a symlinked folder could lead out).
                if !file.canonicalize().is_ok_and(|p| p.starts_with(&home)) {
                    continue;
                }
                if let (Some(since), Ok(meta)) = (since_time, std::fs::metadata(&file)) {
                    let modified = meta.modified().ok().map(DateTime::<chrono::Utc>::from);
                    if modified.is_some_and(|m| m < since) {
                        continue;
                    }
                }
                read_file(&file, src, since, tz, &mut seen, &mut out);
            }
        }
    }
    out
}

fn read_file(
    file: &Path,
    src: &LogSource,
    since: NaiveDate,
    tz: FixedOffset,
    seen: &mut HashSet<String>,
    out: &mut UsageScan,
) {
    let Ok(f) = std::fs::File::open(file) else {
        return;
    };
    out.files_read += 1;
    let mut reader = BufReader::new(f);
    let mut buf = Vec::new();
    // Codex names the folder once per session file; Claude on every line.
    let mut file_cwd: Option<String> = None;
    let mut file_session = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = trim_line(&buf);
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_LINE {
            out.skipped_lines += 1;
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(line) else {
            out.skipped_lines += 1;
            continue;
        };
        let parsed = if src.engine_id == "codex" {
            if let Some((cwd, session)) = codex_meta(&v) {
                if cwd.is_some() {
                    file_cwd = cwd;
                }
                if let Some(s) = session {
                    file_session = s;
                }
                continue;
            }
            codex_line(&v).map(|e| (e, None, None))
        } else {
            claude_line(&v)
        };
        let Some((entry, cwd, session)) = parsed else {
            // Not a usage line (a user turn, a summary): neither counted nor skipped,
            // unless it claimed usage and couldn't be read.
            if v.get("message").and_then(|m| m.get("usage")).is_some()
                || v.pointer("/payload/type").and_then(Value::as_str) == Some("token_count")
            {
                out.skipped_lines += 1;
            }
            continue;
        };
        if entry.tally.total() == 0 {
            continue;
        }
        if let Some(key) = &entry.dedupe {
            if !seen.insert(key.clone()) {
                continue;
            }
        }
        let day = entry.at.with_timezone(&tz).date_naive();
        if day < since {
            continue;
        }
        let folder = cwd.or_else(|| file_cwd.clone()).unwrap_or_default();
        let session = session.unwrap_or_else(|| file_session.clone());
        out.sessions
            .entry((src.engine_id.clone(), src.account_id.clone()))
            .or_default()
            .insert(session);
        out.cells
            .entry((day, src.engine_id.clone(), src.account_id.clone(), folder))
            .or_default()
            .add(&entry.tally);
    }
}

fn trim_line(buf: &[u8]) -> &[u8] {
    let mut end = buf.len();
    while end > 0 && (buf[end - 1] == b'\n' || buf[end - 1] == b'\r') {
        end -= 1;
    }
    &buf[..end]
}

/// One day of the heatmap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UsageDay {
    /// `YYYY-MM-DD` in the user's zone.
    pub date: String,
    pub tokens: u64,
    /// Engine id → tokens that day.
    pub by_engine: BTreeMap<String, u64>,
}

/// Days from `since` through `until`, every one present (zeros included).
pub fn days(scan: &UsageScan, since: NaiveDate, until: NaiveDate) -> Vec<UsageDay> {
    let mut map: BTreeMap<NaiveDate, UsageDay> = BTreeMap::new();
    let mut d = since;
    while d <= until {
        map.insert(
            d,
            UsageDay {
                date: d.format("%Y-%m-%d").to_string(),
                tokens: 0,
                by_engine: BTreeMap::new(),
            },
        );
        let Some(next) = d.succ_opt() else { break };
        d = next;
    }
    for ((day, engine, _, _), tally) in &scan.cells {
        if let Some(entry) = map.get_mut(day) {
            entry.tokens += tally.total();
            *entry.by_engine.entry(engine.clone()).or_default() += tally.total();
        }
    }
    map.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).expect("utc")
    }

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("date")
    }

    fn claude(ts: &str, id: &str, input: u64, output: u64, cwd: &str) -> String {
        serde_json::json!({
            "type": "assistant", "timestamp": ts, "cwd": cwd, "sessionId": "s1", "requestId": "r",
            "message": { "id": id, "model": "claude-sonnet", "content": [{"type": "text", "text": "secret prompt text"}],
                         "usage": { "input_tokens": input, "output_tokens": output, "cache_read_input_tokens": 10 } }
        })
        .to_string()
    }

    fn source(home: &Path, engine: &str, globs: &[&str]) -> LogSource {
        LogSource {
            engine_id: engine.into(),
            account_id: format!("acc_{engine}"),
            home: home.to_path_buf(),
            globs: globs.iter().map(|g| g.to_string()).collect(),
        }
    }

    #[test]
    fn f_ac_07_1_three_days_of_claude_logs_give_the_right_daily_totals() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("projects/-home-ana-shop");
        std::fs::create_dir_all(&p).expect("mkdir");
        let lines = [
            claude("2026-10-01T09:00:00Z", "m1", 100, 50, "/home/ana/shop"),
            // The same message again (another content block): counted once.
            claude("2026-10-01T09:00:01Z", "m1", 100, 50, "/home/ana/shop"),
            claude("2026-10-02T23:30:00Z", "m2", 200, 20, "/home/ana/shop"),
            claude("2026-10-03T08:00:00Z", "m3", 1, 2, "/home/ana/api"),
            serde_json::json!({"type": "user", "timestamp": "2026-10-03T08:00:00Z", "message": {"content": "hi"}}).to_string(),
        ];
        std::fs::write(p.join("a.jsonl"), lines.join("\n")).expect("write");
        let s = scan(
            &[source(dir.path(), "claude", &["projects/**/*.jsonl"])],
            date("2026-09-01"),
            utc(),
        );
        let d = days(&s, date("2026-10-01"), date("2026-10-04"));
        let totals: Vec<(String, u64)> = d.iter().map(|x| (x.date.clone(), x.tokens)).collect();
        assert_eq!(
            totals,
            vec![
                ("2026-10-01".into(), 160),
                ("2026-10-02".into(), 230),
                ("2026-10-03".into(), 13),
                ("2026-10-04".into(), 0),
            ]
        );
        assert_eq!(s.skipped_lines, 0);
        assert_eq!(s.files_read, 1);
        let folders: HashSet<&str> = s.cells.keys().map(|k| k.3.as_str()).collect();
        assert_eq!(folders, HashSet::from(["/home/ana/shop", "/home/ana/api"]));
    }

    #[test]
    fn f_ac_07_2_a_malformed_line_is_skipped_and_counted() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("projects/x");
        std::fs::create_dir_all(&p).expect("mkdir");
        let body = [
            claude("2026-10-01T09:00:00Z", "m1", 10, 10, "/w"),
            "{not json".to_string(),
            // Usage with a date nothing can read.
            claude("yesterday-ish", "m2", 5, 5, "/w"),
            claude("2026-10-01T10:00:00Z", "m3", 10, 10, "/w"),
        ];
        std::fs::write(p.join("a.jsonl"), body.join("\r\n")).expect("write");
        let s = scan(
            &[source(dir.path(), "claude", &["projects/**/*.jsonl"])],
            date("2026-09-01"),
            utc(),
        );
        assert_eq!(s.skipped_lines, 2);
        assert_eq!(
            days(&s, date("2026-10-01"), date("2026-10-01"))[0].tokens,
            60
        );
    }

    #[test]
    fn f_ac_07_3_non_ascii_dates_and_text_dont_crash_parsing() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("projects/ü");
        std::fs::create_dir_all(&p).expect("mkdir");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(claude("２０２６-10-01T09:00:00Z", "m1", 1, 1, "/w").as_bytes());
        bytes.push(b'\n');
        bytes.extend_from_slice(
            claude("2026-10-01T09:00:00+05:30", "m2", 3, 4, "/werkstätte/日本").as_bytes(),
        );
        bytes.push(b'\n');
        bytes.extend_from_slice(&[0xff, 0xfe, b'{', b'\n']);
        bytes.extend_from_slice("1 октября 2026 г.\n".as_bytes());
        std::fs::write(p.join("ü.jsonl"), bytes).expect("write");
        // A zone east of UTC: 09:00+05:30 is still 1 October there.
        let ist = FixedOffset::east_opt(5 * 3600 + 1800).expect("ist");
        let s = scan(
            &[source(dir.path(), "claude", &["projects/**/*.jsonl"])],
            date("2026-09-01"),
            ist,
        );
        assert_eq!(s.skipped_lines, 3);
        let d = days(&s, date("2026-10-01"), date("2026-10-01"));
        assert_eq!(d[0].tokens, 17);
        assert!(s.cells.keys().any(|k| k.3 == "/werkstätte/日本"));
    }

    #[test]
    fn codex_sessions_count_token_events_in_the_session_folder() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("sessions/2026/10/01");
        std::fs::create_dir_all(&p).expect("mkdir");
        let lines = [
            serde_json::json!({"timestamp": "2026-10-01T09:00:00Z", "type": "session_meta",
                               "payload": {"id": "sess-1", "cwd": "/home/ana/shop"}}),
            serde_json::json!({"timestamp": "2026-10-01T09:01:00Z", "type": "response_item",
                               "payload": {"type": "message", "content": [{"text": "secret"}]}}),
            serde_json::json!({"timestamp": "2026-10-01T09:02:00Z", "type": "event_msg",
                               "payload": {"type": "token_count", "info": {"last_token_usage":
                                   {"input_tokens": 1000, "cached_input_tokens": 400, "output_tokens": 50, "reasoning_output_tokens": 25}}}}),
            serde_json::json!({"timestamp": "2026-10-01T09:03:00Z", "type": "event_msg",
                               "payload": {"type": "token_count", "info": null}}),
        ];
        let body: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        std::fs::write(p.join("rollout.jsonl"), body.join("\n")).expect("write");
        let s = scan(
            &[source(dir.path(), "codex", &["sessions/**/*.jsonl"])],
            date("2026-09-01"),
            utc(),
        );
        let (key, tally) = s.cells.iter().next().expect("a cell");
        assert_eq!(key.3, "/home/ana/shop");
        assert_eq!(
            tally,
            &Tally {
                input: 600,
                output: 75,
                cache: 400
            }
        );
        assert_eq!(s.skipped_lines, 1, "a token_count without counts");
        assert_eq!(
            s.sessions[&("codex".to_string(), "acc_codex".to_string())].len(),
            1
        );
    }

    #[test]
    fn globs_stay_inside_the_home() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir_all(dir.path().join("projects/a/b")).expect("mkdir");
        std::fs::write(dir.path().join("projects/a/b/x.jsonl"), "").expect("w");
        std::fs::write(dir.path().join("projects/a/y.txt"), "").expect("w");
        assert_eq!(files_for(dir.path(), "projects/**/*.jsonl").len(), 1);
        assert!(files_for(dir.path(), "../**/*.jsonl").is_empty());
        assert!(files_for(dir.path(), "projects/a/*.jsonl").is_empty());
    }

    #[test]
    fn old_days_are_left_out() {
        let dir = tempfile::tempdir().expect("tmp");
        let p = dir.path().join("projects/x");
        std::fs::create_dir_all(&p).expect("mkdir");
        std::fs::write(
            p.join("a.jsonl"),
            claude("2026-01-01T00:00:00Z", "m1", 9, 9, "/w"),
        )
        .expect("w");
        let s = scan(
            &[source(dir.path(), "claude", &["projects/**/*.jsonl"])],
            date("2026-07-01"),
            utc(),
        );
        assert!(s.cells.is_empty());
    }
}
