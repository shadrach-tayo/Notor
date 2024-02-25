use serde_derive::Deserialize;
use serde_aux::field_attributes::deserialize_number_from_string;

#[derive(Deserialize, Clone, Debug)]
pub struct Settings {
    pub application: ApplicationSettings,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ApplicationSettings {
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    pub base_url: String,
    // #[serde(rename = "GOOGLE_CLIENT_ID")]
    pub google_client_id: String,
    // #[serde(rename = "GOOGLE_CLIENT_SECRET")]
    pub google_client_secret: String,
    // #[serde(rename = "GOOGLE_CALENDAR_API_KEY")]
    pub google_calendar_api_key: String,
    // #[serde(rename = "GOOGLE_REDIRECT_URL")]
    pub google_redirect_url: String,
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
        .add_source(config::File::from(
            config_directory.join("base.yaml"),
        ))
        .add_source(config::File::from(config_directory.join(env_file)))
        .add_source(
            config::Environment::default()
                .prefix("APP")
                .prefix_separator("_")
                .separator("__"),
        ).build()?;

    settings.try_deserialize::<Settings>()
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
                use either `development` or `production`", other
            )),
        }
    }
}