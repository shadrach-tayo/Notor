use crate::server::types::{AppCredentials, AppState, TauriAppState};
use actix_cors::Cors;
use actix_web::{http::header, middleware, web, App, HttpServer};
use google_calendar::Client;
use std::{fs, ops::Add, path::PathBuf};
use std::time::Duration;
use chrono::{DateTime, NaiveTime, TimeZone};
use google_calendar::types::Event;
use tauri::{AppHandle, Manager};
use time::macros::time;
use app::utils::with_local_timezone;
use tauri::api::notification::{Notification, Sound};

use self::types::GoogleAuthToken;

pub mod handlers;
pub mod types;
pub mod utils;

pub async fn open_auth_window(app: &AppHandle) -> Result<(), String> {
    if let Some(auth_window) = app.get_window("auth") {
        auth_window.show().unwrap();
        auth_window.close().unwrap();
    }
    let window = tauri::WindowBuilder::new(
        app,
        "auth",
        tauri::WindowUrl::App("signin".into()),
    )
        .center()
        .title("Notor".to_string())
        .hidden_title(true)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .inner_size(1048f64, 650f64)
        .build()
        .map_err(|_| "Failed to create auth window")?;
    window.show().unwrap();

    Ok(())
}

pub async fn open_alert_window(app: &AppHandle, title: String) -> Result<(), String> {
    println!("show alert {}", &title);
    if let Some(auth_window) = app.get_window("alert") {
        println!("check current alert {}:{}", &auth_window.title().unwrap(), &title);

        auth_window.close().unwrap();
        // auth_window.
        // TODO: emit event to do a reload and refresh the current event displayed
        // TODO: keep record of the missed alert
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    let window = tauri::WindowBuilder::new(
        app,
        "alert",
        tauri::WindowUrl::App("alert".into()),
    )
        .center()
        .title(title)
        .hidden_title(true)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .fullscreen(true)
        .closable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .build()
        .map_err(|_| "Failed to create auth window")?;
    window.show().unwrap();

    let size = window.outer_size();
    if size.is_ok() {
        let size = size.unwrap();
        println!("WINDOW SIZE: {:?}", size);
        *app.state::<AppState>().alert_size.lock().unwrap() = size;
    }

    let position = window.outer_position();
    if position.is_ok() {
        println!("WINDOW POSITION {:?}", position);
        *app.state::<AppState>().alert_position.lock().unwrap() = position.unwrap();
    }

    Ok(())
}

pub async fn run_auth(app: &AppHandle) -> Result<(), String> {
    //  TODO: check if user has auth token saved in local app data
    let data_path = tauri::api::path::app_data_dir(&app.config());

    let token_path: PathBuf = data_path.unwrap_or_else(|| PathBuf::from("")).join("googleauthtoken.json"); //PathBuf::from("");

    let mut token = match fs::read_to_string(token_path.clone()) {
        Ok(token) => {
            serde_json::from_str::<GoogleAuthToken>(&token).map_err(|_| "".to_string())
        }
        Err(_) => Err("".to_string()),
    };


    if let Ok(raw_json_token) = &mut token {
        dbg!(&raw_json_token);

        *app.state::<AppState>()
            .google_auth_credentials
            .lock()
            .unwrap() = raw_json_token.clone();

        let app_config = app
            .state::<AppState>()
            .app_config
            .lock()
            .unwrap()
            .clone();
        println!("Auth refresh {:?}", &app_config);

        // check validity and refresh access token
        let mut client = Client::new(
            // GOOGLE_CLIENT_ID,
            app_config.google_client_id.clone(),
            // GOOGLE_CLIENT_SECRET,
            app_config.google_client_secret.clone(),
            // GOOGLE_REDIRECT_URL,
            app_config.google_redirect_url.clone(),
            raw_json_token.access_token.clone(),
            raw_json_token
                .refresh_token
                .clone()
                .unwrap_or("".to_string()),
        );
        let client = client.set_auto_access_token_refresh(true);

        let expired = client.is_expired().await.unwrap_or(true);
        if expired {
            let access_token = client.refresh_access_token().await;

            if let Ok(access_token) = access_token {
                println!("Access token refreshed");
                dbg!(&access_token);
                raw_json_token.access_token = access_token.access_token;
                raw_json_token.expires_in = access_token.expires_in as u64;
                // raw_json_token.refresh_token = access_token.refresh_token;

                let now = chrono::Utc::now();

                let timestamp = now
                    .timestamp()
                    .add(client.expires_in().await.unwrap().as_millis() as i64);

                raw_json_token.expires_at = Some(timestamp as u64);

                // UPDATE APP STATE WITH New Credentials
                *app.state::<AppState>()
                    .google_auth_credentials
                    .lock()
                    .unwrap() = raw_json_token.clone();

                dbg!(&raw_json_token);

                let mut bytes: Vec<u8> = Vec::new();
                serde_json::to_writer(&mut bytes, &raw_json_token).unwrap();
                fs::write(&token_path, &bytes).map_err(|e| {
                    println!("Error writing refresh token to file");
                    e.to_string()
                })?;
            } else {
                let err = access_token.err().unwrap();
                println!("Error refreshing token {:?}", err);
                let _ = open_auth_window(app).await;
            }
        };
    } else {
        #[warn(unused_variables)]
            let _ = open_auth_window(app).await;
    }
    Ok(())
}

pub async fn get_app_config() -> Result<AppCredentials, reqwest::Error> {
    let api_url = if cfg!(debug_assertions) { "http://localhost:4876" } else { "https://notor-t8pl3.ondigitalocean.app" };
    println!("API URL: {}", api_url);
    let response = reqwest::get(format!("{}/credentials", api_url))
        .await?
        .json::<AppCredentials>()
        .await?;
    println!("Response {:?}", &response);
    Ok(response)
}

pub async fn run_timer_until_stopped(handle: AppHandle) {
    loop {
        println!("Timer ticked {:?}", std::time::SystemTime::now());
        let state = &handle.state::<AppState>().pending_events;
        let mut next_event: Option<Event> = None;
        for (_, event) in state.lock().unwrap().iter() {
            if event.start.is_none() {
                continue;
            }
            let start = event.start.clone().unwrap();

            let start_time = {
                let time = if let Some(date_time) = start.date_time {
                    with_local_timezone(date_time)
                } else {
                    let date = start.date.unwrap();
                    let date_with_time = date.and_time(NaiveTime::default());
                    with_local_timezone(chrono::Local.from_local_datetime(&date_with_time).unwrap().to_utc())
                };
                time
            };
            println!(
                "Check if time is now {} {:?}",
                &event.summary,
                start_time
            );
            let now = with_local_timezone(chrono::Utc::now());
            let diff = start_time.timestamp() - now.timestamp();
            if diff <= 5 || diff.is_negative() {
                // event has started, dispatch notification and exit
                println!("Event has started {}", &event.summary);

                next_event = Some(event.clone());
                break;
            } else {
                println!("Minutes left until {}: {:?} {}", &event.summary, diff / 60, diff);
            }
        }
        if let Some(value) = next_event {
            handle.state::<AppState>().pending_events.lock().unwrap().remove(&value.id);
            let window = handle.get_window("main");
            if window.is_some() {
                window.unwrap().emit("alert", &value).unwrap();
            }
            let _ = open_alert_window(&handle, value.summary.to_owned()).await;

            Notification::new(&handle.config().tauri.bundle.identifier)
                .title(value.summary.to_string())
                .body(format!("{} starts now!", value.summary))
                .sound(Sound::Default)
                .show()
                .unwrap();
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

#[tokio::main]
pub async fn start(app: AppHandle) -> std::io::Result<()> {

    // UPDATE APP STATE WITH New Credentials
    let body = get_app_config().await;
    if body.is_ok() {
        *app
            .state::<AppState>()
            .app_config
            .lock()
            .unwrap() = body.unwrap().clone();
    }
    let _ = run_auth(&app).await;
    let tauri_app = web::Data::new(TauriAppState {
        app: app.clone()
    });

    let server = HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin("http://localhost:3000")
            .allowed_origin("tauri://localhost")
            .allowed_origin("https://notor.vercel.app")
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec![
                header::CONTENT_TYPE,
                header::AUTHORIZATION,
                header::ACCEPT,
            ]);

        App::new()
            .app_data(tauri_app.clone())
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .service(handlers::controllers::health)
            .service(handlers::controllers::google_login)
            .service(handlers::controllers::google_auth_refresh)
    })
        .bind(("127.0.0.1", 4875))?
        .run();

    let event_timer = tokio::spawn(run_timer_until_stopped(app));
    let server_task = tokio::spawn(async { server.await });
    tokio::select! {
        _o = event_timer  => report_exit("Event timer"),
        _o = server_task => report_exit("Server exited"),
    }
    Ok(())
}

fn report_exit(task_name: &str) {
    println!("{task_name} exited...");
}