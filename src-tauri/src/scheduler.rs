//! One timer per account.
//!
//! Tiers slow down when nothing is soon. A failing account backs off on its
//! own. Sleep is detected by comparing the wall clock with the timer clock,
//! which does not advance while the machine is suspended.

use crate::account::{Calendars, LiveAccount};
use crate::clock::WallClock;
use crate::google_api::{CalendarApi, CredKeeper, WakeWatch};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use std::sync::Arc;
use std::time::Duration;

pub struct Scheduler {
    calendars: Arc<Calendars>,
    api: Arc<dyn CalendarApi>,
    clock: Arc<dyn WallClock>,
    wake: WakeWatch,
    last_zone: Tz,
    last_ui: DateTime<Utc>,
    /// Fraction added to a failure backoff. Tests pass 0.
    jitter: f64,
    keeper: Option<Arc<CredKeeper>>,
}

impl Scheduler {
    pub fn new(
        calendars: Arc<Calendars>,
        api: Arc<dyn CalendarApi>,
        clock: Arc<dyn WallClock>,
    ) -> Self {
        let last_zone = clock.timezone();
        let last_ui = clock.now();
        Self {
            wake: WakeWatch::new(clock.as_ref()),
            calendars,
            api,
            clock,
            last_zone,
            last_ui,
            jitter: 0.0,
            keeper: None,
        }
    }

    pub fn set_jitter(&mut self, jitter: f64) {
        self.jitter = jitter;
    }

    pub fn set_keeper(&mut self, keeper: Arc<CredKeeper>) {
        self.keeper = Some(keeper);
    }

    /// One pass of the 5-second check. Polls are started and not awaited.
    pub async fn drive(&mut self) {
        let woke = self.wake.observe(self.clock.as_ref());
        let zone = self.clock.timezone();
        let zone_changed = zone != self.last_zone;
        self.last_zone = zone;
        let now = self.clock.now();
        if woke {
            for account in self.calendars.accounts() {
                account.cancel_poll();
            }
            self.calendars.expedite_except_holds(now);
        } else if zone_changed {
            let _ = self.calendars.publish_snapshot();
            for account in self.calendars.accounts() {
                if !account.is_enabled() || account.needs_signin() || account.rate_limited_at(now) {
                    continue;
                }
                if account.is_running() {
                    account.poll_again();
                } else {
                    account.force_due(now);
                }
            }
        }
        if let Some(keeper) = &self.keeper {
            if keeper.take_recovered() {
                self.calendars.expedite_except_holds(self.clock.now());
            }
        }
        let now = self.clock.now();
        for account in self.calendars.due_accounts(now) {
            self.trigger(account).await;
        }
        if woke
            || zone_changed
            || now.signed_duration_since(self.last_ui) >= chrono::Duration::seconds(30)
        {
            let _ = self.calendars.publish_snapshot();
            self.last_ui = now;
            self.calendars.woken.notify_waiters();
        }
    }

    pub async fn trigger(&self, account: Arc<LiveAccount>) {
        let Some(generation) = account.begin_poll().await else {
            return;
        };
        account.clear_cancel();
        let calendars = Arc::clone(&self.calendars);
        let api = Arc::clone(&self.api);
        let jitter = self.jitter;
        let polled = Arc::clone(&account);
        let handle = tokio::spawn(async move {
            loop {
                if polled.is_cancelled() {
                    break;
                }
                calendars.poll_account(&polled, &api, jitter).await;
                if !polled.finish_poll(generation).await {
                    break;
                }
            }
        });
        account.note_poll_task(handle);
    }

    pub async fn wait_idle(&self) {
        for _ in 0..400 {
            if self
                .calendars
                .accounts()
                .iter()
                .all(|account| !account.is_running())
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    // The follow-up poll keeps the generation `begin_poll` issued.

    /// Production loop. The sleep uses the timer clock, so it pauses during
    /// suspend; [`drive`] compares that with the wall clock and polls on a jump.
    pub async fn run(mut self) {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                _ = self.calendars.woken.notified() => {}
            }
            self.drive().await;
        }
    }
}
