use crate::server::types::{AppCredentials, AppState, TauriAppState};
use actix_cors::Cors;
use actix_web::{http::header, middleware, web, App, HttpServer};
use google_calendar::Client;
// use oauth2::url::Position;
use std::{fs, ops::Add, path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Manager};

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

pub async fn open_alert_window(app: &AppHandle) -> Result<(), String> {
    if let Some(auth_window) = app.get_window("alert") {
        auth_window.show().unwrap();
    }
    let window = tauri::WindowBuilder::new(
        app,
        "alert",
        tauri::WindowUrl::App("alert".into()),
    )
        .center()
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

    let mut token_path: PathBuf = PathBuf::from("");

    let mut token = {
        let token = if let Some(path) = data_path {
            path.join("googleauthtoken.json")
        } else {
            "".into()
        };

        token_path = token.clone();

        match fs::read_to_string(token) {
            Ok(token) => {
                serde_json::from_str::<GoogleAuthToken>(&token).map_err(|_| "".to_string())
            }
            Err(_) => Err("".to_string()),
        }
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
                std::fs::write(&token_path, &bytes).map_err(|e| {
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
        app: Mutex::new(app),
    });


    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin("http://localhost:3000")
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
        .run()
        .await
}
