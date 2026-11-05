//! Behavior of the pure reminder policy. These run under `cargo test --lib`.

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::America::Edmonton;
use notor_core::{
    all_day_start, due_reminders, popup_offsets, AlertCandidate, DueReminder, DEFAULT_ALERT_GRACE,
};

fn at(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 4, 27, hour, minute, 0).unwrap()
}

fn standup(anchor: DateTime<Utc>) -> AlertCandidate {
    AlertCandidate {
        source_index: 0,
        event_id: "evt-1".into(),
        i_cal_uid: String::new(),
        status: "confirmed".into(),
        event_type: "default".into(),
        anchor,
        popup_minutes: vec![10, 0],
        self_declined: false,
        attendee_count: 3,
        has_conference_link: true,
    }
}

fn offsets_of(due: &[DueReminder]) -> Vec<i64> {
    due.iter().map(|reminder| reminder.offset_minutes).collect()
}

#[test]
fn popup_overrides_used_when_use_default_is_false() {
    let minutes = popup_offsets(
        false,
        &[("popup", 0), ("email", 30), ("popup", 10), ("popup", 10)],
        &[("popup", 25)],
    );
    assert_eq!(minutes, vec![0, 10]);
}

#[test]
fn calendar_defaults_used_when_use_default_is_true() {
    let minutes = popup_offsets(true, &[("popup", 5)], &[("popup", 10), ("email", 60)]);
    assert_eq!(minutes, vec![10]);
}

#[test]
fn empty_overrides_suppress_calendar_defaults() {
    let minutes = popup_offsets(false, &[], &[("popup", 10)]);
    assert!(minutes.is_empty());
}

#[test]
fn reminders_at_10_minutes_and_0_minutes_fire_once_each() {
    let anchor = at(15, 0);
    let event = standup(anchor);
    let mut fired = Vec::new();

    let ten_minutes_before = anchor - Duration::minutes(10);
    let first = due_reminders(
        &[event.clone()],
        ten_minutes_before,
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&first), vec![10]);

    let again = due_reminders(
        &[event.clone()],
        ten_minutes_before + Duration::seconds(30),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert!(again.is_empty());

    let at_start = due_reminders(
        &[event.clone()],
        anchor,
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&at_start), vec![0]);

    let start_again = due_reminders(
        &[event],
        anchor + Duration::seconds(30),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert!(start_again.is_empty());
}

#[test]
fn moved_event_rearms_reminders() {
    let anchor = at(15, 0);
    let mut event = standup(anchor);
    let mut fired = Vec::new();
    let first = due_reminders(
        &[event.clone()],
        anchor - Duration::minutes(10),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&first), vec![10]);

    let moved = anchor + Duration::hours(1);
    event.anchor = moved;
    let rearmed = due_reminders(
        &[event],
        moved - Duration::minutes(10),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&rearmed), vec![10]);
    assert_eq!(rearmed[0].fire_at, moved - Duration::minutes(10));
}

#[test]
fn declined_event_never_alerts() {
    let anchor = at(15, 0);
    let mut event = standup(anchor);
    event.self_declined = true;
    let mut fired = Vec::new();

    for now in [anchor - Duration::minutes(10), anchor] {
        let due = due_reminders(
            &[event.clone()],
            now,
            DEFAULT_ALERT_GRACE,
            false,
            &mut fired,
        );
        assert!(due.is_empty(), "declined event alerted at {now}");
    }
    assert!(fired.is_empty());
}

#[test]
fn cancelled_and_skipped_event_types_never_alert() {
    let anchor = at(15, 0);
    let mut fired = Vec::new();
    for (status, event_type) in [
        ("cancelled", "default"),
        ("confirmed", "outOfOffice"),
        ("confirmed", "focusTime"),
        ("confirmed", "workingLocation"),
        ("confirmed", "birthday"),
    ] {
        let mut event = standup(anchor);
        event.status = status.into();
        event.event_type = event_type.into();
        let due = due_reminders(
            &[event],
            anchor - Duration::minutes(10),
            DEFAULT_ALERT_GRACE,
            false,
            &mut fired,
        );
        assert!(due.is_empty(), "{status}/{event_type} alerted");
    }
}

#[test]
fn same_ical_uid_on_two_accounts_alerts_once() {
    let anchor = at(15, 0);
    let mut work = standup(anchor);
    work.event_id = "work-copy".into();
    work.i_cal_uid = "standup@google.com".into();
    let mut personal = standup(anchor);
    personal.event_id = "personal-copy".into();
    personal.i_cal_uid = "standup@google.com".into();

    let mut fired = Vec::new();
    let due = due_reminders(
        &[work.clone(), personal.clone()],
        anchor - Duration::minutes(10),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].event_id, "work-copy");
    assert_eq!(due[0].offset_minutes, 10);

    let again = due_reminders(
        &[work, personal],
        anchor - Duration::minutes(10) + Duration::seconds(20),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert!(again.is_empty());
}

#[test]
fn accepted_copy_still_alerts_when_the_other_copy_is_declined_or_missing() {
    let anchor = at(15, 0);
    let mut declined = standup(anchor);
    declined.event_id = "declined-copy".into();
    declined.i_cal_uid = "standup@google.com".into();
    declined.self_declined = true;
    let mut accepted = standup(anchor);
    accepted.event_id = "accepted-copy".into();
    accepted.i_cal_uid = "standup@google.com".into();

    let mut fired = Vec::new();
    let due = due_reminders(
        &[declined, accepted.clone()],
        anchor,
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].event_id, "accepted-copy");

    // The other account failed to poll: only one copy is in the snapshot.
    let mut only_remaining = accepted;
    only_remaining.event_id = "only-account".into();
    only_remaining.anchor = anchor + Duration::hours(2);
    let due = due_reminders(
        &[only_remaining],
        anchor + Duration::hours(2),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].offset_minutes, 0);
}

#[test]
fn missed_10_minute_reminder_after_sleep_does_not_fire_late_but_start_does() {
    let anchor = at(15, 0);
    let event = standup(anchor);
    let mut fired = Vec::new();
    // Woke 30s after the start, having slept through the 10-minute reminder.
    let woke = anchor + Duration::seconds(30);
    let due = due_reminders(&[event], woke, DEFAULT_ALERT_GRACE, false, &mut fired);
    assert_eq!(offsets_of(&due), vec![0]);
    assert_eq!(due[0].fire_at, anchor);
}

#[test]
fn all_day_reminder_is_measured_from_local_midnight_and_not_days_late() {
    // 2024-04-27 in Edmonton is MDT (UTC-6), so local midnight is 06:00 UTC.
    let date = NaiveDate::from_ymd_opt(2024, 4, 27).unwrap();
    let anchor = all_day_start(Edmonton, date).to_utc();
    assert_eq!(anchor, at(6, 0));

    let mut event = standup(anchor);
    event.popup_minutes = vec![10, 0];
    let mut fired = Vec::new();

    let ten_before = due_reminders(
        &[event.clone()],
        anchor - Duration::minutes(10),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&ten_before), vec![10]);

    // UTC midnight is not the all-day anchor, so nothing is due there.
    fired.clear();
    let utc_midnight = due_reminders(
        &[event.clone()],
        at(0, 0) + Duration::seconds(30),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert!(utc_midnight.is_empty());

    // Wake at local midnight: the 10-minute reminder is late, the start alert fires.
    fired.clear();
    let at_midnight = due_reminders(
        &[event.clone()],
        anchor + Duration::seconds(30),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&at_midnight), vec![0]);

    fired.clear();
    let days_later = due_reminders(
        &[event],
        anchor + Duration::days(2),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert!(days_later.is_empty());
}

#[test]
fn notify_only_meetings_drops_solo_event_without_a_link() {
    let anchor = at(15, 0);
    let mut solo = standup(anchor);
    solo.attendee_count = 1;
    solo.has_conference_link = false;
    let mut fired = Vec::new();

    let dropped = due_reminders(
        &[solo.clone()],
        anchor,
        DEFAULT_ALERT_GRACE,
        true,
        &mut fired,
    );
    assert!(dropped.is_empty());

    let mut with_people = solo.clone();
    with_people.attendee_count = 2;
    let kept = due_reminders(
        &[with_people],
        anchor,
        DEFAULT_ALERT_GRACE,
        true,
        &mut fired,
    );
    assert_eq!(offsets_of(&kept), vec![0]);

    let mut with_link = solo;
    with_link.event_id = "solo-with-link".into();
    with_link.has_conference_link = true;
    let linked = due_reminders(&[with_link], anchor, DEFAULT_ALERT_GRACE, true, &mut fired);
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].event_id, "solo-with-link");
}
