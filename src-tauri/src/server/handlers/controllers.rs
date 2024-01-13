use actix_web::{get, post, web, HttpResponse};
use google_calendar::{calendar_list, types::MinAccessRole, Client, ClientError};
use std::{fs, io::Write, ops::{Add, Mul}};
use tauri::Manager;
use tokio;

use crate::server::{TauriAppState, types::{GoogleAuthToken, AppState}};

#[get("/api/health-check")]
pub async fn health() -> actix_web::Result<String> {
    println!("Health check");
    Ok("running".to_string())
}

#[post("/api/google_auth")]
pub async fn google_login(
    body: web::Bytes,
    app_state: web::Data<TauriAppState>,
) -> actix_web::Result<HttpResponse, actix_web::Error> {
   
    let mut data = serde_json::from_slice::<GoogleAuthToken>(&body)?;
    let now = chrono::Utc::now();

    let timestamp = now
        .timestamp()
        .add(data.expires_in.mul(1000) as i64);

    data.expires_at = Some(timestamp as u64);

    
    dbg!(&data);
    let app_handle = app_state.app.lock().unwrap().clone();
    let auth_window = app_handle.get_window("auth");
    let main_window = app_handle.get_window("main");
    
    // UPDATE APP STATE WITH New Credentials
    *app_state.app.lock().unwrap().state::<AppState>().google_auth_credentials.lock().unwrap() = data.clone();

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
        main.emit("GOOGLE_AUTH_CREDENTIALS", serde_json::to_string(&data).unwrap()).unwrap();
    }

    let client = Client::new("", "", "", data.access_token, data.refresh_token);
    let calendar_list = calendar_list::CalendarList::new(client);
    let response = calendar_list
        .list(20, MinAccessRole::FreeBusyReader, "", true, true)
        .await;

    if let Ok(body) = response {
        dbg!(&body.body);
        Ok(HttpResponse::Ok().json(body.body))
    } else {
        // dbg!(&response.err());
        // let err = response.err().unwrap();
        match &response.err().unwrap() {
            ClientError::HttpError { error, .. } => Ok(HttpResponse::Ok().json(error)),
            _ => Ok(HttpResponse::Ok().json(serde_json::json!({ "error": "Client error"}))),
        }
    }
}