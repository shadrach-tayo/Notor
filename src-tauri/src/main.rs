// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod server;

use std::thread;
use server::types::AppState;
use tauri::{
    CustomMenuItem, Manager, PhysicalPosition, SystemTray, SystemTrayEvent, SystemTrayMenu, Window,
};
use crate::server::types::GoogleAuthToken;
// remember to call `.manage(MyState::default())`
#[tauri::command]
async fn app_loaded(window: Window, state: tauri::State<'_, AppState>) -> Result<GoogleAuthToken, String> {
    println!("App loaded Event {}", window.label());
    let credentials = state.google_auth_credentials.lock().unwrap().clone();
    // window.emit("GOOGLE_AUTH_CREDENTIALS", credentials).unwrap();
    Ok(credentials)
}

#[tokio::main]
async fn main() {
    let quit = CustomMenuItem::new("quit".to_string(), "Quit Notor app");
    let app_tray = SystemTrayMenu::new().add_item(quit);
    let system_tray = SystemTray::new()
        .with_menu(app_tray)
        .with_menu_on_left_click(false);

    // TODO: Add handler to listen to frontend loaded event and emit token auth event
    let app = tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![app_loaded])
        .system_tray(system_tray)
        .on_system_tray_event(move |app, event| match event {
            SystemTrayEvent::LeftClick { position, size, .. } => {
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
            SystemTrayEvent::RightClick {
                position: _,
                size: _,
                ..
            } => {
                println!("system tray received a right click");
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
            // TODO: remove main window from tauri-config and only show when signed in

            // auth_window
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
