//! Standard 5-field cron (F-RT-01, ARCHITECTURE §10.1): minute, hour, day of
//! month, month, day of week. Each field takes `*`, numbers, ranges `a-b`,
//! steps `*/n` `a-b/n` `a/n`, and comma lists; months and days also take
//! names (`JAN`, `MON`). Day of week is 0–7 with both 0 and 7 Sunday, so
//! `6-7` is Saturday and Sunday. When both day of month and day of week are
//! restricted, a day matching either runs (the usual cron rule).

use chrono::{Datelike, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};

use crate::ScheduleError;

/// A parsed expression: one bit per allowed value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CronExpr {
    pub minutes: u64,
    pub hours: u32,
    /// Bits 1–31.
    pub days_of_month: u32,
    /// Bits 1–12.
    pub months: u16,
    /// Bits 0–6, Sunday = 0.
    pub days_of_week: u8,
    pub dom_restricted: bool,
    pub dow_restricted: bool,
}

struct Field {
    name: &'static str,
    min: u32,
    max: u32,
    names: &'static [&'static str],
}

const MINUTE: Field = Field {
    name: "minute",
    min: 0,
    max: 59,
    names: &[],
};
const HOUR: Field = Field {
    name: "hour",
    min: 0,
    max: 23,
    names: &[],
};
const DOM: Field = Field {
    name: "day-of-month",
    min: 1,
    max: 31,
    names: &[],
};
const MONTH: Field = Field {
    name: "month",
    min: 1,
    max: 12,
    names: &[
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ],
};
/// 0–7: both ends are Sunday.
const DOW: Field = Field {
    name: "day-of-week",
    min: 0,
    max: 7,
    names: &["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"],
};

fn invalid(msg: impl Into<String>) -> ScheduleError {
    ScheduleError::Invalid(msg.into())
}

fn value(f: &Field, raw: &str) -> Result<u32, ScheduleError> {
    let upper = raw.to_ascii_uppercase();
    if let Some(i) = f.names.iter().position(|n| *n == upper) {
        // Month names start at 1, day names at 0 (Sunday).
        return Ok(if f.name == "month" {
            i as u32 + 1
        } else {
            i as u32
        });
    }
    let n: u32 = raw
        .parse()
        .map_err(|_| invalid(format!("\"{raw}\" isn't a valid {} value", f.name)))?;
    if n < f.min || n > f.max {
        return Err(invalid(format!(
            "{n} is outside the {} range ({}–{})",
            f.name, f.min, f.max
        )));
    }
    Ok(n)
}

/// Bits for one field; `restricted` is false only for a bare `*`.
fn field(f: &Field, raw: &str) -> Result<(u64, bool), ScheduleError> {
    if raw.is_empty() {
        return Err(invalid(format!("The {} field is empty", f.name)));
    }
    let mut bits = 0u64;
    let mut restricted = true;
    for part in raw.split(',') {
        if part.is_empty() {
            return Err(invalid(format!("The {} field has an empty item", f.name)));
        }
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => {
                let step: u32 = s
                    .parse()
                    .map_err(|_| invalid(format!("\"{s}\" isn't a valid step")))?;
                if step == 0 {
                    return Err(invalid("A step must be at least 1"));
                }
                let span = f.max - f.min + 1;
                // The day-of-week field counts 7 values, not 8.
                let span = if f.name == "day-of-week" { 7 } else { span };
                if step > span {
                    let max = if f.name == "day-of-week" { 6 } else { f.max };
                    return Err(invalid(format!(
                        "Step {step} is larger than the {} range ({}–{})",
                        f.name, f.min, max
                    )));
                }
                (r, step)
            }
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            if step == 1 && raw == "*" {
                restricted = false;
            }
            let hi = if f.name == "day-of-week" { 6 } else { f.max };
            (f.min, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            let (a, mut b) = (value(f, a)?, value(f, b)?);
            // A weekday range ending in Sunday (`SAT-SUN`, `5-0`) ends at 7.
            if f.name == "day-of-week" && b == 0 && a > 0 {
                b = 7;
            }
            if a > b {
                return Err(invalid(format!(
                    "The range {a}-{b} in the {} field runs backwards",
                    f.name
                )));
            }
            (a, b)
        } else {
            let a = value(f, range)?;
            // `a/n` means from a to the end of the range.
            let hi = if part.contains('/') {
                if f.name == "day-of-week" {
                    6
                } else {
                    f.max
                }
            } else {
                a
            };
            (a, hi.max(a))
        };
        let mut v = lo;
        while v <= hi {
            bits |= 1 << v;
            v += step;
        }
    }
    Ok((bits, restricted))
}

impl CronExpr {
    pub fn parse(expr: &str) -> Result<Self, ScheduleError> {
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(invalid(format!(
                "A cron expression has 5 fields (minute hour day month weekday); this one has {}",
                parts.len()
            )));
        }
        let (minutes, _) = field(&MINUTE, parts[0])?;
        let (hours, _) = field(&HOUR, parts[1])?;
        let (dom, dom_restricted) = field(&DOM, parts[2])?;
        let (months, _) = field(&MONTH, parts[3])?;
        let (dow, dow_restricted) = field(&DOW, parts[4])?;
        // 7 is Sunday too.
        let dow = (dow | (dow >> 7)) & 0x7f;
        Ok(Self {
            minutes,
            hours: hours as u32,
            days_of_month: dom as u32,
            months: months as u16,
            days_of_week: dow as u8,
            dom_restricted,
            dow_restricted,
        })
    }

    /// True when the expression runs on `date`.
    pub fn matches_date(&self, date: NaiveDate) -> bool {
        if self.months & (1 << date.month()) == 0 {
            return false;
        }
        let dom = self.days_of_month & (1 << date.day()) != 0;
        let dow = self.days_of_week & (1 << date.weekday().num_days_from_sunday()) != 0;
        match (self.dom_restricted, self.dow_restricted) {
            (true, true) => dom || dow,
            (true, false) => dom,
            (false, true) => dow,
            (false, false) => true,
        }
    }

    /// The day's run times, ascending.
    pub fn times(&self) -> Vec<NaiveTime> {
        let mut out = Vec::new();
        for h in 0..24 {
            if self.hours & (1 << h) == 0 {
                continue;
            }
            for m in 0..60 {
                if self.minutes & (1 << m) != 0 {
                    if let Some(t) = NaiveTime::from_hms_opt(h, m, 0) {
                        out.push(t);
                    }
                }
            }
        }
        out
    }

    pub fn minute_values(&self) -> Vec<u32> {
        (0..60).filter(|m| self.minutes & (1 << m) != 0).collect()
    }

    pub fn hour_values(&self) -> Vec<u32> {
        (0..24).filter(|h| self.hours & (1 << h) != 0).collect()
    }

    pub fn weekday_values(&self) -> Vec<u8> {
        (0..7)
            .filter(|d| self.days_of_week & (1 << d) != 0)
            .collect()
    }

    pub fn all_months(&self) -> bool {
        self.months == 0b1_1111_1111_1110
    }
}
