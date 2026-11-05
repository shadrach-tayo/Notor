//! Pure calendar helpers for Notor alert timing and event bucketing.
//!
//! These APIs take explicit `now` / timezone arguments so they can be unit-tested
//! without the Tauri runtime or the machine clock.

pub mod alert_decision;
pub mod event_grouping;

pub use alert_decision::{
    merge_upcoming_into_pending, next_alert_event_id, reconcile_against_poll, should_fire_alert,
    DEFAULT_ALERT_GRACE,
};
pub use event_grouping::{
    bucket_bounds, classify_event, resolve_local_datetime, EventBucket,
};
