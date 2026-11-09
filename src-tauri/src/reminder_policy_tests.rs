//! Behavior of the pure reminder policy. These run under `cargo test --lib`.

use crate::event_grouping::all_day_start;
use crate::reminder_policy::{
    due_reminders, load_fired_state, popup_offsets, save_fired_state, AlertCandidate, DueReminder,
    FiredState,
};
use crate::DEFAULT_ALERT_GRACE;
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::America::Edmonton;

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
        all_day_date: None,
        original_start_unix: None,
        sequence: 1,
        use_default: false,
        popup_overrides: vec![0, 10],
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
    let mut fired = FiredState::default();

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
    let mut fired = FiredState::default();
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
    let mut fired = FiredState::default();

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
    assert!(fired.reminders.is_empty());
}

#[test]
fn cancelled_and_skipped_event_types_never_alert() {
    let anchor = at(15, 0);
    let mut fired = FiredState::default();
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
    work.event_id = "b-work".into();
    work.i_cal_uid = "standup@google.com".into();
    let mut personal = standup(anchor);
    personal.event_id = "a-personal".into();
    personal.i_cal_uid = "standup@google.com".into();

    let mut fired = FiredState::default();
    let due = due_reminders(
        &[work.clone(), personal.clone()],
        anchor - Duration::minutes(10),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].event_id, "a-personal");
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

    let mut fired = FiredState::default();
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
    let mut fired = FiredState::default();
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
    let mut fired = FiredState::default();

    let ten_before = due_reminders(
        &[event.clone()],
        anchor - Duration::minutes(10),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&ten_before), vec![10]);

    // UTC midnight is not the all-day anchor, so nothing is due there.
    fired.reminders.clear();
    let utc_midnight = due_reminders(
        &[event.clone()],
        at(0, 0) + Duration::seconds(30),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert!(utc_midnight.is_empty());

    // Wake at local midnight: the 10-minute reminder is late, the start alert fires.
    fired.reminders.clear();
    let at_midnight = due_reminders(
        &[event.clone()],
        anchor + Duration::seconds(30),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&at_midnight), vec![0]);

    fired.reminders.clear();
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
    let mut fired = FiredState::default();

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

#[test]
fn event_with_no_reminders_still_alerts_at_start_once() {
    let anchor = at(15, 0);
    let mut event = standup(anchor);
    event.popup_minutes.clear();
    event.popup_overrides.clear();
    let mut fired = FiredState::default();
    let due = due_reminders(
        &[event.clone()],
        anchor,
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&due), vec![0]);
    let again = due_reminders(
        &[event],
        anchor + Duration::seconds(30),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert!(again.is_empty());
}

#[test]
fn sequence_change_rearms_and_reordering_overrides_does_not() {
    let anchor = at(15, 0);
    let mut event = standup(anchor);
    let mut fired = FiredState::default();
    let now = anchor - Duration::minutes(10);
    let first = due_reminders(
        &[event.clone()],
        now,
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&first), vec![10]);

    event.popup_overrides = vec![10, 0];
    let reordered = due_reminders(
        &[event.clone()],
        now + Duration::seconds(20),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert!(reordered.is_empty());

    event.sequence = 2;
    let rearmed = due_reminders(
        &[event],
        now + Duration::seconds(40),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&rearmed), vec![10]);
}

#[test]
fn calendar_default_change_keeps_fired_offset_and_adds_the_new_one() {
    let anchor = at(15, 0);
    let mut event = standup(anchor);
    event.use_default = true;
    event.popup_overrides.clear();
    event.popup_minutes = vec![10];
    let mut fired = FiredState::default();
    let now = anchor - Duration::minutes(10);
    let first = due_reminders(
        &[event.clone()],
        now,
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&first), vec![10]);

    event.popup_minutes = vec![10, 5];
    let still_fired = due_reminders(
        &[event.clone()],
        now + Duration::seconds(30),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert!(still_fired.is_empty());

    let added = due_reminders(
        &[event],
        anchor - Duration::minutes(5),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&added), vec![5]);
}

#[test]
fn copy_that_disappears_and_returns_does_not_rearm() {
    let anchor = at(15, 0);
    let mut event = standup(anchor);
    let mut fired = FiredState::default();
    let now = anchor - Duration::minutes(10);
    assert_eq!(
        offsets_of(&due_reminders(
            &[event.clone()],
            now,
            DEFAULT_ALERT_GRACE,
            false,
            &mut fired
        )),
        vec![10]
    );
    assert!(due_reminders(&[], now, DEFAULT_ALERT_GRACE, false, &mut fired).is_empty());
    event.sequence = 9;
    assert!(due_reminders(&[event], now, DEFAULT_ALERT_GRACE, false, &mut fired).is_empty());
}

#[test]
fn union_of_popup_offsets_across_accounts() {
    let anchor = at(15, 0);
    let mut first = standup(anchor);
    first.event_id = "b-copy".into();
    first.i_cal_uid = "shared@google.com".into();
    first.popup_minutes = vec![10];
    first.popup_overrides = vec![10];
    let mut second = standup(anchor);
    second.event_id = "a-copy".into();
    second.i_cal_uid = "shared@google.com".into();
    second.popup_minutes = vec![5];
    second.popup_overrides = vec![5];
    let mut fired = FiredState::default();

    let ten = due_reminders(
        &[first.clone(), second.clone()],
        anchor - Duration::minutes(10),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&ten), vec![10]);
    assert_eq!(ten[0].event_id, "a-copy");

    let five = due_reminders(
        &[first.clone(), second.clone()],
        anchor - Duration::minutes(5),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&five), vec![5]);

    let start = due_reminders(
        &[first, second],
        anchor,
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(offsets_of(&start), vec![0]);
}

#[test]
fn recurring_occurrences_stay_separate() {
    let first_start = at(15, 0);
    let second_start = at(15, 0) + Duration::days(1);
    let mut today = standup(first_start);
    today.i_cal_uid = "series@google.com".into();
    today.original_start_unix = Some(first_start.timestamp());
    let mut tomorrow = standup(second_start);
    tomorrow.event_id = "evt-2".into();
    tomorrow.i_cal_uid = "series@google.com".into();
    tomorrow.original_start_unix = Some(second_start.timestamp());
    let mut fired = FiredState::default();

    let today_due = due_reminders(
        &[today.clone(), tomorrow.clone()],
        first_start - Duration::minutes(10),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(today_due.len(), 1);
    assert_eq!(today_due[0].event_id, "evt-1");

    let tomorrow_due = due_reminders(
        &[today, tomorrow],
        second_start - Duration::minutes(10),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    assert_eq!(tomorrow_due.len(), 1);
    assert_eq!(tomorrow_due[0].event_id, "evt-2");
}

#[test]
fn both_due_reminders_are_returned_in_one_tick() {
    let anchor = at(15, 0);
    let event = standup(anchor);
    let mut fired = FiredState::default();
    let due = due_reminders(&[event], anchor, Duration::minutes(15), false, &mut fired);
    assert_eq!(offsets_of(&due), vec![10, 0]);
}

#[test]
fn offsets_outside_google_range_are_dropped() {
    let minutes = popup_offsets(
        false,
        &[("popup", -1), ("popup", 40_321), ("popup", 30)],
        &[],
    );
    assert_eq!(minutes, vec![30]);
}

#[test]
fn fired_state_round_trip_and_corrupt_file() {
    let anchor = at(15, 0);
    let event = standup(anchor);
    let mut fired = FiredState::default();
    due_reminders(
        &[event],
        anchor - Duration::minutes(10),
        DEFAULT_ALERT_GRACE,
        false,
        &mut fired,
    );
    let path = std::env::temp_dir().join(format!("notor-fired-{}-round.json", std::process::id()));
    save_fired_state(&path, &fired).unwrap();
    let loaded = load_fired_state(&path);
    assert_eq!(loaded.reminders.len(), 1);
    assert_eq!(loaded.copies.len(), 1);
    let _ = std::fs::remove_file(&path);

    let corrupt = std::env::temp_dir().join(format!("notor-fired-{}-bad.json", std::process::id()));
    std::fs::write(&corrupt, "{not json").unwrap();
    let empty = load_fired_state(&corrupt);
    assert!(empty.reminders.is_empty());
    assert!(empty.copies.is_empty());
    let missing = load_fired_state(&std::env::temp_dir().join("notor-fired-does-not-exist.json"));
    assert!(missing.reminders.is_empty());
    let _ = std::fs::remove_file(&corrupt);
}
