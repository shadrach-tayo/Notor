pub mod account;
pub mod account_store;
pub mod alert_decision;
pub mod alert_map;
pub mod autostart;
pub mod clock;
pub mod event_grouping;
pub mod google_api;
pub mod reminder_policy;
pub mod scheduler;
pub mod storage;
pub mod types;
pub mod utils;

pub use alert_decision::{
    merge_upcoming_into_pending, next_alert_event_id, reconcile_against_poll, should_fire_alert,
    DEFAULT_ALERT_GRACE,
};
pub use event_grouping::{
    all_day_start, bucket_bounds, classify_event, resolve_local_datetime, EventBucket,
};
pub use reminder_policy::{
    due_reminders, load_fired_startup, load_fired_state, popup_offsets, save_fired_state,
    AlertCandidate, DueReminder, FiredReminder, FiredStartup, FiredState,
};

#[cfg(test)]
mod behavior_tests;
#[cfg(test)]
mod reminder_policy_tests;
