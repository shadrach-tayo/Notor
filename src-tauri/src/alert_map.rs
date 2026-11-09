use crate::account::AlertSource;
use crate::event_grouping::all_day_start;
use crate::reminder_policy::{popup_offsets, AlertCandidate};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use google_calendar::types::{Event, EventDateTime};
#[cfg(test)]
use google_calendar::types::{EventAttendee, EventReminder, Reminders};

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

    let popup_overrides: Vec<i64> = overrides
        .iter()
        .filter(|reminder| reminder.method == "popup")
        .map(|reminder| reminder.minutes)
        .collect();
    Some(AlertCandidate {
        source_index: 0,
        event_id: source.event.id.clone(),
        i_cal_uid: source.event.i_cal_uid.clone(),
        status: source.event.status.clone(),
        event_type: source.event.event_type.clone(),
        anchor,
        all_day_date: all_day_date_of(&source.event),
        original_start_unix: original_start_unix(&source.event, fallback_tz),
        sequence: source.event.sequence,
        use_default,
        popup_overrides,
        popup_minutes: popup_offsets(use_default, &override_pairs, &default_pairs),
        self_declined: self_declined(&source.event, &source.account_email),
        attendee_count: source.event.attendees.len(),
        has_conference_link: has_conference_link(&source.event),
    })
}

/// Tray rows hide cancelled, declined, and non-meeting event types.
/// `notify_only_meetings` does not apply here.
pub fn include_in_tray(event: &Event) -> bool {
    if event.status == "cancelled" {
        return false;
    }
    if event
        .attendees
        .iter()
        .any(|attendee| attendee.self_ && attendee.response_status == "declined")
    {
        return false;
    }
    !matches!(
        event.event_type.as_str(),
        "outOfOffice" | "focusTime" | "workingLocation" | "birthday"
    )
}

fn anchor_utc(event: &Event, fallback_tz: Tz) -> Option<DateTime<Utc>> {
    instant_utc(event.start.as_ref()?, fallback_tz)
}

/// Occurrence date for an all-day event. Timed events return none.
fn all_day_date_of(event: &Event) -> Option<chrono::NaiveDate> {
    date_only(event.original_start_time.as_ref()).or_else(|| date_only(event.start.as_ref()))
}

fn date_only(when: Option<&EventDateTime>) -> Option<chrono::NaiveDate> {
    let when = when?;
    if when.date_time.is_some() {
        return None;
    }
    when.date
}

fn original_start_unix(event: &Event, fallback_tz: Tz) -> Option<i64> {
    instant_utc(event.original_start_time.as_ref()?, fallback_tz).map(|instant| instant.timestamp())
}

/// Timed instants are absolute. All-day dates are local midnight in the event's
/// time zone (or `fallback_tz`). Google stores "9am the day before" as 900
/// minutes before that midnight.
fn instant_utc(
    when: &google_calendar::types::EventDateTime,
    fallback_tz: Tz,
) -> Option<DateTime<Utc>> {
    if let Some(date_time) = when.date_time {
        return Some(date_time);
    }
    let date = when.date?;
    let tz = if when.time_zone.is_empty() {
        fallback_tz
    } else {
        when.time_zone.parse().unwrap_or(fallback_tz)
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

/// A meeting link is Hangout/Meet data, or an http(s) URL in the location or
/// description whose host is Meet, Zoom, Teams, or Webex (including subdomains).
fn has_conference_link(event: &Event) -> bool {
    if !event.hangout_link.is_empty() {
        return true;
    }
    if event.conference_data.as_ref().is_some_and(|data| {
        !data.conference_id.is_empty()
            || data.entry_points.iter().any(|point| !point.uri.is_empty())
    }) {
        return true;
    }
    text_has_meeting_link(&event.location) || text_has_meeting_link(&event.description)
}

fn text_has_meeting_link(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = token.trim_matches(|c: char| {
            matches!(
                c,
                '<' | '>' | '(' | ')' | '"' | '\'' | ',' | '.' | ';' | ':'
            )
        });
        (token.starts_with("http://") || token.starts_with("https://"))
            && is_known_meeting_host(token)
    })
}

fn is_known_meeting_host(url: &str) -> bool {
    let Some(host) = url_host(url) else {
        return false;
    };
    const HOSTS: &[&str] = &[
        "meet.google.com",
        "zoom.us",
        "teams.microsoft.com",
        "teams.live.com",
        "webex.com",
    ];
    HOSTS
        .iter()
        .any(|known| host == *known || host.ends_with(&format!(".{known}")))
}

fn url_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.split('@').next_back().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
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

    fn linked(location: &str, description: &str) -> bool {
        let mut event = Event::default();
        event.id = "evt".into();
        event.location = location.into();
        event.description = description.into();
        event.start = Some(EventDateTime {
            date: None,
            date_time: Some(Utc.with_ymd_and_hms(2024, 4, 27, 11, 30, 0).unwrap()),
            time_zone: String::new(),
        });
        candidate_from_source(&source_with(event, vec![], "me@example.com"), Edmonton)
            .unwrap()
            .has_conference_link
    }

    #[test]
    fn meeting_hosts_match_on_host_not_substring() {
        assert!(linked("https://us02web.zoom.us/j/1", ""));
        assert!(linked("", "Join https://teams.microsoft.com/l/meetup"));
        assert!(linked("https://meet.google.com/abc-defg-hij", ""));
        assert!(linked("https://example.webex.com/meet/room", ""));
        assert!(!linked("https://zoom.us.evil.com/j/1", ""));
        assert!(!linked("https://notzoom.us/j/1", ""));
        assert!(!linked("https://docs.google.com/document/d/abc/edit", ""));
        assert!(!linked("https://example.com/call", ""));
    }
}
