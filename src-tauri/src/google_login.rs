//! Desktop half of Google sign-in.
//!
//! The app keeps the PKCE verifier and `state`. The server only checks the
//! challenge and, later, adds the client secret when exchanging the code.
//! Pending sign-ins are capped at 8 and expire after 10 minutes.
//!
//! `default_callback_origins` allows `https://notor.vercel.app`.
//! `http://localhost:3000` is included only in debug builds. Edit that list
//! if `/auth` is hosted somewhere else.

use crate::clock::WallClock;
use crate::types::{GoogleAuthToken, UserInfo};
use actix_web::http::header::{
    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,
    CONTENT_TYPE,
};
use actix_web::http::{Method, StatusCode};
use actix_web::{web, HttpRequest, HttpResponse};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use oauth2::PkceCodeChallenge;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const PENDING_CAP: usize = 8;
const PENDING_TTL_MINUTES: i64 = 10;
const MAX_BODY_BYTES: usize = 4 * 1024;

const CALENDAR_LIST_SCOPE: &str = "https://www.googleapis.com/auth/calendar.calendarlist.readonly";
const CALENDAR_EVENTS_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events.readonly";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInFailure {
    pub code: &'static str,
}

impl SignInFailure {
    fn new(code: &'static str) -> Self {
        Self { code }
    }
}

#[derive(Clone, Debug)]
pub struct Started {
    pub state: String,
    pub url: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SignInUpdate {
    pub ok: bool,
    pub error: Option<String>,
    pub token: Option<GoogleAuthToken>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Disposition {
    Pending,
    Finished,
    Cancelled,
    Denied,
    Blocked,
    Expired,
}

struct Record {
    verifier: String,
    expires: DateTime<Utc>,
    consent_url: String,
    disposition: Disposition,
}

struct Inner {
    records: HashMap<String, Record>,
    active: Option<String>,
}

#[derive(Clone)]
pub struct SignInSession {
    inner: Arc<Mutex<Inner>>,
    clock: Arc<dyn WallClock>,
    origins: Arc<Vec<String>>,
    http: reqwest::Client,
    api_base: String,
    userinfo_url: String,
    commit: Arc<dyn Fn(GoogleAuthToken) -> Result<(), String> + Send + Sync>,
    emit: Arc<dyn Fn(SignInUpdate) + Send + Sync>,
}

impl SignInSession {
    pub fn new(
        clock: Arc<dyn WallClock>,
        origins: Vec<String>,
        api_base: impl Into<String>,
        userinfo_url: impl Into<String>,
        commit: Arc<dyn Fn(GoogleAuthToken) -> Result<(), String> + Send + Sync>,
        emit: Arc<dyn Fn(SignInUpdate) + Send + Sync>,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            inner: Arc::new(Mutex::new(Inner {
                records: HashMap::new(),
                active: None,
            })),
            clock,
            origins: Arc::new(origins),
            http,
            api_base: api_base.into(),
            userinfo_url: userinfo_url.into(),
            commit,
            emit,
        }
    }

    pub async fn begin(&self) -> Result<Started, SignInFailure> {
        self.sweep();
        if self.pending_slots() >= PENDING_CAP {
            return Err(SignInFailure::new("too_many"));
        }
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (_state_challenge, state_verifier) = PkceCodeChallenge::new_random_sha256();
        let state = state_verifier.secret().clone();
        let challenge = challenge.as_str().to_string();
        let verifier = verifier.secret().clone();
        let response = self
            .http
            .post(format!(
                "{}/login/google",
                self.api_base.trim_end_matches('/')
            ))
            .json(&json!({ "state": state, "challenge": challenge }))
            .send()
            .await
            .map_err(|_| SignInFailure::new("offline"))?;
        if !response.status().is_success() {
            return Err(SignInFailure::new("upstream"));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| SignInFailure::new("upstream"))?;
        let url = body
            .get("url")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        if !consent_url_is_safe(&url, &state, &challenge) {
            return Err(SignInFailure::new("unsafe_url"));
        }
        let mut inner = self.inner.lock().expect("sign-in");
        self.sweep_locked(&mut inner);
        if pending_in(&inner, self.clock.now()) >= PENDING_CAP {
            return Err(SignInFailure::new("too_many"));
        }
        inner.records.insert(
            state.clone(),
            Record {
                verifier,
                expires: self.clock.now() + ChronoDuration::minutes(PENDING_TTL_MINUTES),
                consent_url: url.clone(),
                disposition: Disposition::Pending,
            },
        );
        inner.active = Some(state.clone());
        Ok(Started { state, url })
    }

    pub fn cancel_active(&self) {
        let mut inner = self.inner.lock().expect("sign-in");
        if let Some(state) = inner.active.clone() {
            if let Some(record) = inner.records.get_mut(&state) {
                if record.disposition == Disposition::Pending {
                    record.disposition = Disposition::Cancelled;
                }
            }
        }
        inner.active = None;
        drop(inner);
        self.emit_update(false, Some("cancelled_in_app".into()), None);
    }

    pub fn active_url(&self) -> Option<String> {
        let inner = self.inner.lock().expect("sign-in");
        let state = inner.active.as_ref()?;
        let record = inner.records.get(state)?;
        if record.disposition == Disposition::Pending && self.clock.now() < record.expires {
            Some(record.consent_url.clone())
        } else {
            None
        }
    }

    pub async fn finish_callback(
        &self,
        origin: Option<&str>,
        content_type: Option<&str>,
        body: &[u8],
    ) -> CallbackReply {
        if !origin_allowed(origin, &self.origins) {
            return CallbackReply::error(403, "forbidden_origin");
        }
        if content_type
            .map(|value| value.split(';').next().unwrap_or("").trim() != "application/json")
            .unwrap_or(true)
        {
            return CallbackReply::error(415, "rejected");
        }
        if body.len() > MAX_BODY_BYTES {
            return CallbackReply::error(413, "rejected");
        }
        let parsed: Value = match serde_json::from_slice(body) {
            Ok(value) => value,
            Err(_) => return CallbackReply::error(400, "rejected"),
        };
        if parsed.get("access_token").is_some() && parsed.get("code").is_none() {
            return CallbackReply::error(400, "rejected");
        }
        let state = parsed
            .get("state")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let code = parsed
            .get("code")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());
        let google_error = parsed
            .get("error")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());
        if state.is_empty() && code.is_none() && google_error.is_none() {
            return CallbackReply::error(400, "nothing_to_finish");
        }
        if let Some(error) = google_error {
            return self.note_google_error(&state, &error);
        }
        let Some(code) = code else {
            return CallbackReply::error(400, "nothing_to_finish");
        };
        let verifier = {
            let mut inner = self.inner.lock().expect("sign-in");
            self.sweep_locked(&mut inner);
            let Some(record) = inner.records.get_mut(&state) else {
                return CallbackReply::error(400, "unknown_state");
            };
            if self.clock.now() >= record.expires && record.disposition == Disposition::Pending {
                record.disposition = Disposition::Expired;
            }
            match record.disposition {
                Disposition::Pending => {
                    record.disposition = Disposition::Finished;
                    record.verifier.clone()
                }
                Disposition::Finished => return CallbackReply::error(400, "already_finished"),
                Disposition::Cancelled => return CallbackReply::error(400, "cancelled_in_app"),
                Disposition::Denied => return CallbackReply::error(400, "access_denied"),
                Disposition::Blocked => return CallbackReply::error(400, "blocked"),
                Disposition::Expired => return CallbackReply::error(400, "timed_out"),
            }
        };
        let exchanged = match self.exchange(&code, &verifier).await {
            Ok(value) => value,
            Err(failure) => {
                if failure.code == "offline" {
                    let mut inner = self.inner.lock().expect("sign-in");
                    if let Some(record) = inner.records.get_mut(&state) {
                        if record.disposition == Disposition::Finished
                            && self.clock.now() < record.expires
                        {
                            record.disposition = Disposition::Pending;
                        }
                    }
                }
                self.emit_update(false, Some(failure.code.into()), None);
                return CallbackReply::error(502, failure.code);
            }
        };
        if !calendar_access_granted(exchanged.scope.as_str()) {
            self.emit_update(false, Some("needs_calendar".into()), None);
            return CallbackReply::error(400, "needs_calendar");
        }
        let user = match self.user_info(&exchanged.access_token).await {
            Ok(user) => user,
            Err(failure) => {
                self.emit_update(false, Some(failure.code.into()), None);
                return CallbackReply::error(502, failure.code);
            }
        };
        let mut token = exchanged;
        token.user = Some(user);
        token.expires_at = Some(self.clock.now().timestamp() + token.expires_in);
        if self.commit.as_ref()(token.clone()).is_err() {
            self.emit_update(false, Some("upstream".into()), None);
            return CallbackReply::error(500, "upstream");
        }
        self.emit_update(true, None, Some(token));
        CallbackReply::ok()
    }

    fn note_google_error(&self, state: &str, error: &str) -> CallbackReply {
        let code = if error == "access_denied" {
            "access_denied"
        } else {
            "blocked"
        };
        let disposition = if error == "access_denied" {
            Disposition::Denied
        } else {
            Disposition::Blocked
        };
        let mut inner = self.inner.lock().expect("sign-in");
        let Some(record) = inner.records.get_mut(state) else {
            return CallbackReply::error(400, "unknown_state");
        };
        if record.disposition == Disposition::Pending {
            record.disposition = disposition;
        }
        if inner.active.as_deref() == Some(state) {
            inner.active = None;
        }
        drop(inner);
        self.emit_update(false, Some(code.into()), None);
        CallbackReply::error(400, code)
    }

    async fn exchange(&self, code: &str, verifier: &str) -> Result<GoogleAuthToken, SignInFailure> {
        let response = self
            .http
            .post(format!(
                "{}/oauth2/google/token",
                self.api_base.trim_end_matches('/')
            ))
            .json(&json!({ "code": code, "verifier": verifier }))
            .send()
            .await
            .map_err(|_| SignInFailure::new("offline"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|_| SignInFailure::new("upstream"))?;
        if !status.is_success() {
            return Err(SignInFailure::new("upstream"));
        }
        let parsed: Value =
            serde_json::from_str(&body).map_err(|_| SignInFailure::new("upstream"))?;
        let access_token = parsed
            .get("access_token")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        if access_token.is_empty() {
            return Err(SignInFailure::new("upstream"));
        }
        Ok(GoogleAuthToken {
            access_token,
            token_type: parsed
                .get("token_type")
                .and_then(|value| value.as_str())
                .unwrap_or("Bearer")
                .to_string(),
            expires_in: parsed
                .get("expires_in")
                .and_then(|value| value.as_i64())
                .unwrap_or(3600),
            refresh_token: parsed
                .get("refresh_token")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string()),
            scope: parsed
                .get("scope")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string(),
            expires_at: None,
            user: None,
            disabled: None,
            needs_signin: false,
        })
    }

    async fn user_info(&self, access_token: &str) -> Result<UserInfo, SignInFailure> {
        let response = self
            .http
            .get(&self.userinfo_url)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| SignInFailure::new("offline"))?;
        if !response.status().is_success() {
            return Err(SignInFailure::new("upstream"));
        }
        let parsed: Value = response
            .json()
            .await
            .map_err(|_| SignInFailure::new("upstream"))?;
        let email = parsed
            .get("email")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        if email.is_empty() {
            return Err(SignInFailure::new("no_email"));
        }
        Ok(UserInfo {
            id: json_string(&parsed, "id"),
            email,
            verified_email: parsed
                .get("verified_email")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            name: json_string(&parsed, "name"),
            given_name: json_string(&parsed, "given_name"),
            family_name: json_string(&parsed, "family_name"),
            picture: json_string(&parsed, "picture"),
            locale: parsed
                .get("locale")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string()),
        })
    }

    fn pending_slots(&self) -> usize {
        let mut inner = self.inner.lock().expect("sign-in");
        self.sweep_locked(&mut inner);
        pending_in(&inner, self.clock.now())
    }

    fn sweep(&self) {
        let mut inner = self.inner.lock().expect("sign-in");
        self.sweep_locked(&mut inner);
    }

    fn sweep_locked(&self, inner: &mut Inner) {
        let now = self.clock.now();
        for record in inner.records.values_mut() {
            if record.disposition == Disposition::Pending && now >= record.expires {
                record.disposition = Disposition::Expired;
            }
        }
        inner.records.retain(|_, record| {
            record.disposition == Disposition::Pending
                || now < record.expires + ChronoDuration::minutes(PENDING_TTL_MINUTES)
        });
        if let Some(active) = inner.active.clone() {
            let still = inner
                .records
                .get(&active)
                .is_some_and(|record| record.disposition == Disposition::Pending);
            if !still {
                inner.active = None;
            }
        }
    }

    fn emit_update(&self, ok: bool, error: Option<String>, token: Option<GoogleAuthToken>) {
        (self.emit)(SignInUpdate { ok, error, token });
    }

    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.pending_slots()
    }
}

fn pending_in(inner: &Inner, now: DateTime<Utc>) -> usize {
    inner
        .records
        .values()
        .filter(|record| record.disposition == Disposition::Pending && now < record.expires)
        .count()
}

pub fn consent_url_is_safe(url: &str, state: &str, challenge: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" || parsed.host_str() != Some("accounts.google.com") {
        return false;
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    let mut saw_state = false;
    let mut saw_challenge = false;
    for (key, value) in parsed.query_pairs() {
        if key == "state" && value == state {
            saw_state = true;
        }
        if key == "code_challenge" && value == challenge {
            saw_challenge = true;
        }
    }
    saw_state && saw_challenge
}

pub fn calendar_access_granted(scope: &str) -> bool {
    let granted: std::collections::HashSet<&str> = scope.split_whitespace().collect();
    granted.contains(CALENDAR_LIST_SCOPE) && granted.contains(CALENDAR_EVENTS_SCOPE)
}

pub fn callback_origins(debug: bool) -> Vec<String> {
    let mut origins = vec!["https://notor.vercel.app".to_string()];
    if debug {
        origins.push("http://localhost:3000".to_string());
    }
    origins
}

pub fn default_callback_origins() -> Vec<String> {
    callback_origins(cfg!(debug_assertions))
}

pub fn auth_server_base() -> &'static str {
    if cfg!(debug_assertions) {
        "http://localhost:4876"
    } else {
        "https://notor-t8pl3.ondigitalocean.app"
    }
}

fn origin_allowed(origin: Option<&str>, allowed: &[String]) -> bool {
    let Some(origin) = origin else {
        return false;
    };
    let origin = origin.trim().trim_end_matches('/');
    allowed.iter().any(|item| item == origin)
}

fn json_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string()
}

#[derive(Clone, Debug)]
pub struct CallbackReply {
    pub status: u16,
    pub body: Value,
}

impl CallbackReply {
    fn ok() -> Self {
        Self {
            status: 200,
            body: json!({ "ok": true }),
        }
    }

    fn error(status: u16, code: &'static str) -> Self {
        Self {
            status,
            body: json!({ "ok": false, "error": code }),
        }
    }
}

#[actix_web::route("/api/google_auth", method = "POST", method = "OPTIONS")]
pub async fn google_auth_callback(
    session: web::Data<SignInSession>,
    request: HttpRequest,
    body: web::Bytes,
) -> HttpResponse {
    let origin = request
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    if request.method() == Method::OPTIONS {
        return preflight(origin.as_deref(), &session.origins);
    }
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let reply = session
        .finish_callback(origin.as_deref(), content_type, &body)
        .await;
    let status = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::BAD_REQUEST);
    let mut response = HttpResponse::build(status).json(reply.body);
    apply_cors(response.headers_mut(), origin.as_deref(), &session.origins);
    response
}

fn preflight(origin: Option<&str>, allowed: &[String]) -> HttpResponse {
    if !origin_allowed(origin, allowed) {
        return HttpResponse::Forbidden().json(json!({ "ok": false, "error": "forbidden_origin" }));
    }
    let mut response = HttpResponse::NoContent().finish();
    apply_cors(response.headers_mut(), origin, allowed);
    response
}

fn apply_cors(
    headers: &mut actix_web::http::header::HeaderMap,
    origin: Option<&str>,
    allowed: &[String],
) {
    if !origin_allowed(origin, allowed) {
        return;
    }
    let Some(origin) = origin else {
        return;
    };
    if let Ok(value) = actix_web::http::header::HeaderValue::from_str(origin) {
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    headers.insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        actix_web::http::header::HeaderValue::from_static("POST, OPTIONS"),
    );
    headers.insert(
        ACCESS_CONTROL_ALLOW_HEADERS,
        actix_web::http::header::HeaderValue::from_static("content-type"),
    );
    headers.insert(
        actix_web::http::header::HeaderName::from_static("access-control-allow-private-network"),
        actix_web::http::header::HeaderValue::from_static("true"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::Calendars;
    use crate::clock::ManualClock;
    use crate::types::Preferences;
    use actix_web::App;
    use std::sync::Mutex as StdMutex;

    fn clock() -> Arc<ManualClock> {
        Arc::new(ManualClock::fixed_new_york())
    }

    struct HitServer {
        base: String,
        hits: Arc<StdMutex<u32>>,
        mode: Arc<StdMutex<String>>,
    }

    impl HitServer {
        fn start() -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let hits = Arc::new(StdMutex::new(0u32));
            let mode = Arc::new(StdMutex::new("ok".to_string()));
            let hits_task = Arc::clone(&hits);
            let mode_task = Arc::clone(&mode);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 2048];
                    loop {
                        match std::io::Read::read(&mut stream, &mut tmp) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                buf.extend_from_slice(&tmp[..n]);
                                if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                    let header = String::from_utf8_lossy(&buf[..end]).to_string();
                                    let length = header
                                        .lines()
                                        .find_map(|line| {
                                            let (name, value) = line.split_once(':')?;
                                            if name.eq_ignore_ascii_case("content-length") {
                                                value.trim().parse::<usize>().ok()
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or(0);
                                    if buf.len() >= end + 4 + length {
                                        let path = header
                                            .lines()
                                            .next()
                                            .unwrap_or("")
                                            .split_whitespace()
                                            .nth(1)
                                            .unwrap_or("/");
                                        let form = String::from_utf8_lossy(
                                            &buf[end + 4..end + 4 + length],
                                        )
                                        .to_string();
                                        let mode = mode_task.lock().unwrap().clone();
                                        let response = route(&mode, path, &form, &hits_task);
                                        let _ = std::io::Write::write_all(
                                            &mut stream,
                                            response.as_bytes(),
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            });
            Self {
                base: format!("http://127.0.0.1:{port}"),
                hits,
                mode,
            }
        }
    }

    fn route(mode: &str, path: &str, body: &str, hits: &StdMutex<u32>) -> String {
        let (status, payload) = if path.contains("/login/google") {
            if mode == "tamper" {
                (200, r#"{"url":"https://evil.example/auth"}"#.to_string())
            } else if mode == "down" {
                (500, r#"{"error":"down"}"#.to_string())
            } else {
                let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
                let state = parsed["state"].as_str().unwrap_or("");
                let challenge = parsed["challenge"].as_str().unwrap_or("");
                let url = format!(
                    "https://accounts.google.com/o/oauth2/v2/auth?state={state}&code_challenge={challenge}&code_challenge_method=S256"
                );
                (200, json!({ "url": url }).to_string())
            }
        } else if path.contains("/token") {
            *hits.lock().unwrap() += 1;
            if mode == "no-calendar" {
                (
                    200,
                    r#"{"access_token":"access","refresh_token":"refresh","expires_in":3600,"scope":"email profile","token_type":"Bearer"}"#
                        .to_string(),
                )
            } else {
                (
                    200,
                    format!(
                        r#"{{"access_token":"access","refresh_token":"refresh","expires_in":3600,"scope":"{CALENDAR_LIST_SCOPE} {CALENDAR_EVENTS_SCOPE} email profile","token_type":"Bearer"}}"#
                    ),
                )
            }
        } else {
            (
                200,
                r#"{"id":"1","email":"ada@example.com","verified_email":true,"name":"Ada Lovelace","given_name":"Ada","family_name":"Lovelace","picture":"https://example.com/a.png"}"#
                    .to_string(),
            )
        };
        format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        )
    }

    fn session(
        server: &HitServer,
        clock: Arc<ManualClock>,
        saved: Arc<StdMutex<Vec<GoogleAuthToken>>>,
    ) -> SignInSession {
        let saved_commit = Arc::clone(&saved);
        SignInSession::new(
            clock,
            callback_origins(false),
            server.base.clone(),
            format!("{}/userinfo", server.base),
            Arc::new(move |token| {
                saved_commit.lock().unwrap().push(token);
                Ok(())
            }),
            Arc::new(|_| {}),
        )
    }

    #[test]
    fn release_origins_are_only_the_hosted_auth_page() {
        let origins = callback_origins(false);
        assert_eq!(origins, vec!["https://notor.vercel.app".to_string()]);
        assert!(callback_origins(true).contains(&"http://localhost:3000".to_string()));
        assert!(default_callback_origins().contains(&"https://notor.vercel.app".to_string()));
    }

    #[test]
    fn a_tampered_consent_url_is_rejected() {
        assert!(consent_url_is_safe(
            "https://accounts.google.com/o/oauth2/v2/auth?state=abc&code_challenge=xyz",
            "abc",
            "xyz"
        ));
        assert!(!consent_url_is_safe(
            "https://evil.example/o/oauth2/v2/auth?state=abc&code_challenge=xyz",
            "abc",
            "xyz"
        ));
        assert!(!consent_url_is_safe(
            "https://accounts.google.com.evil.example/auth?state=abc&code_challenge=xyz",
            "abc",
            "xyz"
        ));
        assert!(!consent_url_is_safe(
            "http://accounts.google.com/o/oauth2/v2/auth?state=abc&code_challenge=xyz",
            "abc",
            "xyz"
        ));
        assert!(!consent_url_is_safe(
            "https://accounts.google.com/o/oauth2/v2/auth?state=other&code_challenge=xyz",
            "abc",
            "xyz"
        ));
    }

    #[tokio::test]
    async fn the_ninth_pending_sign_in_is_refused() {
        let server = HitServer::start();
        let saved = Arc::new(StdMutex::new(Vec::new()));
        let session = session(&server, clock(), saved);
        for _ in 0..PENDING_CAP {
            session.begin().await.unwrap();
        }
        assert_eq!(session.pending_count(), PENDING_CAP);
        let error = session.begin().await.unwrap_err();
        assert_eq!(error.code, "too_many");
        assert_eq!(session.pending_count(), PENDING_CAP);
    }

    #[tokio::test]
    async fn an_expired_sign_in_cannot_be_finished() {
        let server = HitServer::start();
        let saved = Arc::new(StdMutex::new(Vec::new()));
        let manual = clock();
        let session = session(&server, Arc::clone(&manual), Arc::clone(&saved));
        let started = session.begin().await.unwrap();
        manual.advance(Duration::from_secs(10 * 60 + 1));
        let reply = session
            .finish_callback(
                Some("https://notor.vercel.app"),
                Some("application/json"),
                serde_json::to_vec(&json!({"state": started.state, "code": "abc"}))
                    .unwrap()
                    .as_slice(),
            )
            .await;
        assert_eq!(reply.body["error"], "timed_out");
        assert!(saved.lock().unwrap().is_empty());
        assert_eq!(session.pending_count(), 0);
    }

    #[tokio::test]
    async fn a_callback_from_another_copy_is_unknown() {
        let server = HitServer::start();
        let saved = Arc::new(StdMutex::new(Vec::new()));
        let session = session(&server, clock(), Arc::clone(&saved));
        let reply = session
            .finish_callback(
                Some("https://notor.vercel.app"),
                Some("application/json"),
                br#"{"state":"from-the-other-computer","code":"abc"}"#,
            )
            .await;
        assert_eq!(reply.body["error"], "unknown_state");
        assert!(saved.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn replay_and_cancel_are_distinct() {
        let server = HitServer::start();
        let saved = Arc::new(StdMutex::new(Vec::new()));
        let session = session(&server, clock(), Arc::clone(&saved));
        let started = session.begin().await.unwrap();
        let body = serde_json::to_vec(&json!({"state": started.state, "code": "abc"})).unwrap();
        let first = session
            .finish_callback(
                Some("https://notor.vercel.app"),
                Some("application/json"),
                &body,
            )
            .await;
        assert_eq!(first.body["ok"], true);
        assert_eq!(saved.lock().unwrap().len(), 1);
        let second = session
            .finish_callback(
                Some("https://notor.vercel.app"),
                Some("application/json"),
                &body,
            )
            .await;
        assert_eq!(second.body["error"], "already_finished");
        assert_eq!(*server.hits.lock().unwrap(), 1);

        let started = session.begin().await.unwrap();
        session.cancel_active();
        let cancelled = session
            .finish_callback(
                Some("https://notor.vercel.app"),
                Some("application/json"),
                &serde_json::to_vec(&json!({"state": started.state, "code": "later"})).unwrap(),
            )
            .await;
        assert_eq!(cancelled.body["error"], "cancelled_in_app");
        assert_eq!(saved.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unticked_calendar_access_saves_nothing() {
        let server = HitServer::start();
        *server.mode.lock().unwrap() = "no-calendar".into();
        let saved = Arc::new(StdMutex::new(Vec::new()));
        let session = session(&server, clock(), Arc::clone(&saved));
        let started = session.begin().await.unwrap();
        let reply = session
            .finish_callback(
                Some("https://notor.vercel.app"),
                Some("application/json"),
                &serde_json::to_vec(&json!({"state": started.state, "code": "abc"})).unwrap(),
            )
            .await;
        assert_eq!(reply.body["error"], "needs_calendar");
        assert!(saved.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_tampered_login_response_is_not_opened() {
        let server = HitServer::start();
        *server.mode.lock().unwrap() = "tamper".into();
        let saved = Arc::new(StdMutex::new(Vec::new()));
        let session = session(&server, clock(), saved);
        let error = session.begin().await.unwrap_err();
        assert_eq!(error.code, "unsafe_url");
        assert_eq!(session.pending_count(), 0);
    }

    #[tokio::test]
    async fn foreign_origins_and_the_old_token_push_are_rejected() {
        let server = HitServer::start();
        let saved = Arc::new(StdMutex::new(Vec::new()));
        let session = session(&server, clock(), Arc::clone(&saved));
        let foreign = session
            .finish_callback(
                Some("https://evil.example"),
                Some("application/json"),
                br#"{"access_token":"stolen","refresh_token":"also"}"#,
            )
            .await;
        assert_eq!(foreign.status, 403);
        assert_eq!(foreign.body["error"], "forbidden_origin");
        let pushed = session
            .finish_callback(
                Some("https://notor.vercel.app"),
                Some("application/json"),
                br#"{"access_token":"stolen","refresh_token":"also"}"#,
            )
            .await;
        assert_eq!(pushed.body["error"], "rejected");
        assert!(saved.lock().unwrap().is_empty());
        let direct = session
            .finish_callback(
                Some("https://notor.vercel.app"),
                Some("application/json"),
                b"{}",
            )
            .await;
        assert_eq!(direct.body["error"], "nothing_to_finish");
    }

    #[tokio::test]
    async fn the_route_checks_origin_before_accepting_json() {
        let server = HitServer::start();
        let saved = Arc::new(StdMutex::new(Vec::new()));
        let session = session(&server, clock(), saved);
        let app = actix_web::test::init_service(
            App::new()
                .app_data(web::Data::new(session))
                .service(google_auth_callback),
        )
        .await;
        let forbidden = actix_web::test::call_service(
            &app,
            actix_web::test::TestRequest::post()
                .uri("/api/google_auth")
                .insert_header(("Origin", "https://evil.example"))
                .insert_header(("Content-Type", "application/json"))
                .set_json(json!({"access_token": "stolen"}))
                .to_request(),
        )
        .await;
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
        let preflight = actix_web::test::call_service(
            &app,
            actix_web::test::TestRequest::default()
                .method(Method::OPTIONS)
                .uri("/api/google_auth")
                .insert_header(("Origin", "https://notor.vercel.app"))
                .insert_header(("Access-Control-Request-Method", "POST"))
                .to_request(),
        )
        .await;
        assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            preflight
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "https://notor.vercel.app"
        );
    }

    #[test]
    fn a_new_account_is_written_before_it_is_shown() {
        let dir = std::env::temp_dir().join(format!("notor-signin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let calendars = Calendars::with_clock(Arc::new(ManualClock::fixed_new_york()));
        calendars.open_directory(&dir, &Preferences::default());
        let token = GoogleAuthToken {
            access_token: "access".into(),
            token_type: "Bearer".into(),
            expires_in: 3600,
            refresh_token: Some("refresh".into()),
            scope: format!("{CALENDAR_LIST_SCOPE} {CALENDAR_EVENTS_SCOPE}"),
            expires_at: Some(1),
            user: Some(UserInfo {
                email: "ada@example.com".into(),
                name: "Ada".into(),
                ..UserInfo::default()
            }),
            ..GoogleAuthToken::default()
        };
        calendars
            .commit_signed_in_account(token, &Preferences::default())
            .unwrap();
        assert_eq!(calendars.sign_in_order(), vec!["disk", "memory"]);
        let raw = std::fs::read_to_string(dir.join("notor_accounts.json")).unwrap();
        assert!(raw.contains("ada@example.com"));
        assert_eq!(calendars.accounts()[0].email(), "ada@example.com");

        let blocked = dir.join("blocked");
        std::fs::write(&blocked, b"not a directory").unwrap();
        let calendars = Calendars::with_clock(Arc::new(ManualClock::fixed_new_york()));
        calendars.open_directory(&blocked, &Preferences::default());
        let token = GoogleAuthToken {
            access_token: "access".into(),
            user: Some(UserInfo {
                email: "bea@example.com".into(),
                ..UserInfo::default()
            }),
            ..GoogleAuthToken::default()
        };
        assert!(calendars
            .commit_signed_in_account(token, &Preferences::default())
            .is_err());
        assert!(calendars.accounts().is_empty());
        assert!(!calendars.sign_in_order().contains(&"memory"));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&blocked);
    }
}
