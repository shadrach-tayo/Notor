// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod server;

use std::thread;
use tauri::{
    CustomMenuItem, Manager, PhysicalPosition, SystemTray, SystemTrayEvent, SystemTrayMenu,
};

fn main() {
    let quit = CustomMenuItem::new("quit".to_string(), "Quit Notor app");
    let app_tray = SystemTrayMenu::new().add_item(quit);
    let system_tray = SystemTray::new()
        .with_menu(app_tray)
        .with_menu_on_left_click(false);

    let app = tauri::Builder::default()
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
            },
            _ => {}
        })
        .setup(|app| {
            #[warn(unused_variables)]
            let _auth_window = tauri::WindowBuilder::new(
                app,
                "auth",
                tauri::WindowUrl::External("http://localhost:3000/signin".parse().unwrap()),
            )
            .center()
            .title("Notor".to_string())
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .inner_size(1048f64, 650f64)
            .build()
            .expect("Failed to create auth window");

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

    app
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
