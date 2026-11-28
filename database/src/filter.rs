//! What a list's filters send (RFC 0069): a range of numbers, a span of days,
//! and the `Query` helpers that turn each, and a text or a set of options,
//! into its `WHERE`. An empty filter is no filter.
//!
//! ```ignore
//! Invoice::query()
//!     .filter_options("status", filters.status)
//!     .filter_range("amount", filters.amount)
//!     .filter_dates("due", filters.due)
//!     .filter_text("customer", filters.customer)
//! ```

use crate::model::{Model, Query, Value};

/// The filters' shapes, shared with a Laravel backend's lists (RFC 0071).
pub use substrate_core::filter::{Dates, Range};

/// `(year, month, day)` of an ISO day, or `None` if it isn't one.
fn day_of(text: &str) -> Option<(i32, u32, u32)> {
    let text = text.get(..10)?;
    let bytes = text.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year = text[..4].parse().ok()?;
    let month = text[5..7].parse().ok()?;
    let day = text[8..10].parse().ok()?;
    ((1..=12).contains(&month) && day >= 1 && day <= days_in(year, month))
        .then_some((year, month, day))
}

fn days_in(year: i32, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 31,
    }
}

/// The day after an ISO day, as one.
fn next_day((year, month, day): (i32, u32, u32)) -> String {
    let (year, month, day) = if day < days_in(year, month) {
        (year, month, day + 1)
    } else if month < 12 {
        (year, month + 1, 1)
    } else {
        (year + 1, 1, 1)
    };
    format!("{year:04}-{month:02}-{day:02}")
}

impl<M: Model> Query<M> {
    /// `column` is one of `values`. `None`, or none of them, is no filter.
    pub fn filter_options<V: Into<Value>>(self, column: &str, values: Option<Vec<V>>) -> Self {
        match values {
            Some(values) if !values.is_empty() => self.where_in(column, values),
            _ => self,
        }
    }

    /// `column` is at least `min` and at most `max`, where they're given.
    pub fn filter_range<T: Into<Value>>(self, column: &str, range: Option<Range<T>>) -> Self {
        let Some(Range { min, max }) = range else {
            return self;
        };
        let query = match min {
            Some(min) => self.where_gte(column, min),
            None => self,
        };
        match max {
            Some(max) => query.where_lte(column, max),
            None => query,
        }
    }

    /// `column` is on or after `from` and on or before `to`, by day: `to` is
    /// "before the next day", so a time on it is in. A day that isn't an ISO
    /// day is left out.
    ///
    /// For a column of ISO days or times as text, as Elyra's schema keeps
    /// them (`string`, and `make:resource`'s `date` fields). A Postgres
    /// `DATE` or `TIMESTAMP` column made by raw SQL doesn't compare with a
    /// text parameter: filter it with a cast of your own.
    pub fn filter_dates(self, column: &str, dates: Option<Dates>) -> Self {
        let Some(Dates { from, to }) = dates else {
            return self;
        };
        let query = match from.as_deref().and_then(day_of) {
            Some((y, m, d)) => self.where_gte(column, format!("{y:04}-{m:02}-{d:02}")),
            None => self,
        };
        match to.as_deref().and_then(day_of) {
            Some(day) => query.where_lt(column, next_day(day)),
            None => query,
        }
    }

    /// `column` contains `text`, ignoring case, its `%` and `_` as themselves.
    /// `None`, or only spaces, is no filter.
    pub fn filter_text(self, column: &str, text: Option<String>) -> Self {
        match text.as_deref().map(str::trim) {
            Some(text) if !text.is_empty() => self.where_contains(column, text),
            _ => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_and_the_day_after() {
        assert_eq!(day_of("2026-11-30"), Some((2026, 11, 30)));
        assert_eq!(day_of("2026-11-30T12:00:00Z"), Some((2026, 11, 30)));
        assert_eq!(day_of("2026-02-29"), None);
        assert_eq!(day_of("2024-02-29"), Some((2024, 2, 29)));
        assert_eq!(day_of("30.11.2026"), None);
        assert_eq!(day_of("2026"), None);
        assert_eq!(next_day((2026, 11, 30)), "2026-12-01");
        assert_eq!(next_day((2026, 12, 31)), "2027-01-01");
        assert_eq!(next_day((2024, 2, 28)), "2024-02-29");
        assert_eq!(next_day((2026, 2, 28)), "2026-03-01");
    }

    #[test]
    fn filters_read_from_json_each_end_optional() {
        let range: Range<f64> = serde_json::from_str(r#"{"min": 10000}"#).unwrap();
        assert_eq!(
            range,
            Range {
                min: Some(10000.0),
                max: None
            }
        );
        let dates: Dates = serde_json::from_str(r#"{"to": "2026-11-30"}"#).unwrap();
        assert_eq!(
            dates,
            Dates {
                from: None,
                to: Some("2026-11-30".into())
            }
        );
    }
}
