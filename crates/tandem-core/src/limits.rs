//! Usage-limit messages from terminal engines (F-AC-05, ARCHITECTURE §6.7):
//! the line that matched one of the engine's `limit_patterns`, and when the
//! limit resets.

use std::sync::OnceLock;

use chrono::{DateTime, Duration, FixedOffset, NaiveTime, TimeZone};
use regex::Regex;

/// A limit message seen on a pane's screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitHit {
    /// The matching line, trimmed (at most 200 characters).
    pub message: String,
    /// When the limit resets (Unix ms), when the engine said.
    pub resets_at: Option<i64>,
}

/// `now` in the user's time zone, for times of day like "resets 4pm".
pub fn local(now: DateTime<chrono::Utc>) -> DateTime<FixedOffset> {
    now.with_timezone(&chrono::Local).fixed_offset()
}

/// Compiles an engine's `limit_patterns` (case-insensitive); bad ones are
/// skipped.
pub fn compile(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|p| {
            regex::RegexBuilder::new(p)
                .case_insensitive(true)
                .build()
                .ok()
        })
        .collect()
}

/// The last screen line that matches one of `patterns`, with the reset time
/// read from it and the two lines after it.
pub fn detect(screen: &str, patterns: &[Regex], now: DateTime<FixedOffset>) -> Option<LimitHit> {
    if patterns.is_empty() {
        return None;
    }
    let lines: Vec<&str> = screen.lines().collect();
    let i = lines
        .iter()
        .rposition(|l| patterns.iter().any(|p| p.is_match(l)))?;
    let message: String = lines[i]
        .trim()
        .trim_start_matches(['⏺', '●', '•', '│', '>', '*', '⚠', '✗', '×'])
        .trim()
        .chars()
        .take(200)
        .collect();
    let context = lines[i..(i + 3).min(lines.len())].join(" ");
    Some(LimitHit {
        message,
        resets_at: parse_resets_at(&context, now),
    })
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("built-in pattern"))
}

/// When a limit resets, from the engine's wording: an epoch after `|`
/// ("…limit reached|1760000000"), a delay ("try again in 2h 3m", "in 45
/// minutes", "in 30s"), or a time of day in the user's zone ("resets 4pm",
/// "resets at 4:12 PM", "reset at 16:12"; tomorrow when already past).
pub fn parse_resets_at(text: &str, now: DateTime<FixedOffset>) -> Option<i64> {
    static EPOCH: OnceLock<Regex> = OnceLock::new();
    static DELAY: OnceLock<Regex> = OnceLock::new();
    static PART: OnceLock<Regex> = OnceLock::new();
    static CLOCK: OnceLock<Regex> = OnceLock::new();
    if let Some(c) = re(&EPOCH, r"\|(\d{10})\b").captures(text) {
        return c[1].parse::<i64>().ok().map(|s| s * 1000);
    }
    let delay = re(
        &DELAY,
        r"(?i)\bin\s+((?:\d+\s*(?:hours?|hrs?|h|minutes?|mins?|m|seconds?|secs?|s)\b[\s,]*(?:and\s+)?)+)",
    );
    if let Some(c) = delay.captures(text) {
        let mut total = Duration::zero();
        for p in re(&PART, r"(?i)(\d+)\s*([hms])").captures_iter(&c[1]) {
            let n: i64 = p[1].parse().ok()?;
            total += match p[2].to_ascii_lowercase().as_str() {
                "h" => Duration::hours(n),
                "m" => Duration::minutes(n),
                _ => Duration::seconds(n),
            };
        }
        return (total > Duration::zero()).then(|| (now + total).timestamp_millis());
    }
    let clock = re(
        &CLOCK,
        r"(?i)\bresets?\s+(?:at\s+)?(\d{1,2})(?::(\d{2}))?\s*(?:([ap])\.?\s*m\.?)?(?:\W|$)",
    );
    let c = clock.captures(text)?;
    let mut hour: u32 = c[1].parse().ok()?;
    let minute: u32 = c.get(2).map_or(Some(0), |m| m.as_str().parse().ok())?;
    let pm = c.get(3).map(|m| m.as_str().eq_ignore_ascii_case("p"));
    match pm {
        Some(_) if hour == 0 || hour > 12 => return None,
        Some(pm) => hour = hour % 12 + if pm { 12 } else { 0 },
        None if hour > 23 => return None,
        None => {}
    }
    let time = NaiveTime::from_hms_opt(hour, minute, 0)?;
    let today = now.date_naive().and_time(time);
    let mut at = now.offset().from_local_datetime(&today).single()?;
    if at <= now {
        at += Duration::days(1);
    }
    Some(at.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<FixedOffset> {
        // 2026-10-07 14:24:00 at UTC-7.
        DateTime::parse_from_rfc3339("2026-10-07T14:24:00-07:00").expect("now")
    }

    fn at(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s)
            .expect("time")
            .timestamp_millis()
    }

    #[test]
    fn f_ac_05_reset_times_in_the_engines_own_words() {
        let n = now();
        assert_eq!(
            parse_resets_at("Claude AI usage limit reached|1791418440", n),
            Some(1_791_418_440_000)
        );
        assert_eq!(
            parse_resets_at("Rate limited. Try again in 2h 3m.", n),
            Some(at("2026-10-07T16:27:00-07:00"))
        );
        assert_eq!(
            parse_resets_at("try again in 45 minutes", n),
            Some(at("2026-10-07T15:09:00-07:00"))
        );
        assert_eq!(
            parse_resets_at("retry in 30s", n),
            Some(at("2026-10-07T14:24:30-07:00"))
        );
        assert_eq!(
            parse_resets_at("5-hour limit reached ∙ resets 4pm", n),
            Some(at("2026-10-07T16:00:00-07:00"))
        );
        assert_eq!(
            parse_resets_at("Your limit resets at 4:12 PM.", n),
            Some(at("2026-10-07T16:12:00-07:00"))
        );
        assert_eq!(
            parse_resets_at("limit will reset at 16:12", n),
            Some(at("2026-10-07T16:12:00-07:00"))
        );
        // Already past today: tomorrow.
        assert_eq!(
            parse_resets_at("resets 9am", n),
            Some(at("2026-10-08T09:00:00-07:00"))
        );
        assert_eq!(parse_resets_at("usage limit reached", n), None);
        assert_eq!(parse_resets_at("resets 25:00", n), None);
    }

    #[test]
    fn f_ac_05_1_the_last_matching_line_is_the_limit() {
        let patterns = compile(&["usage limit reached".into(), r"\b429\b".into(), "(".into()]);
        assert_eq!(patterns.len(), 2, "a bad pattern is skipped");
        let screen = "> fix the parser\n⏺ Working…\n⎿ Claude usage limit reached\n  Your limit resets at 4:12 PM.\n> ";
        let hit = detect(screen, &patterns, now()).expect("hit");
        assert_eq!(hit.message, "⎿ Claude usage limit reached");
        assert_eq!(hit.resets_at, Some(at("2026-10-07T16:12:00-07:00")));
        assert_eq!(detect("> all good\n", &patterns, now()), None);
        assert_eq!(detect(screen, &[], now()), None);
    }
}
