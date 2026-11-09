use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use crate::alert_decision::should_fire_alert;

/// Google Calendar rejects reminder offsets outside this range.
pub const MAX_REMINDER_MINUTES: i64 = 40_320;

/// One event copy passed to the alert policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlertCandidate {
    /// Index into the caller's snapshot, so the chosen copy can be shown.
    pub source_index: usize,
    pub event_id: String,
    /// Shared by every copy of a meeting. Recurring occurrences also share it
    /// and are separated by `original_start_unix` or `anchor`.
    pub i_cal_uid: String,
    pub status: String,
    pub event_type: String,
    /// Current start, or local midnight for an all-day event.
    pub anchor: DateTime<Utc>,
    /// Set for all-day events. Identity is this date, not the local midnight instant.
    pub all_day_date: Option<NaiveDate>,
    /// Occurrence identity. Falls back to `anchor` when Google omitted it.
    pub original_start_unix: Option<i64>,
    pub sequence: i64,
    pub use_default: bool,
    /// Popup overrides on this copy, used only to detect edits. Order does not matter.
    pub popup_overrides: Vec<i64>,
    /// Popup offsets already resolved for this copy (overrides or calendar defaults).
    pub popup_minutes: Vec<i64>,
    pub self_declined: bool,
    pub attendee_count: usize,
    pub has_conference_link: bool,
}

/// A reminder that has already been delivered for a meeting.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FiredReminder {
    pub meeting_key: String,
    pub offset_minutes: i64,
    pub start_unix: i64,
}

/// Last-seen fingerprint of one account's copy. Absent copies are dropped
/// without re-arming; a copy that is present again with a new fingerprint re-arms.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenCopy {
    pub event_id: String,
    pub start_unix: i64,
    pub sequence: i64,
    pub use_default: bool,
    pub popup_overrides: Vec<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FiredState {
    pub reminders: Vec<FiredReminder>,
    pub copies: Vec<SeenCopy>,
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
/// `use_default == false` uses `overrides` even when that list is empty.
/// Email reminders are ignored. Minutes outside 0..=40320 are dropped.
/// The result is sorted and de-duplicated. The start alert is added later.
pub fn popup_offsets(
    use_default: bool,
    overrides: &[(&str, i64)],
    defaults: &[(&str, i64)],
) -> Vec<i64> {
    let source = if use_default { defaults } else { overrides };
    let mut minutes: Vec<i64> = source
        .iter()
        .filter(|(method, minutes)| *method == "popup" && in_range(*minutes))
        .map(|(_, minutes)| *minutes)
        .collect();
    minutes.sort_unstable();
    minutes.dedup();
    minutes
}

/// Every reminder whose fire time is inside `[now - grace, now]`.
///
/// Copies of one meeting (same iCalUID and occurrence) alert once per offset.
/// The offset list is the union of each copy's popups plus the start alert.
/// Fire times use the copy with the smallest event id. A copy that stays in
/// the snapshot and changes start, sequence, useDefault, or its overrides
/// clears that meeting's fired reminders so they can alert again. RSVP and
/// title changes are not on this fingerprint, and reordering overrides is not
/// a change. Each due reminder is returned; the caller shows all of them.
pub fn due_reminders(
    candidates: &[AlertCandidate],
    now: DateTime<Utc>,
    grace: Duration,
    notify_only_meetings: bool,
    state: &mut FiredState,
) -> Vec<DueReminder> {
    state
        .reminders
        .retain(|entry| retain_fired(entry, now, grace));

    let alertable: Vec<&AlertCandidate> = candidates
        .iter()
        .filter(|candidate| is_alertable(candidate, notify_only_meetings))
        .collect();

    let mut rearm = HashSet::new();
    for candidate in &alertable {
        if let Some(previous) = state
            .copies
            .iter()
            .find(|copy| copy.event_id == candidate.event_id)
        {
            if fingerprint_changed(previous, candidate) {
                rearm.insert(meeting_key(candidate));
            }
        }
    }
    if !rearm.is_empty() {
        state
            .reminders
            .retain(|entry| !rearm.contains(&entry.meeting_key));
    }
    state.copies = seen_copies(&alertable);

    let mut groups: HashMap<String, Vec<&AlertCandidate>> = HashMap::new();
    for candidate in &alertable {
        groups
            .entry(meeting_key(candidate))
            .or_default()
            .push(*candidate);
    }

    let mut due = Vec::new();
    let mut meeting_keys: Vec<String> = groups.keys().cloned().collect();
    meeting_keys.sort();
    for key in meeting_keys {
        let copies = &groups[&key];
        let displayed = copies
            .iter()
            .min_by(|left, right| left.event_id.cmp(&right.event_id))
            .copied()
            .expect("meeting group is non-empty");
        for offset_minutes in union_offsets(copies) {
            let Some(fire_at) = displayed
                .anchor
                .checked_sub_signed(Duration::minutes(offset_minutes))
            else {
                continue;
            };
            if !should_fire_alert(fire_at, now, grace) {
                continue;
            }
            if state.reminders.iter().any(|entry| {
                entry.offset_minutes == offset_minutes && same_meeting(entry, displayed, &key)
            }) {
                continue;
            }
            state.reminders.push(FiredReminder {
                meeting_key: key.clone(),
                offset_minutes,
                start_unix: fired_start(displayed),
            });
            due.push(DueReminder {
                source_index: displayed.source_index,
                event_id: displayed.event_id.clone(),
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

/// Missing and unreadable files are an empty state, including corrupt JSON.
pub fn load_fired_state(path: &Path) -> FiredState {
    let Ok(text) = fs::read_to_string(path) else {
        return FiredState::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// What startup should do with reminders that are already due.
///
/// `show_due` is false when there is no complete record (the file is missing,
/// unreadable, or only a `.bak` exists). Those reminders are recorded as
/// fired so a restart does not repeat them. One bad entry does not discard
/// the rest. A valid empty file means nothing has fired.
pub struct FiredStartup {
    pub state: FiredState,
    pub show_due: bool,
}

pub fn load_fired_startup(path: &Path) -> FiredStartup {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => {
            return FiredStartup {
                state: FiredState::default(),
                show_due: false,
            };
        }
    };
    if let Some(startup) = parse_fired_document(&bytes) {
        return startup;
    }
    let mut state = FiredState::default();
    for object in crate::storage::complete_objects(&bytes) {
        if let Ok(reminder) = serde_json::from_value::<FiredReminder>(object.clone()) {
            if !reminder.meeting_key.is_empty() {
                state.reminders.push(reminder);
                continue;
            }
        }
        if let Ok(copy) = serde_json::from_value::<SeenCopy>(object) {
            if !copy.event_id.is_empty() {
                state.copies.push(copy);
            }
        }
    }
    let show_due = !state.reminders.is_empty() || !state.copies.is_empty();
    FiredStartup { state, show_due }
}

fn parse_fired_document(bytes: &[u8]) -> Option<FiredStartup> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let obj = value.as_object()?;
    let mut reminders = Vec::new();
    if let Some(items) = obj.get("reminders").and_then(|item| item.as_array()) {
        for item in items {
            if let Ok(reminder) = serde_json::from_value::<FiredReminder>(item.clone()) {
                reminders.push(reminder);
            }
        }
    }
    let mut copies = Vec::new();
    if let Some(items) = obj.get("copies").and_then(|item| item.as_array()) {
        for item in items {
            if let Ok(copy) = serde_json::from_value::<SeenCopy>(item.clone()) {
                copies.push(copy);
            }
        }
    }
    Some(FiredStartup {
        state: FiredState { reminders, copies },
        show_due: true,
    })
}

/// Write via a temp file in the same directory, then rename into place.
/// The previous good file is kept as `.bak`.
pub fn save_fired_state(path: &Path, state: &FiredState) -> io::Result<()> {
    let bytes =
        serde_json::to_vec(state).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
    crate::storage::atomic_write(path, &bytes)
}

fn in_range(minutes: i64) -> bool {
    (0..=MAX_REMINDER_MINUTES).contains(&minutes)
}

fn start_within_grace(start_unix: i64, now: DateTime<Utc>, grace: Duration) -> bool {
    let Some(start) = DateTime::from_timestamp(start_unix, 0) else {
        return false;
    };
    if start > now {
        return true;
    }
    should_fire_alert(start, now, grace)
}

fn is_alertable(candidate: &AlertCandidate, notify_only_meetings: bool) -> bool {
    if candidate.event_id.is_empty() && candidate.i_cal_uid.is_empty() {
        return false;
    }
    if candidate.status == "cancelled" || candidate.self_declined {
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

fn meeting_identity(candidate: &AlertCandidate) -> String {
    if candidate.i_cal_uid.is_empty() {
        format!("id:{}", candidate.event_id)
    } else {
        format!("ical:{}", candidate.i_cal_uid)
    }
}

fn meeting_key(candidate: &AlertCandidate) -> String {
    let identity = meeting_identity(candidate);
    if let Some(date) = candidate.all_day_date {
        format!("{identity}@date:{date}")
    } else {
        let occurrence = candidate
            .original_start_unix
            .unwrap_or_else(|| candidate.anchor.timestamp());
        format!("{identity}@{occurrence}")
    }
}

/// Midnight of `date` in UTC-12, the last zone on Earth to reach that midnight.
pub fn all_day_deadline(date: NaiveDate) -> DateTime<Utc> {
    date.and_hms_opt(12, 0, 0).expect("noon").and_utc()
}

/// `unix` is local midnight of `date` somewhere between UTC-12 and UTC+14.
pub fn is_midnight_of(unix: i64, date: NaiveDate) -> bool {
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .expect("midnight")
        .and_utc()
        .timestamp();
    let offset = midnight - unix;
    (-12 * 3600..=14 * 3600).contains(&offset) && offset % 900 == 0
}

fn fired_start(candidate: &AlertCandidate) -> i64 {
    match candidate.all_day_date {
        Some(date) => all_day_deadline(date).timestamp(),
        None => candidate.anchor.timestamp(),
    }
}

fn seen_start(candidate: &AlertCandidate) -> i64 {
    match candidate.all_day_date {
        Some(date) => day_index(date),
        None => candidate.anchor.timestamp(),
    }
}

/// Days since 0001-01-01. Far from a unix timestamp, so an all-day mark
/// cannot be mistaken for a timed event's start.
fn day_index(date: NaiveDate) -> i64 {
    let epoch = NaiveDate::from_ymd_opt(1, 1, 1).expect("epoch");
    date.signed_duration_since(epoch).num_days()
}

fn same_all_day_mark(stored: i64, date: NaiveDate) -> bool {
    stored == day_index(date) || is_midnight_of(stored, date)
}

fn split_key(key: &str) -> Option<(&str, &str)> {
    key.rsplit_once('@')
}

fn date_from_suffix(suffix: &str) -> Option<NaiveDate> {
    let date = suffix.strip_prefix("date:")?;
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

fn same_meeting(entry: &FiredReminder, candidate: &AlertCandidate, key: &str) -> bool {
    if entry.meeting_key == key {
        return true;
    }
    let Some(date) = candidate.all_day_date else {
        return false;
    };
    let Some((identity, suffix)) = split_key(&entry.meeting_key) else {
        return false;
    };
    if identity != meeting_identity(candidate) {
        return false;
    }
    if let Some(stored) = date_from_suffix(suffix) {
        return stored == date;
    }
    suffix
        .parse::<i64>()
        .ok()
        .is_some_and(|unix| is_midnight_of(unix, date))
}

fn retain_fired(entry: &FiredReminder, now: DateTime<Utc>, grace: Duration) -> bool {
    if let Some((_, suffix)) = split_key(&entry.meeting_key) {
        if let Some(date) = date_from_suffix(suffix) {
            return now <= all_day_deadline(date) + grace;
        }
        if let Ok(unix) = suffix.parse::<i64>() {
            if let Some(date) = date_for_midnight(unix) {
                return now <= all_day_deadline(date) + grace;
            }
        }
    }
    start_within_grace(entry.start_unix, now, grace)
}

fn date_for_midnight(unix: i64) -> Option<NaiveDate> {
    // Search the dates whose worldwide midnight window can contain `unix`.
    let guess = DateTime::from_timestamp(unix, 0)?.date_naive();
    for shift in -1..=1 {
        let Some(date) = guess.checked_add_signed(Duration::days(shift)) else {
            continue;
        };
        if is_midnight_of(unix, date) {
            return Some(date);
        }
    }
    None
}

fn normalized_overrides(minutes: &[i64]) -> Vec<i64> {
    let mut overrides: Vec<i64> = minutes.iter().copied().filter(|m| in_range(*m)).collect();
    overrides.sort_unstable();
    overrides.dedup();
    overrides
}

fn fingerprint_changed(previous: &SeenCopy, candidate: &AlertCandidate) -> bool {
    start_moved(previous, candidate)
        || previous.sequence != candidate.sequence
        || previous.use_default != candidate.use_default
        || previous.popup_overrides != normalized_overrides(&candidate.popup_overrides)
}

fn start_moved(previous: &SeenCopy, candidate: &AlertCandidate) -> bool {
    match candidate.all_day_date {
        Some(date) => !same_all_day_mark(previous.start_unix, date),
        None => previous.start_unix != candidate.anchor.timestamp(),
    }
}

fn seen_copies(alertable: &[&AlertCandidate]) -> Vec<SeenCopy> {
    let mut by_id: HashMap<&str, SeenCopy> = HashMap::new();
    for candidate in alertable {
        by_id.insert(
            candidate.event_id.as_str(),
            SeenCopy {
                event_id: candidate.event_id.clone(),
                start_unix: seen_start(candidate),
                sequence: candidate.sequence,
                use_default: candidate.use_default,
                popup_overrides: normalized_overrides(&candidate.popup_overrides),
            },
        );
    }
    let mut copies: Vec<SeenCopy> = by_id.into_values().collect();
    copies.sort_by(|left, right| left.event_id.cmp(&right.event_id));
    copies
}

fn union_offsets(copies: &[&AlertCandidate]) -> Vec<i64> {
    let mut offsets = Vec::new();
    for copy in copies {
        offsets.extend(copy.popup_minutes.iter().copied().filter(|m| in_range(*m)));
    }
    offsets.push(0);
    offsets.sort_unstable();
    offsets.dedup();
    offsets
}
