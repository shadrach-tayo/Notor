// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod server;

use crate::server::{open_alert_window, open_auth_window};
use app::utils::{EventGroups, get_date_time, get_human_readable_time, time_to_relative_format};
use app::types::{AppState, GoogleAuthToken};
use std::thread;
use google_calendar::types::Event;
use tauri::{
    CustomMenuItem, Manager, PhysicalPosition, Runtime, SystemTray, SystemTrayEvent,
    SystemTrayMenu, SystemTrayMenuItem, Window,
};
use app::autostart;


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
            .insert(
                event.id.clone(),
                event.to_owned()
            );
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

    let _ = open_auth_window(&handle).await;

    println!("User Logged out");
}

#[tauri::command]
async fn build_events<R: Runtime>(
    app: tauri::AppHandle<R>,
    events: EventGroups,
) -> Result<(), String> {
    let mut system_tray_menu = SystemTrayMenu::new();

    let mut ongoing_event_items: Vec<CustomMenuItem> = vec![];
    if !events.now.is_empty() {
        let end_time = time_to_relative_format(events.now.first().unwrap().clone().end.unwrap());

        let ongoing = CustomMenuItem::new("ongoing", format!("Ending {}", end_time))
            .native_image(tauri::NativeImage::StatusAvailable)
            .disabled();

        ongoing_event_items.push(ongoing);

        events.now.iter().for_each(|event| {
            let time = get_date_time(event);
            let time_str = get_human_readable_time(time);
            ongoing_event_items.push(CustomMenuItem::new(
                &event.id,
                format!("{} {}  {}", "   ", time_str, &event.summary),
            ))
        });

        for menu in ongoing_event_items.iter() {
            system_tray_menu = system_tray_menu.add_item(menu.to_owned());
        }
    }

    let mut upcoming_event_items: Vec<CustomMenuItem> = vec![];
    if !events.upcoming.is_empty() {
        let start_time = time_to_relative_format(events.upcoming.first().unwrap().clone().start.unwrap());
        // println!("Upcoming {:?} {start_time}", &events.upcoming.first().unwrap().summary);
        let upcoming = CustomMenuItem::new("upcoming", format!("Upcoming {}", start_time))
            .native_image(tauri::NativeImage::StatusPartiallyAvailable)
            .disabled();
        upcoming_event_items.push(upcoming);

        events.upcoming.iter().for_each(|event| {
            let time = get_date_time(event);
            let time_str = get_human_readable_time(time);
            upcoming_event_items.push(CustomMenuItem::new(
                &event.id,
                format!("{} {}  {}", "   ", time_str, &event.summary),
            ))
        });

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

        events.tomorrow.iter().for_each(|event| {
            let time = get_date_time(event);
            let time_str = get_human_readable_time(time);

            tomorrow_event_items.push(CustomMenuItem::new(
                &event.id,
                format!("{} {}  {}", "   ", time_str, &event.summary),
            ))
        });

        for menu in tomorrow_event_items.iter() {
            system_tray_menu = system_tray_menu.add_item(menu.to_owned());
        }
    }

    let quit = CustomMenuItem::new("quit", "Quit Notor app completely");
    let settings = CustomMenuItem::new("settings", "⚙️ Settings...");

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
    let settings = CustomMenuItem::new("settings", "Settings...");
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
        .invoke_handler(tauri::generate_handler![
            app_loaded,
            logout,
            build_events,
            show_alert,
            dismiss_alert,
            schedule_events
        ])
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
                    // let visible = window.is_visible().unwrap();
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
            // let boxed_handle = Box::new(handle);

            thread::spawn(move || {
                server::start(handle).unwrap();
            });

            let is_debug_mode = if cfg!(debug_assertions) { true } else { false };

            // Enable app auto launch
            let autostart = autostart::update(!is_debug_mode);
            if autostart.is_ok() {
                println!("Auto start {}", if !is_debug_mode { "enabled" } else { "disabled" });
            }

            Ok(())
        });

    app.run(tauri::generate_context!())
        .expect("error while running tauri application");
}
