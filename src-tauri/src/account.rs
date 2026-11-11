//! Per-account calendar state. Network calls never run under the shared list lock.

use crate::account_store::AccountStore;
use crate::alert_map::candidate_from_source;
use crate::clock::{observation, WallClock};
use crate::event_grouping::{bucket_bounds, classify_event, EventBucket};
use crate::google_api::{
    backoff_delay, poll_interval_for, rate_limit_wait, ApiError, ApiKind, CalendarApi, CalendarInfo,
};
use crate::types::{AccountPreference, GoogleAuthToken, Preferences};
use crate::utils::{parse_event_datetime_in, EventGroups};
use chrono::{DateTime, Utc};
use google_calendar::types::Event;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Drop a calendar's cached events this long after the last successful load.
pub const STALE_CALENDAR_AFTER_MINUTES: i64 = 120;

/// One event plus the calendar defaults needed to resolve its reminders.
#[derive(Clone, Debug, Default)]
pub struct AlertSource {
    pub event: Event,
    pub default_reminders: Vec<google_calendar::types::EventReminder>,
    pub account_email: String,
}

#[derive(Clone, Debug, Default)]
pub struct AccountPoll {
    pub events: Vec<Event>,
    pub alert_sources: Vec<AlertSource>,
}

#[derive(Debug)]
pub struct RemoveOutcome {
    pub remaining: usize,
    pub open_sign_in: bool,
}

struct TokenState {
    token: GoogleAuthToken,
    generation: u64,
    needs_save: bool,
    pending_revocation: bool,
}

struct CachedCalendar {
    events: Vec<Event>,
    sources: Vec<AlertSource>,
    loaded_at: DateTime<Utc>,
}

struct CalendarCache {
    list: Vec<CalendarInfo>,
    list_at: Option<DateTime<Utc>>,
    events: HashMap<String, CachedCalendar>,
    /// Calendar id → access role at the time we decided to skip it.
    skipped: HashMap<String, String>,
}

struct PollGate {
    running: bool,
    again: bool,
    generation: u64,
}

struct RefreshFlight {
    generation: u64,
    handle: tokio::task::JoinHandle<()>,
    rx: tokio::sync::watch::Receiver<Option<RefreshDone>>,
}

#[derive(Clone)]
struct RefreshDone {
    generation: u64,
    result: Result<String, ApiKind>,
}

/// One signed-in Google account. Polls and refreshes do not block the others.
pub struct LiveAccount {
    retired: AtomicBool,
    email: Mutex<String>,
    token: Mutex<TokenState>,
    preferences: Mutex<AccountPreference>,
    cache: Mutex<CalendarCache>,
    gate: Mutex<PollGate>,
    refresh: Mutex<Option<RefreshFlight>>,
    cancel: AtomicBool,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    not_before: Mutex<DateTime<Utc>>,
    failure_attempt: AtomicU32,
    rate_limited_until: Mutex<Option<DateTime<Utc>>>,
    user_refresh: AtomicBool,
    polls: AtomicU64,
}

pub struct Calendars {
    pub event_groups: Mutex<EventGroups>,
    clock: Arc<dyn WallClock>,
    accounts: Mutex<Vec<Arc<LiveAccount>>>,
    store: Mutex<AccountStore>,
    dir: Mutex<Option<PathBuf>>,
    events: Mutex<Vec<Event>>,
    reminder_sources: Mutex<Vec<AlertSource>>,
    ever_loaded: AtomicBool,
    save_ready: AtomicBool,
    pub woken: tokio::sync::Notify,
    #[cfg(test)]
    sign_in_order: Mutex<Vec<&'static str>>,
}

impl Default for Calendars {
    fn default() -> Self {
        Self {
            event_groups: Mutex::new(EventGroups::default()),
            clock: Arc::new(crate::clock::SystemClock),
            accounts: Mutex::new(Vec::new()),
            store: Mutex::new(AccountStore::loading()),
            dir: Mutex::new(None),
            events: Mutex::new(Vec::new()),
            reminder_sources: Mutex::new(Vec::new()),
            ever_loaded: AtomicBool::new(false),
            save_ready: AtomicBool::new(false),
            woken: tokio::sync::Notify::new(),
            #[cfg(test)]
            sign_in_order: Mutex::new(Vec::new()),
        }
    }
}

impl Calendars {
    pub fn with_clock(clock: Arc<dyn WallClock>) -> Self {
        Self {
            clock,
            ..Self::default()
        }
    }

    pub async fn new(
        tokens: Vec<GoogleAuthToken>,
        _config: crate::types::AppCredentials,
        preferences: &Preferences,
    ) -> Self {
        let calendars = Self::default();
        calendars.install_tokens(tokens, preferences);
        calendars
    }

    pub fn clock(&self) -> Arc<dyn WallClock> {
        Arc::clone(&self.clock)
    }

    pub fn install_tokens(&self, tokens: Vec<GoogleAuthToken>, preferences: &Preferences) {
        let mut store = AccountStore::loading();
        store.mark_ready();
        for token in tokens {
            store.upsert(token);
        }
        self.install_store(store, preferences);
    }

    /// Load accounts from a data directory. No network. Saves stay refused
    /// until this returns.
    pub fn open_directory(&self, dir: &PathBuf, preferences: &Preferences) -> usize {
        let store = AccountStore::load(dir);
        let count = store.tokens().len();
        *self.dir.lock().expect("dir") = Some(dir.clone());
        self.install_store(store, preferences);
        count
    }

    fn install_store(&self, store: AccountStore, preferences: &Preferences) {
        let tokens = store.tokens();
        *self.store.lock().expect("store") = store;
        let mut accounts = Vec::new();
        for token in tokens {
            let email = token
                .user
                .as_ref()
                .map(|user| user.email.clone())
                .unwrap_or_default();
            let preference = if email.is_empty() {
                AccountPreference::default()
            } else {
                preferences.get_account_preference(&email)
            };
            accounts.push(self.account_from_token(token, preference));
        }
        *self.accounts.lock().expect("accounts") = accounts;
        self.save_ready.store(true, Ordering::SeqCst);
    }

    fn account_from_token(
        &self,
        token: GoogleAuthToken,
        preference: AccountPreference,
    ) -> Arc<LiveAccount> {
        let email = token
            .user
            .as_ref()
            .map(|user| user.email.clone())
            .unwrap_or_default();
        let now = self.clock.now();
        Arc::new(LiveAccount {
            retired: AtomicBool::new(false),
            email: Mutex::new(email),
            token: Mutex::new(TokenState {
                token,
                generation: 1,
                needs_save: false,
                pending_revocation: false,
            }),
            preferences: Mutex::new(preference),
            cache: Mutex::new(CalendarCache {
                list: Vec::new(),
                list_at: None,
                events: HashMap::new(),
                skipped: HashMap::new(),
            }),
            gate: Mutex::new(PollGate {
                running: false,
                again: false,
                generation: 0,
            }),
            refresh: Mutex::new(None),
            cancel: AtomicBool::new(false),
            task: Mutex::new(None),
            not_before: Mutex::new(now),
            failure_attempt: AtomicU32::new(0),
            rate_limited_until: Mutex::new(None),
            user_refresh: AtomicBool::new(false),
            polls: AtomicU64::new(0),
        })
    }

    pub fn accounts(&self) -> Vec<Arc<LiveAccount>> {
        self.accounts.lock().expect("accounts").clone()
    }

    /// Persist the new account, then show it. A failed write leaves the list
    /// unchanged. The locks here are dropped before this function returns, and
    /// the function does not await.
    pub fn commit_signed_in_account(
        &self,
        token: GoogleAuthToken,
        preferences: &Preferences,
    ) -> Result<(), String> {
        let email = token
            .user
            .as_ref()
            .map(|user| user.email.clone())
            .unwrap_or_default();
        if email.is_empty() {
            return Err("Google account had no email".to_string());
        }
        let Some(dir) = self.dir.lock().expect("dir").clone() else {
            return Err("account storage is not ready".to_string());
        };
        #[cfg(test)]
        self.sign_in_order.lock().expect("order").push("disk");
        {
            let mut store = self.store.lock().expect("store");
            if !store.is_ready() {
                return Err("startup loading has not finished".to_string());
            }
            let snapshot = store.clone();
            store.upsert(token.clone());
            if let Err(err) = store.save(&dir) {
                *store = snapshot;
                return Err(err);
            }
        }
        #[cfg(test)]
        self.sign_in_order.lock().expect("order").push("memory");
        self.attach_account(token, preferences);
        Ok(())
    }

    fn attach_account(&self, mut token: GoogleAuthToken, preferences: &Preferences) -> bool {
        let Some(email) = token.user.as_ref().map(|user| user.email.clone()) else {
            return false;
        };
        let preference = preferences.get_account_preference(&email);
        let mut accounts = self.accounts.lock().expect("accounts");
        if let Some(position) = accounts.iter().position(|account| account.email() == email) {
            let previous = &accounts[position];
            token.disabled = previous.token_snapshot().disabled;
            previous.retire();
            let replacement = self.account_from_token(token, preference);
            accounts[position] = replacement;
        } else {
            accounts.push(self.account_from_token(token, preference));
        }
        drop(accounts);
        self.woken.notify_waiters();
        true
    }

    #[cfg(test)]
    pub fn add_account(&self, token: GoogleAuthToken, preferences: &Preferences) -> bool {
        if !self.attach_account(token.clone(), preferences) {
            return false;
        }
        self.store.lock().expect("store").upsert(token);
        true
    }

    #[cfg(test)]
    pub fn sign_in_order(&self) -> Vec<&'static str> {
        self.sign_in_order.lock().expect("order").clone()
    }

    pub fn remove_account(&self, email: &str) -> Result<RemoveOutcome, String> {
        let mut accounts = self.accounts.lock().expect("accounts");
        let Some(position) = accounts.iter().position(|account| account.email() == email) else {
            return Err(format!("unknown account {email}"));
        };
        let removed = accounts.remove(position);
        removed.retire();
        let remaining = accounts.len();
        drop(accounts);
        self.store.lock().expect("store").remove(email);
        self.publish_snapshot();
        if remaining == 0 {
            *self.events.lock().expect("events") = Vec::new();
            *self.reminder_sources.lock().expect("sources") = Vec::new();
            self.group_events();
        }
        self.woken.notify_waiters();
        Ok(RemoveOutcome {
            remaining,
            open_sign_in: remaining == 0,
        })
    }

    pub fn disable_account(&self, email: &str) -> Result<(), String> {
        self.set_disabled(email, true)
    }

    pub fn enable_account(&self, email: &str) -> Result<(), String> {
        self.set_disabled(email, false)?;
        if let Some(account) = self.find(email) {
            account.force_due(self.clock.now());
            self.woken.notify_waiters();
        }
        Ok(())
    }

    fn set_disabled(&self, email: &str, disabled: bool) -> Result<(), String> {
        let Some(account) = self.find(email) else {
            return Err(format!("unknown account {email}"));
        };
        {
            let mut token = account.token.lock().expect("token");
            if account.retired.load(Ordering::SeqCst) {
                return Err(format!("unknown account {email}"));
            }
            token.token.disabled = Some(disabled);
            token.needs_save = true;
        }
        self.publish_snapshot();
        Ok(())
    }

    pub fn set_preferences(&self, email: &str, preferences: &Preferences) {
        if let Some(account) = self.find(email) {
            *account.preferences.lock().expect("preferences") =
                preferences.get_account_preference(email);
            account.force_due(self.clock.now());
            self.woken.notify_waiters();
        }
    }

    pub fn get_tokens(&self) -> Vec<GoogleAuthToken> {
        self.accounts()
            .iter()
            .map(|account| account.token_snapshot())
            .collect()
    }

    pub fn find(&self, email: &str) -> Option<Arc<LiveAccount>> {
        self.accounts()
            .into_iter()
            .find(|account| account.email() == email)
    }

    pub fn sign_in_emails(&self) -> Vec<String> {
        self.accounts()
            .iter()
            .filter(|account| account.needs_signin() && account.is_enabled())
            .map(|account| account.email())
            .filter(|email| !email.is_empty())
            .collect()
    }

    pub fn save(&self) -> Result<(), String> {
        if !self.save_ready.load(Ordering::SeqCst) {
            return Err("startup loading has not finished".to_string());
        }
        let Some(dir) = self.dir.lock().expect("dir").clone() else {
            return Ok(());
        };
        let mut store = self.store.lock().expect("store");
        for account in self.accounts() {
            let mut token = account.token.lock().expect("token");
            if !token.needs_save || account.retired.load(Ordering::SeqCst) {
                continue;
            }
            let email = account.email();
            let stored = if !email.is_empty() && store.update_token(&email, token.token.clone()) {
                true
            } else {
                store.update_token("", token.token.clone())
            };
            if stored {
                token.needs_save = false;
            } else {
                store.upsert(token.token.clone());
                token.needs_save = false;
            }
        }
        store.save(&dir)
    }

    /// Refresh expired tokens, including disabled accounts, and remember the result.
    pub async fn refresh_expired_tokens(&self, api: &Arc<dyn CalendarApi>) {
        let now = self.clock.now();
        for account in self.accounts() {
            if account.retired.load(Ordering::SeqCst) || account.needs_signin() {
                continue;
            }
            let expired = {
                let token = account.token.lock().expect("token");
                token_expired(&token.token, now)
                    && token
                        .token
                        .refresh_token
                        .as_ref()
                        .is_some_and(|value| !value.is_empty())
            };
            if !expired {
                continue;
            }
            let _ = refresh_join(
                Arc::clone(&account),
                Arc::clone(api),
                Arc::clone(&self.clock),
            )
            .await;
        }
        let _ = self.save();
    }

    pub async fn poll_account(
        &self,
        account: &Arc<LiveAccount>,
        api: &Arc<dyn CalendarApi>,
        jitter: f64,
    ) {
        if account.retired.load(Ordering::SeqCst) || !account.is_enabled() {
            return;
        }
        if account.promote_revocation() {
            let _ = self.save();
            self.publish_snapshot();
            self.woken.notify_waiters();
            return;
        }
        if account.needs_signin() {
            return;
        }
        let now = self.clock.now();
        if account.rate_limited(now) {
            return;
        }
        account.polls.fetch_add(1, Ordering::SeqCst);
        account.user_refresh.store(false, Ordering::SeqCst);
        let outcome = fetch_account(account, Arc::clone(api), Arc::clone(&self.clock)).await;
        let now = self.clock.now();
        match outcome {
            FetchOutcome::SignIn => {
                account.mark_signin();
            }
            FetchOutcome::RateLimit { retry_after } => {
                let wait = rate_limit_wait(retry_after);
                *account.rate_limited_until.lock().expect("rate") =
                    Some(now + chrono::Duration::from_std(wait).unwrap_or_default());
                *account.not_before.lock().expect("next") =
                    now + chrono::Duration::from_std(wait).unwrap_or_default();
            }
            FetchOutcome::Backoff => {
                let attempt = account.failure_attempt.fetch_add(1, Ordering::SeqCst);
                let wait = backoff_delay(attempt, jitter);
                *account.not_before.lock().expect("next") =
                    now + chrono::Duration::from_std(wait).unwrap_or_default();
            }
            FetchOutcome::Success { next_in } => {
                account.failure_attempt.store(0, Ordering::SeqCst);
                *account.rate_limited_until.lock().expect("rate") = None;
                let wait = poll_interval_for(next_in);
                *account.not_before.lock().expect("next") =
                    now + chrono::Duration::from_std(wait).unwrap_or_default();
                self.ever_loaded.store(true, Ordering::SeqCst);
            }
        }
        self.publish_snapshot();
        let _ = self.save();
        self.woken.notify_waiters();
    }

    /// Rebuild the tray snapshot from cached events. Stale calendars are dropped.
    pub fn publish_snapshot(&self) -> bool {
        let now = self.clock.now();
        let mut events = Vec::new();
        let mut sources = Vec::new();
        let mut saw_fresh = false;
        for account in self.accounts() {
            if !account.is_enabled() || account.retired.load(Ordering::SeqCst) {
                continue;
            }
            let cache = account.cache.lock().expect("cache");
            for cached in cache.events.values() {
                if stale(cached.loaded_at, now) {
                    continue;
                }
                saw_fresh = true;
                events.extend(cached.events.clone());
                sources.extend(cached.sources.clone());
            }
        }
        if !saw_fresh && !self.ever_loaded.load(Ordering::SeqCst) {
            return false;
        }
        *self.events.lock().expect("events") = events;
        *self.reminder_sources.lock().expect("sources") = sources;
        self.group_events();
        true
    }

    pub fn active_events(&self) -> Vec<Event> {
        self.event_groups.lock().expect("groups").now.clone()
    }

    pub fn upcoming_events(&self) -> Vec<Event> {
        self.event_groups.lock().expect("groups").upcoming.clone()
    }

    pub fn tomorrow_events(&self) -> Vec<Event> {
        self.event_groups.lock().expect("groups").tomorrow.clone()
    }

    pub fn polled_events(&self) -> Vec<Event> {
        self.events.lock().expect("events").clone()
    }

    pub fn alert_sources(&self) -> Vec<AlertSource> {
        self.reminder_sources.lock().expect("sources").clone()
    }

    pub fn group_events(&self) {
        let (instant, zone) = observation(self.clock.as_ref());
        let now = instant.with_timezone(&zone);
        let events = self.events.lock().expect("events");
        let mut groups = EventGroups::default();
        if events.is_empty() {
            *self.event_groups.lock().expect("groups") = groups;
            return;
        }
        let (tomorrow_start, day_after_start) = bucket_bounds(now);
        for event in events.iter() {
            if !crate::alert_map::include_in_tray(event) {
                continue;
            }
            let Some(start_raw) = event.start.clone() else {
                continue;
            };
            let Some(end_raw) = event.end.clone() else {
                continue;
            };
            if start_raw.date_time.is_none() && start_raw.date.is_none() {
                continue;
            }
            if end_raw.date_time.is_none() && end_raw.date.is_none() {
                continue;
            }
            let start = parse_event_datetime_in(start_raw, zone).with_timezone(&zone);
            let end = parse_event_datetime_in(end_raw, zone).with_timezone(&zone);
            match classify_event(start, end, now, tomorrow_start, day_after_start) {
                Some(EventBucket::Now) => groups.now.push(event.clone()),
                Some(EventBucket::Upcoming) => groups.upcoming.push(event.clone()),
                Some(EventBucket::Tomorrow) => groups.tomorrow.push(event.clone()),
                None => {}
            }
        }
        groups
            .now
            .sort_by_key(|event| parse_event_datetime_in(event.end.clone().unwrap(), zone));
        groups
            .upcoming
            .sort_by_key(|event| parse_event_datetime_in(event.start.clone().unwrap(), zone));
        groups
            .tomorrow
            .sort_by_key(|event| parse_event_datetime_in(event.start.clone().unwrap(), zone));
        *self.event_groups.lock().expect("groups") = groups;
    }

    /// Accounts that should be polled at `now`.
    pub fn due_accounts(&self, now: DateTime<Utc>) -> Vec<Arc<LiveAccount>> {
        self.accounts()
            .into_iter()
            .filter(|account| account.is_due(now))
            .collect()
    }

    pub fn expedite_except_holds(&self, now: DateTime<Utc>) {
        for account in self.accounts() {
            if account.is_enabled() && !account.needs_signin() && !account.rate_limited(now) {
                account.force_due(now);
            }
        }
        self.woken.notify_waiters();
    }
}

impl LiveAccount {
    pub fn email(&self) -> String {
        self.email.lock().expect("email").clone()
    }

    pub fn token_snapshot(&self) -> GoogleAuthToken {
        self.token.lock().expect("token").token.clone()
    }

    pub fn is_enabled(&self) -> bool {
        !self.token_snapshot().disabled.unwrap_or(false)
    }

    pub fn needs_signin(&self) -> bool {
        self.token_snapshot().needs_signin
    }

    pub fn poll_count(&self) -> u64 {
        self.polls.load(Ordering::SeqCst)
    }

    pub fn is_running(&self) -> bool {
        self.gate.lock().expect("gate").running
    }

    pub fn clear_cancel(&self) {
        self.cancel.store(false, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn rate_limited_at(&self, now: DateTime<Utc>) -> bool {
        self.rate_limited(now)
    }

    /// Returns the poll generation when this caller owns the poll.
    /// `None` means a poll is already running and this trigger was coalesced.
    pub async fn begin_poll(&self) -> Option<u64> {
        let mut gate = self.gate.lock().expect("gate");
        if gate.running {
            gate.again = true;
            return None;
        }
        gate.running = true;
        gate.generation = gate.generation.wrapping_add(1);
        Some(gate.generation)
    }

    /// `true` when a trigger arrived during the poll and it should run once more.
    pub async fn finish_poll(&self, generation: u64) -> bool {
        let mut gate = self.gate.lock().expect("gate");
        if gate.generation != generation || self.cancel.load(Ordering::SeqCst) {
            if gate.generation == generation {
                gate.running = false;
                gate.again = false;
            }
            return false;
        }
        if gate.again {
            gate.again = false;
            return true;
        }
        gate.running = false;
        false
    }

    pub fn force_due(&self, now: DateTime<Utc>) {
        *self.not_before.lock().expect("next") = now;
        self.user_refresh.store(true, Ordering::SeqCst);
    }

    pub fn poll_again(&self) {
        let mut gate = self.gate.lock().expect("gate");
        if gate.running {
            gate.again = true;
        }
    }

    pub fn request_user_refresh(&self) {
        self.user_refresh.store(true, Ordering::SeqCst);
    }

    fn rate_limited(&self, now: DateTime<Utc>) -> bool {
        self.rate_limited_until
            .lock()
            .expect("rate")
            .is_some_and(|until| now < until)
    }

    pub fn is_due(&self, now: DateTime<Utc>) -> bool {
        if self.retired.load(Ordering::SeqCst) || !self.is_enabled() || self.needs_signin() {
            return false;
        }
        if self.rate_limited(now) {
            return false;
        }
        if self.user_refresh.load(Ordering::SeqCst) {
            return true;
        }
        now >= *self.not_before.lock().expect("next")
    }

    pub fn retire(&self) {
        self.retired.store(true, Ordering::SeqCst);
        self.cancel.store(true, Ordering::SeqCst);
        {
            let mut token = self.token.lock().expect("token");
            token.generation = token.generation.wrapping_add(1);
        }
        if let Some(flight) = self.refresh.lock().expect("refresh").take() {
            flight.handle.abort();
        }
        if let Some(task) = self.task.lock().expect("task").take() {
            task.abort();
        }
    }

    pub fn cancel_poll(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Some(task) = self.task.lock().expect("task").take() {
            task.abort();
        }
        let mut gate = self.gate.lock().expect("gate");
        gate.running = false;
        gate.again = false;
        gate.generation = gate.generation.wrapping_add(1);
    }

    pub fn note_poll_task(&self, handle: tokio::task::JoinHandle<()>) {
        *self.task.lock().expect("task") = Some(handle);
    }

    /// Bump the generation so an in-flight refresh cannot overwrite this token.
    pub fn replace_token(&self, token: GoogleAuthToken) {
        let mut state = self.token.lock().expect("token");
        state.generation = state.generation.wrapping_add(1);
        state.token = token;
        state.pending_revocation = false;
        state.needs_save = true;
    }

    fn promote_revocation(&self) -> bool {
        let mut token = self.token.lock().expect("token");
        if self.retired.load(Ordering::SeqCst) {
            return false;
        }
        if token.pending_revocation {
            token.token.needs_signin = true;
            token.pending_revocation = false;
            token.needs_save = true;
            return true;
        }
        false
    }

    fn mark_signin(&self) {
        let mut token = self.token.lock().expect("token");
        if self.retired.load(Ordering::SeqCst) {
            return;
        }
        token.token.needs_signin = true;
        token.pending_revocation = false;
        token.needs_save = true;
    }
}

async fn wait_refresh(
    account: &LiveAccount,
    mut rx: tokio::sync::watch::Receiver<Option<RefreshDone>>,
    generation: u64,
) -> Result<String, ApiKind> {
    loop {
        if rx.borrow().is_some() {
            break;
        }
        if rx.changed().await.is_err() {
            break;
        }
    }
    let done = rx.borrow().clone();
    let token = account.token.lock().expect("token");
    if account.retired.load(Ordering::SeqCst) || token.generation != generation {
        return Err(ApiKind::Temporary);
    }
    match done {
        Some(done) if done.generation != generation => Err(ApiKind::Temporary),
        Some(done) => match done.result {
            Ok(_) => Ok(token.token.access_token.clone()),
            Err(kind) => Err(kind),
        },
        None => Err(ApiKind::Temporary),
    }
}

fn token_expired(token: &GoogleAuthToken, now: DateTime<Utc>) -> bool {
    match token.expires_at {
        Some(expires_at) => now.timestamp() + 60 >= expires_at,
        None => false,
    }
}

fn stale(loaded_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now.signed_duration_since(loaded_at) >= chrono::Duration::minutes(STALE_CALENDAR_AFTER_MINUTES)
}

enum FetchOutcome {
    Success { next_in: Option<Duration> },
    Backoff,
    RateLimit { retry_after: Option<Duration> },
    SignIn,
}

async fn fetch_account(
    account: &Arc<LiveAccount>,
    api: Arc<dyn CalendarApi>,
    clock: Arc<dyn WallClock>,
) -> FetchOutcome {
    if account.cancel.load(Ordering::SeqCst) {
        return FetchOutcome::Success { next_in: None };
    }
    let now = clock.now();
    let access = {
        let token = account.token.lock().expect("token");
        if token_expired(&token.token, now) {
            None
        } else {
            Some(token.token.access_token.clone())
        }
    };
    let access = if let Some(access) = access {
        access
    } else {
        match refresh_join(Arc::clone(account), Arc::clone(&api), Arc::clone(&clock)).await {
            Ok(access) => access,
            Err(ApiKind::SignIn) => return FetchOutcome::SignIn,
            Err(ApiKind::RateLimit) => {
                return FetchOutcome::RateLimit { retry_after: None };
            }
            Err(_) => return FetchOutcome::Backoff,
        }
    };
    if account.promote_revocation() {
        return FetchOutcome::SignIn;
    }
    let now = clock.now();
    let (zone_name_unused, _) = (now, clock.timezone());
    let _ = zone_name_unused;
    let list_decision = {
        let cache = account.cache.lock().expect("cache");
        match cache.list_at {
            Some(at) if now.signed_duration_since(at) < chrono::Duration::hours(1) => {
                ListDecision::UseCache
            }
            _ => ListDecision::Fetch,
        }
    };
    let mut access = access;
    if list_decision == ListDecision::Fetch {
        let mut refreshed = false;
        loop {
            match api.list_calendars(&access).await {
                Ok(list) => {
                    let mut cache = account.cache.lock().expect("cache");
                    cache.list = list;
                    cache.list_at = Some(clock.now());
                    break;
                }
                Err(err) if err.kind == ApiKind::RateLimit => {
                    return FetchOutcome::RateLimit {
                        retry_after: err.retry_after,
                    };
                }
                Err(err) if err.kind == ApiKind::Unauthorized && !refreshed => {
                    match refresh_after_unauthorized(
                        account,
                        Arc::clone(&api),
                        Arc::clone(&clock),
                        &access,
                    )
                    .await
                    {
                        Ok(fresh) => {
                            if account.promote_revocation() {
                                return FetchOutcome::SignIn;
                            }
                            access = fresh;
                            refreshed = true;
                            continue;
                        }
                        Err(ApiKind::SignIn) => return FetchOutcome::SignIn,
                        Err(_) => return FetchOutcome::Backoff,
                    }
                }
                Err(err)
                    if err.kind == ApiKind::SignIn
                        || err.kind == ApiKind::InsufficientPermissions =>
                {
                    return FetchOutcome::SignIn;
                }
                Err(_) => {
                    let cache = account.cache.lock().expect("cache");
                    if cache.list.is_empty() {
                        return FetchOutcome::Backoff;
                    }
                    break;
                }
            }
        }
    }
    let (list, hidden, skipped) = {
        let cache = account.cache.lock().expect("cache");
        let hidden = account
            .preferences
            .lock()
            .expect("preferences")
            .hidden_calendars
            .clone();
        (cache.list.clone(), hidden, cache.skipped.clone())
    };
    let email = account.email();
    let mut successes = 0usize;
    let mut temporary = 0usize;
    let mut insufficient = 0usize;
    let mut forbidden = 0usize;
    let mut considered = 0usize;
    for calendar in &list {
        if account.cancel.load(Ordering::SeqCst) {
            break;
        }
        if calendar.access_role == "freeBusyReader" || hidden.iter().any(|id| id == &calendar.id) {
            continue;
        }
        if skipped
            .get(&calendar.id)
            .is_some_and(|role| role == &calendar.access_role)
        {
            continue;
        }
        considered += 1;
        match api
            .list_events(
                &access,
                &calendar.id,
                window_start(clock.as_ref()),
                window_end(clock.as_ref()),
            )
            .await
        {
            Ok(events) => {
                successes += 1;
                store_events(account, calendar, events, &email, clock.now());
                account
                    .cache
                    .lock()
                    .expect("cache")
                    .skipped
                    .remove(&calendar.id);
            }
            Err(err) => match err.kind {
                ApiKind::RateLimit => {
                    return FetchOutcome::RateLimit {
                        retry_after: err.retry_after,
                    };
                }
                ApiKind::Unauthorized => {
                    match refresh_after_unauthorized(
                        account,
                        Arc::clone(&api),
                        Arc::clone(&clock),
                        &access,
                    )
                    .await
                    {
                        Ok(fresh) => {
                            if let Ok(events) = api
                                .list_events(
                                    &fresh,
                                    &calendar.id,
                                    window_start(clock.as_ref()),
                                    window_end(clock.as_ref()),
                                )
                                .await
                            {
                                successes += 1;
                                store_events(account, calendar, events, &email, clock.now());
                            } else {
                                temporary += 1;
                            }
                        }
                        Err(ApiKind::SignIn) => return FetchOutcome::SignIn,
                        Err(_) => temporary += 1,
                    }
                }
                ApiKind::SkipCalendar => {
                    forbidden += 1;
                    account
                        .cache
                        .lock()
                        .expect("cache")
                        .skipped
                        .insert(calendar.id.clone(), calendar.access_role.clone());
                }
                ApiKind::InsufficientPermissions => {
                    insufficient += 1;
                    account
                        .cache
                        .lock()
                        .expect("cache")
                        .skipped
                        .insert(calendar.id.clone(), calendar.access_role.clone());
                }
                ApiKind::SignIn => return FetchOutcome::SignIn,
                ApiKind::InvalidClient | ApiKind::Temporary => temporary += 1,
            },
        }
    }
    if considered > 0
        && insufficient == considered
        && successes == 0
        && temporary == 0
        && forbidden == 0
    {
        return FetchOutcome::SignIn;
    }
    if successes == 0 && temporary > 0 {
        return FetchOutcome::Backoff;
    }
    FetchOutcome::Success {
        next_in: next_interest(account, clock.as_ref()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListDecision {
    Fetch,
    UseCache,
}

fn store_events(
    account: &LiveAccount,
    calendar: &CalendarInfo,
    events: Vec<Event>,
    email: &str,
    now: DateTime<Utc>,
) {
    let kept: Vec<Event> = events
        .into_iter()
        .filter(|event| event_visible(event, email))
        .collect();
    let sources = kept
        .iter()
        .map(|event| AlertSource {
            event: event.clone(),
            default_reminders: calendar.default_reminders.clone(),
            account_email: email.to_string(),
        })
        .collect();
    account.cache.lock().expect("cache").events.insert(
        calendar.id.clone(),
        CachedCalendar {
            events: kept,
            sources,
            loaded_at: now,
        },
    );
}

fn event_visible(event: &Event, email: &str) -> bool {
    if email.is_empty() {
        return true;
    }
    let creator = event
        .creator
        .as_ref()
        .is_some_and(|creator| creator.email.eq_ignore_ascii_case(email));
    if creator {
        return true;
    }
    event
        .attendees
        .iter()
        .any(|attendee| attendee.email.eq_ignore_ascii_case(email))
}

pub(crate) fn window_start(clock: &dyn WallClock) -> DateTime<Utc> {
    let (now, zone) = observation(clock);
    let local = now.with_timezone(&zone);
    let midnight = chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap();
    crate::event_grouping::resolve_local_datetime(zone, local.date_naive().and_time(midnight))
        .to_utc()
}

pub(crate) fn window_end(clock: &dyn WallClock) -> DateTime<Utc> {
    let (now, zone) = observation(clock);
    let local = now.with_timezone(&zone);
    let date = local.date_naive() + chrono::Days::new(3);
    let midnight = chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap();
    crate::event_grouping::resolve_local_datetime(zone, date.and_time(midnight)).to_utc()
}

fn next_interest(account: &LiveAccount, clock: &dyn WallClock) -> Option<Duration> {
    let now = clock.now();
    let zone = clock.timezone();
    let cache = account.cache.lock().expect("cache");
    let mut best: Option<Duration> = None;
    for cached in cache.events.values() {
        if stale(cached.loaded_at, now) {
            continue;
        }
        for source in &cached.sources {
            let Some(mut candidate) = candidate_from_source(source, zone) else {
                continue;
            };
            candidate.source_index = 0;
            consider(&mut best, candidate.anchor, now);
            for minutes in &candidate.popup_minutes {
                if let Some(fire_at) = candidate
                    .anchor
                    .checked_sub_signed(chrono::Duration::minutes(*minutes))
                {
                    consider(&mut best, fire_at, now);
                }
            }
        }
    }
    best
}

fn consider(best: &mut Option<Duration>, at: DateTime<Utc>, now: DateTime<Utc>) {
    if at <= now {
        return;
    }
    let wait = (at - now).to_std().unwrap_or(Duration::from_secs(0));
    *best = Some(match *best {
        Some(current) if current < wait => current,
        _ => wait,
    });
}

async fn refresh_after_unauthorized(
    account: &Arc<LiveAccount>,
    api: Arc<dyn CalendarApi>,
    clock: Arc<dyn WallClock>,
    rejected: &str,
) -> Result<String, ApiKind> {
    let current = account.token_snapshot().access_token;
    if current != rejected {
        return Ok(current);
    }
    refresh_join(Arc::clone(account), api, clock).await
}

async fn refresh_join(
    account: Arc<LiveAccount>,
    api: Arc<dyn CalendarApi>,
    clock: Arc<dyn WallClock>,
) -> Result<String, ApiKind> {
    let (generation, refresh_token) = {
        let token = account.token.lock().expect("token");
        if account.retired.load(Ordering::SeqCst) {
            return Err(ApiKind::Temporary);
        }
        let Some(refresh_token) = token
            .token
            .refresh_token
            .clone()
            .filter(|value| !value.is_empty())
        else {
            return Err(ApiKind::SignIn);
        };
        (token.generation, refresh_token)
    };
    let (tx, rx) = tokio::sync::watch::channel(None::<RefreshDone>);
    let account_for_task = Arc::clone(&account);
    let api_for_task = Arc::clone(&api);
    let clock_for_task = Arc::clone(&clock);
    let rx = {
        let mut flight = account.refresh.lock().expect("refresh");
        if let Some(existing) = flight.as_ref() {
            if existing.generation == generation && !existing.handle.is_finished() {
                existing.rx.clone()
            } else {
                launch_refresh(
                    &mut flight,
                    tx,
                    rx,
                    generation,
                    refresh_token,
                    account_for_task,
                    api_for_task,
                    clock_for_task,
                )
            }
        } else {
            launch_refresh(
                &mut flight,
                tx,
                rx,
                generation,
                refresh_token,
                account_for_task,
                api_for_task,
                clock_for_task,
            )
        }
    };
    wait_refresh(&account, rx, generation).await
}

fn launch_refresh(
    flight: &mut Option<RefreshFlight>,
    tx: tokio::sync::watch::Sender<Option<RefreshDone>>,
    rx: tokio::sync::watch::Receiver<Option<RefreshDone>>,
    generation: u64,
    refresh_token: String,
    account: Arc<LiveAccount>,
    api: Arc<dyn CalendarApi>,
    clock: Arc<dyn WallClock>,
) -> tokio::sync::watch::Receiver<Option<RefreshDone>> {
    let handle = tokio::spawn(async move {
        let result = api.refresh_token(&refresh_token).await;
        apply_refresh(&account, generation, clock.now(), &result);
        let done = RefreshDone {
            generation,
            result: match &result {
                Ok(token) => Ok(token.access_token.clone()),
                Err(err) => Err(err.kind.clone()),
            },
        };
        let _ = tx.send(Some(done));
    });
    *flight = Some(RefreshFlight {
        generation,
        handle,
        rx: rx.clone(),
    });
    rx
}

fn apply_refresh(
    account: &LiveAccount,
    generation: u64,
    now: DateTime<Utc>,
    result: &Result<crate::google_api::RefreshedToken, ApiError>,
) {
    let mut token = account.token.lock().expect("token");
    if account.retired.load(Ordering::SeqCst) || token.generation != generation {
        return;
    }
    match result {
        Ok(refreshed) => {
            token.token.access_token = refreshed.access_token.clone();
            if let Some(rotated) = &refreshed.refresh_token {
                token.token.refresh_token = Some(rotated.clone());
            }
            token.token.expires_in = refreshed.expires_in;
            token.token.expires_at = Some(now.timestamp() + refreshed.expires_in);
            token.token.needs_signin = false;
            token.pending_revocation = false;
            token.needs_save = true;
        }
        Err(err) if err.kind == ApiKind::SignIn => {
            token.pending_revocation = true;
            token.needs_save = true;
        }
        Err(_) => {}
    }
}

/// Background lookup for a legacy token that was stored without an email.
pub fn spawn_identify(
    account: Arc<LiveAccount>,
    api: Arc<dyn CalendarApi>,
    clock: Arc<dyn WallClock>,
) {
    tokio::spawn(async move {
        let mut delay = Duration::from_secs(5);
        loop {
            if account.retired.load(Ordering::SeqCst) || !account.email().is_empty() {
                return;
            }
            let access = account.token_snapshot().access_token;
            match api.user_info(&access).await {
                Ok(user) => {
                    let mut token = account.token.lock().expect("token");
                    if account.retired.load(Ordering::SeqCst) {
                        return;
                    }
                    token.token.user = Some(user.clone());
                    token.needs_save = true;
                    drop(token);
                    *account.email.lock().expect("email") = user.email;
                    account.force_due(clock.now());
                    return;
                }
                Err(_) => {
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(120));
                }
            }
        }
    });
}
