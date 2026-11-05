pub mod account;
pub mod alert_decision;
pub mod autostart;
pub mod event_grouping;
pub mod types;
pub mod utils;

pub use alert_decision::{
    merge_upcoming_into_pending, next_alert_event_id, reconcile_against_poll, should_fire_alert,
    DEFAULT_ALERT_GRACE,
};
pub use event_grouping::{bucket_bounds, classify_event, resolve_local_datetime, EventBucket};
