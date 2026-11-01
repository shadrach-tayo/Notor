# Stale/ghost calendar alerts and broken tomorrow bucketing

## Why it matters

Notor is a Tauri menu-bar Google Calendar app. After the laptop wakes from sleep, or after a meeting is deleted, declined, or moved, the app can still fire alerts for meetings that are long over or no longer on the calendar. Separately, “tomorrow” event bucketing misclassifies all-day and late-day events. Users lose trust when the menu bar cries wolf.

## Problem (verified in code)

1. **Stale alerts** — In `run_timer_until_stopped` (`src-tauri/src/server/mod.rs`), an alert fires for any pending event whose start time is in the past (`diff.is_negative()`), with **no lower bound**. After sleep, meetings that ended hours ago still alert.

2. **Ghost alerts** — Events are inserted into `pending_events` from `upcoming_events` and only removed when they fire. Nothing reconciles the map against the latest poll, so a deleted/declined/moved event still fires from the stale entry.

3. **Tomorrow bucketing** — `Calendars::group_events` (`src-tauri/src/account.rs`):
   - `tomorrow` keeps the current nanoseconds after zeroing h/m/s, so an all-day event tomorrow (local midnight) can land in today’s upcoming list.
   - `tomorrow_end` is `23:59:00`, so events starting in the last minute of the day are dropped.
   - `parse_event_datetime` (`src-tauri/src/utils.rs`) calls `.unwrap()` on `from_local_datetime`, which panics when local midnight does not exist (DST spring-forward gap) or is ambiguous.

## What to do

- Extract alert decision and pending-event reconciliation into **pure functions** that take `now` (and a grace `Duration`) so they can be tested without the Tauri runtime.
- Default grace: alert only if the event **started within the last 2 minutes** (`start <= now` and `now - start <= 2 minutes`).
- After each poll, reconcile `pending_events`: drop ids missing from the latest upcoming set; refresh stored events when the poll returns a newer copy (so a moved start time wins); insert new upcoming events.
- Extract event bucketing into pure functions that take `now`. Tomorrow starts at local midnight with **zero nanoseconds**. The tomorrow window is `[tomorrow_start, day_after_tomorrow_start)` (exclusive end), so `23:59:30` tomorrow is included.
- Replace the DST `unwrap` with a safe conversion that returns a defined instant (or `Option`) without panicking.
- Wire the helpers into `run_timer_until_stopped` and `group_events` with the **smallest** surrounding change. Do not rewrite the Tauri shell.

## Check

From `src-tauri` (pure helpers live in the `notor-core` crate so they compile without linking the Tauri/WebKit stack):

```bash
cargo test -p notor-core -- --nocapture
```

To exercise only the suites named in this brief:

```bash
cargo test -p notor-core alert_decision group_events -- --nocapture
```

Required cases (as regression tests):

| Case | Expected |
|------|----------|
| Event started 3 hours ago | no alert |
| Event removed from the latest poll | dropped from pending; no alert |
| Event start moved to a later time | fires at the new time, not the old one |
| All-day event tomorrow | `tomorrow` bucket, not `upcoming` |
| Event at 23:59:30 tomorrow | still in `tomorrow` |
| DST spring-forward local midnight | no panic; returns a defined instant |
| Failed poll (`poll_succeeded = false`) | pending preserved (not wiped) |
| Successful empty poll | pending cleared |

**Mutation check:** change the grace-window comparison (e.g. widen grace to 3 hours, or flip `<=` to `<` incorrectly), re-run the suite — at least one staleness/grace test must fail — then restore.

Also run:

```bash
cargo test
```

Existing behavior outside these helpers should keep compiling.

## Note to reviewer

- Pure helpers live in `src-tauri/notor-core/` (`alert_decision.rs`, `event_grouping.rs`) with unit tests. The `app` crate depends on it via a path dependency (keeps tests runnable without linking Tauri/WebKit).
- `server/mod.rs`, `account.rs`, and `utils.rs` should call those helpers rather than keep the old inline logic.
- Failed polls must not clear pending: `get_calendar_events` returns `Result`, `poll_events` returns `bool`, and reconcile takes `poll_succeeded`.
- Tray UI bucketing in `src/slices/hooks.ts` should use the same `[tomorrow, dayAfter)` window (not `23:59:00`).
- Full `cargo test` / `cargo check` for the Tauri `app` binary needs WebKitGTK 4.0 (not available on Ubuntu 24.04’s 4.1-only packages). Evaluating the behavior change via `cargo test -p notor-core` is intentional and sufficient for this task.
- Prefer the smallest behavior change outside those helpers.
- Root `.gitignore` should ignore `src-tauri/target`, `src-server/target`, and `src-tauri/notor-core/target`.

## Out of scope

- Filtering declined/cancelled attendees (`response_status`) — separate task.
- OAuth token storage / keychain — separate task.
- `src-server` PKCE/`state` hardening — separate task.
- Join-meeting UI (Meet/Zoom buttons already exist on the alert page).
