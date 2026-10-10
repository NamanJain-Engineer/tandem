//! Routine schedules and DST-correct next runs (F-RT-01, ARCHITECTURE §10.1).
//!
//! Times are wall-clock times in the routine's IANA time zone. The rule for
//! daylight-saving changes (documented in the app and the docs):
//! - a wall time that doesn't exist that day (clocks spring forward over it)
//!   runs shifted forward by the gap: 2:30 AM runs at 3:30 AM;
//! - a wall time that happens twice (clocks fall back over it) runs once, at
//!   its first occurrence.
//!
//! Intervals don't follow the wall clock: "every 30 minutes" is every 30
//! minutes of real time, at multiples of the interval since the Unix epoch.

use chrono::{
    DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset,
    TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cron::CronExpr;
use crate::ScheduleError;

/// The shortest interval a routine may repeat at.
pub const MIN_INTERVAL_MINUTES: u32 = 15;
/// The longest (a week); longer schedules are weekly.
pub const MAX_INTERVAL_MINUTES: u32 = 7 * 24 * 60;
/// How far ahead a next run is looked for (a Feb 29 cron is the rarest).
const SEARCH_DAYS: i64 = 8 * 366;

/// What a routine's schedule is, as the form and the store keep it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Schedule {
    /// Every day at each of `times` (`HH:MM`).
    Daily { times: Vec<String> },
    /// On `days` (0 = Sunday … 6 = Saturday) at each of `times`.
    Weekly { days: Vec<u8>, times: Vec<String> },
    /// Every `every_minutes` (≥ 15).
    Interval { every_minutes: u32 },
    /// A 5-field cron expression.
    Cron { expr: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScheduleSpec {
    pub schedule: Schedule,
    /// IANA zone name (`Europe/Berlin`); the machine's zone when a routine
    /// is made.
    pub timezone: String,
}

/// A schedule in words, for the UI to say through i18n (UI keys
/// `routines.words.*`) and for [`describe_en`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum ScheduleWords {
    /// "Every day at 7:30 AM"
    EveryDay { times: Vec<String> },
    /// "Weekdays at 9:00 AM"
    Weekdays { times: Vec<String> },
    /// "Weekends at 10:00 AM"
    Weekends { times: Vec<String> },
    /// "Mondays and Thursdays at 9:00 AM"
    Days { days: Vec<u8>, times: Vec<String> },
    /// "Every 30 minutes"
    EveryMinutes { minutes: u32 },
    /// "Every hour at :15"
    Hourly { minute: u32 },
    /// "Custom schedule (cron 0 9 1 * *)"
    Cron { expr: String },
}

/// A parsed, ready-to-run schedule.
#[derive(Debug, Clone)]
pub struct Resolved {
    kind: Kind,
    tz: Tz,
}

#[derive(Debug, Clone)]
enum Kind {
    Days {
        /// Which dates run.
        on: DayRule,
        times: Vec<NaiveTime>,
    },
    Interval(u32),
}

#[derive(Debug, Clone)]
enum DayRule {
    Every,
    Weekdays(u8),
    Cron(CronExpr),
}

pub fn parse_time(raw: &str) -> Result<NaiveTime, ScheduleError> {
    let bad = || ScheduleError::Invalid(format!("\"{raw}\" isn't a time (HH:MM)"));
    let (h, m) = raw.split_once(':').ok_or_else(bad)?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(h) || h.len() > 2 || !digits(m) || m.len() != 2 {
        return Err(bad());
    }
    let (h, m): (u32, u32) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    NaiveTime::from_hms_opt(h, m, 0).ok_or_else(bad)
}

pub fn format_time(t: NaiveTime) -> String {
    format!("{:02}:{:02}", t.hour(), t.minute())
}

fn parse_times(raw: &[String]) -> Result<Vec<NaiveTime>, ScheduleError> {
    if raw.is_empty() {
        return Err(ScheduleError::Invalid("Pick at least one time".into()));
    }
    if raw.len() > 24 {
        return Err(ScheduleError::Invalid("At most 24 times a day".into()));
    }
    let mut out = raw
        .iter()
        .map(|t| parse_time(t))
        .collect::<Result<Vec<_>, _>>()?;
    out.sort();
    out.dedup();
    Ok(out)
}

pub fn parse_tz(name: &str) -> Result<Tz, ScheduleError> {
    name.parse::<Tz>()
        .map_err(|_| ScheduleError::Invalid(format!("\"{name}\" isn't a known time zone")))
}

impl ScheduleSpec {
    pub fn resolve(&self) -> Result<Resolved, ScheduleError> {
        let tz = parse_tz(&self.timezone)?;
        let kind = match &self.schedule {
            Schedule::Daily { times } => Kind::Days {
                on: DayRule::Every,
                times: parse_times(times)?,
            },
            Schedule::Weekly { days, times } => {
                if days.is_empty() {
                    return Err(ScheduleError::Invalid("Pick at least one day".into()));
                }
                let mut bits = 0u8;
                for d in days {
                    if *d > 6 {
                        return Err(ScheduleError::Invalid(format!(
                            "{d} isn't a day (0 = Sunday … 6 = Saturday)"
                        )));
                    }
                    bits |= 1 << d;
                }
                Kind::Days {
                    on: DayRule::Weekdays(bits),
                    times: parse_times(times)?,
                }
            }
            Schedule::Interval { every_minutes } => {
                if *every_minutes < MIN_INTERVAL_MINUTES {
                    return Err(ScheduleError::Invalid(format!(
                        "Routines run at most every {MIN_INTERVAL_MINUTES} minutes"
                    )));
                }
                if *every_minutes > MAX_INTERVAL_MINUTES {
                    return Err(ScheduleError::Invalid(
                        "An interval is at most a week; use a weekly schedule".into(),
                    ));
                }
                Kind::Interval(*every_minutes)
            }
            Schedule::Cron { expr } => {
                let cron = CronExpr::parse(expr)?;
                let times = cron.times();
                // A cron that fires more often than the interval floor would
                // get round the 15-minute minimum.
                if let Some(gap) = min_gap_minutes(&times) {
                    if gap < MIN_INTERVAL_MINUTES {
                        return Err(ScheduleError::Invalid(format!(
                            "Routines run at most every {MIN_INTERVAL_MINUTES} minutes; this runs every {gap}"
                        )));
                    }
                }
                Kind::Days {
                    on: DayRule::Cron(cron),
                    times,
                }
            }
        };
        let resolved = Resolved { kind, tz };
        Ok(resolved)
    }

    pub fn next_after(&self, t: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, ScheduleError> {
        Ok(self.resolve()?.next_after(t))
    }

    pub fn next_n(&self, t: DateTime<Utc>, n: usize) -> Result<Vec<DateTime<Utc>>, ScheduleError> {
        Ok(self.resolve()?.next_n(t, n))
    }
}

/// The smallest gap between consecutive run times within a day and across
/// midnight.
fn min_gap_minutes(times: &[NaiveTime]) -> Option<u32> {
    if times.len() < 2 {
        return None;
    }
    let mins: Vec<u32> = times.iter().map(|t| t.hour() * 60 + t.minute()).collect();
    let mut gap = mins[0] + 1440 - mins[mins.len() - 1];
    for w in mins.windows(2) {
        gap = gap.min(w[1] - w[0]);
    }
    Some(gap)
}

impl Resolved {
    pub fn tz(&self) -> Tz {
        self.tz
    }

    fn runs_on(&self, date: NaiveDate) -> bool {
        match &self.kind {
            Kind::Days { on, .. } => match on {
                DayRule::Every => true,
                DayRule::Weekdays(bits) => bits & (1 << date.weekday().num_days_from_sunday()) != 0,
                DayRule::Cron(c) => c.matches_date(date),
            },
            Kind::Interval(_) => true,
        }
    }

    /// The instant a local wall time runs at, by the DST rule above.
    pub fn instant(&self, local: NaiveDateTime) -> DateTime<Utc> {
        match self.tz.from_local_datetime(&local) {
            LocalResult::Single(t) => t.with_timezone(&Utc),
            LocalResult::Ambiguous(first, _) => first.with_timezone(&Utc),
            LocalResult::None => {
                // In a spring-forward gap: read the wall time with the offset
                // in force before the gap, which lands it the gap's length
                // later (2:30 → 3:30).
                let before = local - Duration::hours(24);
                let offset = match self.tz.from_local_datetime(&before) {
                    LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => {
                        t.offset().fix().local_minus_utc()
                    }
                    LocalResult::None => 0,
                };
                (local - Duration::seconds(i64::from(offset))).and_utc()
            }
        }
    }

    /// The first run strictly after `t`.
    pub fn next_after(&self, t: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match &self.kind {
            Kind::Interval(every) => {
                let step = i64::from(*every) * 60;
                let secs = t.timestamp();
                let next = (secs.div_euclid(step) + 1) * step;
                DateTime::from_timestamp(next, 0)
            }
            Kind::Days { times, .. } => {
                // Start a day early: a wall time on the previous local date
                // can be after `t` in UTC around a DST change.
                let start = t.with_timezone(&self.tz).date_naive() - Duration::days(1);
                for offset in 0..SEARCH_DAYS {
                    let date = start + Duration::days(offset);
                    if !self.runs_on(date) {
                        continue;
                    }
                    let mut best: Option<DateTime<Utc>> = None;
                    for time in times {
                        let at = self.instant(date.and_time(*time));
                        if at > t && best.is_none_or(|b| at < b) {
                            best = Some(at);
                        }
                    }
                    if best.is_some() {
                        return best;
                    }
                }
                None
            }
        }
    }

    pub fn next_n(&self, t: DateTime<Utc>, n: usize) -> Vec<DateTime<Utc>> {
        let mut out = Vec::with_capacity(n);
        let mut at = t;
        while out.len() < n {
            match self.next_after(at) {
                Some(next) => {
                    out.push(next);
                    at = next;
                }
                None => break,
            }
        }
        out
    }
}

fn times_of(times: &[NaiveTime]) -> Vec<String> {
    times.iter().map(|t| format_time(*t)).collect()
}

const WEEKDAYS: u8 = 0b0011_1110;
const WEEKENDS: u8 = 0b0100_0001;

/// The schedule in words.
pub fn words(schedule: &Schedule) -> Result<ScheduleWords, ScheduleError> {
    Ok(match schedule {
        Schedule::Daily { times } => ScheduleWords::EveryDay {
            times: times_of(&parse_times(times)?),
        },
        Schedule::Weekly { days, times } => {
            let times = times_of(&parse_times(times)?);
            let bits = days.iter().fold(0u8, |b, d| b | (1 << d.min(&6)));
            day_words(bits, times)
        }
        Schedule::Interval { every_minutes } => ScheduleWords::EveryMinutes {
            minutes: *every_minutes,
        },
        Schedule::Cron { expr } => {
            let c = CronExpr::parse(expr)?;
            let fallback = ScheduleWords::Cron {
                expr: expr.split_whitespace().collect::<Vec<_>>().join(" "),
            };
            if !c.all_months() || c.dom_restricted {
                return Ok(fallback);
            }
            let (mins, hours) = (c.minute_values(), c.hour_values());
            if hours.len() == 24 && !c.dow_restricted {
                if mins.len() == 1 {
                    return Ok(ScheduleWords::Hourly { minute: mins[0] });
                }
                // `*/n` with n dividing the hour.
                if let Some(step) = even_step(&mins) {
                    return Ok(ScheduleWords::EveryMinutes { minutes: step });
                }
                return Ok(fallback);
            }
            let times = c.times();
            if times.is_empty() || times.len() > 6 {
                return Ok(fallback);
            }
            day_words(c.days_of_week, times_of(&times))
        }
    })
}

fn even_step(mins: &[u32]) -> Option<u32> {
    if mins.len() < 2 || mins[0] != 0 {
        return None;
    }
    let step = mins[1];
    (60 % step == 0
        && mins.iter().enumerate().all(|(i, m)| *m == i as u32 * step)
        && mins.len() as u32 == 60 / step)
        .then_some(step)
}

fn day_words(bits: u8, times: Vec<String>) -> ScheduleWords {
    match bits & 0x7f {
        0x7f => ScheduleWords::EveryDay { times },
        WEEKDAYS => ScheduleWords::Weekdays { times },
        WEEKENDS => ScheduleWords::Weekends { times },
        b => ScheduleWords::Days {
            days: (0..7).filter(|d| b & (1 << d) != 0).collect(),
            times,
        },
    }
}

/// `09:00` → `9:00 AM`.
pub fn time_12h(raw: &str) -> String {
    let Ok(t) = parse_time(raw) else {
        return raw.to_string();
    };
    let (pm, h) = t.hour12();
    format!("{h}:{:02} {}", t.minute(), if pm { "PM" } else { "AM" })
}

fn list_en(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

const DAY_PLURALS: [&str; 7] = [
    "Sundays",
    "Mondays",
    "Tuesdays",
    "Wednesdays",
    "Thursdays",
    "Fridays",
    "Saturdays",
];

/// English words for a schedule ("Weekdays at 9:00 AM"); the UI says the
/// same through its i18n strings.
pub fn describe_en(w: &ScheduleWords) -> String {
    let at = |times: &[String]| list_en(&times.iter().map(|t| time_12h(t)).collect::<Vec<_>>());
    match w {
        ScheduleWords::EveryDay { times } => format!("Every day at {}", at(times)),
        ScheduleWords::Weekdays { times } => format!("Weekdays at {}", at(times)),
        ScheduleWords::Weekends { times } => format!("Weekends at {}", at(times)),
        ScheduleWords::Days { days, times } => {
            let names: Vec<String> = days
                .iter()
                .map(|d| DAY_PLURALS[usize::from(*d % 7)].to_string())
                .collect();
            format!("{} at {}", list_en(&names), at(times))
        }
        ScheduleWords::EveryMinutes { minutes } => {
            if minutes % 60 == 0 && *minutes >= 60 {
                let h = minutes / 60;
                if h == 1 {
                    "Every hour".into()
                } else {
                    format!("Every {h} hours")
                }
            } else {
                format!("Every {minutes} minutes")
            }
        }
        ScheduleWords::Hourly { minute } => format!("Every hour at :{minute:02}"),
        ScheduleWords::Cron { expr } => format!("Custom schedule (cron {expr})"),
    }
}

/// The machine's IANA time zone, else UTC.
pub fn local_timezone() -> String {
    iana_time_zone::get_timezone()
        .ok()
        .filter(|n| n.parse::<Tz>().is_ok())
        .unwrap_or_else(|| "UTC".into())
}
