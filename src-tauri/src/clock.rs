//! The only clock the library is allowed to read.
//!
//! Unit tests and the `clock` feature panic if they touch the real wall clock
//! or the system timezone. Production code passes a [`WallClock`].

use chrono::{DateTime, Duration as ChronoDuration, TimeZone, Utc};
use chrono_tz::Tz;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Current time, the system zone, and a timer that stops while the machine sleeps.
pub trait WallClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    fn timezone(&self) -> Tz;
    /// Elapsed time on a clock that does not jump forward across sleep.
    fn timer_elapsed(&self) -> Duration;
}

/// The process clock. Panics under `cfg(test)` and the `clock` feature.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl WallClock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        system_now()
    }

    fn timezone(&self) -> Tz {
        system_timezone()
    }

    fn timer_elapsed(&self) -> Duration {
        if real_clock_forbidden() {
            panic!("unit tests must not read the real clock; use WallClock");
        }
        timer_origin().elapsed()
    }
}

fn timer_origin() -> Instant {
    use std::sync::OnceLock;
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    *ORIGIN.get_or_init(Instant::now)
}

pub fn real_clock_forbidden() -> bool {
    cfg!(any(test, feature = "clock"))
}

pub fn system_now() -> DateTime<Utc> {
    if real_clock_forbidden() {
        panic!("unit tests must not read the real clock; use WallClock");
    }
    Utc::now()
}

pub fn system_timezone() -> Tz {
    if real_clock_forbidden() {
        panic!("unit tests must not read the system timezone; use WallClock");
    }
    let name = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string());
    name.parse().unwrap_or(Tz::UTC)
}

struct ManualState {
    now: DateTime<Utc>,
    tz: Tz,
    timer: Duration,
}

/// A clock tests move by hand. `advance` moves wall time and the sleep timer
/// together. `jump_wall` moves only the wall clock, which is what a wake looks like.
pub struct ManualClock {
    inner: Mutex<ManualState>,
}

impl ManualClock {
    pub fn new(now: DateTime<Utc>, tz: Tz) -> Self {
        Self {
            inner: Mutex::new(ManualState {
                now,
                tz,
                timer: Duration::ZERO,
            }),
        }
    }

    /// Wednesday 2025-06-18 10:00 in America/New_York.
    pub fn fixed_new_york() -> Self {
        let tz = chrono_tz::America::New_York;
        let local = tz
            .with_ymd_and_hms(2025, 6, 18, 10, 0, 0)
            .single()
            .expect("10:00 in New York is a real local time");
        Self::new(local.to_utc(), tz)
    }

    pub fn advance(&self, delta: Duration) {
        let mut state = self.inner.lock().expect("manual clock");
        state.timer = state.timer.saturating_add(delta);
        let chrono_delta = ChronoDuration::from_std(delta).unwrap_or_else(|_| {
            ChronoDuration::seconds(delta.as_secs() as i64)
                + ChronoDuration::nanoseconds(delta.subsec_nanos() as i64)
        });
        state.now += chrono_delta;
    }

    pub fn jump_wall(&self, delta: ChronoDuration) {
        let mut state = self.inner.lock().expect("manual clock");
        state.now += delta;
    }

    pub fn set_timezone(&self, tz: Tz) {
        self.inner.lock().expect("manual clock").tz = tz;
    }

    pub fn set_now(&self, now: DateTime<Utc>) {
        self.inner.lock().expect("manual clock").now = now;
    }
}

impl WallClock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        self.inner.lock().expect("manual clock").now
    }

    fn timezone(&self) -> Tz {
        self.inner.lock().expect("manual clock").tz
    }

    fn timer_elapsed(&self) -> Duration {
        self.inner.lock().expect("manual clock").timer
    }
}

/// One zone and one instant, captured together for a tray or alert refresh.
pub fn observation(clock: &dyn WallClock) -> (DateTime<Utc>, Tz) {
    (clock.now(), clock.timezone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn fixed_clock_is_wednesday_morning_in_new_york() {
        let clock = ManualClock::fixed_new_york();
        let local = clock.now().with_timezone(&clock.timezone());
        assert_eq!(
            local.date_naive(),
            chrono::NaiveDate::from_ymd_opt(2025, 6, 18).unwrap()
        );
        assert_eq!(
            local.time(),
            chrono::NaiveTime::from_hms_opt(10, 0, 0).unwrap()
        );
        assert_eq!(clock.timezone(), "America/New_York".parse::<Tz>().unwrap());
        // 2025-06-18 is a Wednesday.
        assert_eq!(local.weekday(), chrono::Weekday::Wed);
    }

    #[test]
    fn advance_moves_wall_and_timer_together() {
        let clock = ManualClock::fixed_new_york();
        let before = clock.now();
        clock.advance(Duration::from_secs(30));
        assert_eq!(clock.now(), before + ChronoDuration::seconds(30));
        assert_eq!(clock.timer_elapsed(), Duration::from_secs(30));
    }

    #[test]
    fn jump_wall_leaves_the_timer_behind() {
        let clock = ManualClock::fixed_new_york();
        clock.advance(Duration::from_secs(5));
        clock.jump_wall(ChronoDuration::hours(3));
        assert_eq!(clock.timer_elapsed(), Duration::from_secs(5));
        let local = clock.now().with_timezone(&clock.timezone());
        assert_eq!(local.hour(), 13);
    }

    #[test]
    #[should_panic(expected = "real clock")]
    fn system_clock_panics_in_unit_tests() {
        let _ = SystemClock.now();
    }

    #[test]
    #[should_panic(expected = "system timezone")]
    fn system_timezone_panics_in_unit_tests() {
        let _ = system_timezone();
    }

    #[test]
    fn set_timezone_is_read_back() {
        let clock = ManualClock::fixed_new_york();
        let la: Tz = "America/Los_Angeles".parse().unwrap();
        clock.set_timezone(la);
        assert_eq!(clock.timezone(), la);
        let _ = Utc.with_ymd_and_hms(2025, 6, 18, 14, 0, 0).unwrap();
    }
}
