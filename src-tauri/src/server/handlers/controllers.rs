use actix_web::{get, HttpResponse, post, web};
use google_calendar::{calendar_list, Client, ClientError, types::MinAccessRole};
use std::{
    fs,
    io::Write,
    path::PathBuf,
};
use std::time::{SystemTime, UNIX_EPOCH};
use chrono::DateTime;
use tauri::Manager;
use tokio;

use crate::server::{
    TauriAppState,
    utils::e500,
};
use app::types::{AppState, GoogleAuthToken};
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

    dbg!(&auth_token);
    let app_handle = &app_state.app;
    let auth_window = app_handle.get_window("auth");
    let main_window = app_handle.get_window("main");

    let data_path = tauri::api::path::app_data_dir(&app_handle.config());
    let token_path: PathBuf = data_path.unwrap_or_else(|| PathBuf::from("")).join("googleauthtoken.json");

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
            dbg!(&access_token);
            auth_token.access_token = access_token.access_token;
            auth_token.expires_in = access_token.expires_in;


            let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("cannot retrieve system time");
            let expiry_date = chrono::DateTime::from_timestamp(now.as_secs() as i64 + access_token.expires_in, now.subsec_nanos()).unwrap_or(DateTime::default());
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
                main.emit(
                    "GOOGLE_AUTH_CREDENTIALS",
                    &auth_token,
                )
                    .unwrap();
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

    let now = SystemTime::now().duration_since(UNIX_EPOCH).expect("cannot retrieve system time");
    let expiry_date = chrono::DateTime::from_timestamp(now.as_secs() as i64 + data.expires_in, now.subsec_nanos()).unwrap_or(DateTime::default());
    let expiry_date = with_local_timezone(expiry_date);

    data.expires_at = Some(expiry_date.timestamp());

    dbg!(&data);
    let auth_window = &app_state.app.get_window("auth");
    let main_window = &app_state.app.get_window("main");

    // UPDATE APP STATE WITH New Credentials
    // *app_state
    //     .app
    //     .state::<AppState>()
    //     .google_auth_credentials
    //     .lock()
    //     .unwrap() = data.clone();

    // save_auth_token(&body, app_handle).await;
    let app_handle = &app_state.app;
    let storage_path = tauri::api::path::app_data_dir(&app_handle.config());

    if storage_path.is_some() {
        let data_path = tauri::api::path::app_data_dir(&app_handle.config()).unwrap_or(PathBuf::default());
        let data_path: PathBuf = data_path.join("notor_accounts.json");

        // let data_path = data_path.unwrap();
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

        dbg!(&data_path);

        println!("Locked---------+++++++");
        app_state
            .app
            .state::<AppState>()
            .calendars
            .lock()
            .await
            .add_account(data.clone())
            .await
            .unwrap();


        let auth_tokens = app_state
            .app
            .state::<AppState>()
            .calendars
            .lock()
            .await
            .get_tokens()
            .await;

        if let Ok(auth_tokens) = auth_tokens {
            println!("Auth tokens {:?}", &auth_tokens);
            let auth_tokens = auth_tokens.iter().map(|token| serde_json::json!({"token": token })).collect::<Vec<serde_json::Value>>();
            println!("Data to save {:?}", &auth_tokens);
            let mut file = fs::File::create(data_path)?;
            let mut bytes: Vec<u8> = Vec::new();
            serde_json::to_writer(&mut bytes, &auth_tokens).unwrap();
            // file.write_all(&mut bytes)?;
            match file.write(&bytes) {
                Ok(_size) => println!("Token data saved"),
                Err(err) => {
                    println!("Error saving token response {:?}", err);
                }
            }
        }
    } else {
        println!("No data path found");
    }

    if let Some(window) = auth_window {
        let _ = window.close();
    }

    if let Some(main) = main_window {
        main.emit(
            "GOOGLE_AUTH_CREDENTIALS",
            data.clone(),
        )
            .unwrap();
    }

    let client = Client::new(
        "",
        "",
        "",
        data.access_token,
        data.refresh_token.unwrap_or("".to_string()),
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
