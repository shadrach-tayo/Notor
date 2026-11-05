use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use chrono_tz::Tz;

/// Which tray bucket an event belongs in relative to `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventBucket {
    Now,
    Upcoming,
    Tomorrow,
}

/// Resolve a local wall time in `tz` without panicking on DST gaps/ambiguity.
///
/// - Single: that instant
/// - Ambiguous (fall back): earliest
/// - None (spring forward gap): step forward one hour at a time until valid
pub fn resolve_local_datetime(tz: Tz, naive: NaiveDateTime) -> DateTime<Tz> {
    match tz.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(earliest, _latest) => earliest,
        chrono::LocalResult::None => {
            let mut candidate = naive + Duration::hours(1);
            for _ in 0..12 {
                match tz.from_local_datetime(&candidate) {
                    chrono::LocalResult::Single(dt) => return dt,
                    chrono::LocalResult::Ambiguous(earliest, _) => return earliest,
                    chrono::LocalResult::None => candidate += Duration::hours(1),
                }
            }
            // Last resort: treat the naive clock as UTC and convert (no panic).
            tz.from_utc_datetime(&naive)
        }
    }
}

/// Local midnight at the start of tomorrow, and exclusive end (start of day-after).
///
/// Nanoseconds are zeroed so all-day events that parse as local midnight land in
/// the tomorrow bucket rather than today's upcoming list.
pub fn bucket_bounds(now: DateTime<Tz>) -> (DateTime<Tz>, DateTime<Tz>) {
    let tz = now.timezone();
    let today = now.date_naive();
    let tomorrow_date = today.succ_opt().expect("valid calendar day after today");
    let day_after_date = tomorrow_date
        .succ_opt()
        .expect("valid calendar day after tomorrow");

    let midnight = NaiveTime::from_hms_nano_opt(0, 0, 0, 0).unwrap();
    let tomorrow_start = resolve_local_datetime(tz, tomorrow_date.and_time(midnight));
    let day_after_start = resolve_local_datetime(tz, day_after_date.and_time(midnight));
    (tomorrow_start, day_after_start)
}

/// Classify an event into now / upcoming / tomorrow using exclusive end-of-tomorrow.
pub fn classify_event(
    start: DateTime<Tz>,
    end: DateTime<Tz>,
    now: DateTime<Tz>,
    tomorrow_start: DateTime<Tz>,
    day_after_start: DateTime<Tz>,
) -> Option<EventBucket> {
    if now > start && now < end {
        Some(EventBucket::Now)
    } else if now < start && start < tomorrow_start {
        Some(EventBucket::Upcoming)
    } else if start >= tomorrow_start && start < day_after_start {
        Some(EventBucket::Tomorrow)
    } else {
        None
    }
}

/// Helper for all-day Google Calendar dates: interpret `date` at local midnight in `tz`.
pub fn all_day_start(tz: Tz, date: NaiveDate) -> DateTime<Tz> {
    let midnight = NaiveTime::from_hms_nano_opt(0, 0, 0, 0).unwrap();
    resolve_local_datetime(tz, date.and_time(midnight))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};
    use chrono_tz::America::Denver;
    use chrono_tz::America::New_York;

    fn denver(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Tz> {
        Denver
            .with_ymd_and_hms(y, m, d, h, min, s)
            .single()
            .expect("valid Denver time")
    }

    #[test]
    fn tomorrow_bounds_zero_nanos_and_exclusive_end() {
        // Include a non-zero sub-second on `now` — bounds must still be exact midnight.
        let now = Denver
            .with_ymd_and_hms(2024, 6, 15, 14, 30, 45)
            .single()
            .unwrap()
            + Duration::nanoseconds(123_456_789);

        let (tomorrow_start, day_after) = bucket_bounds(now);
        assert_eq!(tomorrow_start.hour(), 0);
        assert_eq!(tomorrow_start.minute(), 0);
        assert_eq!(tomorrow_start.second(), 0);
        assert_eq!(tomorrow_start.nanosecond(), 0);
        assert_eq!(
            tomorrow_start.date_naive(),
            NaiveDate::from_ymd_opt(2024, 6, 16).unwrap()
        );
        assert_eq!(
            day_after.date_naive(),
            NaiveDate::from_ymd_opt(2024, 6, 17).unwrap()
        );
        assert_eq!(day_after.nanosecond(), 0);
    }

    #[test]
    fn all_day_tomorrow_lands_in_tomorrow_not_upcoming() {
        let now = denver(2024, 6, 15, 9, 0, 0);
        let (tomorrow_start, day_after) = bucket_bounds(now);

        let start = all_day_start(Denver, NaiveDate::from_ymd_opt(2024, 6, 16).unwrap());
        let end = all_day_start(Denver, NaiveDate::from_ymd_opt(2024, 6, 17).unwrap());

        assert_eq!(
            classify_event(start, end, now, tomorrow_start, day_after),
            Some(EventBucket::Tomorrow)
        );
    }

    #[test]
    fn event_at_last_minute_of_tomorrow_is_included() {
        let now = denver(2024, 6, 15, 9, 0, 0);
        let (tomorrow_start, day_after) = bucket_bounds(now);

        let start = denver(2024, 6, 16, 23, 59, 30);
        let end = denver(2024, 6, 17, 0, 30, 0);

        assert_eq!(
            classify_event(start, end, now, tomorrow_start, day_after),
            Some(EventBucket::Tomorrow)
        );
    }

    #[test]
    fn timed_event_later_today_is_upcoming() {
        let now = denver(2024, 6, 15, 9, 0, 0);
        let (tomorrow_start, day_after) = bucket_bounds(now);
        let start = denver(2024, 6, 15, 15, 0, 0);
        let end = denver(2024, 6, 15, 16, 0, 0);

        assert_eq!(
            classify_event(start, end, now, tomorrow_start, day_after),
            Some(EventBucket::Upcoming)
        );
    }

    #[test]
    fn in_progress_event_is_now() {
        let now = denver(2024, 6, 15, 15, 30, 0);
        let (tomorrow_start, day_after) = bucket_bounds(now);
        let start = denver(2024, 6, 15, 15, 0, 0);
        let end = denver(2024, 6, 15, 16, 0, 0);

        assert_eq!(
            classify_event(start, end, now, tomorrow_start, day_after),
            Some(EventBucket::Now)
        );
    }

    #[test]
    fn dst_spring_forward_midnight_does_not_panic() {
        // US spring forward 2024-03-10: 02:00 → 03:00 in America/New_York.
        // Local midnight still exists; probe the gap hour itself as well.
        let midnight = NaiveDate::from_ymd_opt(2024, 3, 10)
            .unwrap()
            .and_hms_nano_opt(0, 0, 0, 0)
            .unwrap();
        let resolved_midnight = resolve_local_datetime(New_York, midnight);
        assert_eq!(resolved_midnight.hour(), 0);

        let gap = NaiveDate::from_ymd_opt(2024, 3, 10)
            .unwrap()
            .and_hms_nano_opt(2, 30, 0, 0)
            .unwrap();
        let resolved_gap = resolve_local_datetime(New_York, gap);
        // Must return a defined instant without panicking.
        assert!(resolved_gap.timezone() == New_York);
    }

    #[test]
    fn dst_fall_back_ambiguous_picks_earliest() {
        // US fall back 2024-11-03: 01:30 occurs twice in America/New_York.
        let ambiguous = NaiveDate::from_ymd_opt(2024, 11, 3)
            .unwrap()
            .and_hms_nano_opt(1, 30, 0, 0)
            .unwrap();
        let resolved = resolve_local_datetime(New_York, ambiguous);
        assert_eq!(resolved.hour(), 1);
        assert_eq!(resolved.minute(), 30);
    }
}
