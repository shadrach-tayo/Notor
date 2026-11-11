use serde::de::{self, Deserializer, Visitor};
use serde_aux::field_attributes::deserialize_number_from_string;
use serde_derive::{Deserialize, Serialize};
use std::fmt;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    pub application: ApplicationSettings,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApplicationSettings {
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    pub base_url: String,
    /// Empty when the variable is missing. Startup validation names it.
    #[serde(default)]
    pub google_client_id: String,
    #[serde(default)]
    pub google_client_secret: String,
    #[serde(default)]
    pub google_calendar_api_key: String,
    #[serde(default)]
    pub google_redirect_url: String,
    /// Old desktop builds still send the secret to Google. New builds send
    /// `X-Notor-Token-Refresh: server` and never receive it. Default off.
    /// `configuration/production.yaml` turns it on for the rollout.
    #[serde(default, deserialize_with = "deserialize_bool_loose")]
    pub serve_client_secret_to_legacy_clients: bool,
}

pub fn get_configuration() -> Result<Settings, config::ConfigError> {
    let base_path = std::env::current_dir().expect("Failed to determine current directory");
    let config_directory = base_path.join("configuration");

    let environment: Environment = std::env::var("APP_ENVIRONMENT")
        .unwrap_or_else(|_| "development".into())
        .try_into()
        .expect("Failed to parse APP_ENVIRONMENT");
    let env_file = format!("{}.yaml", environment.as_str());
    let settings = config::Config::builder()
        .add_source(config::File::from(config_directory.join("base.yaml")))
        .add_source(config::File::from(config_directory.join(env_file)))
        .add_source(
            config::Environment::default()
                .prefix("APP")
                .prefix_separator("_")
                .separator("__"),
        )
        .build()?;

    settings.try_deserialize::<Settings>()
}

/// Every problem, so an empty `.env.example` names each variable at once.
pub fn validate_settings(settings: &Settings) -> Vec<String> {
    let mut problems = Vec::new();
    if settings.application.google_client_id.trim().is_empty() {
        problems.push(
            "application.google_client_id (APP_APPLICATION__GOOGLE_CLIENT_ID) is not set"
                .to_string(),
        );
    }
    if settings.application.google_client_secret.trim().is_empty() {
        problems.push(
            "application.google_client_secret (APP_APPLICATION__GOOGLE_CLIENT_SECRET) is not set"
                .to_string(),
        );
    }
    let redirect = settings.application.google_redirect_url.trim();
    if redirect.is_empty() {
        problems.push(
            "application.google_redirect_url (APP_APPLICATION__GOOGLE_REDIRECT_URL) is not set"
                .to_string(),
        );
    } else if !is_absolute_http_url(redirect) {
        problems.push(
            "application.google_redirect_url (APP_APPLICATION__GOOGLE_REDIRECT_URL) is not a full http(s) URL"
                .to_string(),
        );
    }
    problems
}

pub fn is_absolute_http_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https") && url.host().is_some()
}

fn deserialize_bool_loose<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    struct BoolVisitor;
    impl Visitor<'_> for BoolVisitor {
        type Value = bool;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a boolean")
        }
        fn visit_bool<E: de::Error>(self, value: bool) -> Result<bool, E> {
            Ok(value)
        }
        fn visit_str<E: de::Error>(self, value: &str) -> Result<bool, E> {
            match value {
                "1" | "true" | "True" | "TRUE" => Ok(true),
                "0" | "false" | "False" | "FALSE" | "" => Ok(false),
                other => Err(E::custom(format!("invalid boolean {other}"))),
            }
        }
        fn visit_u64<E: de::Error>(self, value: u64) -> Result<bool, E> {
            Ok(value != 0)
        }
        fn visit_i64<E: de::Error>(self, value: i64) -> Result<bool, E> {
            Ok(value != 0)
        }
    }
    deserializer.deserialize_any(BoolVisitor)
}

pub enum Environment {
    Development,
    Production,
}

impl Environment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Environment::Production => "production",
            Environment::Development => "development",
        }
    }
}

impl TryFrom<String> for Environment {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.to_lowercase().as_str() {
            "development" => Ok(Self::Development),
            "production" => Ok(Self::Production),
            other => Err(format!(
                "{} is not a supported environment, \
                use either `development` or `production`",
                other
            )),
        }
    }
}
