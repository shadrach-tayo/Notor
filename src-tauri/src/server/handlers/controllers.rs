use actix_web::{get, post, web, HttpResponse};
use chrono::DateTime;
use google_calendar::{calendar_list, types::MinAccessRole, Client, ClientError};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, path::PathBuf};
use tauri::Manager;
use tokio;

use crate::server::{utils::e500, TauriAppState};
use crate::state::AppState;
use app::types::GoogleAuthToken;
use app::utils::with_local_timezone;

#[get("/api/health-check")]
pub async fn health() -> actix_web::Result<String> {
    println!("Health check");
    Ok("running".to_string())
}

#[post("/api/google_auth/refresh")]
pub async fn google_auth_refresh(
    app_state: web::Data<TauriAppState>,
) -> actix_web::Result<HttpResponse, actix_web::Error> {
    let mut auth_token = app_state
        .app
        .state::<AppState>()
        .google_auth_credentials
        .lock()
        .unwrap()
        .clone();

    let app_handle = &app_state.app;
    let auth_window = app_handle.get_window("auth");
    let main_window = app_handle.get_window("main");

    let data_path = tauri::api::path::app_data_dir(&app_handle.config());
    let token_path: PathBuf = data_path
        .unwrap_or_else(|| PathBuf::from(""))
        .join("googleauthtoken.json");

    // check validity and refresh access token
    let app_config = app_state
        .app
        .state::<AppState>()
        .app_config
        .lock()
        .unwrap()
        .clone();

    let mut client = Client::new(
        // GOOGLE_CLIENT_ID,
        app_config.google_client_id,
        // GOOGLE_CLIENT_SECRET,
        app_config.google_client_secret,
        // GOOGLE_REDIRECT_URL,
        app_config.google_redirect_url,
        auth_token.access_token.clone(),
        auth_token.refresh_token.clone().unwrap_or("".to_string()),
    );
    let client = client.set_auto_access_token_refresh(true);

    let expired = client.is_expired().await.unwrap_or(true);
    if expired {
        let access_token = client.refresh_access_token().await;

        if let Ok(access_token) = access_token {
            println!("refreshed token");
            auth_token.access_token = access_token.access_token;
            auth_token.expires_in = access_token.expires_in;

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("cannot retrieve system time");
            let expiry_date = chrono::DateTime::from_timestamp(
                now.as_secs() as i64 + access_token.expires_in,
                now.subsec_nanos(),
            )
            .unwrap_or(DateTime::default());
            let expiry_date = with_local_timezone(expiry_date);
            auth_token.expires_at = Some(expiry_date.timestamp());

            // UPDATE APP STATE WITH New Credentials
            *app_state
                .app
                .state::<AppState>()
                .google_auth_credentials
                .lock()
                .unwrap() = auth_token.clone();

            if let Some(window) = auth_window {
                let _ = window.close();
            }

            if let Some(main) = main_window {
                main.emit("GOOGLE_AUTH_CREDENTIALS", &auth_token).unwrap();
            }

            let mut bytes: Vec<u8> = Vec::new();
            serde_json::to_writer(&mut bytes, &auth_token).unwrap();
            std::fs::write(&token_path, &bytes).map_err(|e| {
                println!("Error writing refresh token to file");
                e500(e.to_string())
            })?;
        } else {
            let err = access_token.err().unwrap();
            println!("Error refreshing token {:?}", err);
            // let _ = open_auth_window(app).await;
        }
    };

    Ok(HttpResponse::Ok().json(serde_json::json!(auth_token)))
}

#[post("/api/google_auth")]
pub async fn google_login(
    body: web::Bytes,
    app_state: web::Data<TauriAppState>,
) -> actix_web::Result<HttpResponse, actix_web::Error> {
    let mut data = serde_json::from_slice::<GoogleAuthToken>(&body)?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("cannot retrieve system time");
    let expiry_date = chrono::DateTime::from_timestamp(
        now.as_secs() as i64 + data.expires_in,
        now.subsec_nanos(),
    )
    .unwrap_or(DateTime::default());
    let expiry_date = with_local_timezone(expiry_date);

    data.expires_at = Some(expiry_date.timestamp());

    let auth_window = &app_state.app.get_window("auth");
    let main_window = &app_state.app.get_window("main");

    // save_auth_token(&body, app_handle).await;
    let app_handle = &app_state.app;
    let storage_path = tauri::api::path::app_data_dir(&app_handle.config());

    if storage_path.is_some() {
        let data_path =
            tauri::api::path::app_data_dir(&app_handle.config()).unwrap_or(PathBuf::default());

        let exists = tokio::fs::try_exists(&data_path).await?;
        if !exists {
            println!("Create storage data path {:?}", &data_path);
            match fs::create_dir(&data_path) {
                Ok(_) => println!("Dir created: {:?}", &data_path),
                Err(err) => println!("Error creating data directory {:?}", err),
            }
        }

        let data_path: PathBuf = data_path.join("notor_accounts.json");
        dbg!(&data_path);

        {
            let state = app_handle.state::<AppState>();
            let pref = state.preferences.lock().await.clone();
            let calendars = state.calendars.clone();
            if !calendars.add_account(data.clone(), &pref) {
                println!("Google login had no account email");
            }
            if let Err(err) = calendars.save() {
                println!("Error saving accounts: {err}");
            }
        }

        if let Some(google) = app_state
            .app
            .state::<AppState>()
            .google
            .lock()
            .unwrap()
            .clone()
        {
            let now = app_state.app.state::<AppState>().calendars.clock().now();
            if let Ok(creds) = google.keeper().get(now).await {
                *app_state.app.state::<AppState>().app_config.lock().unwrap() = creds;
            }
        }
    } else {
        println!("No data path found");
    }

    if let Some(window) = auth_window {
        let _ = window.close();
    }

    if let Some(main) = main_window {
        main.emit("GOOGLE_AUTH_CREDENTIALS", data.clone()).unwrap();
    }

    let config = app_state
        .app
        .state::<AppState>()
        .app_config
        .lock()
        .unwrap()
        .clone();
    let client = Client::new(
        config.google_client_id,
        config.google_client_secret,
        config.google_redirect_url,
        data.access_token,
        data.refresh_token.unwrap_or_default(),
    );
    let calendar_list = calendar_list::CalendarList::new(client);
    let response = calendar_list
        .list(20, MinAccessRole::FreeBusyReader, "", true, true)
        .await;

    if let Ok(body) = response {
        // dbg!(&body.body);
        Ok(HttpResponse::Ok().json(body.body))
    } else {
        match &response.err().unwrap() {
            ClientError::HttpError { error, .. } => Ok(HttpResponse::Ok().json(error)),
            _ => Ok(HttpResponse::Ok().json(serde_json::json!({ "error": "Client error"}))),
        }
    }
}
