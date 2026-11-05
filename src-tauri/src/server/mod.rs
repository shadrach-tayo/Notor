use actix_cors::Cors;
use actix_web::{http::header, middleware, web, App, HttpServer};
use app::types::{
    AppCredentials, AppState, GoogleAuthToken, Preferences, StateToken, TauriAppState,
};
use std::io::Write;
use std::{fs, path::PathBuf};

use crate::update_try_app;
use app::account::Calendars;
use app::{
    due_reminders, merge_upcoming_into_pending, reconcile_against_poll, DEFAULT_ALERT_GRACE,
};
use google_calendar::types::Event;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tauri::api::notification::{Notification, Sound};
use tauri::{AppHandle, Manager};

pub mod handlers;
pub mod utils;

pub fn open_auth_window(app: &AppHandle) -> Result<(), String> {
    if let Some(auth_window) = app.get_window("auth") {
        auth_window.show().unwrap();
        auth_window.close().unwrap();
    }
    let mut window_builder =
        tauri::WindowBuilder::new(app, "auth", tauri::WindowUrl::App("signin".into()))
            .center()
            .title("Notor".to_string());
    #[cfg(target_os = "macos")]
    {
        window_builder = window_builder
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay);
    }
    let window = window_builder
        .inner_size(1048f64, 650f64)
        .build()
        .map_err(|_| "Failed to create auth window")?;
    window.show().unwrap();

    Ok(())
}

pub async fn open_alert_window(app: &AppHandle, title: String) -> Result<(), String> {
    if let Some(auth_window) = app.get_window("alert") {
        println!(
            "check current alert {}:{}",
            &auth_window.title().unwrap(),
            &title
        );

        auth_window.close().unwrap();
        // auth_window.
        // TODO: emit event to do a reload and refresh the current event displayed
        // TODO: keep record of the missed alert
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    let mut window_builder =
        tauri::WindowBuilder::new(app, "alert", tauri::WindowUrl::App("alert".into()))
            .center()
            .title(title);
    #[cfg(target_os = "macos")]
    {
        window_builder = window_builder
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay);
    }
    let window = window_builder
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
        // println!("WINDOW SIZE: {:?}", size);
        *app.state::<AppState>().alert_size.lock().unwrap() = size;
    }

    let position = window.outer_position();
    if position.is_ok() {
        // println!("WINDOW POSITION {:?}", position);
        *app.state::<AppState>().alert_position.lock().unwrap() = position.unwrap();
    }

    Ok(())
}

pub async fn get_app_config() -> Result<AppCredentials, reqwest::Error> {
    let api_url = if cfg!(debug_assertions) {
        "http://localhost:4876"
    } else {
        "https://notor-t8pl3.ondigitalocean.app"
    };
    println!("API URL: {}", api_url);
    let response = reqwest::get(format!("{}/credentials", api_url))
        .await?
        .json::<AppCredentials>()
        .await?;
    println!("App config loaded");
    Ok(response)
}

pub async fn run_timer_until_stopped(handle: AppHandle) -> Result<(), anyhow::Error> {
    loop {
        let app_state = handle.state::<AppState>();
        let poll_succeeded = app_state.calendars.lock().await.poll_events().await;

        let (polled_events, upcoming_events) = {
            let calendars = app_state.calendars.lock().await;
            (calendars.polled_events(), calendars.upcoming_events())
        };

        let polled_by_id: HashMap<String, Event> = polled_events
            .into_iter()
            .map(|event| (event.id.clone(), event))
            .collect();

        {
            let mut pending = app_state.pending_events.lock().unwrap();
            let pending_owned = std::mem::take(&mut *pending);
            let reconciled = reconcile_against_poll(pending_owned, &polled_by_id, poll_succeeded);
            let upcoming_pairs = upcoming_events
                .into_iter()
                .map(|event| (event.id.clone(), event));
            *pending = merge_upcoming_into_pending(reconciled, upcoming_pairs);
        }

        let now = chrono::Utc::now();
        let notify_only_meetings = app_state.preferences.lock().await.notify_only_meetings();
        let sources = app_state.calendars.lock().await.alert_sources();
        let candidates: Vec<_> = sources
            .iter()
            .enumerate()
            .filter_map(|(source_index, source)| {
                let mut candidate =
                    app::alert_map::candidate_from_source(source, app::utils::system_timezone())?;
                candidate.source_index = source_index;
                Some(candidate)
            })
            .collect();
        let due = {
            let mut fired = app_state.fired_reminders.lock().unwrap();
            due_reminders(
                &candidates,
                now,
                DEFAULT_ALERT_GRACE,
                notify_only_meetings,
                &mut fired,
            )
        };

        let alerts: Vec<(Event, i64)> = due
            .iter()
            .filter_map(|reminder| {
                let event = sources.get(reminder.source_index)?.event.clone();
                println!(
                    "Event reminder due {} offset {}",
                    event.summary, reminder.offset_minutes
                );
                Some((event, reminder.offset_minutes))
            })
            .collect();

        for (event, offset_minutes) in alerts {
            let window = handle.get_window("main");
            println!(
                "================Alert Event : {} {:?} offset {}=========",
                &event.summary, &event.start, offset_minutes
            );
            if window.is_some() {
                window.unwrap().emit("alert", &event).unwrap();
            }
            let _ = open_alert_window(&handle, event.summary.to_owned()).await;

            let body = if offset_minutes == 0 {
                format!("{} starts now!", event.summary)
            } else {
                format!("{} starts in {} minutes", event.summary, offset_minutes)
            };
            Notification::new(&handle.config().tauri.bundle.identifier)
                .title(event.summary.to_string())
                .body(body)
                .sound(Sound::Default)
                .show()
                .unwrap();
        }

        let _ = update_try_app(&handle).await;
        println!("Timer ticked end {:?}", SystemTime::now());
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

/// Migrate app state from google_auth.json to accounts.json file
pub async fn migrate_app_state(app_handle: &AppHandle) -> Result<(), String> {
    let data_path =
        tauri::api::path::app_data_dir(&app_handle.config()).unwrap_or(PathBuf::default());
    let old_path: PathBuf = data_path.join("googleauthtoken.json");
    let new_path: PathBuf = data_path.join("notor_accounts.json");

    if new_path.is_file() {
        return Ok(());
    }

    if old_path.is_file() {
        let token = match fs::read_to_string(&old_path) {
            Ok(token) => {
                serde_json::from_str::<GoogleAuthToken>(&token).map_err(|_| "".to_string())
            }
            Err(_) => Err("".to_string()),
        };

        if token.is_ok() {
            let token = token.unwrap();
            let mut state: Vec<serde_json::Value> = vec![];
            state.push(serde_json::json!({"token": token }));

            let mut file = fs::File::create(new_path).map_err(|err| err.to_string())?;
            let mut bytes: Vec<u8> = Vec::new();
            serde_json::to_writer(&mut bytes, &state).unwrap();
            match file.write(&bytes) {
                Ok(_size) => Ok(()),
                Err(err) => Err(err.to_string()),
            }
        } else {
            Ok(())
        }
    } else {
        Ok(())
    }
}

pub async fn read_account_state(app_handle: &AppHandle) -> Result<Vec<StateToken>, String> {
    let data_path =
        tauri::api::path::app_data_dir(&app_handle.config()).unwrap_or(PathBuf::default());
    let token_path: PathBuf = data_path.join("notor_accounts.json");
    let tokens = match fs::read_to_string(token_path.clone()) {
        Ok(tokens) => {
            serde_json::from_str::<Vec<StateToken>>(&tokens).map_err(|err| err.to_string())
        }
        Err(err) => {
            println!("Error {:?}", &err);
            Err(err.to_string())
        }
    };
    // println!("Tokens {:?}", tokens.unwrap().len());
    let tokens = tokens?
        .iter()
        .filter_map(|t| {
            if t.token.user.is_some() {
                Some(t.to_owned())
            } else {
                None
            }
        })
        .collect::<Vec<StateToken>>();
    println!("Tokens {}", tokens.len());
    Ok(tokens)
}

pub async fn get_app_preferences(app_handle: &AppHandle) -> Result<Preferences, String> {
    let storage_path =
        tauri::api::path::app_data_dir(&app_handle.config()).unwrap_or(PathBuf::default());
    Preferences::load_from_file(storage_path).await
}

#[tokio::main]
pub async fn start(app: AppHandle) -> std::io::Result<()> {
    let migrated = migrate_app_state(&app).await;
    if migrated.is_ok() {
        println!("State migrated successfully")
    } else {
        println!("Error migrating app state: {:?}", migrated.err().unwrap())
    }

    // UPDATE APP STATE WITH New Credentials
    let body = get_app_config().await;
    if body.is_ok() {
        *app.state::<AppState>().app_config.lock().unwrap() = body.unwrap().clone();
    }

    // load app preferences and add to tauri state
    let preferences = get_app_preferences(&app).await;
    let preferences = preferences.map_or(Preferences::default(), |pref| pref);

    println!("Preferences: {:?}", &preferences);
    *app.state::<AppState>().preferences.lock().await = preferences;

    let tokens = read_account_state(&app).await;
    if tokens.is_ok() {
        let tokens = tokens
            .unwrap()
            .iter()
            .map(|t| t.token.to_owned())
            .collect::<Vec<GoogleAuthToken>>();
        if tokens.len() == 0 {
            let _ = open_auth_window(&app);
        } else {
            let config = app.state::<AppState>().app_config.lock().unwrap().clone();
            let state = app.state::<AppState>();
            let preferences = state.preferences.lock().await;
            let calendar = Calendars::new(tokens, config, &preferences).await;
            *app.state::<AppState>().calendars.lock().await = calendar;
        }
    } else {
        dbg!(tokens.err());
        let _ = open_auth_window(&app);
    }

    let tauri_app = web::Data::new(TauriAppState { app: app.clone() });

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
            // .service(handlers::controllers::health)
            .service(handlers::controllers::google_login)
        // .service(handlers::controllers::google_auth_refresh)
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
