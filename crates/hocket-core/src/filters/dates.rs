//! Calendar arithmetic without a timezone crate: proleptic Gregorian civil
//! dates ↔ epoch milliseconds, and `YYYY-MM-DD` parsing/formatting.
//!
//! Used by filters (NSP date rules), stats (hour/weekday buckets, streaks)
//! and settings. The algorithms are Howard Hinnant's `days_from_civil` /
//! `civil_from_days`, exact for the full range we care about.

pub const MS_PER_SECOND: f64 = 1000.0;
pub const MS_PER_DAY: f64 = 86_400_000.0;

/// A calendar date in UTC (or in whatever zone the caller has already shifted into).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CivilDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl CivilDate {
    /// Days since 1970-01-01 (negative before the epoch).
    pub fn to_days(self) -> i64 {
        let y = if self.month <= 2 { self.year as i64 - 1 } else { self.year as i64 };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let m = self.month as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// From days since the epoch.
    pub fn from_days(days: i64) -> CivilDate {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        CivilDate { year: (if m <= 2 { y + 1 } else { y }) as i32, month: m, day: d }
    }

    /// Midnight at the start of this date, epoch milliseconds.
    pub fn start_ms(self) -> f64 {
        self.to_days() as f64 * MS_PER_DAY
    }

    /// First millisecond of the following day (exclusive end of this date).
    pub fn end_ms_exclusive(self) -> f64 {
        (self.to_days() + 1) as f64 * MS_PER_DAY
    }

    /// The date containing `ms` (UTC).
    pub fn from_ms(ms: f64) -> CivilDate {
        CivilDate::from_days((ms / MS_PER_DAY).floor() as i64)
    }

    /// Parses `YYYY-MM-DD`. Also accepts a full RFC 3339 timestamp by taking
    /// its date part, which is what Navidrome writes for `date*` tags.
    pub fn parse(s: &str) -> Option<CivilDate> {
        let s = s.trim();
        let date_part = s.split(['T', ' ']).next()?;
        let mut parts = date_part.split('-');
        let year: i32 = parts.next()?.parse().ok()?;
        let month: u32 = parts.next()?.parse().ok()?;
        let day: u32 = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        let d = CivilDate { year, month, day };
        d.is_valid().then_some(d)
    }

    pub fn is_valid(self) -> bool {
        if !(1..=12).contains(&self.month) || self.day == 0 || !(0..=9999).contains(&self.year) {
            return false;
        }
        self.day <= days_in_month(self.year, self.month)
    }

    /// `YYYY-MM-DD`.
    pub fn format(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// 0 = Monday … 6 = Sunday.
    pub fn weekday_monday0(self) -> u32 {
        // 1970-01-01 was a Thursday (Monday-based index 3).
        (self.to_days().rem_euclid(7) as u32 + 3) % 7
    }
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Hour of day (0–23) for an epoch-ms instant shifted by a zone offset in minutes.
pub fn hour_of_day(ms: f64, tz_offset_minutes: i32) -> u32 {
    let local = ms + tz_offset_minutes as f64 * 60_000.0;
    let ms_in_day = local.rem_euclid(MS_PER_DAY);
    (ms_in_day / 3_600_000.0).floor() as u32 % 24
}

/// Local calendar date for an instant shifted by a zone offset in minutes.
pub fn local_date(ms: f64, tz_offset_minutes: i32) -> CivilDate {
    CivilDate::from_ms(ms + tz_offset_minutes as f64 * 60_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_round_trips() {
        for days in [-1_000_000i64, -1, 0, 1, 19_000, 365 * 50, 2_000_000] {
            let d = CivilDate::from_days(days);
            assert_eq!(d.to_days(), days, "{d:?}");
        }
    }

    #[test]
    fn known_dates() {
        assert_eq!(CivilDate { year: 1970, month: 1, day: 1 }.to_days(), 0);
        assert_eq!(CivilDate { year: 2000, month: 3, day: 1 }.to_days(), 11_017);
        assert_eq!(CivilDate::from_days(19_723), CivilDate { year: 2024, month: 1, day: 1 });
        assert_eq!(CivilDate::parse("2024-02-29"), Some(CivilDate { year: 2024, month: 2, day: 29 }));
        assert_eq!(CivilDate::parse("2023-02-29"), None);
        assert_eq!(CivilDate::parse("2023-13-01"), None);
        assert_eq!(CivilDate::parse("2023-01-01T10:00:00Z"), Some(CivilDate { year: 2023, month: 1, day: 1 }));
        assert_eq!(CivilDate::parse("garbage"), None);
        assert_eq!(CivilDate { year: 2024, month: 1, day: 1 }.format(), "2024-01-01");
    }

    #[test]
    fn weekdays() {
        // 1970-01-01 Thursday, 2024-01-01 Monday, 2024-01-07 Sunday.
        assert_eq!(CivilDate::from_days(0).weekday_monday0(), 3);
        assert_eq!(CivilDate::parse("2024-01-01").unwrap().weekday_monday0(), 0);
        assert_eq!(CivilDate::parse("2024-01-07").unwrap().weekday_monday0(), 6);
        assert_eq!(CivilDate::parse("1969-12-31").unwrap().weekday_monday0(), 2);
    }

    #[test]
    fn hours_with_offsets() {
        let ms = 1_704_067_200_000.0; // 2024-01-01T00:00:00Z
        assert_eq!(hour_of_day(ms, 0), 0);
        assert_eq!(hour_of_day(ms, 600), 10); // AEST
        assert_eq!(hour_of_day(ms, -300), 19); // EST, previous day
        assert_eq!(local_date(ms, -300), CivilDate { year: 2023, month: 12, day: 31 });
        assert_eq!(local_date(ms, 600).format(), "2024-01-01");
    }
}
