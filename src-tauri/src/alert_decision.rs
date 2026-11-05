use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::hash::Hash;

/// Default grace window: only alert if the event started within the last 2 minutes.
pub const DEFAULT_ALERT_GRACE: Duration = Duration::seconds(120);

/// Return true when `start` is in the past (or now) but not older than `grace`.
///
/// This prevents firing alerts for meetings that ended hours ago after the
/// laptop wakes from sleep.
pub fn should_fire_alert(start: DateTime<Utc>, now: DateTime<Utc>, grace: Duration) -> bool {
    if start > now {
        return false;
    }
    now.signed_duration_since(start) <= grace
}

/// Among pending `(id, start)` pairs, pick the earliest start that should fire.
pub fn next_alert_event_id<'a, S>(
    pending: impl IntoIterator<Item = (&'a S, DateTime<Utc>)>,
    now: DateTime<Utc>,
    grace: Duration,
) -> Option<&'a S>
where
    S: Eq + Hash + ?Sized,
{
    pending
        .into_iter()
        .filter(|(_, start)| should_fire_alert(*start, now, grace))
        .min_by_key(|(_, start)| *start)
        .map(|(id, _)| id)
}

/// Drop pending entries whose ids are absent from the latest poll, and replace
/// kept entries with the polled copy so moved start times win.
///
/// When `poll_succeeded` is false (API/auth failure), pending is left unchanged.
/// A successful poll with an empty map still clears pending — that means the
/// calendar truly has no events in range, not that the fetch failed.
pub fn reconcile_against_poll<K, T>(
    pending: HashMap<K, T>,
    polled: &HashMap<K, T>,
    poll_succeeded: bool,
) -> HashMap<K, T>
where
    K: Eq + Hash + Clone,
    T: Clone,
{
    if !poll_succeeded {
        return pending;
    }
    pending
        .into_iter()
        .filter_map(|(id, _old)| polled.get(&id).cloned().map(|event| (id, event)))
        .collect()
}

/// Insert or refresh upcoming events into the pending map.
pub fn merge_upcoming_into_pending<K, T>(
    mut pending: HashMap<K, T>,
    upcoming: impl IntoIterator<Item = (K, T)>,
) -> HashMap<K, T>
where
    K: Eq + Hash,
{
    for (id, event) in upcoming {
        pending.insert(id, event);
    }
    pending
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(hours_offset: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 6, 15, 12, 0, 0).unwrap() + Duration::hours(hours_offset)
    }

    #[test]
    fn started_three_hours_ago_gives_no_alert() {
        let now = ts(0);
        let start = ts(-3);
        assert!(!should_fire_alert(start, now, DEFAULT_ALERT_GRACE));
    }

    #[test]
    fn started_within_grace_fires() {
        let now = ts(0);
        let start = now - Duration::seconds(90);
        assert!(should_fire_alert(start, now, DEFAULT_ALERT_GRACE));
    }

    #[test]
    fn future_start_does_not_fire() {
        let now = ts(0);
        let start = now + Duration::minutes(5);
        assert!(!should_fire_alert(start, now, DEFAULT_ALERT_GRACE));
    }

    #[test]
    fn exactly_at_grace_boundary_fires() {
        let now = ts(0);
        let start = now - DEFAULT_ALERT_GRACE;
        assert!(should_fire_alert(start, now, DEFAULT_ALERT_GRACE));
    }

    #[test]
    fn just_outside_grace_does_not_fire() {
        let now = ts(0);
        let start = now - DEFAULT_ALERT_GRACE - Duration::seconds(1);
        assert!(!should_fire_alert(start, now, DEFAULT_ALERT_GRACE));
    }

    #[test]
    fn removed_from_latest_poll_is_dropped() {
        let mut pending = HashMap::new();
        pending.insert("keep".to_string(), "old-keep");
        pending.insert("gone".to_string(), "ghost");

        let mut polled = HashMap::new();
        polled.insert("keep".to_string(), "new-keep");

        let reconciled = reconcile_against_poll(pending, &polled, true);
        assert!(reconciled.get("gone").is_none());
        assert_eq!(reconciled.get("keep"), Some(&"new-keep"));
    }

    #[test]
    fn failed_poll_preserves_pending() {
        let mut pending = HashMap::new();
        pending.insert("a".to_string(), "keep-me");
        let polled: HashMap<String, &str> = HashMap::new();

        let reconciled = reconcile_against_poll(pending, &polled, false);
        assert_eq!(reconciled.get("a"), Some(&"keep-me"));
    }

    #[test]
    fn successful_empty_poll_clears_pending() {
        let mut pending = HashMap::new();
        pending.insert("ghost".to_string(), "drop-me");
        let polled: HashMap<String, &str> = HashMap::new();

        let reconciled = reconcile_against_poll(pending, &polled, true);
        assert!(reconciled.is_empty());
    }

    #[test]
    fn moved_start_time_uses_polled_copy() {
        let now = ts(0);
        let old_start = now - Duration::hours(3);
        let new_start = now + Duration::hours(1);

        let mut pending = HashMap::new();
        pending.insert("evt".to_string(), old_start);

        let mut polled = HashMap::new();
        polled.insert("evt".to_string(), new_start);

        let reconciled = reconcile_against_poll(pending, &polled, true);
        let start = *reconciled.get("evt").unwrap();

        // Old start would have been "stale"; after reconcile it must not fire yet.
        assert!(!should_fire_alert(start, now, DEFAULT_ALERT_GRACE));
        assert_eq!(
            next_alert_event_id(
                reconciled.iter().map(|(k, v)| (k.as_str(), *v)),
                now,
                DEFAULT_ALERT_GRACE
            ),
            None
        );

        // At the new start time it fires.
        assert!(should_fire_alert(start, new_start, DEFAULT_ALERT_GRACE));
        assert_eq!(
            next_alert_event_id(
                reconciled.iter().map(|(k, v)| (k.as_str(), *v)),
                new_start,
                DEFAULT_ALERT_GRACE
            ),
            Some("evt")
        );
    }

    #[test]
    fn merge_upcoming_adds_and_refreshes() {
        let mut pending = HashMap::new();
        pending.insert("a".to_string(), 1);

        let upcoming = vec![("a".to_string(), 2), ("b".to_string(), 3)];
        let pending = merge_upcoming_into_pending(pending, upcoming);

        assert_eq!(pending.get("a"), Some(&2));
        assert_eq!(pending.get("b"), Some(&3));
    }

    #[test]
    fn next_alert_picks_earliest_eligible() {
        let now = ts(0);
        let mut pending = HashMap::new();
        pending.insert("later".to_string(), now - Duration::seconds(10));
        pending.insert("earlier".to_string(), now - Duration::seconds(60));
        pending.insert("stale".to_string(), now - Duration::hours(5));

        let id = next_alert_event_id(
            pending.iter().map(|(k, v)| (k.as_str(), *v)),
            now,
            DEFAULT_ALERT_GRACE,
        );
        assert_eq!(id, Some("earlier"));
    }
}
