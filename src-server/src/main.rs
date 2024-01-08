use actix_cors::Cors;
use actix_web::{http::header, middleware, web, App, HttpServer};
// use anyhow::Context;
use dotenv::dotenv;
use lazy_static::lazy_static;
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, ClientId, ClientSecret, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, TokenUrl,
};
use std::fmt::{Debug, Display};
use tokio::task::JoinError;

lazy_static! {
    static ref OAUTH2_CHALLENGE: web::Data<(PkceCodeChallenge, PkceCodeVerifier)> = web::Data::new(PkceCodeChallenge::new_random_sha256());
}

mod handlers {

    use actix_web::body::BoxBody;
    use actix_web::http::StatusCode;
    use actix_web::{get, web, HttpResponse, ResponseError};
    use oauth2::basic::BasicClient;
    use oauth2::{
        AuthorizationCode, CsrfToken, PkceCodeChallenge, PkceCodeVerifier, RequestTokenError,
        Scope, TokenResponse,
    };
    use serde_derive::{Deserialize, Serialize};

    #[get("/health-check")]
    pub async fn health_check() -> HttpResponse {
        println!("Health check");
        HttpResponse::Ok().finish()
    }

    #[derive(Serialize, Debug)]
    struct GoogleLoginResponse {
        pub url: String,
    }

    #[get("/login/google")]
    pub async fn google_login(
        oauth2_client: web::Data<BasicClient>,
        oauth2_challenge: web::Data<(PkceCodeChallenge, PkceCodeVerifier)>,
    ) -> HttpResponse {
        println!("Auth api");

        let (authorization_url, _) = oauth2_client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/calendar".to_string(),
            ))
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/plus.me".to_string(),
            ))
            .add_extra_param("access_type", "offline")
            .set_pkce_challenge(oauth2_challenge.0.clone())
            .url();

        let response = GoogleLoginResponse {
            url: authorization_url.to_string(),
        };
        dbg!(&response);
        HttpResponse::Ok().json(response)
    }

    #[derive(thiserror::Error, Debug)]
    pub enum OauthCallbackError {
        #[error("{0}")]
        AuthenticationError(String),

        #[error(transparent)]
        UnexpectedError(#[from] anyhow::Error),
    }

    impl ResponseError for OauthCallbackError {
        fn status_code(&self) -> actix_web::http::StatusCode {
            match self {
                OauthCallbackError::AuthenticationError(_) => StatusCode::UNAUTHORIZED,
                Self::UnexpectedError(_) => StatusCode::BAD_REQUEST,
            }
        }

        fn error_response(&self) -> HttpResponse<BoxBody> {
            match self {
                OauthCallbackError::AuthenticationError(err) => {
                    HttpResponse::build(StatusCode::UNAUTHORIZED).json(&serde_json::json!({ "error": err }))
                }
                Self::UnexpectedError(_) => HttpResponse::build(StatusCode::BAD_REQUEST).finish(),
            }
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct CallbackParam {
        pub code: String,
        pub state: String,
        pub scope: String,
    }

    #[derive(Serialize, Debug)]
    struct GoogleAuthResponse {
        pub token: String,
        pub secret: String,
    }

    #[get("/oauth2callback/google")]
    pub async fn google_oauth_callback(
        oauth2_client: web::Data<BasicClient>,
        oauth2_challenge: web::Data<(PkceCodeChallenge, PkceCodeVerifier)>,
        params: web::Query<CallbackParam>,
    ) -> Result<HttpResponse, actix_web::Error> {
        dbg!(&params);
        let code = AuthorizationCode::new(params.code.clone());
        let state = CsrfToken::new(params.state.clone());
        let _scope = params.scope.clone();
        let verifier_string = oauth2_challenge.1.secret();

        // Exchange the code with a token.
        let token = oauth2_client
            .exchange_code(code)
            .set_pkce_verifier(PkceCodeVerifier::new(verifier_string.clone()))
            .request_async(oauth2::reqwest::async_http_client)
            .await
            .map_err(|err| {
                dbg!(&err);
                match err {
                    RequestTokenError::ServerResponse(error) => {
                        OauthCallbackError::AuthenticationError(error.to_string())
                    }
                    _ => OauthCallbackError::UnexpectedError(err.into()),
                }
            })?;
        // let token_response = token.request_async(oauth2_client).await?;
        dbg!(&token);
        // let response = GoogleAuthResponse {
        //     token: token.access_token().secret().to_string(),
        //     secret: state.secret().to_string(),
        // };
        // dbg!(&response);
        Ok(HttpResponse::Ok().json(token))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    println!("Running Main...");
    // let pkce_challenge = &OAUTH2_CHALLENGE.0;
    // let pkce_verifier = &OAUTH2_CHALLENGE.1;

    let server = HttpServer::new(|| {
        let google_client_id = ClientId::new(
            std::env::var("GOOGLE_CLIENT_ID").expect("Failed to read google client id"),
        );
        let google_client_secret = ClientSecret::new(
            std::env::var("GOOGLE_CLIENT_SECRET").expect("Failed to read google secret"),
        );

        let authorisation_url =
            AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
                .expect("Invalid authorisation endpoint");
        let token_url = TokenUrl::new("https://www.googleapis.com/oauth2/v3/token".to_string())
            .expect("Invalid token endpoint");

        let client = BasicClient::new(
            google_client_id,
            Some(google_client_secret),
            authorisation_url,
            Some(token_url),
        )
        .set_redirect_uri(
            RedirectUrl::new("http://localhost:3000/auth".to_string())
                .expect("Invalid redirect URL"),
        );
        let wrapped_client = web::Data::new(client);

        let cors = Cors::default()
            .allowed_origin("http://localhost:3000")
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec![
                header::CONTENT_TYPE,
                header::AUTHORIZATION,
                header::ACCEPT,
            ]);

        println!("Starting server...");
        App::new()
            .app_data(wrapped_client.clone())
            .app_data(OAUTH2_CHALLENGE.clone())
            .wrap(middleware::Logger::default())
            .wrap(cors)
            .service(handlers::health_check)
            .service(handlers::google_login)
            .service(handlers::google_oauth_callback)
    })
    .bind(("127.0.0.1", 4876))?
    .run();
    let application_task = tokio::spawn(server);
    tokio::select! {
        o = application_task => report_exit("Application API", o)
    }
    Ok(())
}

fn report_exit(task_name: &str, outcome: Result<Result<(), impl Debug + Display>, JoinError>) {
    match outcome {
        Ok(Ok(())) => {
            println!("{} has exited", task_name)
        }
        Ok(Err(e)) => {
            println!("App failed to run {:?}", e)
            // println!(error.cause_chain = ?e, error.message = %e, "{} has failed", task_name)
        }
        Err(e) => {
            println!("App failed to complete {:?}", e)
            // println!(error.cause_chain = ?e, error.message = %e, "{} task failed to complete", task_name)
        }
    }
}
