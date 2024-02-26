use actix_web::{get, post, web, HttpResponse};
use google_calendar::{calendar_list, types::MinAccessRole, Client, ClientError};
use std::{
    fs,
    io::Write,
    ops::{Add, Mul},
    path::PathBuf,
};
use tauri::Manager;
use tokio;

use crate::server::{
    types::{AppState, GoogleAuthToken},
    utils::e500,
    TauriAppState, // GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET, GOOGLE_REDIRECT_URL,
};

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
        .lock()
        .unwrap()
        .state::<AppState>()
        .google_auth_credentials
        .lock()
        .unwrap()
        .clone();

    dbg!(&auth_token);
    let app_handle = app_state.app.lock().unwrap().clone();
    let auth_window = app_handle.get_window("auth");
    let main_window = app_handle.get_window("main");

    // UPDATE APP STATE WITH New Credentials
    // *app_state.app.lock().unwrap().state::<AppState>().google_auth_credentials.lock().unwrap() = data.clone();

    // save_auth_token(&body, app_handle).await;
    let data_path = tauri::api::path::app_data_dir(&app_handle.config());
    let mut token_path: PathBuf = PathBuf::from("");

    {
        let token = if let Some(path) = data_path {
            path.join("googleauthtoken.json")
        } else {
            "".into()
        };

        token_path = token.clone();
    };

    // drop the lock early as it is not used anywhere in this scope again;
    drop(app_handle);

    // check validity and refresh access token
    let app_config = app_state
        .app
        .lock()
        .unwrap()
        .state::<AppState>()
        .app_config
        .lock()
        .unwrap()
        .clone();
    println!("Auth refresh {:?}", &app_config);
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
            auth_token.expires_in = access_token.expires_in as u64;

            let now = chrono::Utc::now();

            let timestamp = now
                .timestamp()
                .add(client.expires_in().await.unwrap().as_millis() as i64);

            auth_token.expires_at = Some(timestamp as u64);

            // UPDATE APP STATE WITH New Credentials
            *app_state
                .app
                .lock()
                .unwrap()
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
                    serde_json::to_string(&auth_token).unwrap(),
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
    let now = chrono::Utc::now();

    let timestamp = now.timestamp().add(data.expires_in.mul(1000) as i64);

    data.expires_at = Some(timestamp as u64);

    dbg!(&data);
    let app_handle = app_state.app.lock().unwrap().clone();
    let auth_window = app_handle.get_window("auth");
    let main_window = app_handle.get_window("main");

    // UPDATE APP STATE WITH New Credentials
    *app_state
        .app
        .lock()
        .unwrap()
        .state::<AppState>()
        .google_auth_credentials
        .lock()
        .unwrap() = data.clone();

    // save_auth_token(&body, app_handle).await;
    let data_path = tauri::api::path::app_data_dir(&app_handle.config());

    // drop the lock early as it is not used anywhere in this scope again;
    drop(app_handle);
    // using app_handle again will lead to move error

    if data_path.is_some() {
        let data_path = data_path.unwrap();
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
        let data_path = &data_path.join("googleauthtoken.json");
        dbg!(&data_path);
        let mut file = fs::File::create(data_path)?;
        let mut bytes: Vec<u8> = Vec::new();
        serde_json::to_writer(&mut bytes, &data).unwrap();
        match file.write(&bytes) {
            Ok(_) => println!("Token data saved"),
            Err(err) => {
                println!("Error saving token response {:?}", err);
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
        dbg!(&body.body);
        Ok(HttpResponse::Ok().json(body.body))
    } else {
        match &response.err().unwrap() {
            ClientError::HttpError { error, .. } => Ok(HttpResponse::Ok().json(error)),
            _ => Ok(HttpResponse::Ok().json(serde_json::json!({ "error": "Client error"}))),
        }
    }
}
