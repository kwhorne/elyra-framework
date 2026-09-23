//! Five-field cron expressions, evaluated in a time zone with DST.
//!
//! `minute hour day-of-month month day-of-week`, like crontab and Laravel's
//! `->cron()`. Each field takes `*`, a value, a range `a-b`, a step `*/n` or
//! `a-b/n`, and comma-separated lists of those. Months and weekdays also take
//! three-letter names (`jan`, `mon`); weekday `0` and `7` are both Sunday. When
//! both day fields are restricted, a day matches if **either** does — the
//! classic cron rule (`0 9 1 * mon` = the 1st of the month *and* every Monday).

use std::fmt;

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;
use jiff::{ToSpan, Zoned};

/// A parsed five-field cron expression.
#[derive(Clone, PartialEq, Eq)]
pub struct Cron {
    source: String,
    minutes: u64,
    hours: u64,
    days: u64,
    months: u64,
    weekdays: u64,
    /// Whether day-of-month / day-of-week were `*` (for the either-matches rule).
    any_day: bool,
    any_weekday: bool,
}

/// Why an expression didn't parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid cron expression `{expression}`: {reason}")]
pub struct CronError {
    expression: String,
    reason: String,
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const WEEKDAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

impl Cron {
    /// Parse an expression like `"*/15 9-17 * * mon-fri"`.
    pub fn parse(expression: &str) -> Result<Self, CronError> {
        let fail = |reason: String| CronError {
            expression: expression.to_owned(),
            reason,
        };
        let fields: Vec<&str> = expression.split_whitespace().collect();
        if fields.len() != 5 {
            return Err(fail(format!(
                "expected 5 fields (minute hour day month weekday), got {}",
                fields.len()
            )));
        }
        let minutes = field(fields[0], 0, 59, None, "minute").map_err(fail)?;
        let hours = field(fields[1], 0, 23, None, "hour").map_err(fail)?;
        let days = field(fields[2], 1, 31, None, "day-of-month").map_err(fail)?;
        let months = field(fields[3], 1, 12, Some((&MONTHS, 1)), "month").map_err(fail)?;
        let mut weekdays =
            field(fields[4], 0, 7, Some((&WEEKDAYS, 0)), "day-of-week").map_err(fail)?;
        if weekdays & (1 << 7) != 0 {
            weekdays = (weekdays & !(1 << 7)) | 1; // 7 is Sunday too
        }
        Ok(Self {
            source: fields.join(" "),
            minutes,
            hours,
            days,
            months,
            weekdays,
            any_day: fields[2] == "*",
            any_weekday: fields[4] == "*",
        })
    }

    /// The expression as written (whitespace normalised).
    pub fn as_str(&self) -> &str {
        &self.source
    }

    fn day_matches(&self, date: Date) -> bool {
        let dom = self.days & (1 << date.day()) != 0;
        let dow = self.weekdays & (1 << date.weekday().to_sunday_zero_offset()) != 0;
        match (self.any_day, self.any_weekday) {
            (true, true) => true,
            (false, true) => dom,
            (true, false) => dow,
            (false, false) => dom || dow,
        }
    }

    /// The first time strictly after `after` (at minute resolution) that the
    /// expression matches, in `after`'s time zone. `None` if it never does
    /// (`0 0 30 feb *`).
    ///
    /// DST: a local time skipped by a spring-forward runs just after the gap
    /// (`02:30` on that day fires at `03:30`); a local time repeated by a
    /// fall-back fires once, at its first occurrence.
    pub fn next_after(&self, after: &Zoned) -> Option<Zoned> {
        let tz = after.time_zone().clone();
        let start = after.datetime();
        // Start at the next whole minute of the *local* clock.
        let mut dt = start
            .with()
            .second(0)
            .subsec_nanosecond(0)
            .build()
            .ok()?
            .checked_add(1.minute())
            .ok()?;
        // Five years covers every satisfiable expression (Feb 29 included).
        let limit = start.checked_add(5.years()).ok()?;
        while dt < limit {
            if self.months & (1 << dt.month()) == 0 {
                dt = first_of_next_month(dt.date())?;
                continue;
            }
            if !self.day_matches(dt.date()) {
                dt = dt.date().tomorrow().ok()?.to_datetime(Time::midnight());
                continue;
            }
            if self.hours & (1 << dt.hour()) == 0 {
                dt = dt
                    .with()
                    .minute(0)
                    .build()
                    .ok()?
                    .checked_add(1.hour())
                    .ok()?;
                continue;
            }
            if self.minutes & (1 << dt.minute()) == 0 {
                dt = dt.checked_add(1.minute()).ok()?;
                continue;
            }
            if let Some(zoned) = resolve(&tz, dt) {
                if zoned > *after {
                    return Some(zoned);
                }
            }
            dt = dt.checked_add(1.minute()).ok()?;
        }
        None
    }
}

/// Map a local wall-clock time to an instant: a gap moves forward past it, a
/// fold takes the earlier occurrence (RFC 5545's "compatible" rule).
fn resolve(tz: &TimeZone, dt: DateTime) -> Option<Zoned> {
    tz.to_ambiguous_zoned(dt).compatible().ok()
}

fn first_of_next_month(date: Date) -> Option<DateTime> {
    let first = date.first_of_month().checked_add(1.month()).ok()?;
    Some(first.to_datetime(Time::midnight()))
}

impl fmt::Debug for Cron {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cron({:?})", self.source)
    }
}

impl fmt::Display for Cron {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

impl std::str::FromStr for Cron {
    type Err = CronError;
    fn from_str(s: &str) -> Result<Self, CronError> {
        Cron::parse(s)
    }
}

/// Names a field accepts, and the value of the first name.
type Names = Option<(&'static [&'static str], u32)>;

/// Parse one field into a bitmask over `min..=max`.
fn field(src: &str, min: u32, max: u32, names: Names, what: &str) -> Result<u64, String> {
    let mut mask = 0u64;
    for part in src.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => {
                let step: u32 = step
                    .parse()
                    .map_err(|_| format!("{what}: `{step}` is not a step"))?;
                if step == 0 {
                    return Err(format!("{what}: a step of 0 never advances"));
                }
                (range, step)
            }
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (
                value(a, min, max, names, what)?,
                value(b, min, max, names, what)?,
            )
        } else {
            let v = value(range, min, max, names, what)?;
            // `5/15` means "from 5, every 15" (to the end of the range).
            (v, if step > 1 { max } else { v })
        };
        if lo > hi {
            return Err(format!("{what}: range `{range}` runs backwards"));
        }
        let mut v = lo;
        while v <= hi {
            mask |= 1 << v;
            v += step;
        }
    }
    Ok(mask)
}

fn value(src: &str, min: u32, max: u32, names: Names, what: &str) -> Result<u32, String> {
    if let Some((list, first)) = names {
        let lower = src.to_ascii_lowercase();
        if let Some(i) = list.iter().position(|n| *n == lower) {
            return Ok(first + i as u32);
        }
    }
    let v: u32 = src
        .parse()
        .map_err(|_| format!("{what}: `{src}` is not a number or a name"))?;
    if v < min || v > max {
        return Err(format!("{what}: {v} is outside {min}-{max}"));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oslo(s: &str) -> Zoned {
        format!("{s}[Europe/Oslo]").parse().unwrap()
    }

    fn next(expr: &str, after: &str) -> String {
        Cron::parse(expr)
            .unwrap()
            .next_after(&oslo(after))
            .map(|z| z.strftime("%Y-%m-%d %H:%M %:z").to_string())
            .unwrap_or_else(|| "never".into())
    }

    #[test]
    fn simple_times() {
        assert_eq!(
            next("0 9 * * *", "2026-06-10T08:00"),
            "2026-06-10 09:00 +02:00"
        );
        // Exactly at the time: strictly *after*, so tomorrow.
        assert_eq!(
            next("0 9 * * *", "2026-06-10T09:00"),
            "2026-06-11 09:00 +02:00"
        );
        assert_eq!(
            next("0 9 * * *", "2026-06-10T09:00:30"),
            "2026-06-11 09:00 +02:00"
        );
        assert_eq!(
            next("*/15 * * * *", "2026-06-10T08:07"),
            "2026-06-10 08:15 +02:00"
        );
        assert_eq!(
            next("30 * * * *", "2026-06-10T08:31"),
            "2026-06-10 09:30 +02:00"
        );
    }

    #[test]
    fn ranges_steps_lists_and_names() {
        // Weekdays 9-17 every 30 minutes; Friday 17:30 -> Monday 09:00.
        assert_eq!(
            next("*/30 9-17 * * mon-fri", "2026-06-12T17:31"),
            "2026-06-15 09:00 +02:00"
        );
        assert_eq!(
            next("0 8,12,18 * * *", "2026-06-10T12:00"),
            "2026-06-10 18:00 +02:00"
        );
        assert_eq!(
            next("5/20 * * * *", "2026-06-10T08:06"),
            "2026-06-10 08:25 +02:00"
        );
        assert_eq!(
            next("0 0 1 jan,jul *", "2026-06-10T00:00"),
            "2026-07-01 00:00 +02:00"
        );
        // 0 and 7 are both Sunday.
        assert_eq!(
            next("0 10 * * 0", "2026-06-10T00:00"),
            "2026-06-14 10:00 +02:00"
        );
        assert_eq!(
            next("0 10 * * 7", "2026-06-10T00:00"),
            "2026-06-14 10:00 +02:00"
        );
        assert_eq!(
            next("0 10 * * sun", "2026-06-10T00:00"),
            "2026-06-14 10:00 +02:00"
        );
    }

    #[test]
    fn both_day_fields_match_either() {
        // The 15th of the month OR any Monday, whichever comes first.
        assert_eq!(
            next("0 9 15 * mon", "2026-07-01T00:00"),
            "2026-07-06 09:00 +02:00"
        );
        assert_eq!(
            next("0 9 15 * mon", "2026-07-13T10:00"),
            "2026-07-15 09:00 +02:00"
        );
    }

    #[test]
    fn leap_days_and_impossible_dates() {
        assert_eq!(
            next("0 0 29 feb *", "2026-03-01T00:00"),
            "2028-02-29 00:00 +01:00"
        );
        assert_eq!(next("0 0 30 feb *", "2026-03-01T00:00"), "never");
    }

    #[test]
    fn dst_spring_forward_runs_just_after_the_gap() {
        // Europe/Oslo 2026-03-29: 02:00 -> 03:00. 02:30 doesn't exist that night.
        assert_eq!(
            next("30 2 * * *", "2026-03-29T01:00"),
            "2026-03-29 03:30 +02:00"
        );
        // And the next night is normal again.
        assert_eq!(
            next("30 2 * * *", "2026-03-29T04:00"),
            "2026-03-30 02:30 +02:00"
        );
    }

    #[test]
    fn dst_fall_back_runs_a_repeated_time_once() {
        // Europe/Oslo 2026-10-25: 03:00 -> 02:00, so 02:30 happens twice.
        let cron = Cron::parse("30 2 * * *").unwrap();
        let first = cron.next_after(&oslo("2026-10-25T01:00")).unwrap();
        assert_eq!(first.strftime("%H:%M %:z").to_string(), "02:30 +02:00");
        let second = cron.next_after(&first).unwrap();
        assert_eq!(
            second.strftime("%Y-%m-%d %H:%M %:z").to_string(),
            "2026-10-26 02:30 +01:00",
            "the second 02:30 that night must not fire again"
        );
    }

    #[test]
    fn invalid_expressions_say_what_is_wrong() {
        let err = |e: &str| Cron::parse(e).unwrap_err().to_string();
        assert!(err("* * * *").contains("expected 5 fields"));
        assert!(err("60 * * * *").contains("minute: 60 is outside 0-59"));
        assert!(err("* 25 * * *").contains("hour"));
        assert!(err("*/0 * * * *").contains("step of 0"));
        assert!(err("* * * * funday").contains("day-of-week"));
        assert!(err("10-5 * * * *").contains("backwards"));
        assert!(
            Cron::parse("0 9 * * MON-FRI").is_ok(),
            "names are case-insensitive"
        );
    }
}
