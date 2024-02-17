// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod server;

use crate::server::{open_auth_window, types::GoogleAuthToken};
use chrono::TimeZone;
use chrono_humanize;
use serde::Deserialize;
use server::types::AppState;
use std::thread;
use tauri::{
    CustomMenuItem, Manager, PhysicalPosition, Runtime, SystemTray, SystemTrayEvent,
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
async fn logout(window: Window) {
    let handle = window.app_handle();

    let data_path = tauri::api::path::app_data_dir(&handle.config());

    let data_path = if let Some(path) = data_path {
        path.join("googleauthtoken.json")
    } else {
        "".into()
    };

    let _ = std::fs::remove_file(data_path);

    let _ = open_auth_window(&handle).await;

    println!("User Logged out");
}

#[derive(Debug, Deserialize)]
struct EventGroups {
    now: Vec<google_calendar::types::Event>,
    upcoming: Vec<google_calendar::types::Event>,
    tomorrow: Vec<google_calendar::types::Event>,
}

fn get_human_end_time(event: google_calendar::types::Event) -> String {
    let dt = {
        let t = event.end.clone().unwrap().date_time;

        if let Some(t) = t {
            t
        } else {
            let a = chrono::NaiveDateTime::parse_from_str(
                &event.end.clone().unwrap().date.unwrap().to_string(),
                "%Y-%m-%dT%H:%M:%S",
            )
            .unwrap();
            let r = chrono::Local.from_local_datetime(&a).unwrap();
            println!("From naive date time {:?}", r);
            r.into()
        }
    };

    chrono_humanize::HumanTime::from(dt).to_string()
}

fn get_human_start_time(event: google_calendar::types::Event) -> String {
    let dt = {
        let t = event.start.clone().unwrap().date_time;

        if let Some(t) = t {
            t
        } else {
            let a = chrono::NaiveDateTime::parse_from_str(
                &event
                    .start
                    .clone()
                    .unwrap()
                    .date
                    .unwrap()
                    .clone()
                    .to_string(),
                "%Y-%m-%dT%H:%M:%S",
            )
            .unwrap();
            let r = chrono::Local.from_local_datetime(&a).unwrap();
            println!("From naive date time {:?}", r);
            r.into()
        }
    };

    chrono_humanize::HumanTime::from(dt).to_string()
}

#[tauri::command]
async fn build_events<R: Runtime>(
    app: tauri::AppHandle<R>,
    // window: tauri::Window<R>,
    events: EventGroups,
) -> Result<(), String> {
    println!(
        "Events now {:?}, upcoming {:?}, tomorrow {:?}",
        events.now.len(),
        events.upcoming.len(),
        events.tomorrow.len()
    );

    let mut system_tray_menu = SystemTrayMenu::new();

    let mut ongoing_event_items: Vec<CustomMenuItem> = vec![];
    if !events.now.is_empty() {
        let end_time = get_human_end_time(events.now.first().unwrap().clone());

        let ongoing = CustomMenuItem::new("ongoing", format!("Ending {}", end_time))
            .native_image(tauri::NativeImage::StatusAvailable)
            .disabled();

        ongoing_event_items.push(ongoing);

        events.now.iter().for_each(|event| {
            ongoing_event_items.push(CustomMenuItem::new(
                &event.id,
                format!("{} {}", "▕   ", &event.summary),
            ))
        });

        for menu in ongoing_event_items.iter() {
            system_tray_menu = system_tray_menu.add_item(menu.to_owned());
        }
    }

    let mut upcoming_event_items: Vec<CustomMenuItem> = vec![];
    if !events.upcoming.is_empty() {
        let start_time = get_human_start_time(events.upcoming.first().unwrap().clone());

        let upcoming = CustomMenuItem::new("upcoming", format!("Upcoming {}", start_time))
            .native_image(tauri::NativeImage::StatusPartiallyAvailable)
            .disabled();
        upcoming_event_items.push(upcoming);

        events.upcoming.iter().for_each(|event| {
            upcoming_event_items.push(CustomMenuItem::new(
                &event.id,
                format!("{} {}", "▕   ", &event.summary),
            ))
        });

        for menu in upcoming_event_items.iter() {
            system_tray_menu = system_tray_menu.add_item(menu.to_owned());
        }
    }

    let mut tomorrow_event_items: Vec<CustomMenuItem> = vec![];
    if !events.tomorrow.is_empty() {
        let upcoming = CustomMenuItem::new("tomorrow", "Tomorrow")
            .native_image(tauri::NativeImage::StatusUnavailable)
            .disabled();
        tomorrow_event_items.push(upcoming);

        events.tomorrow.iter().for_each(|event| {
            tomorrow_event_items.push(CustomMenuItem::new(
                &event.id,
                format!("{} {}", "▕   ", &event.summary),
            ))
        });

        for menu in tomorrow_event_items.iter() {
            system_tray_menu = system_tray_menu.add_item(menu.to_owned());
        }
    }

    let quit = CustomMenuItem::new("quit", "Quit Notor app completely             ❌");
    let settings = CustomMenuItem::new("settings", "⚙️ Settings..."); //.native_image(tauri::NativeImage::SmartBadge);

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
        .build(&app);

    Ok(())
}

fn build_tray_app(app_handle: &tauri::App) -> Result<(), ()> {
    let quit = CustomMenuItem::new("quit", "Quit Notor app completely             ❌");
    let settings = CustomMenuItem::new("settings", "Settings..."); //.native_image(tauri::NativeImage::SmartBadge);
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

#[tokio::main]
async fn main() {
    let app = tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![app_loaded, logout, build_events])
        // .system_tray(system_tray)
        .on_system_tray_event(move |app, event| match event {
            SystemTrayEvent::RightClick { position, size, .. } => {
                println!("system tray received a right click");
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
                    let visible = window.is_visible().unwrap();
                    window.show().unwrap();
                    window.set_focus().unwrap();
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
            }
            _ => {}
        })
        .setup(|app| {
            // let tray_handle = SystemTray::new().build(app)?;
            build_tray_app(app).unwrap();

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let window = app.get_window("main").unwrap();
            window.hide().expect("Failed to hide main window");

            #[cfg(target_os = "macos")]
            window.set_always_on_top(false).unwrap();

            let handle = app.handle();
            let boxed_handle = Box::new(handle);

            thread::spawn(move || {
                server::start(*boxed_handle).unwrap();
            });

            Ok(())
        });

    app.run(tauri::generate_context!())
        .expect("error while running tauri application");
}
