//! Showing commit times.

use chrono::{DateTime, TimeZone};

/// Formats a Unix time as `2026-10-31 14:05` in the given time zone (the user's local zone in
/// the app; a fixed one in tests). Times outside chrono's range show as a dash.
///
/// Generic over the time zone type `Tz` (like a C++ template parameter) so tests can pass `Utc`.
pub fn format_time<Tz: TimeZone>(unix_seconds: i64, zone: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    match DateTime::from_timestamp(unix_seconds, 0) {
        Some(utc) => utc.with_timezone(zone).format("%Y-%m-%d %H:%M").to_string(),
        None => "—".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};

    #[test]
    fn formats_in_the_given_zone() {
        assert_eq!(format_time(1_700_000_000, &Utc), "2023-11-14 22:13");
        let plus_ten = FixedOffset::east_opt(10 * 3600).unwrap();
        assert_eq!(format_time(1_700_000_000, &plus_ten), "2023-11-15 08:13");
    }

    #[test]
    fn handles_old_and_impossible_times() {
        assert_eq!(format_time(-86_400, &Utc), "1969-12-31 00:00");
        assert_eq!(format_time(i64::MAX, &Utc), "—");
    }
}
