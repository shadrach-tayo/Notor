//! Scheduler, account health, token races, stale events, time, and upgrade.

use crate::account::{spawn_identify, window_end, window_start, Calendars};
use crate::clock::{ManualClock, WallClock};
use crate::google_api::{ApiError, ApiKind, CalendarApi, CalendarInfo, GoogleClient, ScriptedApi};
use crate::reminder_policy::{
    due_reminders, load_fired_startup, save_fired_state, AlertCandidate, FiredState,
};
use crate::scheduler::Scheduler;
use crate::types::{GoogleAuthToken, Preferences, UserInfo};
use crate::DEFAULT_ALERT_GRACE;
use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use google_calendar::types::Event;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::fixed_new_york())
}

fn harness() -> (
    Arc<ManualClock>,
    Arc<Calendars>,
    Arc<ScriptedApi>,
    Scheduler,
) {
    let clock = clock();
    let calendars = Arc::new(Calendars::with_clock(
        Arc::clone(&clock) as Arc<dyn WallClock>
    ));
    let api = Arc::new(ScriptedApi::new());
    let scheduler = Scheduler::new(
        Arc::clone(&calendars),
        Arc::clone(&api) as Arc<dyn CalendarApi>,
        Arc::clone(&clock) as Arc<dyn WallClock>,
    );
    (clock, calendars, api, scheduler)
}

fn user(email: &str) -> UserInfo {
    UserInfo {
        email: email.to_string(),
        name: email.to_string(),
        ..UserInfo::default()
    }
}

fn token(email: &str, access: &str, refresh: &str, expires_at: i64) -> GoogleAuthToken {
    GoogleAuthToken {
        access_token: access.to_string(),
        refresh_token: Some(refresh.to_string()),
        expires_at: Some(expires_at),
        expires_in: 3600,
        user: Some(user(email)),
        ..GoogleAuthToken::default()
    }
}

fn meeting(id: &str, email: &str, start: DateTime<Utc>) -> Event {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "summary": "Standup",
        "status": "confirmed",
        "eventType": "default",
        "iCalUID": format!("{id}-uid"),
        "creator": {"email": email},
        "start": {"dateTime": start.to_rfc3339()},
        "end": {"dateTime": (start + ChronoDuration::minutes(30)).to_rfc3339()},
        "reminders": {"useDefault": false, "overrides": [{"method": "popup", "minutes": 10}]}
    }))
    .unwrap()
}

fn all_day(id: &str, email: &str, date: NaiveDate) -> Event {
    let end = date.succ_opt().unwrap();
    serde_json::from_value(serde_json::json!({
        "id": id,
        "summary": "Holiday",
        "status": "confirmed",
        "eventType": "default",
        "iCalUID": format!("{id}-uid"),
        "creator": {"email": email},
        "start": {"date": date.to_string()},
        "end": {"date": end.to_string()}
    }))
    .unwrap()
}

fn primary() -> CalendarInfo {
    CalendarInfo {
        id: "primary".into(),
        access_role: "owner".into(),
        default_reminders: Vec::new(),
    }
}

async fn pump(scheduler: &mut Scheduler) {
    scheduler.drive().await;
    scheduler.wait_idle().await;
}

fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("notor-behavior-{name}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn failing_account_backs_off_while_a_healthy_one_keeps_its_pace() {
    let (clock, calendars, api, mut scheduler) = harness();
    let now = clock.now().timestamp();
    api.set_calendars_for("bad-access", Err(ApiError::temporary("down")));
    calendars.install_tokens(
        vec![
            token("bad@example.com", "bad-access", "bad-refresh", now + 3600),
            token(
                "good@example.com",
                "good-access",
                "good-refresh",
                now + 3600,
            ),
        ],
        &Preferences::default(),
    );
    let mut elapsed = 0u64;
    while elapsed <= 600 {
        pump(&mut scheduler).await;
        clock.advance(Duration::from_secs(15));
        elapsed += 15;
    }
    let bad = calendars.find("bad@example.com").unwrap().poll_count();
    let good = calendars.find("good@example.com").unwrap().poll_count();
    assert_eq!(
        bad, 6,
        "15s, 30s, 60s, 120s, 240s then the next wait is past 10 minutes"
    );
    assert_eq!(good, 3, "an idle account stays on the 5 minute tier");
}

#[tokio::test]
async fn wake_polls_immediately_and_a_rate_limit_still_holds() {
    let (clock, calendars, api, mut scheduler) = harness();
    let now = clock.now().timestamp();
    api.set_calendars_for(
        "limited",
        Err(ApiError {
            kind: ApiKind::RateLimit,
            retry_after: Some(Duration::from_secs(6 * 60 * 60)),
            status: Some(429),
            body: "slow".into(),
        }),
    );
    calendars.install_tokens(
        vec![
            token("limited@example.com", "limited", "r", now + 3600),
            token("ready@example.com", "ready", "r2", now + 3600),
        ],
        &Preferences::default(),
    );
    pump(&mut scheduler).await;
    assert_eq!(calendars.find("ready@example.com").unwrap().poll_count(), 1);
    assert_eq!(
        calendars.find("limited@example.com").unwrap().poll_count(),
        1
    );
    clock.jump_wall(ChronoDuration::hours(3));
    pump(&mut scheduler).await;
    assert_eq!(calendars.find("ready@example.com").unwrap().poll_count(), 2);
    assert_eq!(
        calendars.find("limited@example.com").unwrap().poll_count(),
        1
    );
    calendars
        .find("limited@example.com")
        .unwrap()
        .force_due(clock.now());
    pump(&mut scheduler).await;
    assert_eq!(
        calendars.find("limited@example.com").unwrap().poll_count(),
        1,
        "a user refresh does not skip Retry-After"
    );
}

#[tokio::test]
async fn a_second_trigger_waits_until_the_poll_finishes() {
    let (clock, calendars, api, mut scheduler) = harness();
    let now = clock.now().timestamp();
    calendars.install_tokens(
        vec![token("a@example.com", "access", "refresh", now + 3600)],
        &Preferences::default(),
    );
    api.hold_events(true);
    scheduler.drive().await;
    for _ in 0..50 {
        if api.calls().iter().any(|call| call.starts_with("events:")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let account = calendars.find("a@example.com").unwrap();
    scheduler.trigger(account).await;
    assert_eq!(api.max_in_flight(), 1);
    api.hold_events(false);
    scheduler.wait_idle().await;
    assert_eq!(calendars.find("a@example.com").unwrap().poll_count(), 2);
    assert_eq!(api.max_in_flight(), 1);
}

#[tokio::test]
async fn adding_an_account_does_not_poll_the_others_and_removing_the_last_clears_events() {
    let (clock, calendars, api, mut scheduler) = harness();
    let now = clock.now().timestamp();
    let start = clock.now() + ChronoDuration::minutes(20);
    api.set_events(
        "primary",
        Ok(vec![meeting("m", "first@example.com", start)]),
    );
    calendars.install_tokens(
        vec![token("first@example.com", "access", "refresh", now + 3600)],
        &Preferences::default(),
    );
    pump(&mut scheduler).await;
    assert_eq!(calendars.polled_events().len(), 1);
    let calls_before = api.calls().len();
    assert!(calendars.add_account(
        token("second@example.com", "access-2", "refresh-2", now + 3600),
        &Preferences::default(),
    ));
    assert_eq!(
        api.calls().len(),
        calls_before,
        "adding an account makes no network call"
    );
    pump(&mut scheduler).await;
    assert_eq!(calendars.find("first@example.com").unwrap().poll_count(), 1);
    assert_eq!(
        calendars.find("second@example.com").unwrap().poll_count(),
        1
    );
    let err = calendars.remove_account("missing@example.com").unwrap_err();
    assert!(err.contains("unknown account"));
    let outcome = calendars.remove_account("second@example.com").unwrap();
    assert!(!outcome.open_sign_in);
    let outcome = calendars.remove_account("first@example.com").unwrap();
    assert!(outcome.open_sign_in);
    assert_eq!(outcome.remaining, 0);
    assert!(calendars.polled_events().is_empty());
}

#[tokio::test]
async fn revoke_stops_polling_and_sign_in_again_keeps_disabled() {
    let (clock, calendars, api, mut scheduler) = harness();
    let now = clock.now().timestamp();
    api.set_events(
        "primary",
        Err(ApiError::kind(
            ApiKind::InsufficientPermissions,
            "withdrawn",
        )),
    );
    let mut disabled = token("person@example.com", "access", "refresh", now + 3600);
    disabled.disabled = Some(true);
    calendars.install_tokens(vec![disabled], &Preferences::default());
    calendars.enable_account("person@example.com").unwrap();
    pump(&mut scheduler).await;
    let account = calendars.find("person@example.com").unwrap();
    assert!(account.needs_signin());
    assert_eq!(
        calendars.sign_in_emails(),
        vec!["person@example.com".to_string()]
    );
    let polls = account.poll_count();
    pump(&mut scheduler).await;
    assert_eq!(account.poll_count(), polls);
    calendars.disable_account("person@example.com").unwrap();
    assert!(calendars.sign_in_emails().is_empty());
    let mut again = token("person@example.com", "fresh", "fresh-refresh", now + 3600);
    again.needs_signin = false;
    assert!(calendars.add_account(again, &Preferences::default()));
    let replaced = calendars.find("person@example.com").unwrap();
    assert_eq!(replaced.token_snapshot().disabled, Some(true));
    assert!(!replaced.needs_signin());
    assert_ne!(
        replaced.token_snapshot().access_token,
        account.token_snapshot().access_token
    );
}

#[tokio::test]
async fn refresh_survives_a_cancelled_poll_and_the_next_poll_saves_it() {
    let (clock, calendars, api, mut scheduler) = harness();
    let dir = scratch("refresh-save");
    calendars.open_directory(&dir, &Preferences::default());
    let expired = token(
        "person@example.com",
        "old-access",
        "refresh-1",
        clock.now().timestamp() - 120,
    );
    assert!(calendars.add_account(expired, &Preferences::default()));
    api.set_refresh(
        "refresh-1",
        Ok(crate::google_api::RefreshedToken {
            access_token: "rotated-access".into(),
            refresh_token: Some("rotated-refresh".into()),
            expires_in: 3600,
        }),
    );
    api.hold_refresh(true);
    scheduler.drive().await;
    for _ in 0..50 {
        if api.calls().iter().any(|call| call.starts_with("refresh:")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    calendars.find("person@example.com").unwrap().cancel_poll();
    api.hold_refresh(false);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let saved = calendars
        .find("person@example.com")
        .unwrap()
        .token_snapshot();
    assert_eq!(saved.access_token, "rotated-access");
    assert_eq!(saved.refresh_token.as_deref(), Some("rotated-refresh"));
    pump(&mut scheduler).await;
    calendars.save().unwrap();
    let reloaded = Calendars::with_clock(Arc::clone(&clock) as Arc<dyn WallClock>);
    assert_eq!(reloaded.open_directory(&dir, &Preferences::default()), 1);
    let token = &reloaded.get_tokens()[0];
    assert_eq!(token.access_token, "rotated-access");
    assert_eq!(token.refresh_token.as_deref(), Some("rotated-refresh"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn one_refresh_is_shared_and_a_newer_token_wins() {
    let (clock, calendars, api, _scheduler) = harness();
    let now = clock.now().timestamp();
    calendars.install_tokens(
        vec![token("person@example.com", "old", "refresh-1", now - 120)],
        &Preferences::default(),
    );
    api.hold_refresh(true);
    let api_dyn: Arc<dyn CalendarApi> = Arc::clone(&api) as Arc<dyn CalendarApi>;
    let first = {
        let calendars = Arc::clone(&calendars);
        let api = Arc::clone(&api_dyn);
        tokio::spawn(async move { calendars.refresh_expired_tokens(&api).await })
    };
    for _ in 0..50 {
        if api.calls().iter().any(|call| call.starts_with("refresh:")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let account = calendars.find("person@example.com").unwrap();
    let second = {
        let calendars = Arc::clone(&calendars);
        let api = Arc::clone(&api_dyn);
        let account = Arc::clone(&account);
        tokio::spawn(async move { calendars.poll_account(&account, &api, 0.0).await })
    };
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(
        api.calls()
            .iter()
            .filter(|call| call.starts_with("refresh:"))
            .count(),
        1
    );
    account.replace_token(token(
        "person@example.com",
        "signed-in-again",
        "new-refresh",
        now + 3600,
    ));
    api.hold_refresh(false);
    first.await.unwrap();
    second.await.unwrap();
    assert_eq!(
        api.calls()
            .iter()
            .filter(|call| call.starts_with("refresh:"))
            .count(),
        1
    );
    assert_eq!(account.token_snapshot().access_token, "signed-in-again");
}

#[tokio::test]
async fn a_revocation_found_by_a_cancelled_poll_is_reported_next_time() {
    let (clock, calendars, api, mut scheduler) = harness();
    calendars.install_tokens(
        vec![token(
            "person@example.com",
            "old",
            "refresh-1",
            clock.now().timestamp() - 60,
        )],
        &Preferences::default(),
    );
    api.set_refresh(
        "refresh-1",
        Err(ApiError::kind(ApiKind::SignIn, "invalid_grant")),
    );
    api.hold_refresh(true);
    scheduler.drive().await;
    for _ in 0..50 {
        if api.calls().iter().any(|call| call.starts_with("refresh:")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    calendars.find("person@example.com").unwrap().cancel_poll();
    api.hold_refresh(false);
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert!(!calendars.find("person@example.com").unwrap().needs_signin());
    pump(&mut scheduler).await;
    assert!(calendars.find("person@example.com").unwrap().needs_signin());
    let refreshes = api
        .calls()
        .iter()
        .filter(|call| call.starts_with("refresh:"))
        .count();
    assert_eq!(refreshes, 1);
}

#[tokio::test]
async fn removing_an_account_drops_a_late_refresh() {
    let (clock, calendars, api, mut scheduler) = harness();
    calendars.install_tokens(
        vec![token(
            "person@example.com",
            "old",
            "refresh-1",
            clock.now().timestamp() - 60,
        )],
        &Preferences::default(),
    );
    api.hold_refresh(true);
    scheduler.drive().await;
    for _ in 0..50 {
        if api.calls().iter().any(|call| call.starts_with("refresh:")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    calendars.remove_account("person@example.com").unwrap();
    let now = clock.now().timestamp();
    assert!(calendars.add_account(
        token(
            "person@example.com",
            "brand-new",
            "brand-refresh",
            now + 3600
        ),
        &Preferences::default(),
    ));
    api.hold_refresh(false);
    tokio::time::sleep(Duration::from_millis(40)).await;
    let token = calendars
        .find("person@example.com")
        .unwrap()
        .token_snapshot();
    assert_eq!(token.access_token, "brand-new");
    assert!(!token.needs_signin);
}

#[tokio::test]
async fn forbidden_calendars_are_skipped_and_free_busy_is_not_requested() {
    let (clock, calendars, api, mut scheduler) = harness();
    api.set_calendars(Ok(vec![
        primary(),
        CalendarInfo {
            id: "secret".into(),
            access_role: "reader".into(),
            default_reminders: Vec::new(),
        },
        CalendarInfo {
            id: "busy".into(),
            access_role: "freeBusyReader".into(),
            default_reminders: Vec::new(),
        },
    ]));
    api.set_events(
        "secret",
        Err(ApiError::kind(ApiKind::SkipCalendar, "forbidden")),
    );
    api.set_events(
        "busy",
        Err(ApiError::kind(ApiKind::SignIn, "should not be called")),
    );
    calendars.install_tokens(
        vec![token(
            "person@example.com",
            "access",
            "refresh",
            clock.now().timestamp() + 3600,
        )],
        &Preferences::default(),
    );
    pump(&mut scheduler).await;
    assert!(!calendars.find("person@example.com").unwrap().needs_signin());
    assert!(!api.calls().iter().any(|call| call == "events:busy"));
    calendars
        .find("person@example.com")
        .unwrap()
        .force_due(clock.now());
    pump(&mut scheduler).await;
    assert_eq!(
        api.calls()
            .iter()
            .filter(|call| call.as_str() == "events:secret")
            .count(),
        1
    );
    assert_eq!(
        api.calls()
            .iter()
            .filter(|call| call.as_str() == "events:primary")
            .count(),
        2
    );
}

#[tokio::test]
async fn a_rate_limited_calendar_list_does_not_use_the_cache() {
    let (clock, calendars, api, mut scheduler) = harness();
    let now = clock.now().timestamp();
    calendars.install_tokens(
        vec![token("person@example.com", "access", "refresh", now + 3600)],
        &Preferences::default(),
    );
    pump(&mut scheduler).await;
    api.set_calendars(Err(ApiError {
        kind: ApiKind::RateLimit,
        retry_after: Some(Duration::from_secs(30)),
        status: Some(429),
        body: "quota".into(),
    }));
    clock.advance(Duration::from_secs(3600));
    calendars
        .find("person@example.com")
        .unwrap()
        .force_due(clock.now());
    pump(&mut scheduler).await;
    assert!(calendars
        .find("person@example.com")
        .unwrap()
        .rate_limited_at(clock.now()));
    clock.advance(Duration::from_secs(30));
    api.set_calendars(Ok(vec![primary()]));
    pump(&mut scheduler).await;
    assert!(!calendars
        .find("person@example.com")
        .unwrap()
        .rate_limited_at(clock.now()));
}

#[tokio::test]
async fn stale_events_expire_from_the_original_load_and_return_on_success() {
    let (clock, calendars, api, mut scheduler) = harness();
    let start = clock.now() + ChronoDuration::hours(6);
    api.set_events(
        "primary",
        Ok(vec![meeting("m", "person@example.com", start)]),
    );
    calendars.install_tokens(
        vec![token(
            "person@example.com",
            "access",
            "refresh",
            clock.now().timestamp() + 3600,
        )],
        &Preferences::default(),
    );
    pump(&mut scheduler).await;
    assert_eq!(calendars.polled_events().len(), 1);
    clock.advance(Duration::from_secs(100 * 60));
    api.set_events("primary", Err(ApiError::temporary("blip")));
    calendars
        .find("person@example.com")
        .unwrap()
        .force_due(clock.now());
    pump(&mut scheduler).await;
    assert_eq!(calendars.polled_events().len(), 1);
    clock.advance(Duration::from_secs(21 * 60));
    assert!(calendars.publish_snapshot());
    assert!(calendars.polled_events().is_empty());
    api.set_events(
        "primary",
        Ok(vec![meeting("m", "person@example.com", start)]),
    );
    calendars
        .find("person@example.com")
        .unwrap()
        .force_due(clock.now());
    pump(&mut scheduler).await;
    assert_eq!(calendars.polled_events().len(), 1);
}

#[tokio::test]
async fn a_failed_first_poll_does_not_publish_an_empty_snapshot() {
    let (clock, calendars, api, mut scheduler) = harness();
    api.set_calendars(Err(ApiError::temporary("offline")));
    calendars.install_tokens(
        vec![token(
            "person@example.com",
            "access",
            "refresh",
            clock.now().timestamp() + 3600,
        )],
        &Preferences::default(),
    );
    pump(&mut scheduler).await;
    assert!(!calendars.publish_snapshot());
}

#[tokio::test]
async fn an_unauthorized_list_is_retried_with_the_refreshed_token() {
    let (clock, calendars, api, mut scheduler) = harness();
    api.set_calendars_for("old", Err(ApiError::kind(ApiKind::Unauthorized, "401")));
    api.set_calendars_for("access-from-refresh-1", Ok(vec![primary()]));
    calendars.install_tokens(
        vec![token(
            "person@example.com",
            "old",
            "refresh-1",
            clock.now().timestamp() + 3600,
        )],
        &Preferences::default(),
    );
    pump(&mut scheduler).await;
    assert!(api.calls().iter().any(|call| call == "calendars:old"));
    assert!(api
        .calls()
        .iter()
        .any(|call| call == "calendars:access-from-refresh-1"));
    assert!(!calendars.find("person@example.com").unwrap().needs_signin());
    assert_eq!(
        calendars
            .find("person@example.com")
            .unwrap()
            .token_snapshot()
            .access_token,
        "access-from-refresh-1"
    );
}

#[tokio::test]
async fn timezone_change_polls_and_intervals_follow_the_new_zone() {
    let (clock, calendars, api, mut scheduler) = harness();
    api.set_events(
        "primary",
        Ok(vec![all_day(
            "holiday",
            "person@example.com",
            NaiveDate::from_ymd_opt(2025, 6, 19).unwrap(),
        )]),
    );
    calendars.install_tokens(
        vec![token(
            "person@example.com",
            "access",
            "refresh",
            clock.now().timestamp() + 3600,
        )],
        &Preferences::default(),
    );
    pump(&mut scheduler).await;
    let account = calendars.find("person@example.com").unwrap();
    assert_eq!(account.poll_count(), 1);
    let ny_start = window_start(clock.as_ref());
    clock.set_timezone("Asia/Shanghai".parse::<Tz>().unwrap());
    pump(&mut scheduler).await;
    assert_eq!(account.poll_count(), 2);
    assert_ne!(window_start(clock.as_ref()), ny_start);
    assert!(window_end(clock.as_ref()) > window_start(clock.as_ref()));
    clock.advance(Duration::from_secs(130));
    pump(&mut scheduler).await;
    assert_eq!(
        account.poll_count(),
        3,
        "Shanghai midnight is inside two hours, so the tier is 2 minutes"
    );
}

#[tokio::test]
async fn a_timezone_change_does_not_cut_off_a_poll_or_ignore_sign_in() {
    let (clock, calendars, api, mut scheduler) = harness();
    calendars.install_tokens(
        vec![token(
            "person@example.com",
            "access",
            "refresh",
            clock.now().timestamp() + 3600,
        )],
        &Preferences::default(),
    );
    api.hold_events(true);
    scheduler.drive().await;
    for _ in 0..50 {
        if api.calls().iter().any(|call| call.starts_with("events:")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    clock.set_timezone("America/Los_Angeles".parse::<Tz>().unwrap());
    scheduler.drive().await;
    assert_eq!(api.max_in_flight(), 1);
    api.hold_events(false);
    scheduler.wait_idle().await;
    assert_eq!(
        calendars.find("person@example.com").unwrap().poll_count(),
        2
    );
    api.set_events(
        "primary",
        Err(ApiError::kind(ApiKind::InsufficientPermissions, "gone")),
    );
    calendars
        .find("person@example.com")
        .unwrap()
        .force_due(clock.now());
    pump(&mut scheduler).await;
    assert!(calendars.find("person@example.com").unwrap().needs_signin());
    let polls = calendars.find("person@example.com").unwrap().poll_count();
    clock.set_timezone("Europe/London".parse::<Tz>().unwrap());
    pump(&mut scheduler).await;
    assert_eq!(
        calendars.find("person@example.com").unwrap().poll_count(),
        polls
    );
}

#[test]
fn all_day_reminders_follow_the_date_across_zones() {
    let date = NaiveDate::from_ymd_opt(2025, 6, 18).unwrap();
    let ny: Tz = "America/New_York".parse().unwrap();
    let la: Tz = "America/Los_Angeles".parse().unwrap();
    let london: Tz = "Europe/London".parse().unwrap();
    let ny_midnight = crate::event_grouping::all_day_start(ny, date).to_utc();
    let la_midnight = crate::event_grouping::all_day_start(la, date).to_utc();
    let london_midnight = crate::event_grouping::all_day_start(london, date).to_utc();
    let mut candidate = AlertCandidate {
        source_index: 0,
        event_id: "holiday".into(),
        i_cal_uid: String::new(),
        status: "confirmed".into(),
        event_type: "default".into(),
        anchor: ny_midnight,
        all_day_date: Some(date),
        original_start_unix: None,
        sequence: 0,
        use_default: false,
        popup_overrides: vec![0],
        popup_minutes: vec![0],
        self_declined: false,
        attendee_count: 0,
        has_conference_link: false,
    };
    let fired_at = ny_midnight + ChronoDuration::seconds(30);
    let mut state = FiredState::default();
    let first = due_reminders(
        &[candidate.clone()],
        fired_at,
        DEFAULT_ALERT_GRACE,
        false,
        &mut state,
    );
    assert_eq!(first.len(), 1);
    candidate.anchor = la_midnight;
    let again = due_reminders(
        &[candidate.clone()],
        fired_at,
        DEFAULT_ALERT_GRACE,
        false,
        &mut state,
    );
    assert!(
        again.is_empty(),
        "a fired all-day reminder stays fired after travelling west"
    );
    let legacy_unix = ny_midnight.timestamp();
    state.reminders[0].meeting_key = format!("id:holiday@{legacy_unix}");
    let recognised = due_reminders(
        &[candidate.clone()],
        fired_at,
        DEFAULT_ALERT_GRACE,
        false,
        &mut state,
    );
    assert!(
        recognised.is_empty(),
        "a reminder stored under local midnight is still the same date"
    );

    let mut unfired = candidate.clone();
    unfired.event_id = "other".into();
    unfired.anchor = la_midnight;
    let mut fresh = FiredState::default();
    let before = due_reminders(
        &[unfired.clone()],
        ny_midnight,
        DEFAULT_ALERT_GRACE,
        false,
        &mut fresh,
    );
    assert!(
        before.is_empty(),
        "Los Angeles midnight has not arrived yet"
    );
    let at_la = due_reminders(
        &[unfired.clone()],
        la_midnight + ChronoDuration::seconds(20),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fresh,
    );
    assert_eq!(at_la.len(), 1);

    unfired.anchor = london_midnight;
    unfired.event_id = "east".into();
    let mut east = FiredState::default();
    let skipped = due_reminders(&[unfired], fired_at, DEFAULT_ALERT_GRACE, false, &mut east);
    assert!(
        skipped.is_empty(),
        "a reminder whose new local time is past the grace window is skipped"
    );
    assert!(east.reminders.is_empty());
}

#[test]
fn tray_groups_follow_one_zone() {
    let clock = clock();
    let calendars = Calendars::with_clock(Arc::clone(&clock) as Arc<dyn WallClock>);
    let start = Utc.with_ymd_and_hms(2025, 6, 18, 15, 0, 0).unwrap();
    let _api = ScriptedApi::new();
    _api.set_events(
        "primary",
        Ok(vec![meeting("m", "person@example.com", start)]),
    );
    calendars.install_tokens(
        vec![token(
            "person@example.com",
            "access",
            "refresh",
            clock.now().timestamp() + 3600,
        )],
        &Preferences::default(),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(calendars.poll_account(
        &calendars.accounts()[0],
        &(Arc::new(_api) as Arc<dyn CalendarApi>),
        0.0,
    ));
    assert_eq!(calendars.event_groups.lock().unwrap().upcoming.len(), 1);
    clock.set_timezone("Australia/Adelaide".parse().unwrap());
    calendars.publish_snapshot();
    let groups = calendars.event_groups.lock().unwrap().clone();
    assert!(groups.upcoming.is_empty());
    assert_eq!(groups.tomorrow.len(), 1);
}

#[tokio::test]
async fn credentials_that_arrive_after_a_failure_poll_every_account() {
    let (clock, calendars, _api, mut scheduler) = harness();
    calendars.install_tokens(
        vec![token(
            "person@example.com",
            "access",
            "refresh",
            clock.now().timestamp() + 3600,
        )],
        &Preferences::default(),
    );
    pump(&mut scheduler).await;
    assert_eq!(
        calendars.find("person@example.com").unwrap().poll_count(),
        1
    );
    let phase = Arc::new(std::sync::Mutex::new(0u8));
    let phase_task = Arc::clone(&phase);
    let keeper = crate::google_api::CredKeeper::new(Arc::new(move || {
        let phase = Arc::clone(&phase_task);
        Box::pin(async move {
            let mut phase = phase.lock().unwrap();
            *phase += 1;
            if *phase == 1 {
                Err(ApiError::temporary("offline"))
            } else {
                Ok(crate::types::AppCredentials {
                    google_client_id: "id".into(),
                    ..crate::types::AppCredentials::default()
                })
            }
        })
    }));
    let err = keeper.get(clock.now()).await.unwrap_err();
    assert_eq!(err.kind, ApiKind::Temporary);
    clock.advance(Duration::from_secs(5));
    keeper.get(clock.now()).await.unwrap();
    scheduler.set_keeper(keeper);
    pump(&mut scheduler).await;
    assert_eq!(
        calendars.find("person@example.com").unwrap().poll_count(),
        2
    );
}

#[tokio::test]
async fn startup_from_a_release_directory_keeps_accounts_preferences_and_fired_reminders() {
    let clock = clock();
    let dir = scratch("upgrade");
    let prefs = br#"{"storage_path":"/tmp/notor/preferences.json","notify_only_meetings":false,"accounts_preferences":{"person@example.com":{"hidden_calendars":["holidays"]}},"theme":"dusk"}"#;
    std::fs::write(dir.join("preferences.json"), prefs).unwrap();
    let accounts = serde_json::json!([
        {"token": {
            "access_token": "live",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "refresh-live",
            "scope": "calendar",
            "expires_at": clock.now().timestamp() + 3600,
            "user": {"id": "1", "email": "person@example.com", "verified_email": true, "name": "Person", "given_name": "Per", "family_name": "Son", "picture": "", "locale": "en"}
        }},
        {"token": {
            "access_token": "expired",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "refresh-old",
            "scope": "calendar",
            "expires_at": 10,
            "disabled": true,
            "user": {"id": "2", "email": "other@example.com", "verified_email": true, "name": "Other", "given_name": "O", "family_name": "Ther", "picture": "", "locale": "en"}
        }}
    ]);
    std::fs::write(
        dir.join("notor_accounts.json"),
        serde_json::to_vec(&accounts).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("googleauthtoken.json"),
        br#"{"access_token":"ghost","refresh_token":"g","user":{"email":"ghost@example.com"}}"#,
    )
    .unwrap();

    let start = clock.now() + ChronoDuration::minutes(9) + ChronoDuration::seconds(30);
    let candidate = AlertCandidate {
        anchor: start,
        popup_minutes: vec![10, 0],
        popup_overrides: vec![10, 0],
        event_id: "standup".into(),
        ..blank_candidate()
    };
    let mut fired = FiredState::default();
    let shown = due_reminders(
        &[candidate.clone()],
        clock.now(),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(
        shown
            .iter()
            .map(|item| item.offset_minutes)
            .collect::<Vec<_>>(),
        vec![10]
    );
    save_fired_state(&dir.join("fired_alerts.json"), &fired).unwrap();

    let calendars = Calendars::with_clock(Arc::clone(&clock) as Arc<dyn WallClock>);
    let preferences = Preferences::load_from_file(dir.clone()).await.unwrap();
    assert_eq!(calendars.open_directory(&dir, &preferences), 2);
    let emails: Vec<_> = calendars
        .get_tokens()
        .iter()
        .filter_map(|token| token.user.as_ref().map(|user| user.email.clone()))
        .collect();
    assert_eq!(
        emails,
        vec![
            "person@example.com".to_string(),
            "other@example.com".to_string()
        ]
    );
    assert_eq!(
        calendars
            .find("other@example.com")
            .unwrap()
            .token_snapshot()
            .disabled,
        Some(true)
    );
    let api = Arc::new(ScriptedApi::new());
    let api_dyn: Arc<dyn CalendarApi> = Arc::clone(&api) as Arc<dyn CalendarApi>;
    calendars.refresh_expired_tokens(&api_dyn).await;
    let refreshed = calendars
        .find("other@example.com")
        .unwrap()
        .token_snapshot();
    assert_eq!(refreshed.access_token, "access-from-refresh-old");
    assert_eq!(refreshed.disabled, Some(true));
    assert_eq!(
        calendars
            .find("person@example.com")
            .unwrap()
            .token_snapshot()
            .access_token,
        "live"
    );
    calendars.save().unwrap();
    assert_eq!(std::fs::read(dir.join("preferences.json")).unwrap(), prefs);

    let loaded = load_fired_startup(&dir.join("fired_alerts.json"));
    assert!(loaded.show_due);
    let mut state = loaded.state;
    let repeated = due_reminders(
        &[candidate.clone()],
        clock.now(),
        DEFAULT_ALERT_GRACE,
        false,
        &mut state,
    );
    assert!(repeated.is_empty());
    let started = due_reminders(&[candidate], start, DEFAULT_ALERT_GRACE, false, &mut state);
    assert_eq!(
        started
            .iter()
            .map(|item| item.offset_minutes)
            .collect::<Vec<_>>(),
        vec![0]
    );
    let once = due_reminders(
        &[started_candidate(start)],
        start,
        DEFAULT_ALERT_GRACE,
        false,
        &mut state,
    );
    assert!(once.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

fn blank_candidate() -> AlertCandidate {
    AlertCandidate {
        source_index: 0,
        event_id: String::new(),
        i_cal_uid: String::new(),
        status: "confirmed".into(),
        event_type: "default".into(),
        anchor: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        all_day_date: None,
        original_start_unix: None,
        sequence: 1,
        use_default: false,
        popup_overrides: Vec::new(),
        popup_minutes: Vec::new(),
        self_declined: false,
        attendee_count: 2,
        has_conference_link: true,
    }
}

fn started_candidate(start: DateTime<Utc>) -> AlertCandidate {
    AlertCandidate {
        anchor: start,
        popup_minutes: vec![10, 0],
        popup_overrides: vec![10, 0],
        event_id: "standup".into(),
        ..blank_candidate()
    }
}

#[tokio::test]
async fn a_legacy_token_without_an_email_is_identified() {
    let clock = clock();
    let dir = scratch("no-email");
    std::fs::write(
        dir.join("google_auth.json"),
        br#"{"access_token":"anon","token_type":"Bearer","expires_in":3600,"refresh_token":"r","scope":"calendar"}"#,
    )
    .unwrap();
    let calendars = Arc::new(Calendars::with_clock(
        Arc::clone(&clock) as Arc<dyn WallClock>
    ));
    assert_eq!(calendars.open_directory(&dir, &Preferences::default()), 1);
    assert!(calendars.accounts()[0].email().is_empty());
    let api = Arc::new(ScriptedApi::new());
    api.set_user(Ok(user("found@example.com")));
    spawn_identify(
        Arc::clone(&calendars.accounts()[0]),
        Arc::clone(&api) as Arc<dyn CalendarApi>,
        Arc::clone(&clock) as Arc<dyn WallClock>,
    );
    for _ in 0..40 {
        if calendars.accounts()[0].email() == "found@example.com" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(calendars.accounts()[0].email(), "found@example.com");
    calendars.save().unwrap();
    let raw = std::fs::read_to_string(dir.join("notor_accounts.json")).unwrap();
    assert!(raw.contains("found@example.com"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn google_errors_come_back_through_the_real_client() {
    let server = FakeGoogle::start();
    let clock = Arc::new(ManualClock::fixed_new_york());
    let mut client = GoogleClient::with_urls(
        format!("{}/credentials", server.base),
        format!("{}/token", server.base),
        format!("{}/calendar/v3", server.base),
        format!("{}/userinfo", server.base),
    );
    client.with_clock(Arc::clone(&clock) as Arc<dyn WallClock>);
    server.set(FakeReply {
        creds: 500,
        token: 200,
        token_body: r#"{"access_token":"x","expires_in":3600}"#.into(),
        calendar: 200,
        calendar_body: r#"{"items":[]}"#.into(),
        events: 200,
        events_body: r#"{"items":[]}"#.into(),
        retry_after: None,
    });
    let refreshed = client.refresh_token("refresh").await.unwrap();
    assert_eq!(refreshed.access_token, "x");
    assert_eq!(server.cred_hits(), 0);
    assert_eq!(server.token_hits(), 1);
    let raw = server.last_refresh();
    assert!(raw
        .to_ascii_lowercase()
        .contains("x-notor-token-refresh: server"));
    assert!(raw.contains("refresh_token"));
    assert!(!raw.contains("client_secret"));
    assert!(!raw.contains("client_id"));

    server.set(FakeReply {
        creds: 200,
        token: 200,
        token_body: r#"{"access_token":"fresh","expires_in":3600,"refresh_token":"rotated"}"#
            .into(),
        ..server.get()
    });
    let refreshed = client.refresh_token("refresh").await.unwrap();
    assert_eq!(refreshed.access_token, "fresh");
    assert_eq!(refreshed.refresh_token.as_deref(), Some("rotated"));
    assert_eq!(server.token_hits(), 2);
    assert_eq!(server.cred_hits(), 0);

    server.set(FakeReply {
        token: 400,
        token_body: r#"{"error":"invalid_grant"}"#.into(),
        ..server.get()
    });
    assert_eq!(
        client.refresh_token("refresh").await.unwrap_err().kind,
        ApiKind::SignIn
    );

    let hits = server.cred_hits();
    server.set(FakeReply {
        token: 401,
        token_body: r#"{"error":"invalid_client"}"#.into(),
        ..server.get()
    });
    assert_eq!(
        client.refresh_token("refresh").await.unwrap_err().kind,
        ApiKind::InvalidClient
    );
    server.set(FakeReply {
        creds: 200,
        token: 200,
        token_body: r#"{"access_token":"after","expires_in":1}"#.into(),
        ..server.get()
    });
    client.refresh_token("refresh").await.unwrap();
    assert_eq!(server.cred_hits(), hits);
    let _ = client.user_info("access").await;
    let creds = server.last_credentials();
    assert!(creds
        .to_ascii_lowercase()
        .contains("x-notor-token-refresh: server"));

    for (status, body, kind) in [
        (
            429,
            r#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#,
            ApiKind::RateLimit,
        ),
        (
            403,
            r#"{"error":{"errors":[{"reason":"userRateLimitExceeded"}]}}"#,
            ApiKind::RateLimit,
        ),
        (
            403,
            r#"{"error":{"code":403,"status":"RESOURCE_EXHAUSTED","message":"q"}}"#,
            ApiKind::RateLimit,
        ),
        (
            403,
            r#"{"error":{"errors":[{"reason":"forbidden"}]}}"#,
            ApiKind::SkipCalendar,
        ),
        (
            403,
            r#"{"error":{"errors":[{"reason":"requiredAccessLevel"}]}}"#,
            ApiKind::SkipCalendar,
        ),
        (
            403,
            r#"{"error":{"errors":[{"reason":"insufficientPermissions"}]}}"#,
            ApiKind::InsufficientPermissions,
        ),
        (
            403,
            r#"{"error":{"errors":[{"reason":"accessNotConfigured"}]}}"#,
            ApiKind::Temporary,
        ),
    ] {
        server.set(FakeReply {
            calendar: status,
            calendar_body: body.into(),
            retry_after: (status == 429).then_some(7),
            ..server.get()
        });
        let err = client.list_calendars("access").await.unwrap_err();
        assert_eq!(err.kind, kind, "{body}");
        if status == 429 {
            assert_eq!(err.retry_after, Some(Duration::from_secs(7)));
        }
    }
}

#[derive(Clone)]
struct FakeReply {
    creds: u16,
    token: u16,
    token_body: String,
    calendar: u16,
    calendar_body: String,
    events: u16,
    events_body: String,
    retry_after: Option<u64>,
}

struct FakeGoogle {
    base: String,
    state: Arc<std::sync::Mutex<FakeState>>,
}

struct FakeState {
    reply: FakeReply,
    cred_hits: u64,
    token_hits: u64,
    last_refresh: String,
    last_credentials: String,
}

impl FakeGoogle {
    fn start() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(std::sync::Mutex::new(FakeState {
            reply: FakeReply {
                creds: 200,
                token: 200,
                token_body: r#"{"access_token":"fresh","expires_in":3600}"#.into(),
                calendar: 200,
                calendar_body: r#"{"items":[{"id":"primary","accessRole":"owner"}]}"#.into(),
                events: 200,
                events_body: r#"{"items":[]}"#.into(),
                retry_after: None,
            },
            cred_hits: 0,
            token_hits: 0,
            last_refresh: String::new(),
            last_credentials: String::new(),
        }));
        let shared = Arc::clone(&state);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
                let mut buf = Vec::new();
                let mut tmp = [0u8; 2048];
                loop {
                    match std::io::Read::read(&mut stream, &mut tmp) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&tmp[..n]);
                            if buf.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
                if buf.is_empty() {
                    continue;
                }
                let text = String::from_utf8_lossy(&buf);
                let path = text.split_whitespace().nth(1).unwrap_or("/");
                let reply = {
                    let mut state = shared.lock().unwrap();
                    if path.contains("/credentials") {
                        state.cred_hits += 1;
                        state.last_credentials = text.to_string();
                        (state.reply.creds, creds_body(), None)
                    } else if path.contains("/oauth2/google/refresh") || path.contains("/token") {
                        state.token_hits += 1;
                        state.last_refresh = text.to_string();
                        (
                            state.reply.token,
                            state.reply.token_body.clone(),
                            state.reply.retry_after,
                        )
                    } else if path.contains("/events") {
                        (
                            state.reply.events,
                            state.reply.events_body.clone(),
                            state.reply.retry_after,
                        )
                    } else {
                        (
                            state.reply.calendar,
                            state.reply.calendar_body.clone(),
                            state.reply.retry_after,
                        )
                    }
                };
                let (status, body, retry) = reply;
                let retry_line = retry
                    .map(|secs| format!("Retry-After: {secs}\r\n"))
                    .unwrap_or_default();
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{retry_line}Connection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
            }
        });
        Self {
            base: format!("http://127.0.0.1:{port}"),
            state,
        }
    }

    fn set(&self, reply: FakeReply) {
        self.state.lock().unwrap().reply = reply;
    }

    fn get(&self) -> FakeReply {
        self.state.lock().unwrap().reply.clone()
    }

    fn token_hits(&self) -> u64 {
        self.state.lock().unwrap().token_hits
    }

    fn cred_hits(&self) -> u64 {
        self.state.lock().unwrap().cred_hits
    }

    fn last_refresh(&self) -> String {
        self.state.lock().unwrap().last_refresh.clone()
    }

    fn last_credentials(&self) -> String {
        self.state.lock().unwrap().last_credentials.clone()
    }
}

fn creds_body() -> String {
    r#"{"google_client_id":"id","google_calendar_api_key":"k","google_redirect_url":"http://localhost"}"#
        .to_string()
}

#[test]
fn fired_startup_hides_due_reminders_without_a_complete_record() {
    let dir = scratch("fired");
    let missing = load_fired_startup(&dir.join("fired_alerts.json"));
    assert!(!missing.show_due);
    std::fs::write(dir.join("fired_alerts.json"), b"{not json").unwrap();
    assert!(!load_fired_startup(&dir.join("fired_alerts.json")).show_due);
    std::fs::write(
        dir.join("fired_alerts.json"),
        br#"{"reminders":[{"meeting_key":"id:a@1","offset_minutes":10,"start_unix":1},"nope"],"copies":[]}"#,
    )
    .unwrap();
    let partial = load_fired_startup(&dir.join("fired_alerts.json"));
    assert!(partial.show_due);
    assert_eq!(partial.state.reminders.len(), 1);
    std::fs::write(
        dir.join("fired_alerts.json"),
        br#"{"reminders":[],"copies":[]}"#,
    )
    .unwrap();
    assert!(load_fired_startup(&dir.join("fired_alerts.json")).show_due);
    let _ = std::fs::remove_dir_all(&dir);
}
