use serde::Deserialize;
use tauri::Manager;
// use time::OffsetDateTime;

use actix_web::{get, post, web};

use crate::server::TauriAppState;


#[get("/api/health-check")]
pub async fn health() -> actix_web::Result<String> {
    println!("Health check");
    Ok("running".to_string())
}


#[derive(Clone, Debug, Deserialize)]
pub struct RawJsonToken {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: String,
    scope: String,
    // extra_fields: EmptyExtraTokenFields,
}


#[post("/api/google_auth")]
pub async fn google_login(body: web::Bytes, app_state: web::Data<TauriAppState>) -> actix_web::Result<String> {
    println!("Raw Json token");
    let data = serde_json::from_slice::<RawJsonToken>(&body)?;
    dbg!(&data.access_token);
    let app_handle = app_state.app.lock().unwrap();
    let auth_window = app_handle.get_window("auth");
    let main_window = app_handle.get_window("main");
   
    if let Some(window) = auth_window {
        let _ = window.close();
    }
    if let Some(main) = main_window {
        let _ = main.show();
    }

    // Todo: 
    // Store TokenResponse using DiskTokenStorage
    // Close Sign in desktop window 
    // Open Calendar view desktop window

    Ok("running".to_string())
}