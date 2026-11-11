//! Google Calendar access.
//!
//! Every call goes through [`CalendarApi`]. The real client uses
//! `google_calendar` for calendar requests. Refresh goes through Notor's
//! server, which holds the client secret. The `google_calendar` crate treats
//! a revoked refresh as an empty token and an `Ok`, so this module does not.

use crate::types::{AppCredentials, UserInfo};
use chrono::{DateTime, Utc};
use google_calendar::types::{Event, EventReminder, MinAccessRole};
use google_calendar::ClientError;
#[cfg(test)]
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

const REFRESH_HEADER: &str = "X-Notor-Token-Refresh";
const CALENDAR_HOST: &str = "https://www.googleapis.com/calendar/v3";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiKind {
    /// The access token was rejected. Refresh it, then retry.
    Unauthorized,
    /// Per-user quota. Back the whole account off.
    RateLimit,
    /// This calendar is forbidden, free/busy-only, or gone. Skip it.
    SkipCalendar,
    /// The token is missing the calendar scope for this call.
    InsufficientPermissions,
    /// Network, 5xx, or an app-side failure. Retry later. Not a sign-in.
    Temporary,
    /// The user has to sign in again (`invalid_grant`).
    SignIn,
    /// Notor's own OAuth client was rejected. Refetch credentials and retry.
    InvalidClient,
}

#[derive(Clone, Debug)]
pub struct ApiError {
    pub kind: ApiKind,
    pub retry_after: Option<Duration>,
    pub status: Option<u16>,
    pub body: String,
}

impl ApiError {
    pub fn temporary(body: impl Into<String>) -> Self {
        Self {
            kind: ApiKind::Temporary,
            retry_after: None,
            status: None,
            body: body.into(),
        }
    }

    pub fn kind(kind: ApiKind, body: impl Into<String>) -> Self {
        Self {
            kind,
            retry_after: None,
            status: None,
            body: body.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CalendarInfo {
    pub id: String,
    pub access_role: String,
    pub default_reminders: Vec<EventReminder>,
}

#[derive(Clone, Debug)]
pub struct RefreshedToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: i64,
}

pub trait CalendarApi: Send + Sync {
    fn list_calendars<'a>(
        &'a self,
        access_token: &'a str,
    ) -> BoxFuture<'a, Result<Vec<CalendarInfo>, ApiError>>;

    fn list_events<'a>(
        &'a self,
        access_token: &'a str,
        calendar_id: &'a str,
        time_min: DateTime<Utc>,
        time_max: DateTime<Utc>,
    ) -> BoxFuture<'a, Result<Vec<Event>, ApiError>>;

    fn refresh_token<'a>(
        &'a self,
        refresh_token: &'a str,
    ) -> BoxFuture<'a, Result<RefreshedToken, ApiError>>;

    fn user_info<'a>(&'a self, access_token: &'a str) -> BoxFuture<'a, Result<UserInfo, ApiError>>;
}

/// Sort a Google HTTP response into the next thing Notor should do.
pub fn classify_google_http(status: u16, body: &str) -> ApiKind {
    let reasons = google_reasons(body);
    if reasons.iter().any(|reason| reason == "invalid_grant") {
        return ApiKind::SignIn;
    }
    if reasons
        .iter()
        .any(|reason| reason == "invalid_client" || reason == "unauthorized_client")
    {
        return ApiKind::InvalidClient;
    }
    if status == 401 {
        return ApiKind::Unauthorized;
    }
    if status == 429 || (status == 403 && reasons.iter().any(|reason| is_rate_limit(reason))) {
        return ApiKind::RateLimit;
    }
    if status == 403 {
        if reasons
            .iter()
            .any(|reason| reason == "insufficientPermissions")
        {
            return ApiKind::InsufficientPermissions;
        }
        if reasons
            .iter()
            .any(|reason| reason == "forbidden" || reason == "requiredAccessLevel")
        {
            return ApiKind::SkipCalendar;
        }
        return ApiKind::Temporary;
    }
    if status == 404 || status == 410 {
        return ApiKind::SkipCalendar;
    }
    ApiKind::Temporary
}

fn is_rate_limit(reason: &str) -> bool {
    matches!(
        reason,
        "rateLimitExceeded"
            | "userRateLimitExceeded"
            | "quotaExceeded"
            | "dailyLimitExceeded"
            | "RATE_LIMIT_EXCEEDED"
            | "RESOURCE_EXHAUSTED"
    )
}

fn google_reasons(body: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let mut reasons = Vec::new();
    match value.get("error") {
        Some(serde_json::Value::String(text)) => reasons.push(text.clone()),
        Some(serde_json::Value::Object(object)) => {
            if let Some(status) = object.get("status").and_then(|value| value.as_str()) {
                reasons.push(status.to_string());
            }
            if let Some(errors) = object.get("errors").and_then(|value| value.as_array()) {
                for err in errors {
                    if let Some(reason) = err.get("reason").and_then(|value| value.as_str()) {
                        reasons.push(reason.to_string());
                    }
                }
            }
        }
        _ => {}
    }
    reasons
}

/// Shared OAuth client credentials. One fetch at a time, with a retry window
/// that fails fast instead of calling the auth server again.
pub struct CredKeeper {
    fetch: Arc<dyn Fn() -> BoxFuture<'static, Result<AppCredentials, ApiError>> + Send + Sync>,
    state: Mutex<CredState>,
    notify: tokio::sync::Notify,
}

struct CredState {
    ready: Option<AppCredentials>,
    retry_not_before: Option<DateTime<Utc>>,
    next_delay_secs: u64,
    inflight: bool,
    ever_failed: bool,
    recovered: bool,
}

impl CredKeeper {
    pub fn new(
        fetch: Arc<dyn Fn() -> BoxFuture<'static, Result<AppCredentials, ApiError>> + Send + Sync>,
    ) -> Arc<Self> {
        Arc::new(Self {
            fetch,
            state: Mutex::new(CredState {
                ready: None,
                retry_not_before: None,
                next_delay_secs: 5,
                inflight: false,
                ever_failed: false,
                recovered: false,
            }),
            notify: tokio::sync::Notify::new(),
        })
    }

    pub fn ready(&self) -> Option<AppCredentials> {
        self.state.lock().expect("credentials").ready.clone()
    }

    /// True once, after credentials arrive following a failed load.
    pub fn take_recovered(&self) -> bool {
        let mut state = self.state.lock().expect("credentials");
        let recovered = state.recovered;
        state.recovered = false;
        recovered
    }

    /// Drop cached credentials so the next `get` hits the server immediately.
    pub fn force_reload(&self) {
        let mut state = self.state.lock().expect("credentials");
        if state.ready.is_some() {
            state.ever_failed = true;
        }
        state.ready = None;
        state.retry_not_before = None;
        state.next_delay_secs = 5;
        self.notify.notify_waiters();
    }

    pub async fn get(&self, now: DateTime<Utc>) -> Result<AppCredentials, ApiError> {
        loop {
            let should_wait = {
                let state = self.state.lock().expect("credentials");
                if let Some(ready) = state.ready.clone() {
                    return Ok(ready);
                }
                if state.inflight {
                    true
                } else if state.retry_not_before.is_some_and(|until| now < until) {
                    return Err(ApiError::temporary(
                        "client credentials are in the retry window",
                    ));
                } else {
                    false
                }
            };
            if should_wait {
                self.notify.notified().await;
                continue;
            }
            {
                let mut state = self.state.lock().expect("credentials");
                if state.ready.is_some() || state.inflight {
                    continue;
                }
                if state.retry_not_before.is_some_and(|until| now < until) {
                    return Err(ApiError::temporary(
                        "client credentials are in the retry window",
                    ));
                }
                state.inflight = true;
            }
            let fetched = (self.fetch)().await;
            let mut state = self.state.lock().expect("credentials");
            state.inflight = false;
            match fetched {
                Ok(creds) if credentials_complete(&creds) => {
                    if state.ever_failed {
                        state.recovered = true;
                    }
                    state.ever_failed = false;
                    state.retry_not_before = None;
                    state.next_delay_secs = 5;
                    state.ready = Some(creds.clone());
                    drop(state);
                    self.notify.notify_waiters();
                    return Ok(creds);
                }
                Ok(_) => {
                    note_failure(&mut state, now);
                    drop(state);
                    self.notify.notify_waiters();
                    return Err(ApiError::temporary("client credentials are incomplete"));
                }
                Err(err) => {
                    note_failure(&mut state, now);
                    drop(state);
                    self.notify.notify_waiters();
                    return Err(err);
                }
            }
        }
    }

    /// Background retries: 5s, 10s, 20s, … capped at 2 minutes.
    pub fn spawn(
        self: &Arc<Self>,
        clock: Arc<dyn crate::clock::WallClock>,
    ) -> tokio::task::JoinHandle<()> {
        let keeper = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                match keeper.get(clock.now()).await {
                    Ok(_) => {
                        tokio::select! {
                            _ = keeper.notify.notified() => {}
                            _ = tokio::time::sleep(Duration::from_secs(120)) => {}
                        }
                    }
                    Err(_) => {
                        let wait = keeper
                            .state
                            .lock()
                            .expect("credentials")
                            .retry_not_before
                            .map(|until| {
                                let secs = (until - clock.now()).num_seconds().max(1) as u64;
                                Duration::from_secs(secs.min(120))
                            })
                            .unwrap_or(Duration::from_secs(5));
                        tokio::time::sleep(wait).await;
                    }
                }
            }
        })
    }
}

fn credentials_complete(creds: &AppCredentials) -> bool {
    !creds.google_client_id.is_empty()
}

fn refresh_endpoint(credentials_url: &str) -> String {
    let trimmed = credentials_url.trim_end_matches('/');
    let base = trimmed.trim_end_matches("/credentials");
    format!("{base}/oauth2/google/refresh")
}

fn note_failure(state: &mut CredState, now: DateTime<Utc>) {
    state.ever_failed = true;
    state.ready = None;
    let delay = state.next_delay_secs.max(5);
    state.next_delay_secs = (delay.saturating_mul(2)).min(120);
    state.retry_not_before = Some(now + chrono::Duration::seconds(delay as i64));
}

/// Production Google client. Calendar calls go through `google_calendar` with
/// automatic token refresh turned off.
pub struct GoogleClient {
    http: reqwest::Client,
    keeper: Arc<CredKeeper>,
    clock: Arc<dyn crate::clock::WallClock>,
    credentials_url: String,
    calendar_host: String,
    userinfo_url: String,
}

impl GoogleClient {
    pub fn production(credentials_url: impl Into<String>) -> Arc<Self> {
        Self::with_urls(
            credentials_url,
            "unused-refresh-goes-through-the-server",
            CALENDAR_HOST,
            USERINFO_URL,
        )
    }

    pub fn with_urls(
        credentials_url: impl Into<String>,
        _token_url: impl Into<String>,
        calendar_host: impl Into<String>,
        userinfo_url: impl Into<String>,
    ) -> Arc<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let credentials_url = credentials_url.into();
        let http_for_fetch = http.clone();
        let url_for_fetch = credentials_url.clone();
        let keeper = CredKeeper::new(Arc::new(move || {
            let http = http_for_fetch.clone();
            let url = url_for_fetch.clone();
            Box::pin(async move { fetch_credentials(&http, &url).await })
        }));
        Arc::new(Self {
            http,
            keeper,
            clock: Arc::new(crate::clock::SystemClock),
            credentials_url,
            calendar_host: calendar_host.into(),
            userinfo_url: userinfo_url.into(),
        })
    }

    pub fn with_clock(self: &mut Arc<Self>, clock: Arc<dyn crate::clock::WallClock>) {
        // Arc::get_mut only works before the client is shared. Tests set this
        // immediately after construction; production uses SystemClock.
        if let Some(client) = Arc::get_mut(self) {
            client.clock = clock;
        }
    }

    pub fn keeper(&self) -> Arc<CredKeeper> {
        Arc::clone(&self.keeper)
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    async fn credentials(&self) -> Result<AppCredentials, ApiError> {
        self.keeper.get(self.now()).await
    }
}

async fn fetch_credentials(http: &reqwest::Client, url: &str) -> Result<AppCredentials, ApiError> {
    let response = http
        .get(url)
        .header(REFRESH_HEADER, "server")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|err| ApiError::temporary(err.to_string()))?;
    if !response.status().is_success() {
        return Err(ApiError::temporary(format!(
            "credentials http {}",
            response.status()
        )));
    }
    let creds = response
        .json::<AppCredentials>()
        .await
        .map_err(|err| ApiError::temporary(err.to_string()))?;
    if !credentials_complete(&creds) {
        return Err(ApiError::temporary("client credentials are incomplete"));
    }
    Ok(creds)
}

impl CalendarApi for GoogleClient {
    fn list_calendars<'a>(
        &'a self,
        access_token: &'a str,
    ) -> BoxFuture<'a, Result<Vec<CalendarInfo>, ApiError>> {
        Box::pin(async move {
            let creds = self.credentials().await?;
            let client = google_client(&creds, access_token, &self.calendar_host);
            let listed = client
                .calendar_list()
                .list_all(MinAccessRole::FreeBusyReader, false, false)
                .await
                .map_err(map_client_error)?;
            Ok(listed
                .body
                .into_iter()
                .map(|entry| CalendarInfo {
                    id: entry.id,
                    access_role: entry.access_role,
                    default_reminders: entry.default_reminders,
                })
                .collect())
        })
    }

    fn list_events<'a>(
        &'a self,
        access_token: &'a str,
        calendar_id: &'a str,
        time_min: DateTime<Utc>,
        time_max: DateTime<Utc>,
    ) -> BoxFuture<'a, Result<Vec<Event>, ApiError>> {
        Box::pin(async move {
            let creds = self.credentials().await?;
            let client = google_client(&creds, access_token, &self.calendar_host);
            let listed = client
                .events()
                .list(
                    calendar_id,
                    "",
                    0,
                    2500,
                    google_calendar::types::OrderBy::Noop,
                    "",
                    &[],
                    "",
                    &[],
                    false,
                    false,
                    true,
                    &time_max.to_rfc3339(),
                    &time_min.to_rfc3339(),
                    "",
                    "",
                )
                .await
                .map_err(map_client_error)?;
            Ok(listed.body)
        })
    }

    fn refresh_token<'a>(
        &'a self,
        refresh_token: &'a str,
    ) -> BoxFuture<'a, Result<RefreshedToken, ApiError>> {
        Box::pin(async move {
            let response = self
                .http
                .post(refresh_endpoint(&self.credentials_url))
                .header(REFRESH_HEADER, "server")
                .timeout(REQUEST_TIMEOUT)
                .json(&serde_json::json!({ "refresh_token": refresh_token }))
                .send()
                .await
                .map_err(|err| ApiError::temporary(err.to_string()))?;
            let status = response.status().as_u16();
            let retry_after = retry_after_header(response.headers());
            let body = response
                .text()
                .await
                .map_err(|err| ApiError::temporary(err.to_string()))?;
            let kind = classify_google_http(status, &body);
            if kind == ApiKind::InvalidClient {
                self.keeper.force_reload();
            }
            if kind != ApiKind::Temporary || !(200..300).contains(&status) {
                if !(200..300).contains(&status) {
                    return Err(ApiError {
                        kind,
                        retry_after,
                        status: Some(status),
                        body,
                    });
                }
            }
            let parsed: serde_json::Value =
                serde_json::from_str(&body).map_err(|err| ApiError::temporary(err.to_string()))?;
            let access = parsed
                .get("access_token")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            if access.is_empty() {
                // The google_calendar crate would have returned this as success.
                let kind = if google_reasons(&body).iter().any(|r| r == "invalid_grant") {
                    ApiKind::SignIn
                } else {
                    ApiKind::Temporary
                };
                return Err(ApiError {
                    kind,
                    retry_after,
                    status: Some(status),
                    body,
                });
            }
            let new_refresh = parsed
                .get("refresh_token")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string());
            let expires_in = parsed
                .get("expires_in")
                .and_then(|value| value.as_i64())
                .unwrap_or(3600);
            Ok(RefreshedToken {
                access_token: access,
                refresh_token: new_refresh,
                expires_in,
            })
        })
    }

    fn user_info<'a>(&'a self, access_token: &'a str) -> BoxFuture<'a, Result<UserInfo, ApiError>> {
        Box::pin(async move {
            let _creds = self.credentials().await?;
            let response = self
                .http
                .get(&self.userinfo_url)
                .timeout(REQUEST_TIMEOUT)
                .bearer_auth(access_token)
                .send()
                .await
                .map_err(|err| ApiError::temporary(err.to_string()))?;
            let status = response.status().as_u16();
            let body = response
                .text()
                .await
                .map_err(|err| ApiError::temporary(err.to_string()))?;
            if !(200..300).contains(&status) {
                return Err(ApiError {
                    kind: classify_google_http(status, &body),
                    retry_after: None,
                    status: Some(status),
                    body,
                });
            }
            let value: serde_json::Value =
                serde_json::from_str(&body).map_err(|err| ApiError::temporary(err.to_string()))?;
            let email = value
                .get("email")
                .and_then(|item| item.as_str())
                .unwrap_or("")
                .to_string();
            if email.is_empty() {
                return Err(ApiError::temporary("userinfo had no email"));
            }
            Ok(UserInfo {
                id: json_string(&value, "id"),
                email,
                verified_email: value
                    .get("verified_email")
                    .and_then(|item| item.as_bool())
                    .unwrap_or(false),
                name: json_string(&value, "name"),
                given_name: json_string(&value, "given_name"),
                family_name: json_string(&value, "family_name"),
                picture: json_string(&value, "picture"),
                locale: value
                    .get("locale")
                    .and_then(|item| item.as_str())
                    .map(|item| item.to_string()),
            })
        })
    }
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string()
}

fn google_client(
    creds: &AppCredentials,
    access_token: &str,
    host: &str,
) -> google_calendar::Client {
    let mut client = google_calendar::Client::new(
        creds.google_client_id.clone(),
        String::new(),
        creds.google_redirect_url.clone(),
        access_token,
        "",
    );
    client.set_auto_access_token_refresh(false);
    client.with_host_override(host);
    client.clone()
}

fn map_client_error(err: ClientError) -> ApiError {
    match err {
        ClientError::HttpError {
            status,
            headers,
            error,
        } => {
            let retry_after = retry_after_header(&headers);
            ApiError {
                kind: classify_google_http(status.as_u16(), &error),
                retry_after,
                status: Some(status.as_u16()),
                body: error,
            }
        }
        other => ApiError::temporary(other.to_string()),
    }
}

fn retry_after_header(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    value.parse::<u64>().ok().map(Duration::from_secs)
}

/// How long a rate limit holds an account. `Retry-After` wins, including when
/// it is shorter than the 60 second default.
pub fn rate_limit_wait(retry_after: Option<Duration>) -> Duration {
    retry_after.unwrap_or(Duration::from_secs(60))
}

/// Failure backoff: 15s, 30s, 60s, … capped at 15 minutes, then jitter.
/// `jitter` is a fraction in about -0.1..0.1. Zero leaves the base delay.
pub fn backoff_delay(attempt: u32, jitter: f64) -> Duration {
    let shift = attempt.min(10);
    let base = 15u64.saturating_mul(1u64 << shift).min(15 * 60);
    let factor = (1.0 + jitter.clamp(-0.1, 0.1)).max(0.5);
    let secs = (base as f64 * factor).round().max(1.0) as u64;
    Duration::from_secs(secs.min(15 * 60))
}

/// Poll spacing from the time until the next reminder or event start.
pub fn poll_interval_for(next_in: Option<Duration>) -> Duration {
    match next_in {
        Some(wait) if wait <= Duration::from_secs(10 * 60) => Duration::from_secs(30),
        Some(wait) if wait <= Duration::from_secs(60 * 60) => Duration::from_secs(60),
        Some(wait) if wait <= Duration::from_secs(4 * 60 * 60) => Duration::from_secs(120),
        _ => Duration::from_secs(5 * 60),
    }
}

/// Wall clock versus the sleep-pausing timer. A jump larger than two seconds
/// means the machine woke (or the clock stepped).
pub fn woke(wall_delta_ms: i64, timer_delta: Duration) -> bool {
    let timer_ms = timer_delta.as_millis() as i64;
    wall_delta_ms > timer_ms + 2_000 || wall_delta_ms < -2_000
}

pub struct WakeWatch {
    last_wall: DateTime<Utc>,
    last_timer: Duration,
}

impl WakeWatch {
    pub fn new(clock: &dyn crate::clock::WallClock) -> Self {
        Self {
            last_wall: clock.now(),
            last_timer: clock.timer_elapsed(),
        }
    }

    pub fn observe(&mut self, clock: &dyn crate::clock::WallClock) -> bool {
        let wall = clock.now();
        let timer = clock.timer_elapsed();
        let wall_delta = wall
            .signed_duration_since(self.last_wall)
            .num_milliseconds();
        let timer_delta = timer.saturating_sub(self.last_timer);
        self.last_wall = wall;
        self.last_timer = timer;
        woke(wall_delta, timer_delta)
    }
}

#[cfg(test)]
pub(crate) struct ScriptedApi {
    inner: Arc<Mutex<Script>>,
    release: Arc<tokio::sync::Notify>,
}

#[cfg(test)]
struct Script {
    calendars: Result<Vec<CalendarInfo>, ApiError>,
    calendars_by_token: HashMap<String, Result<Vec<CalendarInfo>, ApiError>>,
    events: HashMap<String, Result<Vec<Event>, ApiError>>,
    refresh: HashMap<String, Result<RefreshedToken, ApiError>>,
    user: Result<UserInfo, ApiError>,
    calls: Vec<String>,
    hold_events: bool,
    hold_refresh: bool,
    in_flight: usize,
    max_in_flight: usize,
}

#[cfg(test)]
impl ScriptedApi {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Script {
                calendars: Ok(vec![CalendarInfo {
                    id: "primary".into(),
                    access_role: "owner".into(),
                    default_reminders: Vec::new(),
                }]),
                calendars_by_token: HashMap::new(),
                events: HashMap::new(),
                refresh: HashMap::new(),
                user: Ok(UserInfo {
                    id: "1".into(),
                    email: "person@example.com".into(),
                    verified_email: true,
                    name: "Person".into(),
                    given_name: "Per".into(),
                    family_name: "Son".into(),
                    picture: String::new(),
                    locale: Some("en".into()),
                }),
                calls: Vec::new(),
                hold_events: false,
                hold_refresh: false,
                in_flight: 0,
                max_in_flight: 0,
            })),
            release: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub fn set_events(&self, calendar_id: &str, events: Result<Vec<Event>, ApiError>) {
        self.inner
            .lock()
            .unwrap()
            .events
            .insert(calendar_id.to_string(), events);
    }

    pub fn set_calendars(&self, calendars: Result<Vec<CalendarInfo>, ApiError>) {
        self.inner.lock().unwrap().calendars = calendars;
    }

    pub fn set_calendars_for(
        &self,
        access_token: &str,
        calendars: Result<Vec<CalendarInfo>, ApiError>,
    ) {
        self.inner
            .lock()
            .unwrap()
            .calendars_by_token
            .insert(access_token.to_string(), calendars);
    }

    pub fn set_refresh(&self, refresh_token: &str, result: Result<RefreshedToken, ApiError>) {
        self.inner
            .lock()
            .unwrap()
            .refresh
            .insert(refresh_token.to_string(), result);
    }

    pub fn set_user(&self, user: Result<UserInfo, ApiError>) {
        self.inner.lock().unwrap().user = user;
    }

    pub fn hold_events(&self, hold: bool) {
        self.inner.lock().unwrap().hold_events = hold;
        if !hold {
            self.release.notify_waiters();
        }
    }

    pub fn hold_refresh(&self, hold: bool) {
        self.inner.lock().unwrap().hold_refresh = hold;
        if !hold {
            self.release.notify_waiters();
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.inner.lock().unwrap().calls.clone()
    }

    pub fn max_in_flight(&self) -> usize {
        self.inner.lock().unwrap().max_in_flight
    }
}

#[cfg(test)]
impl Default for ScriptedApi {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl CalendarApi for ScriptedApi {
    fn list_calendars<'a>(
        &'a self,
        _access_token: &'a str,
    ) -> BoxFuture<'a, Result<Vec<CalendarInfo>, ApiError>> {
        Box::pin(async move {
            let mut script = self.inner.lock().unwrap();
            script.calls.push(format!("calendars:{_access_token}"));
            script
                .calendars_by_token
                .get(_access_token)
                .cloned()
                .unwrap_or_else(|| script.calendars.clone())
        })
    }

    fn list_events<'a>(
        &'a self,
        _access_token: &'a str,
        calendar_id: &'a str,
        _time_min: DateTime<Utc>,
        _time_max: DateTime<Utc>,
    ) -> BoxFuture<'a, Result<Vec<Event>, ApiError>> {
        let calendar_id = calendar_id.to_string();
        Box::pin(async move {
            let hold = {
                let mut script = self.inner.lock().unwrap();
                script.calls.push(format!("events:{calendar_id}"));
                script.in_flight += 1;
                script.max_in_flight = script.max_in_flight.max(script.in_flight);
                script.hold_events
            };
            if hold {
                self.release.notified().await;
            }
            let mut script = self.inner.lock().unwrap();
            script.in_flight = script.in_flight.saturating_sub(1);
            script
                .events
                .get(&calendar_id)
                .cloned()
                .unwrap_or_else(|| Ok(Vec::new()))
        })
    }

    fn refresh_token<'a>(
        &'a self,
        refresh_token: &'a str,
    ) -> BoxFuture<'a, Result<RefreshedToken, ApiError>> {
        let refresh_token = refresh_token.to_string();
        Box::pin(async move {
            let hold = {
                let mut script = self.inner.lock().unwrap();
                script.calls.push(format!("refresh:{refresh_token}"));
                script.hold_refresh
            };
            if hold {
                self.release.notified().await;
            }
            let script = self.inner.lock().unwrap();
            script
                .refresh
                .get(&refresh_token)
                .cloned()
                .unwrap_or(Ok(RefreshedToken {
                    access_token: format!("access-from-{refresh_token}"),
                    refresh_token: None,
                    expires_in: 3600,
                }))
        })
    }

    fn user_info<'a>(
        &'a self,
        _access_token: &'a str,
    ) -> BoxFuture<'a, Result<UserInfo, ApiError>> {
        Box::pin(async move {
            self.inner.lock().unwrap().calls.push("userinfo".into());
            self.inner.lock().unwrap().user.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{ManualClock, WallClock};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn reason_body(reason: &str) -> String {
        format!(
            r#"{{"error":{{"errors":[{{"domain":"usageLimits","reason":"{reason}","message":"nope"}}],"code":403,"message":"nope"}}}}"#
        )
    }

    #[test]
    fn classifies_each_403_reason_and_429() {
        assert_eq!(
            classify_google_http(403, &reason_body("rateLimitExceeded")),
            ApiKind::RateLimit
        );
        assert_eq!(
            classify_google_http(403, &reason_body("userRateLimitExceeded")),
            ApiKind::RateLimit
        );
        assert_eq!(
            classify_google_http(403, &reason_body("quotaExceeded")),
            ApiKind::RateLimit
        );
        assert_eq!(
            classify_google_http(403, &reason_body("dailyLimitExceeded")),
            ApiKind::RateLimit
        );
        let resource = r#"{"error":{"code":403,"message":"quota","status":"RESOURCE_EXHAUSTED"}}"#;
        assert_eq!(classify_google_http(403, resource), ApiKind::RateLimit);
        let newer = r#"{"error":{"code":403,"message":"slow","status":"RATE_LIMIT_EXCEEDED"}}"#;
        assert_eq!(classify_google_http(403, newer), ApiKind::RateLimit);
        assert_eq!(
            classify_google_http(429, &reason_body("rateLimitExceeded")),
            ApiKind::RateLimit
        );
        assert_eq!(
            classify_google_http(403, &reason_body("forbidden")),
            ApiKind::SkipCalendar
        );
        assert_eq!(
            classify_google_http(403, &reason_body("requiredAccessLevel")),
            ApiKind::SkipCalendar
        );
        assert_eq!(
            classify_google_http(403, &reason_body("insufficientPermissions")),
            ApiKind::InsufficientPermissions
        );
        assert_eq!(
            classify_google_http(403, &reason_body("accessNotConfigured")),
            ApiKind::Temporary
        );
        assert_eq!(classify_google_http(401, ""), ApiKind::Unauthorized);
        assert_eq!(
            classify_google_http(
                400,
                r#"{"error":"invalid_grant","error_description":"revoked"}"#
            ),
            ApiKind::SignIn
        );
        assert_eq!(
            classify_google_http(401, r#"{"error":"invalid_client"}"#),
            ApiKind::InvalidClient
        );
        assert_eq!(
            classify_google_http(401, r#"{"error":"unauthorized_client"}"#),
            ApiKind::InvalidClient
        );
    }

    #[test]
    fn backoff_grows_and_caps_and_rate_limit_respects_retry_after() {
        assert_eq!(backoff_delay(0, 0.0), Duration::from_secs(15));
        assert_eq!(backoff_delay(1, 0.0), Duration::from_secs(30));
        assert_eq!(backoff_delay(2, 0.0), Duration::from_secs(60));
        assert_eq!(backoff_delay(3, 0.0), Duration::from_secs(120));
        assert_eq!(backoff_delay(9, 0.0), Duration::from_secs(15 * 60));
        assert_eq!(backoff_delay(20, 0.0), Duration::from_secs(15 * 60));
        assert_eq!(rate_limit_wait(None), Duration::from_secs(60));
        assert_eq!(
            rate_limit_wait(Some(Duration::from_secs(90))),
            Duration::from_secs(90)
        );
        assert_eq!(
            rate_limit_wait(Some(Duration::from_secs(10))),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn poll_tiers_follow_the_next_reminder() {
        assert_eq!(
            poll_interval_for(Some(Duration::from_secs(5 * 60))),
            Duration::from_secs(30)
        );
        assert_eq!(
            poll_interval_for(Some(Duration::from_secs(30 * 60))),
            Duration::from_secs(60)
        );
        assert_eq!(
            poll_interval_for(Some(Duration::from_secs(2 * 60 * 60))),
            Duration::from_secs(120)
        );
        assert_eq!(poll_interval_for(None), Duration::from_secs(300));
        assert_eq!(
            poll_interval_for(Some(Duration::from_secs(5 * 60 * 60))),
            Duration::from_secs(300)
        );
    }

    #[test]
    fn wake_is_a_wall_jump_the_timer_did_not_see() {
        assert!(!woke(5_000, Duration::from_secs(5)));
        assert!(woke(3_600_000, Duration::from_secs(5)));
        assert!(woke(-10_000, Duration::from_secs(5)));
        let clock = ManualClock::fixed_new_york();
        let mut watch = WakeWatch::new(&clock);
        clock.advance(Duration::from_secs(5));
        assert!(!watch.observe(&clock));
        clock.jump_wall(chrono::Duration::hours(2));
        assert!(watch.observe(&clock));
    }

    #[tokio::test]
    async fn credentials_are_not_fetched_again_inside_the_retry_window() {
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_task = Arc::clone(&hits);
        let keeper = CredKeeper::new(Arc::new(move || {
            hits_task.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(ApiError::temporary("offline")) })
        }));
        let clock = ManualClock::fixed_new_york();
        let err = keeper.get(clock.now()).await.unwrap_err();
        assert_eq!(err.kind, ApiKind::Temporary);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let err = keeper.get(clock.now()).await.unwrap_err();
        assert!(err.body.contains("retry window"));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        clock.advance(Duration::from_secs(5));
        let _ = keeper.get(clock.now()).await;
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn incomplete_credentials_are_refused_and_a_later_success_is_recovered() {
        let phase = Arc::new(Mutex::new(0u8));
        let phase_task = Arc::clone(&phase);
        let keeper = CredKeeper::new(Arc::new(move || {
            let phase = Arc::clone(&phase_task);
            Box::pin(async move {
                let mut phase = phase.lock().unwrap();
                *phase += 1;
                if *phase == 1 {
                    Ok(AppCredentials {
                        google_client_id: String::new(),
                        ..AppCredentials::default()
                    })
                } else {
                    Ok(AppCredentials {
                        google_client_id: "id".into(),
                        google_redirect_url: "http://localhost".into(),
                        ..AppCredentials::default()
                    })
                }
            })
        }));
        let clock = ManualClock::fixed_new_york();
        assert!(keeper.get(clock.now()).await.is_err());
        assert!(!keeper.take_recovered());
        clock.advance(Duration::from_secs(5));
        let creds = keeper.get(clock.now()).await.unwrap();
        assert_eq!(creds.google_client_id, "id");
        assert!(keeper.take_recovered());
    }

    #[tokio::test]
    async fn one_credential_fetch_is_shared() {
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_task = Arc::clone(&hits);
        let keeper = CredKeeper::new(Arc::new(move || {
            hits_task.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                Ok(AppCredentials {
                    google_client_id: "id".into(),
                    google_redirect_url: "http://localhost".into(),
                    ..AppCredentials::default()
                })
            })
        }));
        let now = ManualClock::fixed_new_york().now();
        let first = keeper.get(now);
        let second = keeper.get(now);
        let (a, b) = tokio::join!(first, second);
        assert_eq!(a.unwrap().google_client_id, "id");
        assert_eq!(b.unwrap().google_client_id, "id");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }
}
