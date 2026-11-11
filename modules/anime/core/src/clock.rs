//! The local time: the broadcast day the calendar starts on, and where a
//! time falls in it. A broadcast day runs from 05:00 to 05:00, so a show at
//! 01:30 belongs to the evening before, at hour 25.5, as Japanese TV lists
//! it.

use bern::date::CalendarDate;

const DAY: i64 = 86_400;
/// When one broadcast day turns into the next, in hours after midnight.
const DAY_STARTS: f64 = 5.0;

/// A moment, with the local time zone's offset from UTC.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    /// Seconds since 1970, UTC.
    pub unix: i64,
    /// Seconds the local time is ahead of UTC.
    pub offset: i64,
}

impl Clock {
    /// Now, where the app runs.
    pub fn now() -> Clock {
        let now = bern::date::now();
        Clock {
            unix: now.unix,
            offset: now.offset,
        }
    }

    /// The broadcast day of `unix` and its hour in that day (5 to 29).
    pub fn broadcast(&self, unix: i64) -> (CalendarDate, f64) {
        let local = unix + self.offset;
        let mut days = local.div_euclid(DAY);
        let mut hour = local.rem_euclid(DAY) as f64 / 3600.0;
        if hour < DAY_STARTS {
            days -= 1;
            hour += 24.0;
        }
        (date_of(days), hour)
    }

    /// Today's broadcast day, and the hour now within it.
    pub fn today(&self) -> (CalendarDate, f64) {
        self.broadcast(self.unix)
    }

    /// The local time of day now, in seconds after midnight.
    pub fn seconds(&self) -> i64 {
        (self.unix + self.offset).rem_euclid(DAY)
    }
}

/// Days since 1970-01-01 of a date (Howard Hinnant's `days_from_civil`).
pub fn days_of(date: CalendarDate) -> i64 {
    let (year, month, day) = (date.year, date.month, date.day);
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date `days` after 1970-01-01 (`civil_from_days`).
pub fn date_of(days: i64) -> CalendarDate {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    CalendarDate::new(year, month, day)
}

/// Validate the date part before using it as identity evidence or displaying it.
pub fn parse_date(text: &str) -> Option<CalendarDate> {
    let text = text.trim().get(..10)?;
    let mut parts = text.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some()
        || year.len() != 4
        || month.len() != 2
        || day.len() != 2
        || ![year, month, day]
            .iter()
            .all(|s| s.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let date = CalendarDate::new(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?);
    if !(1..=9999).contains(&date.year)
        || !(1..=12).contains(&date.month)
        || !(1..=31).contains(&date.day)
    {
        return None;
    }
    (date_of(days_of(date)) == date).then_some(date)
}

/// Seconds since 1970 of an ISO 8601 UTC time such as
/// `2023-09-29T14:00:00.000Z`, or of a date alone.
pub fn parse_utc(text: &str) -> Option<i64> {
    let text = text.trim();
    let date = text.get(0..10)?;
    let mut parts = date.split('-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    let mut seconds = days_of(CalendarDate::new(year, month, day)) * DAY;
    if let Some(time) = text.get(11..19) {
        let mut parts = time.split(':').map(|part| part.parse::<i64>().ok());
        let (hour, minute, second) = (parts.next()??, parts.next()??, parts.next()??);
        seconds += hour * 3600 + minute * 60 + second;
    }
    Some(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_small_hours_belong_to_the_evening_before() {
        let beijing = Clock {
            unix: 0,
            offset: 8 * 3600,
        };
        // 2023-09-29T14:00Z is 22:00 in Beijing, on a Friday.
        let friday_night = parse_utc("2023-09-29T14:00:00.000Z").unwrap();
        assert_eq!(
            beijing.broadcast(friday_night),
            (CalendarDate::new(2023, 9, 29), 22.0)
        );
        // 17:30Z is 01:30 Saturday, still Friday's broadcast day.
        let late = parse_utc("2023-09-29T17:30:00.000Z").unwrap();
        assert_eq!(
            beijing.broadcast(late),
            (CalendarDate::new(2023, 9, 29), 25.5)
        );
        assert_eq!(CalendarDate::new(2023, 9, 29).weekday(), 4);
    }

    #[test]
    fn dates_round_trip_through_days() {
        for date in [
            CalendarDate::new(1970, 1, 1),
            CalendarDate::new(2024, 2, 29),
            CalendarDate::new(2026, 10, 21),
        ] {
            assert_eq!(date_of(days_of(date)), date);
        }
        assert_eq!(days_of(CalendarDate::new(1970, 1, 2)), 1);
        assert_eq!(parse_utc("1970-01-02"), Some(DAY));
    }
}
