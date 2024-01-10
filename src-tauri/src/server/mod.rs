use std::sync::Mutex;
use actix_cors::Cors;
use actix_web::{App, HttpServer, web, middleware, http::header};
use tauri::AppHandle;
use crate::server::types::TauriAppState;

mod handlers;
mod types;

#[tokio::main]
pub async fn start(app: AppHandle) -> std::io::Result<()> {
    let tauri_app = web::Data::new(TauriAppState {
        app: Mutex::new(app)
    });

    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin("http://localhost:3000")
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
    })
        .bind(("127.0.0.1", 4875))?
        .run()
        .await
}