mod configuration;

use crate::configuration::{get_configuration, validate_settings, Settings};
use actix_web::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use actix_web::{get, post, web, App, HttpRequest, HttpResponse, HttpServer};
use dotenv::dotenv;
use serde_json::{json, Value};
use std::fmt::{Debug, Display};
use std::net::TcpListener;
use std::time::Duration;
use tokio::task::JoinError;

/// SHA-256 PKCE challenges are 32 bytes, base64url, with no padding.
const S256_CHALLENGE_LEN: usize = 43;
const MIN_STATE_LEN: usize = 16;
const MAX_STATE_LEN: usize = 128;
/// RFC 7636 section 4.1.
const MIN_VERIFIER_LEN: usize = 43;
const MAX_VERIFIER_LEN: usize = 128;
const MAX_CODE_LEN: usize = 2048;
const MAX_REFRESH_TOKEN_LEN: usize = 2048;
const MAX_BODY_BYTES: usize = 4 * 1024;

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

const CALENDAR_LIST_SCOPE: &str = "https://www.googleapis.com/auth/calendar.calendarlist.readonly";
const CALENDAR_EVENTS_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events.readonly";

const LEGACY_SECRET_LOG: &str = "Gave the client secret to a legacy client";
const REFRESH_HEADER: &str = "X-Notor-Token-Refresh";

#[get("/health-check")]
async fn health_check() -> HttpResponse {
    println!("Health check");
    HttpResponse::Ok().finish()
}

#[derive(serde::Deserialize)]
struct LoginRequest {
    state: String,
    challenge: String,
}

#[derive(serde::Deserialize)]
struct TokenRequest {
    code: String,
    verifier: String,
}

/// No `Debug`: a log of this value would print the refresh token.
#[derive(serde::Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

#[post("/login/google")]
async fn google_login(
    body: web::Json<LoginRequest>,
    settings: web::Data<Settings>,
) -> HttpResponse {
    if let Some(error) = validate_login(&body.state, &body.challenge) {
        return json_error(400, error);
    }
    let url = consent_url(&settings, &body.state, &body.challenge);
    no_store(HttpResponse::Ok()).json(json!({ "url": url }))
}

#[post("/oauth2/google/token")]
async fn google_token(
    body: web::Json<TokenRequest>,
    settings: web::Data<Settings>,
    http: web::Data<reqwest::Client>,
    token_url: web::Data<String>,
) -> HttpResponse {
    if body.code.is_empty() || body.code.len() > MAX_CODE_LEN {
        return json_error(400, "invalid_code");
    }
    if !valid_verifier(&body.verifier) {
        return json_error(400, "invalid_verifier");
    }
    let form = [
        ("grant_type", "authorization_code"),
        ("code", body.code.as_str()),
        ("code_verifier", body.verifier.as_str()),
        ("client_id", settings.application.google_client_id.as_str()),
        (
            "client_secret",
            settings.application.google_client_secret.as_str(),
        ),
        (
            "redirect_uri",
            settings.application.google_redirect_url.as_str(),
        ),
    ];
    exchange(&http, token_url.as_str(), &form).await
}

#[post("/oauth2/google/refresh")]
async fn google_refresh(
    body: web::Json<RefreshRequest>,
    settings: web::Data<Settings>,
    http: web::Data<reqwest::Client>,
    token_url: web::Data<String>,
) -> HttpResponse {
    if body.refresh_token.is_empty() || body.refresh_token.len() > MAX_REFRESH_TOKEN_LEN {
        return json_error(400, "invalid_refresh_token");
    }
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", body.refresh_token.as_str()),
        ("client_id", settings.application.google_client_id.as_str()),
        (
            "client_secret",
            settings.application.google_client_secret.as_str(),
        ),
    ];
    exchange(&http, token_url.as_str(), &form).await
}

#[get("/credentials")]
async fn get_credentials(request: HttpRequest, settings: web::Data<Settings>) -> HttpResponse {
    let (body, handout) = credentials_document(&settings, new_client(&request));
    if handout {
        println!("{LEGACY_SECRET_LOG}");
    }
    no_store(HttpResponse::Ok()).json(body)
}

fn new_client(request: &HttpRequest) -> bool {
    request
        .headers()
        .get(REFRESH_HEADER)
        .and_then(|value| value.to_str().ok())
        == Some("server")
}

/// `(body, log a legacy handout)`. The header wins over the rollout switch.
fn credentials_document(settings: &Settings, new_client: bool) -> (Value, bool) {
    let mut body = json!({
        "google_client_id": settings.application.google_client_id,
        "google_calendar_api_key": settings.application.google_calendar_api_key,
        "google_redirect_url": settings.application.google_redirect_url,
    });
    let handout = !new_client && settings.application.serve_client_secret_to_legacy_clients;
    if handout {
        body["google_client_secret"] = json!(settings.application.google_client_secret);
    }
    (body, handout)
}

fn validate_login(state: &str, challenge: &str) -> Option<&'static str> {
    if !(MIN_STATE_LEN..=MAX_STATE_LEN).contains(&state.len()) || !is_base64url(state) {
        return Some("invalid_state");
    }
    if challenge.len() != S256_CHALLENGE_LEN || !is_base64url(challenge) {
        return Some("invalid_challenge");
    }
    None
}

fn valid_verifier(verifier: &str) -> bool {
    (MIN_VERIFIER_LEN..=MAX_VERIFIER_LEN).contains(&verifier.len()) && is_rfc7636_verifier(verifier)
}

fn is_base64url(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// RFC 7636 section 4.1 unreserved characters.
fn is_rfc7636_verifier(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn consent_url(settings: &Settings, state: &str, challenge: &str) -> String {
    let mut url = url::Url::parse(GOOGLE_AUTH_URL).expect("google auth url");
    let scope = format!("{CALENDAR_LIST_SCOPE} {CALENDAR_EVENTS_SCOPE} email profile");
    url.query_pairs_mut()
        .append_pair("client_id", &settings.application.google_client_id)
        .append_pair("redirect_uri", &settings.application.google_redirect_url)
        .append_pair("response_type", "code")
        .append_pair("scope", &scope)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    url.to_string()
}

async fn exchange(http: &reqwest::Client, token_url: &str, form: &[(&str, &str)]) -> HttpResponse {
    match http.post(token_url).form(form).send().await {
        Ok(response) => relay(response).await,
        Err(_) => json_error(502, "unreachable"),
    }
}

async fn relay(response: reqwest::Response) -> HttpResponse {
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let body = response.bytes().await.unwrap_or_default();
    no_store(HttpResponse::build(
        actix_web::http::StatusCode::from_u16(status.as_u16())
            .unwrap_or(actix_web::http::StatusCode::BAD_GATEWAY),
    ))
    .insert_header((CONTENT_TYPE, content_type))
    .body(body)
}

fn json_error(status: u16, error: &str) -> HttpResponse {
    no_store(HttpResponse::build(
        actix_web::http::StatusCode::from_u16(status)
            .unwrap_or(actix_web::http::StatusCode::BAD_REQUEST),
    ))
    .json(json!({ "error": error }))
}

fn no_store(mut builder: actix_web::HttpResponseBuilder) -> actix_web::HttpResponseBuilder {
    builder.insert_header((CACHE_CONTROL, "no-store"));
    builder
}

fn json_config() -> web::JsonConfig {
    web::JsonConfig::default()
        .limit(MAX_BODY_BYTES)
        .error_handler(|err, _| {
            let response = match &err {
                actix_web::error::JsonPayloadError::Overflow { .. } => {
                    json_error(413, "payload_too_large")
                }
                _ => json_error(400, "invalid_request"),
            };
            actix_web::error::InternalError::from_response(err, response).into()
        })
}

fn build_app(
    settings: web::Data<Settings>,
    http: web::Data<reqwest::Client>,
    token_url: web::Data<String>,
) -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    App::new()
        .app_data(settings)
        .app_data(http)
        .app_data(token_url)
        .app_data(json_config())
        .app_data(web::PayloadConfig::default().limit(MAX_BODY_BYTES))
        .service(health_check)
        .service(google_login)
        .service(google_token)
        .service(google_refresh)
        .service(get_credentials)
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("http client")
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    dotenv().ok();
    let configuration = match get_configuration() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("Error: missing or invalid settings:\n  {err}");
            std::process::exit(1);
        }
    };
    let problems = validate_settings(&configuration);
    if !problems.is_empty() {
        eprintln!("Error: missing or invalid settings:");
        for problem in &problems {
            eprintln!("  {problem}");
        }
        std::process::exit(1);
    }

    let address = format!(
        "{}:{}",
        configuration.application.host, configuration.application.port
    );
    let listener = TcpListener::bind(&address).expect("Failed to bind port");
    let port = listener.local_addr().unwrap().port();
    println!("Running Main on Port:{port}...");

    let settings = web::Data::new(configuration);
    let http = web::Data::new(http_client());
    let token_url = web::Data::new(GOOGLE_TOKEN_URL.to_string());
    let server =
        HttpServer::new(move || build_app(settings.clone(), http.clone(), token_url.clone()))
            .listen(listener)?
            .run();
    let application_task = tokio::spawn(server);
    tokio::select! {
        outcome = application_task => report_exit("Application API", outcome)
    }
    Ok(())
}

fn report_exit(task_name: &str, outcome: Result<Result<(), impl Debug + Display>, JoinError>) {
    match outcome {
        Ok(Ok(())) => println!("{} has exited", task_name),
        Ok(Err(err)) => println!("App failed to run {:?}", err),
        Err(err) => println!("App failed to complete {:?}", err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::StatusCode;
    use actix_web::test;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    fn settings(legacy: bool) -> web::Data<Settings> {
        web::Data::new(Settings {
            application: configuration::ApplicationSettings {
                port: 0,
                host: "127.0.0.1".into(),
                base_url: "http://127.0.0.1".into(),
                google_client_id: "client-id".into(),
                google_client_secret: "client-secret".into(),
                google_calendar_api_key: "api-key".into(),
                google_redirect_url: "https://notor.vercel.app/auth".into(),
                serve_client_secret_to_legacy_clients: legacy,
            },
        })
    }

    fn test_app(
        token_url: &str,
        legacy: bool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        build_app(
            settings(legacy),
            web::Data::new(http_client()),
            web::Data::new(token_url.to_string()),
        )
    }

    fn challenge() -> String {
        "a".repeat(S256_CHALLENGE_LEN)
    }

    fn state() -> String {
        "b".repeat(MIN_STATE_LEN)
    }

    fn verifier() -> String {
        "c".repeat(MIN_VERIFIER_LEN)
    }

    struct FakeGoogle {
        base: String,
        requests: Arc<Mutex<Vec<String>>>,
    }

    impl FakeGoogle {
        fn start(status: u16, body: &str) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let shared = Arc::clone(&requests);
            let body = body.to_string();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 1024];
                    loop {
                        match stream.read(&mut tmp) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                buf.extend_from_slice(&tmp[..n]);
                                if let Some(header_end) =
                                    buf.windows(4).position(|w| w == b"\r\n\r\n")
                                {
                                    let header = String::from_utf8_lossy(&buf[..header_end]);
                                    let length = header
                                        .lines()
                                        .find_map(|line| {
                                            line.split_once(':')
                                                .filter(|(name, _)| {
                                                    name.eq_ignore_ascii_case("content-length")
                                                })
                                                .and_then(|(_, value)| {
                                                    value.trim().parse::<usize>().ok()
                                                })
                                        })
                                        .unwrap_or(0);
                                    let have = buf.len() - (header_end + 4);
                                    if have >= length {
                                        let form = String::from_utf8_lossy(
                                            &buf[header_end + 4..header_end + 4 + length],
                                        )
                                        .to_string();
                                        shared.lock().unwrap().push(form);
                                        let response = format!(
                                            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                            body.len()
                                        );
                                        let _ = stream.write_all(response.as_bytes());
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            });
            Self {
                base: format!("http://127.0.0.1:{port}/token"),
                requests,
            }
        }
    }

    #[actix_web::test]
    async fn health_check_returns_ok() {
        let app = test::init_service(test_app("http://127.0.0.1:1/token", false)).await;
        let request = test::TestRequest::get().uri("/health-check").to_request();
        let response = test::call_service(&app, request).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn login_rejects_a_short_state() {
        let app = test::init_service(test_app("http://127.0.0.1:1", false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/login/google")
                .set_json(json!({"state": "short", "challenge": challenge()}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = test::read_body(response).await;
        assert_eq!(&body[..], &br#"{"error":"invalid_state"}"#[..]);
    }

    #[actix_web::test]
    async fn login_rejects_a_long_state() {
        let app = test::init_service(test_app("http://127.0.0.1:1", false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/login/google")
                .set_json(json!({"state": "d".repeat(MAX_STATE_LEN + 1), "challenge": challenge()}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn login_rejects_a_state_outside_base64url() {
        let app = test::init_service(test_app("http://127.0.0.1:1", false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/login/google")
                .set_json(json!({"state": format!("{}+", "e".repeat(MIN_STATE_LEN - 1)), "challenge": challenge()}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn login_rejects_a_challenge_of_the_wrong_length() {
        let app = test::init_service(test_app("http://127.0.0.1:1", false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/login/google")
                .set_json(json!({"state": state(), "challenge": "a".repeat(42)}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = test::read_body(response).await;
        assert_eq!(&body[..], &br#"{"error":"invalid_challenge"}"#[..]);
    }

    #[actix_web::test]
    async fn login_rejects_a_challenge_outside_base64url() {
        let app = test::init_service(test_app("http://127.0.0.1:1", false)).await;
        let mut challenge = challenge();
        challenge.pop();
        challenge.push('+');
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/login/google")
                .set_json(json!({"state": state(), "challenge": challenge}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn login_returns_a_google_consent_url_without_plus_or_full_calendar() {
        let app = test::init_service(test_app("http://127.0.0.1:1", false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/login/google")
                .set_json(json!({"state": state(), "challenge": challenge()}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(CACHE_CONTROL).unwrap(), "no-store");
        let body: Value = serde_json::from_slice(&test::read_body(response).await).unwrap();
        let url = url::Url::parse(body["url"].as_str().unwrap()).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("accounts.google.com"));
        let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert!(query.iter().any(|(k, v)| k == "state" && v == &state()));
        assert!(query
            .iter()
            .any(|(k, v)| k == "code_challenge" && v == &challenge()));
        assert!(query
            .iter()
            .any(|(k, v)| k == "code_challenge_method" && v == "S256"));
        assert!(query
            .iter()
            .any(|(k, v)| k == "access_type" && v == "offline"));
        assert!(query.iter().any(|(k, v)| k == "prompt" && v == "consent"));
        let scope = query
            .iter()
            .find(|(k, _)| k == "scope")
            .unwrap()
            .1
            .split(' ')
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert!(scope.contains(&CALENDAR_LIST_SCOPE.to_string()));
        assert!(scope.contains(&CALENDAR_EVENTS_SCOPE.to_string()));
        assert!(scope.iter().any(|item| item == "email"));
        assert!(scope.iter().any(|item| item == "profile"));
        assert!(!scope
            .iter()
            .any(|item| item == "https://www.googleapis.com/auth/calendar"));
        assert!(!scope.iter().any(|item| item.contains("plus.me")));
        assert!(!body["url"].as_str().unwrap().contains("plus.me"));
        assert!(query
            .iter()
            .any(|(key, value)| key == "redirect_uri" && value == "https://notor.vercel.app/auth"));
    }

    #[actix_web::test]
    async fn token_exchange_forwards_each_verifier_and_the_secret() {
        let fake = FakeGoogle::start(
            200,
            r#"{"access_token":"access","refresh_token":"refresh","expires_in":3600,"scope":"email","token_type":"Bearer"}"#,
        );
        let app = test::init_service(test_app(&fake.base, false)).await;
        for verifier in ["v".repeat(MIN_VERIFIER_LEN), "w".repeat(MIN_VERIFIER_LEN)] {
            let response = test::call_service(
                &app,
                test::TestRequest::post()
                    .uri("/oauth2/google/token")
                    .set_json(json!({"code": format!("code-{verifier}"), "verifier": verifier}))
                    .to_request(),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers().get(CACHE_CONTROL).unwrap(), "no-store");
            let body: Value = serde_json::from_slice(&test::read_body(response).await).unwrap();
            assert_eq!(body["access_token"], "access");
        }
        let seen = fake.requests.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        assert!(seen[0].contains(&format!("code_verifier={}", "v".repeat(MIN_VERIFIER_LEN))));
        assert!(seen[1].contains(&format!("code_verifier={}", "w".repeat(MIN_VERIFIER_LEN))));
        assert!(seen[0].contains("client_secret=client-secret"));
        assert!(!seen[0].contains(&"v".repeat(MIN_VERIFIER_LEN).replace('v', "w")));
    }

    #[actix_web::test]
    async fn token_exchange_relays_a_google_rejection() {
        let fake = FakeGoogle::start(400, r#"{"error":"invalid_grant"}"#);
        let app = test::init_service(test_app(&fake.base, false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/oauth2/google/token")
                .set_json(json!({"code": "abc", "verifier": verifier()}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = test::read_body(response).await;
        assert_eq!(&body[..], &br#"{"error":"invalid_grant"}"#[..]);
    }

    #[actix_web::test]
    async fn token_exchange_is_bad_gateway_when_google_is_down() {
        let app = test::init_service(test_app("http://127.0.0.1:1/token", false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/oauth2/google/token")
                .set_json(json!({"code": "abc", "verifier": verifier()}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = test::read_body(response).await;
        assert_eq!(&body[..], &br#"{"error":"unreachable"}"#[..]);
    }

    #[actix_web::test]
    async fn refresh_relays_google_status_and_body() {
        let fake = FakeGoogle::start(
            400,
            r#"{"error":"invalid_grant","error_description":"revoked"}"#,
        );
        let app = test::init_service(test_app(&fake.base, false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/oauth2/google/refresh")
                .set_json(json!({"refresh_token": "refresh-token"}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = test::read_body(response).await;
        assert_eq!(
            &body[..],
            &br#"{"error":"invalid_grant","error_description":"revoked"}"#[..]
        );
        let seen = fake.requests.lock().unwrap().clone();
        assert!(seen[0].contains("grant_type=refresh_token"));
        assert!(seen[0].contains("client_secret=client-secret"));
        assert!(seen[0].contains("refresh_token=refresh-token"));
    }

    #[actix_web::test]
    async fn a_dotted_verifier_is_forwarded() {
        let fake = FakeGoogle::start(200, r#"{"access_token":"access"}"#);
        let app = test::init_service(test_app(&fake.base, false)).await;
        let verifier = format!("ab.~{}", "d".repeat(MIN_VERIFIER_LEN - 4));
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/oauth2/google/token")
                .set_json(json!({"code": "abc", "verifier": verifier}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let seen = fake.requests.lock().unwrap().clone();
        assert!(seen[0].contains("code_verifier=ab."));
    }

    #[actix_web::test]
    async fn refresh_relays_a_rate_limit() {
        let fake = FakeGoogle::start(429, r#"{"error":"rate_limit"}"#);
        let app = test::init_service(test_app(&fake.base, false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/oauth2/google/refresh")
                .set_json(json!({"refresh_token": "refresh-token"}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = test::read_body(response).await;
        assert_eq!(&body[..], &br#"{"error":"rate_limit"}"#[..]);
    }

    #[actix_web::test]
    async fn refresh_is_bad_gateway_when_google_is_down() {
        let app = test::init_service(test_app("http://127.0.0.1:1/token", false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/oauth2/google/refresh")
                .set_json(json!({"refresh_token": "refresh-token"}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }

    #[actix_web::test]
    async fn refresh_rejects_an_overlong_token() {
        let app = test::init_service(test_app("http://127.0.0.1:1/token", false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/oauth2/google/refresh")
                .set_json(json!({"refresh_token": "r".repeat(MAX_REFRESH_TOKEN_LEN + 1)}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn verifier_and_code_lengths_are_rejected() {
        let app = test::init_service(test_app("http://127.0.0.1:1/token", false)).await;
        let short = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/oauth2/google/token")
                .set_json(json!({"code": "abc", "verifier": "short"}))
                .to_request(),
        )
        .await;
        assert_eq!(short.status(), StatusCode::BAD_REQUEST);
        let long_code = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/oauth2/google/token")
                .set_json(json!({"code": "c".repeat(MAX_CODE_LEN + 1), "verifier": verifier()}))
                .to_request(),
        )
        .await;
        assert_eq!(long_code.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn credentials_hide_the_secret_from_new_clients() {
        let app = test::init_service(test_app("http://127.0.0.1:1", true)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/credentials")
                .insert_header((REFRESH_HEADER, "server"))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(&test::read_body(response).await).unwrap();
        assert_eq!(body["google_client_id"], "client-id");
        assert_eq!(body["google_calendar_api_key"], "api-key");
        assert_eq!(body["google_redirect_url"], "https://notor.vercel.app/auth");
        assert!(body.get("google_client_secret").is_none());
    }

    #[actix_web::test]
    async fn credentials_hide_the_secret_when_the_switch_is_off() {
        let app = test::init_service(test_app("http://127.0.0.1:1", false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::get().uri("/credentials").to_request(),
        )
        .await;
        let body: Value = serde_json::from_slice(&test::read_body(response).await).unwrap();
        assert!(body.get("google_client_secret").is_none());
    }

    #[actix_web::test]
    async fn legacy_clients_receive_the_secret_only_while_the_switch_is_on() {
        let (body, handout) = credentials_document(&settings(true), false);
        assert_eq!(body["google_client_secret"], "client-secret");
        assert!(handout);
        assert_eq!(
            LEGACY_SECRET_LOG,
            "Gave the client secret to a legacy client"
        );
        let (hidden, no_log) = credentials_document(&settings(true), true);
        assert!(hidden.get("google_client_secret").is_none());
        assert!(!no_log);
    }

    #[actix_web::test]
    async fn a_missing_field_is_a_json_error() {
        let app = test::init_service(test_app("http://127.0.0.1:1", false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/login/google")
                .set_json(json!({"state": state()}))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = serde_json::from_slice(&test::read_body(response).await).unwrap();
        assert_eq!(body["error"], "invalid_request");
    }

    #[actix_web::test]
    async fn an_oversized_body_is_rejected() {
        let app = test::init_service(test_app("http://127.0.0.1:1", false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/login/google")
                .insert_header((CONTENT_TYPE, "application/json"))
                .set_payload(vec![b'a'; MAX_BODY_BYTES + 50])
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[actix_web::test]
    async fn a_malformed_body_is_json() {
        let app = test::init_service(test_app("http://127.0.0.1:1", false)).await;
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/oauth2/google/token")
                .insert_header((CONTENT_TYPE, "application/json"))
                .set_payload(br#"{"code":1}"#.to_vec())
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = serde_json::from_slice(&test::read_body(response).await).unwrap();
        assert_eq!(body["error"], "invalid_request");
    }

    #[std::prelude::v1::test]
    fn startup_validation_names_every_empty_setting() {
        let mut settings = settings(false).as_ref().clone();
        settings.application.google_client_id.clear();
        settings.application.google_client_secret.clear();
        settings.application.google_redirect_url.clear();
        let problems = validate_settings(&settings);
        assert_eq!(
            problems,
            vec![
                "application.google_client_id (APP_APPLICATION__GOOGLE_CLIENT_ID) is not set",
                "application.google_client_secret (APP_APPLICATION__GOOGLE_CLIENT_SECRET) is not set",
                "application.google_redirect_url (APP_APPLICATION__GOOGLE_REDIRECT_URL) is not set",
            ]
        );
        settings.application.google_client_id = "id".into();
        settings.application.google_client_secret = "secret".into();
        settings.application.google_redirect_url = "not-a-url".into();
        let problems = validate_settings(&settings);
        assert_eq!(
            problems,
            vec!["application.google_redirect_url (APP_APPLICATION__GOOGLE_REDIRECT_URL) is not a full http(s) URL"]
        );
    }
}
