use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use google_calendar::types::{Event, EventAttendee, EventDateTime, EventReminder, Reminders};
use notor_core::{all_day_start, popup_offsets, AlertCandidate};

use crate::account::AlertSource;

/// Map a polled Google event onto the pure alert candidate.
///
/// All-day events (a date and no dateTime) anchor at local midnight in the
/// event's time zone, falling back to `fallback_tz`. Popup offsets come from
/// the event overrides when `useDefault` is false, otherwise from the calendar's
/// default reminders carried on the poll snapshot.
pub fn candidate_from_source(source: &AlertSource, fallback_tz: Tz) -> Option<AlertCandidate> {
    let anchor = anchor_utc(&source.event, fallback_tz)?;
    let (use_default, overrides) = match &source.event.reminders {
        Some(reminders) => (reminders.use_default, reminders.overrides.as_slice()),
        None => (true, &[][..]),
    };
    let override_pairs: Vec<(&str, i64)> = overrides
        .iter()
        .map(|reminder| (reminder.method.as_str(), reminder.minutes))
        .collect();
    let default_pairs: Vec<(&str, i64)> = source
        .default_reminders
        .iter()
        .map(|reminder| (reminder.method.as_str(), reminder.minutes))
        .collect();

    Some(AlertCandidate {
        source_index: 0,
        event_id: source.event.id.clone(),
        i_cal_uid: source.event.i_cal_uid.clone(),
        status: source.event.status.clone(),
        event_type: source.event.event_type.clone(),
        anchor,
        popup_minutes: popup_offsets(use_default, &override_pairs, &default_pairs),
        self_declined: self_declined(&source.event, &source.account_email),
        attendee_count: source.event.attendees.len(),
        has_conference_link: has_conference_link(&source.event),
    })
}

fn anchor_utc(event: &Event, fallback_tz: Tz) -> Option<DateTime<Utc>> {
    let start = event.start.as_ref()?;
    if let Some(date_time) = start.date_time {
        return Some(date_time);
    }
    let date = start.date?;
    let tz = if start.time_zone.is_empty() {
        fallback_tz
    } else {
        start.time_zone.parse().unwrap_or(fallback_tz)
    };
    Some(all_day_start(tz, date).to_utc())
}

fn self_declined(event: &Event, account_email: &str) -> bool {
    event.attendees.iter().any(|attendee| {
        let is_user = attendee.self_
            || (!account_email.is_empty() && attendee.email.eq_ignore_ascii_case(account_email));
        is_user && attendee.response_status == "declined"
    })
}

/// A meeting link is a Hangout/Meet link, a conference entry-point URI, or a
/// URL in the location field (the standup fixture stores Zoom there).
fn has_conference_link(event: &Event) -> bool {
    if !event.hangout_link.is_empty() || event.location.contains("://") {
        return true;
    }
    event
        .conference_data
        .as_ref()
        .map(|data| data.entry_points.iter().any(|point| !point.uri.is_empty()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono_tz::America::Edmonton;

    fn reminder(method: &str, minutes: i64) -> EventReminder {
        EventReminder {
            method: method.into(),
            minutes,
        }
    }

    fn attendee(email: &str, status: &str, self_: bool) -> EventAttendee {
        EventAttendee {
            additional_guests: 0,
            comment: String::new(),
            display_name: String::new(),
            email: email.into(),
            id: String::new(),
            optional: false,
            organizer: false,
            resource: false,
            response_status: status.into(),
            self_,
        }
    }

    fn source_with(event: Event, defaults: Vec<EventReminder>, email: &str) -> AlertSource {
        AlertSource {
            event,
            default_reminders: defaults,
            account_email: email.into(),
        }
    }

    #[test]
    fn overrides_win_when_use_default_is_false() {
        let mut event = Event::default();
        event.id = "evt".into();
        event.start = Some(EventDateTime {
            date: None,
            date_time: Some(Utc.with_ymd_and_hms(2024, 4, 27, 11, 30, 0).unwrap()),
            time_zone: "America/New_York".into(),
        });
        event.reminders = Some(Reminders {
            use_default: false,
            overrides: vec![
                reminder("popup", 0),
                reminder("popup", 10),
                reminder("email", 15),
            ],
        });
        let candidate = candidate_from_source(
            &source_with(event, vec![reminder("popup", 30)], "me@example.com"),
            Edmonton,
        )
        .unwrap();
        assert_eq!(candidate.popup_minutes, vec![0, 10]);
        assert_eq!(
            candidate.anchor,
            Utc.with_ymd_and_hms(2024, 4, 27, 11, 30, 0).unwrap()
        );
    }

    #[test]
    fn defaults_apply_when_use_default_is_true_and_self_decline_is_detected() {
        let mut event = Event::default();
        event.id = "evt".into();
        event.start = Some(EventDateTime {
            date: None,
            date_time: Some(Utc.with_ymd_and_hms(2024, 4, 27, 11, 30, 0).unwrap()),
            time_zone: String::new(),
        });
        event.reminders = Some(Reminders {
            use_default: true,
            overrides: vec![reminder("popup", 5)],
        });
        event.attendees = vec![
            attendee("other@example.com", "declined", false),
            attendee("me@example.com", "declined", true),
        ];
        let candidate = candidate_from_source(
            &source_with(
                event,
                vec![reminder("popup", 10), reminder("email", 60)],
                "me@example.com",
            ),
            Edmonton,
        )
        .unwrap();
        assert_eq!(candidate.popup_minutes, vec![10]);
        assert!(candidate.self_declined);
        assert_eq!(candidate.attendee_count, 2);
    }

    #[test]
    fn all_day_anchor_uses_the_event_time_zone_and_location_counts_as_a_link() {
        let mut event = Event::default();
        event.id = "all-day".into();
        event.location = "https://us02web.zoom.us/j/81727881719".into();
        event.start = Some(EventDateTime {
            date: Some(chrono::NaiveDate::from_ymd_opt(2024, 4, 27).unwrap()),
            date_time: None,
            time_zone: "America/Edmonton".into(),
        });
        let candidate = candidate_from_source(
            &source_with(event, vec![reminder("popup", 0)], "me@example.com"),
            Edmonton,
        )
        .unwrap();
        assert_eq!(
            candidate.anchor,
            Utc.with_ymd_and_hms(2024, 4, 27, 6, 0, 0).unwrap()
        );
        assert!(candidate.has_conference_link);
        assert_eq!(candidate.attendee_count, 0);
    }
}
