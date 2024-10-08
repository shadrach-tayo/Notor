// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod server;

use crate::server::{open_alert_window, open_auth_window};
use app::autostart;
use app::types::{AppState, GoogleAuthToken, Preferences};
use app::utils::{get_date_time, get_human_readable_time, time_to_relative_format};
use google_calendar::types::Event;
use std::path::PathBuf;
use std::{fs, io::Write, thread};
use tauri::{
    AppHandle, CustomMenuItem, Manager, PhysicalPosition, SystemTray, SystemTrayEvent,
    SystemTrayMenu, SystemTrayMenuItem, Window,
};

#[tauri::command]
async fn app_loaded(
    window: Window,
    state: tauri::State<'_, AppState>,
) -> Result<GoogleAuthToken, String> {
    println!("App loaded Event {}", window.label());
    let credentials = state.google_auth_credentials.lock().unwrap().clone();
    Ok(credentials)
}

#[tauri::command]
async fn show_alert(window: Window, title: String) -> Result<(), String> {
    println!("show_alert Event {}", window.label());
    let handle = window.app_handle();
    let _ = open_alert_window(&handle, title).await;
    Ok(())
}

#[tauri::command]
async fn schedule_events(window: Window, events: Vec<Event>) -> Result<(), String> {
    // println!("schedule_events {}: {}", events.len(), events.first().unwrap().summary);
    for event in events.iter() {
        window
            .app_handle()
            .state::<AppState>()
            .pending_events
            .lock()
            .unwrap()
            .insert(event.id.clone(), event.to_owned());
    }
    Ok(())
}

#[tauri::command]
async fn dismiss_alert(window: Window) -> Result<(), String> {
    println!("dismiss_alert Event {}", window.label());
    if window.close().is_ok() {
        Ok(())
    } else {
        Err("Error closing alert window".to_string())
    }
}

#[tauri::command]
async fn logout(window: Window) {
    let handle = window.app_handle();

    let data_path = tauri::api::path::app_data_dir(&handle.config());

    let data_path = if let Some(path) = data_path {
        path.join("googleauthtoken.json")
    } else {
        "".into()
    };

    let _ = std::fs::remove_file(data_path);

    let _ = open_auth_window(&handle);

    println!("User Logged out");
}

fn event_to_relative_time_string(
    event: &Event,
    menu: &mut Vec<CustomMenuItem>,
) -> Vec<CustomMenuItem> {
    let time = get_date_time(event);
    let time_str = get_human_readable_time(time);
    menu.push(CustomMenuItem::new(
        &event.id,
        format!("{} {}  {}", "   ", time_str, &event.summary),
    ));
    menu.to_owned()
}

pub async fn update_try_app(app: &AppHandle) -> Result<(), String> {
    let events = app
        .state::<AppState>()
        .calendars
        .lock()
        .await
        .event_groups
        .lock()
        .unwrap()
        .clone();

    // println!("Now Groups {:?}", events.now.iter().map(|g| &g.summary).collect::<Vec<&String>>());
    // println!("Upcoming Groups {:?}", events.upcoming.iter().map(|g| &g.summary).collect::<Vec<&String>>());
    // println!("Tomorrow Groups {:?}", events.tomorrow.iter().map(|g| &g.summary).collect::<Vec<&String>>());
    if events.now.is_empty() && events.upcoming.is_empty() && events.tomorrow.is_empty() {
        return Ok(());
    }

    let mut system_tray_menu = SystemTrayMenu::new();

    let mut ongoing_event_items: Vec<CustomMenuItem> = vec![];
    if !events.now.is_empty() {
        let end_time = time_to_relative_format(events.now.first().unwrap().clone().end.unwrap());

        let ongoing = CustomMenuItem::new("ongoing", format!("Ending {}", end_time))
            .native_image(tauri::NativeImage::StatusAvailable)
            .disabled();

        ongoing_event_items.push(ongoing);

        for event in events.now.iter() {
            ongoing_event_items = event_to_relative_time_string(event, &mut ongoing_event_items);
        }

        for menu in ongoing_event_items.iter() {
            system_tray_menu = system_tray_menu.add_item(menu.to_owned());
        }
    }

    let mut upcoming_event_items: Vec<CustomMenuItem> = vec![];
    if !events.upcoming.is_empty() {
        let start_time =
            time_to_relative_format(events.upcoming.first().unwrap().clone().start.unwrap());
        let upcoming = CustomMenuItem::new("upcoming", format!("Upcoming {}", start_time))
            .native_image(tauri::NativeImage::StatusPartiallyAvailable)
            .disabled();
        upcoming_event_items.push(upcoming);

        for event in events.upcoming.iter() {
            upcoming_event_items = event_to_relative_time_string(event, &mut upcoming_event_items);
        }

        for menu in upcoming_event_items.iter() {
            system_tray_menu = system_tray_menu.add_item(menu.to_owned());
        }
    }

    let mut tomorrow_event_items: Vec<CustomMenuItem> = vec![];
    if !events.tomorrow.is_empty() {
        let tomorrow = CustomMenuItem::new("tomorrow", "Tomorrow")
            .native_image(tauri::NativeImage::StatusUnavailable)
            .disabled();
        tomorrow_event_items.push(tomorrow);

        for event in events.tomorrow.iter() {
            tomorrow_event_items = event_to_relative_time_string(event, &mut tomorrow_event_items);
        }

        for menu in tomorrow_event_items.iter() {
            system_tray_menu = system_tray_menu.add_item(menu.to_owned());
        }
    }

    let quit = CustomMenuItem::new("quit", "Quit Notor app completely");
    let settings = CustomMenuItem::new("settings", "Add new account");

    system_tray_menu = system_tray_menu
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(CustomMenuItem::new("show_app", "Notor App"))
        .add_item(settings)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(quit);

    let _ = SystemTray::new()
        .with_id("events_tray")
        .with_title("Event in 2mins")
        .with_menu(system_tray_menu)
        .build(app);

    Ok(())
}

fn build_tray_app(app_handle: &tauri::App) -> Result<(), ()> {
    let quit = CustomMenuItem::new("quit", "Quit Notor app completely             ❌");
    let settings = CustomMenuItem::new("settings", "Add new account");
    let system_tray_menu = SystemTrayMenu::new()
        .add_item(CustomMenuItem::new("show_app", "Notor App"))
        .add_item(settings)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(quit);

    let _ = SystemTray::new()
        .with_id("events_tray")
        .with_title("Event in 2mins")
        .with_menu(system_tray_menu)
        .build(app_handle);
    Ok(())
}

#[tauri::command]
async fn list_accounts(window: Window) -> Result<Vec<GoogleAuthToken>, String> {
    let tokens = window
        .app_handle()
        .state::<AppState>()
        .calendars
        .lock()
        .await
        .get_tokens()
        .await;
    Ok(tokens.unwrap())
}

#[tauri::command]
async fn remove_account(window: Window, email: String) -> Result<(), String> {
    let handle = window.app_handle();
    let state = handle.state::<AppState>();
    let pref = state.preferences.lock().await;
    let _ = handle
        .state::<AppState>()
        .calendars
        .lock()
        .await
        .remove_account(email, &pref)
        .await;
    save_app_state(window.app_handle()).await;
    Ok(())
}

#[tauri::command]
async fn disable_account(window: Window, email: String) -> Result<(), String> {
    println!("Disabled acct {}", &email);
    let _ = window
        .app_handle()
        .state::<AppState>()
        .calendars
        .lock()
        .await
        .disable_account(email)
        .await;
    save_app_state(window.app_handle()).await;
    Ok(())
}

#[tauri::command]
async fn enable_account(window: Window, email: String) -> Result<(), String> {
    println!("Enable acct {}", &email);
    let _ = window
        .app_handle()
        .state::<AppState>()
        .calendars
        .lock()
        .await
        .enable_account(email)
        .await;
    save_app_state(window.app_handle()).await;
    Ok(())
}

#[tauri::command]
async fn hide_calendar(window: Window, email: String, calendar_id: String) -> Result<(), String> {
    println!("Hide calendar: {} - {}", &email, &calendar_id);
    let handle = window.app_handle();
    let _ = handle
        .state::<AppState>()
        .preferences
        .lock()
        .await
        .hide_calendar(email.clone(), calendar_id)
        .await;

    let preferences = handle
        .state::<AppState>()
        .preferences
        .lock()
        .await
        .get_state();

    handle
        .state::<AppState>()
        .calendars
        .lock()
        .await
        .set_preferences(email, &preferences)
        .await;
    Ok(())
}

#[tauri::command]
async fn show_calendar(window: Window, email: String, calendar_id: String) -> Result<(), String> {
    println!("Hide calendar: {} - {}", &email, &calendar_id);
    let handle = window.app_handle();
    let _ = handle
        .state::<AppState>()
        .preferences
        .lock()
        .await
        .show_calendar(email.clone(), calendar_id)
        .await;

    let preferences = handle
        .state::<AppState>()
        .preferences
        .lock()
        .await
        .get_state();

    handle
        .state::<AppState>()
        .calendars
        .lock()
        .await
        .set_preferences(email, &preferences)
        .await;
    Ok(())
}

#[tauri::command]
async fn get_preferences(window: Window) -> Result<Preferences, String> {
    println!("Get Preferences");
    let preferences = window
        .app_handle()
        .state::<AppState>()
        .preferences
        .lock()
        .await
        .get_state();

    Ok(preferences)
}

async fn save_app_state(app_handle: AppHandle) {
    let storage_path = get_state_path(&app_handle).await;

    if !storage_path.is_ok() {
        return;
    }

    let storage_path = storage_path.unwrap();

    let tokens = app_handle
        .state::<AppState>()
        .calendars
        .lock()
        .await
        .get_tokens()
        .await;

    if let Ok(tokens) = tokens {
        println!("Auth tokens {:?}", &tokens);
        let tokens = tokens
            .iter()
            .map(|token| serde_json::json!({"token": token }))
            .collect::<Vec<serde_json::Value>>();
        println!("Data to save {:?}", &tokens);
        let open_file = fs::File::create(storage_path);
        if open_file.is_ok() {
            let mut file = open_file.unwrap();
            let mut bytes: Vec<u8> = Vec::new();
            serde_json::to_writer(&mut bytes, &tokens).unwrap();
            match file.write(&bytes) {
                Ok(_size) => println!("Token data saved"),
                Err(err) => {
                    println!("Error saving token response {:?}", err);
                }
            }
        }
    }
}

pub async fn get_state_path(app_handle: &AppHandle) -> std::io::Result<PathBuf> {
    let data_path =
        tauri::api::path::app_data_dir(&app_handle.config()).unwrap_or(PathBuf::default());
    let new_path: PathBuf = data_path.join("notor_accounts.json");

    let path = data_path.to_str().unwrap();

    let exists = tokio::fs::try_exists(path).await?;
    println!("Save Json token");
    if !exists {
        println!("Create data path {:?}", &data_path);
        match fs::create_dir(&data_path) {
            Ok(_) => println!("Dir created: {:?}", &data_path),
            Err(err) => println!("Error created data directory {:?}", err),
        }
    }

    Ok(new_path)
}

#[tokio::main]
async fn main() {
    let app = tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            app_loaded,
            logout,
            show_alert,
            dismiss_alert,
            schedule_events,
            list_accounts,
            remove_account,
            disable_account,
            enable_account,
            hide_calendar,
            show_calendar,
            get_preferences
        ])
        .on_system_tray_event(move |app, event| match event {
            SystemTrayEvent::RightClick { position, size, .. } => {
                let window = app.get_window("main").unwrap();
                let visible = window.is_visible().unwrap();
                if visible {
                    window.hide().unwrap();
                } else {
                    let window_size = window.outer_size().unwrap();
                    let physical_pos = PhysicalPosition {
                        x: position.x as i32 + (size.width as i32) / 2,
                        y: position.y as i32 - window_size.height as i32,
                    };

                    let _ = window.set_position(tauri::Position::Physical(physical_pos));
                    window.show().unwrap();
                    window.set_focus().unwrap();
                }
            }
            SystemTrayEvent::LeftClick {
                position: _,
                size: _,
                ..
            } => {
                println!("system tray received a left click");
            }
            SystemTrayEvent::DoubleClick {
                position: _,
                size: _,
                ..
            } => {
                println!("system tray received a double click");
            }
            SystemTrayEvent::MenuItemClick { id, .. } => {
                if id.as_str() == "quit" {
                    std::process::exit(0);
                } else if id.as_str() == "show_app" {
                    println!("show app");
                    let window = app.get_window("main").unwrap();
                    // let visible = window.is_visible().unwrap();
                    window.show().unwrap();
                    window.set_focus().unwrap();
                } else if id.as_str() == "settings" {
                    let result = open_auth_window(&app);
                    if result.is_err() {
                        // log error
                    }
                }
            }
            _ => {}
        })
        .on_window_event(|event| match event.event() {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                event.window().hide().unwrap();
                api.prevent_close();
            }
            tauri::WindowEvent::Focused(false) => {
                if event.window().label() == "main" {
                    event.window().hide().unwrap();
                }

                if event.window().label() == "alert" {
                    if !cfg!(debug_assertions) {
                        event.window().show().unwrap();
                        event.window().set_focus().unwrap();
                    };
                }
            }
            tauri::WindowEvent::Resized(size) => {
                if event.window().label() == "alert" {
                    println!("Resized {:?}", size);
                    let app = event.window().app_handle();
                    let state = app.state::<AppState>();
                    if let Ok(initial_size) = app.state::<AppState>().alert_size.lock() {
                        println!("Initial size {:?}", &initial_size);
                        event.window().set_size(initial_size.to_owned()).unwrap();
                        let position = state.alert_position.lock().unwrap();
                        println!("Initial position {:?}", &position);
                        event.window().set_position(position.to_owned()).unwrap();
                    };
                }
            }
            _ => {}
        })
        .setup(|app| {
            build_tray_app(app).unwrap();

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let window = app.get_window("main").unwrap();
            window.hide().expect("Failed to hide main window");

            #[cfg(target_os = "macos")]
            window.set_always_on_top(false).unwrap();

            let handle = app.handle();
            let shared_handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                // let shared_handle = handle.clone();
                match tauri::updater::builder(shared_handle).check().await {
                    Ok(update) => {
                        println!("Notor Update: {}", update.is_update_available());
                        if update.is_update_available() {
                            update.download_and_install().await.unwrap();
                        }
                    }
                    Err(e) => {
                        println!("Notor Update failed to get update: {}", e);
                    }
                }
            });
            // let boxed_handle = Box::new(handle);

            thread::spawn(move || {
                server::start(handle).unwrap();
            });

            let is_debug_mode = if cfg!(debug_assertions) { true } else { false };

            // Enable app auto launch
            let autostart = autostart::update(!is_debug_mode);
            if autostart.is_ok() {
                println!(
                    "Auto start {}",
                    if !is_debug_mode {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
            }

            Ok(())
        });

    app.run(tauri::generate_context!())
        .expect("error while running tauri application");
}
