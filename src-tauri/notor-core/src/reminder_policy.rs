use chrono::{DateTime, Duration, Utc};

use crate::alert_decision::should_fire_alert;

/// One popup reminder on an event, already resolved to minutes before the anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlertCandidate {
    /// Index into the caller's snapshot, so the chosen copy can be shown
    /// even when another copy shares the event id.
    pub source_index: usize,
    pub event_id: String,
    /// Shared across the copies of one meeting on different accounts.
    /// Recurring occurrences also share this value and are distinguished by `anchor`.
    pub i_cal_uid: String,
    pub status: String,
    pub event_type: String,
    /// Timed start, or local midnight for an all-day event.
    pub anchor: DateTime<Utc>,
    pub popup_minutes: Vec<i64>,
    pub self_declined: bool,
    pub attendee_count: usize,
    pub has_conference_link: bool,
}

/// Identity of a reminder that has already fired inside the grace window.
///
/// The anchor is part of the key, so a moved event (new start) is a new reminder
/// and fires again. Copies that share an iCalUID and anchor collapse to one key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FiredReminder {
    pub dedupe_key: String,
    pub offset_minutes: i64,
    pub fire_at_unix: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DueReminder {
    pub source_index: usize,
    pub event_id: String,
    pub offset_minutes: i64,
    pub fire_at: DateTime<Utc>,
}

/// Popup minutes from event overrides, or from the calendar defaults.
///
/// `use_default == false` uses `overrides` even when that list is empty, which is
/// how Google expresses "no reminders". Email reminders are ignored. Minutes are
/// returned sorted and de-duplicated.
pub fn popup_offsets(
    use_default: bool,
    overrides: &[(&str, i64)],
    defaults: &[(&str, i64)],
) -> Vec<i64> {
    let source = if use_default { defaults } else { overrides };
    let mut minutes: Vec<i64> = source
        .iter()
        .filter(|(method, minutes)| *method == "popup" && *minutes >= 0)
        .map(|(_, minutes)| *minutes)
        .collect();
    minutes.sort_unstable();
    minutes.dedup();
    minutes
}

/// Reminders whose fire time is inside `[now - grace, now]`.
///
/// Each `(occurrence, offset)` is recorded in `fired` so a later tick inside the
/// same grace window does not alert again. The grace window is applied to the
/// reminder instant, not only to the event start: a 10-minute reminder that is
/// older than `grace` stays quiet, while a start reminder (0 minutes) still
/// fires when the machine wakes inside the grace window of the start.
///
/// Declined, cancelled, and non-meeting event types are skipped before dedupe,
/// so a declined copy does not hide an accepted copy on another account, and a
/// missing account (its copy simply absent) does not hide the remaining one.
pub fn due_reminders(
    candidates: &[AlertCandidate],
    now: DateTime<Utc>,
    grace: Duration,
    notify_only_meetings: bool,
    fired: &mut Vec<FiredReminder>,
) -> Vec<DueReminder> {
    fired.retain(
        |entry| match DateTime::from_timestamp(entry.fire_at_unix, 0) {
            Some(fire_at) => should_fire_alert(fire_at, now, grace),
            None => false,
        },
    );

    let mut due = Vec::new();
    for candidate in candidates {
        if !is_alertable(candidate, notify_only_meetings) {
            continue;
        }
        let mut offsets = candidate.popup_minutes.clone();
        offsets.sort_unstable();
        offsets.dedup();
        for offset_minutes in offsets {
            let Some(fire_at) = candidate
                .anchor
                .checked_sub_signed(Duration::minutes(offset_minutes))
            else {
                continue;
            };
            if !should_fire_alert(fire_at, now, grace) {
                continue;
            }
            let entry = FiredReminder {
                dedupe_key: occurrence_key(candidate),
                offset_minutes,
                fire_at_unix: fire_at.timestamp(),
            };
            if fired.iter().any(|existing| existing == &entry) {
                continue;
            }
            fired.push(entry);
            due.push(DueReminder {
                source_index: candidate.source_index,
                event_id: candidate.event_id.clone(),
                offset_minutes,
                fire_at,
            });
        }
    }

    due.sort_by(|left, right| {
        left.fire_at
            .cmp(&right.fire_at)
            .then(left.offset_minutes.cmp(&right.offset_minutes))
            .then(left.event_id.cmp(&right.event_id))
    });
    due
}

fn is_alertable(candidate: &AlertCandidate, notify_only_meetings: bool) -> bool {
    if candidate.event_id.is_empty() && candidate.i_cal_uid.is_empty() {
        return false;
    }
    if candidate.status == "cancelled" {
        return false;
    }
    if candidate.self_declined {
        return false;
    }
    if matches!(
        candidate.event_type.as_str(),
        "outOfOffice" | "focusTime" | "workingLocation" | "birthday"
    ) {
        return false;
    }
    if notify_only_meetings && !is_meeting(candidate) {
        return false;
    }
    true
}

fn is_meeting(candidate: &AlertCandidate) -> bool {
    candidate.has_conference_link || candidate.attendee_count >= 2
}

fn occurrence_key(candidate: &AlertCandidate) -> String {
    let identity = if candidate.i_cal_uid.is_empty() {
        format!("id:{}", candidate.event_id)
    } else {
        format!("ical:{}", candidate.i_cal_uid)
    };
    format!("{identity}@{}", candidate.anchor.timestamp())
}
