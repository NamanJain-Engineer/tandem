//! F-RT-01: cron parsing, schedules in words and DST-correct next runs.

use chrono::{DateTime, Datelike, TimeZone, Utc, Weekday};
use chrono_tz::Tz;
use tandem_routines::cron::CronExpr;
use tandem_routines::schedule::*;
use tandem_routines::ScheduleError;

fn utc(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .expect("time")
        .with_timezone(&Utc)
}

fn cron(expr: &str, tz: &str) -> ScheduleSpec {
    ScheduleSpec {
        schedule: Schedule::Cron { expr: expr.into() },
        timezone: tz.into(),
    }
}

fn daily(times: &[&str], tz: &str) -> ScheduleSpec {
    ScheduleSpec {
        schedule: Schedule::Daily {
            times: times.iter().map(|t| t.to_string()).collect(),
        },
        timezone: tz.into(),
    }
}

fn words_en(s: &Schedule) -> String {
    describe_en(&words(s).expect("words"))
}

fn err(expr: &str) -> String {
    match CronExpr::parse(expr) {
        Err(ScheduleError::Invalid(m)) => m,
        Ok(_) => panic!("{expr} should be refused"),
    }
}

#[test]
fn f_rt_01_1_weekdays_at_9_in_words_and_next_runs_skip_the_weekend() {
    let s = cron("0 9 * * 1-5", "America/New_York");
    assert_eq!(words_en(&s.schedule), "Weekdays at 9:00 AM");
    // Friday 2026-10-09 10:00 local: the next runs are Mon, Tue, Wed.
    let from = utc("2026-10-09T14:00:00Z");
    let next = s.next_n(from, 3).expect("next");
    let tz: Tz = "America/New_York".parse().expect("tz");
    let days: Vec<Weekday> = next
        .iter()
        .map(|t| t.with_timezone(&tz).weekday())
        .collect();
    assert_eq!(days, [Weekday::Mon, Weekday::Tue, Weekday::Wed]);
    for t in &next {
        assert_eq!(t.with_timezone(&tz).format("%H:%M").to_string(), "09:00");
    }
    // Ten weeks of runs never land on a weekend.
    for t in s.next_n(from, 50).expect("next") {
        let d = t.with_timezone(&tz).weekday();
        assert!(!matches!(d, Weekday::Sat | Weekday::Sun), "{t}");
    }
}

#[test]
fn f_rt_01_2_weekend_range_ending_in_sunday_runs_saturday_and_sunday() {
    let s = cron("0 10 * * 6-7", "UTC");
    assert_eq!(words_en(&s.schedule), "Weekends at 10:00 AM");
    let next = s.next_n(utc("2026-10-05T00:00:00Z"), 4).expect("next");
    let days: Vec<Weekday> = next.iter().map(|t| t.weekday()).collect();
    assert_eq!(
        days,
        [Weekday::Sat, Weekday::Sun, Weekday::Sat, Weekday::Sun]
    );
    // SUN as 0, 7 and the name are the same day.
    for expr in ["0 10 * * 0", "0 10 * * 7", "0 10 * * SUN", "0 10 * * sun"] {
        let c = CronExpr::parse(expr).expect(expr);
        assert_eq!(c.weekday_values(), vec![0], "{expr}");
    }
    for expr in ["0 10 * * SAT-SUN", "0 10 * * 6-0", "0 10 * * sat,sun"] {
        assert_eq!(
            CronExpr::parse(expr).expect(expr).weekday_values(),
            vec![0, 6],
            "{expr}"
        );
    }
    assert_eq!(
        CronExpr::parse("0 10 * * 5-7")
            .expect("5-7")
            .weekday_values(),
        vec![0, 5, 6]
    );
}

#[test]
fn f_rt_01_3_spring_forward_runs_2_30_at_3_30_and_never_twice() {
    // US: 2026-03-08, 2:00 → 3:00 in New York.
    let s = daily(&["02:30"], "America/New_York");
    let runs = s.next_n(utc("2026-03-06T12:00:00Z"), 4).expect("next");
    let tz: Tz = "America/New_York".parse().expect("tz");
    let local: Vec<String> = runs
        .iter()
        .map(|t| t.with_timezone(&tz).format("%m-%d %H:%M %Z").to_string())
        .collect();
    assert_eq!(
        local,
        [
            "03-07 02:30 EST",
            "03-08 03:30 EDT",
            "03-09 02:30 EDT",
            "03-10 02:30 EDT"
        ]
    );
    // One run on the day of the change.
    assert_eq!(
        runs.iter()
            .filter(|t| t.with_timezone(&tz).day() == 8)
            .count(),
        1
    );
    // The same in Europe (2026-03-29, 2:00 → 3:00 in Berlin) and the
    // southern hemisphere (2026-10-04, 2:00 → 3:00 in Sydney).
    for (zone, from, want) in [
        ("Europe/Berlin", "2026-03-28T12:00:00Z", "03-29 03:30 CEST"),
        (
            "Australia/Sydney",
            "2026-10-03T00:00:00Z",
            "10-04 03:30 AEDT",
        ),
    ] {
        let s = daily(&["02:30"], zone);
        let tz: Tz = zone.parse().expect("tz");
        let next = s.next_after(utc(from)).expect("ok").expect("a run");
        assert_eq!(
            next.with_timezone(&tz).format("%m-%d %H:%M %Z").to_string(),
            want
        );
    }
    // A routine at both 2:30 and 3:30 runs once at 3:30 that day.
    let both = daily(&["02:30", "03:30"], "America/New_York");
    let runs = both.next_n(utc("2026-03-08T05:00:00Z"), 2).expect("next");
    assert_eq!(runs[0], utc("2026-03-08T07:30:00Z"));
    assert_eq!(runs[1], utc("2026-03-09T06:30:00Z"));
    // Cron follows the same rule.
    let c = cron("30 2 * * *", "America/New_York");
    assert_eq!(
        c.next_after(utc("2026-03-08T05:00:00Z")).expect("ok"),
        Some(utc("2026-03-08T07:30:00Z"))
    );
}

#[test]
fn f_rt_01_3_fall_back_runs_an_ambiguous_time_once_at_its_first_occurrence() {
    // US: 2026-11-01, 2:00 → 1:00 in New York; 1:30 happens twice.
    let s = daily(&["01:30"], "America/New_York");
    let runs = s.next_n(utc("2026-10-31T12:00:00Z"), 3).expect("next");
    assert_eq!(
        runs[0],
        utc("2026-11-01T05:30:00Z"),
        "first occurrence (EDT)"
    );
    assert_eq!(
        runs[1],
        utc("2026-11-02T06:30:00Z"),
        "then the next day (EST)"
    );
    // Hourly at :30 skips the repeated hour's second 1:30.
    let hourly = cron("30 * * * *", "America/New_York");
    let runs = hourly.next_n(utc("2026-11-01T04:00:00Z"), 3).expect("next");
    assert_eq!(
        runs,
        [
            utc("2026-11-01T04:30:00Z"),
            utc("2026-11-01T05:30:00Z"),
            utc("2026-11-01T07:30:00Z")
        ]
    );
}

#[test]
fn f_rt_01_cron_table() {
    struct Case {
        expr: &'static str,
        minutes: Vec<u32>,
        hours: Vec<u32>,
        weekdays: Vec<u8>,
    }
    let all_days: Vec<u8> = (0..7).collect();
    let cases = [
        Case {
            expr: "* * * * *",
            minutes: (0..60).collect(),
            hours: (0..24).collect(),
            weekdays: all_days.clone(),
        },
        Case {
            expr: "*/15 * * * *",
            minutes: vec![0, 15, 30, 45],
            hours: (0..24).collect(),
            weekdays: all_days.clone(),
        },
        Case {
            expr: "5,35 9-11 * * *",
            minutes: vec![5, 35],
            hours: vec![9, 10, 11],
            weekdays: all_days.clone(),
        },
        Case {
            expr: "0 8-18/4 * * *",
            minutes: vec![0],
            hours: vec![8, 12, 16],
            weekdays: all_days.clone(),
        },
        Case {
            expr: "0 22/1 * * *",
            minutes: vec![0],
            hours: vec![22, 23],
            weekdays: all_days.clone(),
        },
        Case {
            expr: "0 0 * * MON,WED,FRI",
            minutes: vec![0],
            hours: vec![0],
            weekdays: vec![1, 3, 5],
        },
        Case {
            expr: "0 0 * * mon-fri",
            minutes: vec![0],
            hours: vec![0],
            weekdays: vec![1, 2, 3, 4, 5],
        },
        Case {
            expr: "0 0 * * */2",
            minutes: vec![0],
            hours: vec![0],
            weekdays: vec![0, 2, 4, 6],
        },
        Case {
            expr: "59 23 * * 0-7",
            minutes: vec![59],
            hours: vec![23],
            weekdays: all_days.clone(),
        },
        Case {
            expr: "0 12 * JAN-MAR 1",
            minutes: vec![0],
            hours: vec![12],
            weekdays: vec![1],
        },
    ];
    for c in cases {
        let p = CronExpr::parse(c.expr).unwrap_or_else(|e| panic!("{}: {e}", c.expr));
        assert_eq!(p.minute_values(), c.minutes, "{} minutes", c.expr);
        assert_eq!(p.hour_values(), c.hours, "{} hours", c.expr);
        assert_eq!(p.weekday_values(), c.weekdays, "{} weekdays", c.expr);
    }
    // Months by name and number.
    let q = CronExpr::parse("0 0 1 JAN,APR,JUL,OCT *").expect("quarterly");
    assert_eq!(q, CronExpr::parse("0 0 1 1,4,7,10 *").expect("numbers"));
}

#[test]
fn f_rt_01_oversized_steps_and_bad_fields_are_refused_with_a_message() {
    assert_eq!(
        err("*/61 * * * *"),
        "Step 61 is larger than the minute range (0–59)"
    );
    assert_eq!(
        err("0 */25 * * *"),
        "Step 25 is larger than the hour range (0–23)"
    );
    assert_eq!(
        err("0 0 */32 * *"),
        "Step 32 is larger than the day-of-month range (1–31)"
    );
    assert_eq!(
        err("0 0 * */13 *"),
        "Step 13 is larger than the month range (1–12)"
    );
    assert_eq!(
        err("0 0 * * */8"),
        "Step 8 is larger than the day-of-week range (0–6)"
    );
    assert_eq!(err("*/0 * * * *"), "A step must be at least 1");
    assert_eq!(err("60 * * * *"), "60 is outside the minute range (0–59)");
    assert_eq!(err("0 24 * * *"), "24 is outside the hour range (0–23)");
    assert_eq!(
        err("0 0 0 * *"),
        "0 is outside the day-of-month range (1–31)"
    );
    assert_eq!(err("0 0 * 13 *"), "13 is outside the month range (1–12)");
    assert_eq!(err("0 0 * * 8"), "8 is outside the day-of-week range (0–7)");
    assert_eq!(
        err("0 9 * *"),
        "A cron expression has 5 fields (minute hour day month weekday); this one has 4"
    );
    assert_eq!(
        err("0 9 * * * *"),
        "A cron expression has 5 fields (minute hour day month weekday); this one has 6"
    );
    assert_eq!(
        err("5-1 * * * *"),
        "The range 5-1 in the minute field runs backwards"
    );
    assert_eq!(err("a * * * *"), "\"a\" isn't a valid minute value");
    assert_eq!(err("1,,2 * * * *"), "The minute field has an empty item");
    assert_eq!(
        err("0 0 * * FUNDAY"),
        "\"FUNDAY\" isn't a valid day-of-week value"
    );
}

#[test]
fn f_rt_01_day_of_month_or_day_of_week() {
    // Both restricted: either matches (the 13th, or any Friday).
    let s = cron("0 9 13 * 5", "UTC");
    let next = s.next_n(utc("2026-10-01T00:00:00Z"), 4).expect("next");
    let days: Vec<u32> = next.iter().map(|t| t.day()).collect();
    assert_eq!(days, [2, 9, 13, 16]);
    // Only day of month.
    let monthly = cron("0 9 1 * *", "UTC");
    assert_eq!(
        monthly.next_after(utc("2026-10-09T00:00:00Z")).expect("ok"),
        Some(utc("2026-11-01T09:00:00Z"))
    );
    // February 29 waits for a leap year.
    let leap = cron("0 9 29 2 *", "UTC");
    assert_eq!(
        leap.next_after(utc("2026-10-09T00:00:00Z")).expect("ok"),
        Some(utc("2028-02-29T09:00:00Z"))
    );
    // February 30 never comes.
    let never = cron("0 9 30 2 *", "UTC");
    assert_eq!(
        never.next_after(utc("2026-10-09T00:00:00Z")).expect("ok"),
        None
    );
}

#[test]
fn f_rt_01_schedules_in_words() {
    let s = |expr: &str| words_en(&Schedule::Cron { expr: expr.into() });
    assert_eq!(s("0 9 * * 1-5"), "Weekdays at 9:00 AM");
    assert_eq!(s("0 10 * * 6-7"), "Weekends at 10:00 AM");
    assert_eq!(s("30 7 * * *"), "Every day at 7:30 AM");
    assert_eq!(s("0 9,17 * * *"), "Every day at 9:00 AM and 5:00 PM");
    assert_eq!(s("0 9 * * MON,THU"), "Mondays and Thursdays at 9:00 AM");
    assert_eq!(s("*/30 * * * *"), "Every 30 minutes");
    assert_eq!(s("15 * * * *"), "Every hour at :15");
    assert_eq!(s("0 0 * * 0"), "Sundays at 12:00 AM");
    assert_eq!(s("0 9 1 * *"), "Custom schedule (cron 0 9 1 * *)");
    assert_eq!(s("0  9  *  *  *"), "Every day at 9:00 AM");
    assert_eq!(
        words_en(&Schedule::Daily {
            times: vec!["12:00".into(), "07:30".into(), "18:45".into()]
        }),
        "Every day at 7:30 AM, 12:00 PM, and 6:45 PM"
    );
    assert_eq!(
        words_en(&Schedule::Weekly {
            days: vec![1, 2, 3, 4, 5],
            times: vec!["09:00".into()]
        }),
        "Weekdays at 9:00 AM"
    );
    assert_eq!(
        words_en(&Schedule::Interval { every_minutes: 45 }),
        "Every 45 minutes"
    );
    assert_eq!(
        words_en(&Schedule::Interval { every_minutes: 120 }),
        "Every 2 hours"
    );
}

#[test]
fn f_rt_01_intervals_daily_and_weekly() {
    let every = |m: u32| ScheduleSpec {
        schedule: Schedule::Interval { every_minutes: m },
        timezone: "UTC".into(),
    };
    assert_eq!(
        every(14).resolve().err(),
        Some(ScheduleError::Invalid(
            "Routines run at most every 15 minutes".into()
        ))
    );
    assert_eq!(
        every(30)
            .next_n(utc("2026-10-09T10:07:00Z"), 3)
            .expect("next"),
        [
            utc("2026-10-09T10:30:00Z"),
            utc("2026-10-09T11:00:00Z"),
            utc("2026-10-09T11:30:00Z")
        ]
    );
    // A cron that fires more often than every 15 minutes is refused too.
    assert!(cron("*/5 * * * *", "UTC").resolve().is_err());
    assert!(cron("0,10 9 * * *", "UTC").resolve().is_err());
    assert!(cron("*/15 * * * *", "UTC").resolve().is_ok());
    let weekly = ScheduleSpec {
        schedule: Schedule::Weekly {
            days: vec![2],
            times: vec!["08:15".into()],
        },
        timezone: "Asia/Kolkata".into(),
    };
    // Tuesday 08:15 IST = 02:45 UTC.
    assert_eq!(
        weekly.next_after(utc("2026-10-09T00:00:00Z")).expect("ok"),
        Some(utc("2026-10-13T02:45:00Z"))
    );
    assert!(daily(&["9:00"], "UTC").resolve().is_ok());
    for bad in ["25:00", "09:60", "0900", "09:0", "", "x:y"] {
        assert!(daily(&[bad], "UTC").resolve().is_err(), "{bad}");
    }
    assert!(daily(&["09:00"], "Mars/Olympus").resolve().is_err());
    assert!(daily(&[], "UTC").resolve().is_err());
    // Exactly at a run time, the next one is the day after.
    assert_eq!(
        daily(&["09:00"], "UTC")
            .next_after(utc("2026-10-09T09:00:00Z"))
            .expect("ok"),
        Some(utc("2026-10-10T09:00:00Z"))
    );
    let _ = Utc.timestamp_opt(0, 0);
}

#[test]
fn f_rt_01_full_access_must_be_chosen_explicitly() {
    use tandem_core::access::Access;
    use tandem_routines::{RoutineInput, RunMode, FULL_WARNING};
    let input = RoutineInput {
        id: None,
        teammate_id: "tm_1".into(),
        name: " Morning summary ".into(),
        instruction: "Summarize yesterday's merged pull requests in five bullets.".into(),
        schedule: cron("0 9 * * 1-5", "UTC"),
        access: Access::Edits,
        full_confirmed: false,
        run_mode: RunMode::InApp,
    };
    assert_eq!(input.validate().expect("valid"), "Morning summary");
    let full = RoutineInput {
        access: Access::Full,
        ..input.clone()
    };
    let err = full.validate().expect_err("needs the explicit choice");
    assert!(err.to_string().contains(FULL_WARNING));
    assert_eq!(FULL_WARNING, "This routine will act without asking.");
    let chosen = RoutineInput {
        full_confirmed: true,
        ..full
    };
    assert!(chosen.validate().is_ok());
    let long = RoutineInput {
        instruction: "x".repeat(8_001),
        ..input.clone()
    };
    assert_eq!(
        long.validate().err(),
        Some(ScheduleError::Invalid(
            "An instruction is at most 8,000 characters".into()
        ))
    );
    assert!(RoutineInput {
        instruction: "x".repeat(8_000),
        ..input.clone()
    }
    .validate()
    .is_ok());
    assert!(RoutineInput {
        name: "  ".into(),
        ..input.clone()
    }
    .validate()
    .is_err());
    assert!(RoutineInput {
        schedule: cron("*/61 * * * *", "UTC"),
        ..input
    }
    .validate()
    .is_err());
}
