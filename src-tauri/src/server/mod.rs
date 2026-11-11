use crate::state::AppState;
use actix_web::{middleware, web, App, HttpServer};
use app::account::spawn_identify;
use app::clock::observation;
use app::google_api::{CalendarApi, GoogleClient};
use app::scheduler::Scheduler;
use app::types::{GoogleAuthToken, Preferences, StateToken};
use app::{
    due_reminders, load_fired_startup, load_fired_state, merge_upcoming_into_pending,
    reconcile_against_poll, save_fired_state, FiredStartup, DEFAULT_ALERT_GRACE,
};
use google_calendar::types::Event;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use std::{fs, path::PathBuf};
use tauri::api::notification::{Notification, Sound};
use tauri::{AppHandle, Manager};

use crate::update_try_app;

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

pub async fn run_timer_until_stopped(handle: AppHandle) -> Result<(), anyhow::Error> {
    let mut startup = true;
    loop {
        let app_state = handle.state::<AppState>();
        let calendars = app_state.calendars.clone();
        let poll_succeeded = calendars.publish_snapshot();
        let (now, zone) = observation(calendars.clock().as_ref());

        let (polled_events, upcoming_events) =
            (calendars.polled_events(), calendars.upcoming_events());

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

        let notify_only_meetings = app_state.preferences.lock().await.notify_only_meetings();
        let mut sources = calendars.alert_sources();
        let known: HashSet<String> = sources
            .iter()
            .map(|source| source.event.id.clone())
            .collect();
        let extras: Vec<Event> = {
            let pending = app_state.pending_events.lock().unwrap();
            pending
                .values()
                .filter(|event| !event.id.is_empty() && !known.contains(&event.id))
                .cloned()
                .collect()
        };
        for event in extras {
            sources.push(app::account::AlertSource {
                event,
                default_reminders: Vec::new(),
                account_email: String::new(),
            });
        }
        let candidates: Vec<_> = sources
            .iter()
            .enumerate()
            .filter_map(|(source_index, source)| {
                let mut candidate = app::alert_map::candidate_from_source(source, zone)?;
                candidate.source_index = source_index;
                Some(candidate)
            })
            .collect();
        let fired_path = fired_alerts_path(&handle);
        let mut fired = if startup {
            startup = false;
            load_fired_startup(&fired_path)
        } else {
            FiredStartup {
                state: load_fired_state(&fired_path),
                show_due: true,
            }
        };
        let due = due_reminders(
            &candidates,
            now,
            DEFAULT_ALERT_GRACE,
            notify_only_meetings,
            &mut fired.state,
        );
        if let Err(err) = save_fired_state(&fired_path, &fired.state) {
            println!("Failed to save fired alerts: {err}");
        }

        if fired.show_due {
            for reminder in due {
                let Some(event) = sources
                    .get(reminder.source_index)
                    .map(|source| source.event.clone())
                else {
                    continue;
                };
                println!(
                    "Event reminder due {} offset {}",
                    event.summary, reminder.offset_minutes
                );
                let window = handle.get_window("main");
                if let Some(window) = window {
                    if let Err(err) = window.emit("alert", &event) {
                        println!("Failed to emit alert: {err}");
                    }
                }
                let _ = open_alert_window(&handle, event.summary.to_owned()).await;
                let body = if reminder.offset_minutes == 0 {
                    format!("{} starts now!", event.summary)
                } else {
                    format!(
                        "{} starts in {} minutes",
                        event.summary, reminder.offset_minutes
                    )
                };
                if let Err(err) = Notification::new(&handle.config().tauri.bundle.identifier)
                    .title(event.summary.to_string())
                    .body(body)
                    .sound(Sound::Default)
                    .show()
                {
                    println!("Failed to show notification: {err}");
                }
            }
        }

        let _ = update_try_app(&handle).await;
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(30)) => {}
            _ = calendars.woken.notified() => {}
        }
    }
}

fn fired_alerts_path(handle: &AppHandle) -> PathBuf {
    tauri::api::path::app_data_dir(&handle.config())
        .unwrap_or_default()
        .join("fired_alerts.json")
}

/// Migrate app state from google_auth.json to accounts.json file.
/// Startup loading goes through `AccountStore`, which also reads the legacy
/// files, so this is no longer called.
#[allow(dead_code)]
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

#[allow(dead_code)]
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
    let data_path = tauri::api::path::app_data_dir(&app.config()).unwrap_or(PathBuf::default());
    let preferences = get_app_preferences(&app)
        .await
        .unwrap_or_else(|_| Preferences::default());
    *app.state::<AppState>().preferences.lock().await = preferences.clone();

    let calendars = app.state::<AppState>().calendars.clone();
    let count = calendars.open_directory(&data_path, &preferences);
    if count == 0 {
        let _ = open_auth_window(&app);
    }

    let credentials_url = format!("{}/credentials", credentials_base());
    let api = GoogleClient::production(credentials_url);
    api.keeper().spawn(calendars.clock());
    *app.state::<AppState>().google.lock().unwrap() = Some(Arc::clone(&api));
    if let Some(creds) = api.keeper().ready() {
        *app.state::<AppState>().app_config.lock().unwrap() = creds;
    }

    let api_trait: Arc<dyn CalendarApi> = api;
    for account in calendars.accounts() {
        if account.email().is_empty() {
            spawn_identify(account, Arc::clone(&api_trait), calendars.clock());
        }
    }
    calendars.refresh_expired_tokens(&api_trait).await;

    let mut scheduler = Scheduler::new(Arc::clone(&calendars), api_trait, calendars.clock());
    if let Some(api) = app.state::<AppState>().google.lock().unwrap().clone() {
        scheduler.set_keeper(api.keeper());
    }
    let scheduler_task = tokio::spawn(async move { scheduler.run().await });

    let session = sign_in_session(app.clone(), Arc::clone(&calendars));
    *app.state::<AppState>().signin.lock().unwrap() = Some(Arc::clone(&session));
    let session_data = web::Data::new((*session).clone());

    let server = HttpServer::new(move || {
        App::new()
            .app_data(session_data.clone())
            .wrap(middleware::Logger::default())
            .service(app::google_login::google_auth_callback)
    })
    .bind(("127.0.0.1", 4875))?
    .run();

    let event_timer = tokio::spawn(run_timer_until_stopped(app));
    let server_task = tokio::spawn(async { server.await });
    tokio::select! {
        _o = scheduler_task => report_exit("Scheduler"),
        _o = event_timer  => report_exit("Event timer"),
        _o = server_task => report_exit("Server exited"),
    }
    Ok(())
}

fn credentials_base() -> &'static str {
    app::google_login::auth_server_base()
}

fn sign_in_session(
    app: AppHandle,
    calendars: Arc<app::account::Calendars>,
) -> Arc<app::google_login::SignInSession> {
    let app_for_commit = app.clone();
    let app_for_events = app.clone();
    let session = app::google_login::SignInSession::new(
        calendars.clock(),
        app::google_login::default_callback_origins(),
        app::google_login::auth_server_base(),
        "https://www.googleapis.com/oauth2/v2/userinfo",
        Arc::new(move |token| {
            let prefs = app_for_commit
                .state::<AppState>()
                .preferences
                .blocking_lock()
                .clone();
            calendars.commit_signed_in_account(token, &prefs)
        }),
        Arc::new(move |update| {
            if let Some(auth) = app_for_events.get_window("auth") {
                let _ = auth.emit("google-signin", &update);
            }
            if update.ok {
                if let Some(token) = update.token.clone() {
                    if let Some(main) = app_for_events.get_window("main") {
                        let _ = main.emit("GOOGLE_AUTH_CREDENTIALS", &token);
                    }
                    *app_for_events
                        .state::<AppState>()
                        .google_auth_credentials
                        .lock()
                        .unwrap() = token;
                }
                if let Some(auth) = app_for_events.get_window("auth") {
                    let _ = auth.close();
                }
            }
        }),
    );
    Arc::new(session)
}

fn report_exit(task_name: &str) {
    println!("{task_name} exited...");
}
