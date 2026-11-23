//! Dates and times: local time as the system's zone has it, and the few formats the apps read.
//! Every `*_in` takes the zone, so a test can fix it.

use jiff::{civil, tz::TimeZone, Timestamp};

/// `secs` since 1970 written as `format` (strftime), in local time; `None` past what a date holds.
pub fn local(secs: i64, format: &str) -> Option<String> {
    local_in(secs, &TimeZone::system(), format)
}

pub fn local_in(secs: i64, zone: &TimeZone, format: &str) -> Option<String> {
    Some(Timestamp::from_second(secs).ok()?.to_zoned_datetime(zone.clone()).strftime(format).to_string())
}

/// `secs` since 1970 written as `format`, in UTC.
pub fn utc(secs: i64, format: &str) -> Option<String> {
    Some(Timestamp::from_second(secs).ok()?.strftime(format).to_string())
}

/// Now, written as `format`, in local time.
pub fn now_local(format: &str) -> String {
    jiff::ZonedDateTime::now().strftime(format).to_string()
}

/// Today's date in UTC, `2026-10-16`.
pub fn today_utc() -> String {
    Timestamp::now().strftime("%Y-%m-%d").to_string()
}

/// Seconds since 1970 of a local date and time, as a zone's clock has it: of two moments the
/// earlier one; a time a clock change skipped takes the one after.
pub fn from_local(t: civil::DateTime) -> Option<i64> {
    from_local_in(t, &TimeZone::system())
}

pub fn from_local_in(t: civil::DateTime, zone: &TimeZone) -> Option<i64> {
    Some(zone.to_ambiguous_timestamp(t).compatible().ok()?.as_second())
}

/// `s` as local date and time by `format` (strptime), as seconds since 1970; all of `s` must fit.
pub fn parse_local(s: &str, format: &str) -> Option<i64> {
    from_local(civil::DateTime::strptime(format, s).ok()?)
}

/// An RFC 3339 time (with its offset) as seconds since 1970.
pub fn rfc3339(s: &str) -> Option<i64> {
    s.parse::<Timestamp>().ok().map(|t| t.as_second())
}

/// A message's date as mail writes it (RFC 2822, UTC): `Thu, 01 Jan 1970 00:00:00 +0000`.
pub fn rfc2822(secs: i64) -> Option<String> {
    utc(secs, "%a, %d %b %Y %H:%M:%S +0000")
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::tz::offset;

    const T: i64 = 1_790_856_000; // 2026-10-01 12:00:00 UTC

    #[test]
    fn local_time_follows_the_zone() {
        let plus2 = TimeZone::fixed(offset(2));
        assert_eq!(local_in(T, &plus2, "%Y-%m-%d %H:%M:%S").unwrap(), "2026-10-01 14:00:00");
        assert_eq!(local_in(T, &TimeZone::UTC, "%Y%m%d-%H%M%S").unwrap(), "20261001-120000");
        assert_eq!(utc(T, "%Y-%m-%d %H:%M UTC").unwrap(), "2026-10-01 12:00 UTC");
        assert_eq!(local(i64::MAX, "%Y"), None);
    }

    #[test]
    fn local_times_turn_into_moments_and_back() {
        let plus2 = TimeZone::fixed(offset(2));
        let t = civil::DateTime::strptime("%Y-%m-%dT%H:%M:%S", "2026-10-01T14:00:00").unwrap();
        assert_eq!(from_local_in(t, &plus2), Some(T));
        // The clock goes back at 03:00 to 02:00 on the last Sunday of October: the earlier one.
        let eu = TimeZone::posix("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        let twice = civil::date(2026, 10, 25).at(2, 30, 0, 0);
        assert_eq!(from_local_in(twice, &eu), Some(1_792_888_200)); // 00:30 UTC, still CEST
        // 02:30 on 2026-03-29 never was: the time after.
        let skipped = civil::date(2026, 3, 29).at(2, 30, 0, 0);
        assert_eq!(from_local_in(skipped, &eu), Some(1_774_747_800)); // 03:30 CEST = 01:30 UTC
    }

    #[test]
    fn parsing_wants_the_whole_text() {
        assert!(parse_local("2026-10-01 12:00:00", "%Y-%m-%d %H:%M:%S").is_some());
        assert_eq!(parse_local("2026-10-01 12:00:00x", "%Y-%m-%d %H:%M:%S"), None);
        assert_eq!(parse_local("2026-13-01 12:00:00", "%Y-%m-%d %H:%M:%S"), None);
        assert_eq!(rfc3339("2026-10-01T14:00:00+02:00"), Some(T));
        assert_eq!(rfc3339("2026-10-01T12:00:00Z"), Some(T));
        assert_eq!(rfc3339("2026-10-01"), None);
    }

    #[test]
    fn mail_dates_are_rfc_2822() {
        assert_eq!(rfc2822(0).unwrap(), "Thu, 01 Jan 1970 00:00:00 +0000");
        assert_eq!(rfc2822(T).unwrap(), "Thu, 01 Oct 2026 12:00:00 +0000");
    }
}
