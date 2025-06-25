use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub jwt_secret: String,
    pub host: String,
    pub port: u16,
    pub upload_dir: String,
    pub max_file_size: usize,
    pub allowed_origins: Vec<String>,

    // OAuth
    pub google_client_id: String,
    pub google_client_secret: String,
    pub apple_client_id: String,
    pub apple_team_id: Option<String>,
    pub apple_key_id: Option<String>,
    pub apple_private_key: Option<String>,

    // Email - SendGrid
    pub sendgrid_api_key: String,
    pub sendgrid_from_email: String,
    pub sendgrid_from_name: String,

    // SMS - Twilio
    pub twilio_account_sid: String,
    pub twilio_auth_token: String,
    pub twilio_phone_number: String,

    // App settings
    pub app_name: String,
    pub base_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, env::VarError> {
        Ok(Self {
            database_url: env::var("DATABASE_URL")?,
            redis_url: env::var("REDIS_URL")?,
            jwt_secret: env::var("JWT_SECRET")?,
            host: env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .unwrap_or(3000),
            upload_dir: env::var("UPLOAD_DIR").unwrap_or_else(|_| "./uploads".to_string()),
            max_file_size: env::var("MAX_FILE_SIZE")
                .unwrap_or_else(|_| "10485760".to_string())
                .parse()
                .unwrap_or(10485760),
            allowed_origins: env::var("ALLOWED_ORIGINS")
                .unwrap_or_else(|_| "http://localhost:3000".to_string())
                .split(',')
                .map(|s| s.trim().to_string())
                .collect(),

            // OAuth
            google_client_id: env::var("GOOGLE_CLIENT_ID").unwrap_or_default(),
            google_client_secret: env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default(),
            apple_client_id: env::var("APPLE_CLIENT_ID").unwrap_or_default(),
            apple_team_id: env::var("APPLE_TEAM_ID").ok(),
            apple_key_id: env::var("APPLE_KEY_ID").ok(),
            apple_private_key: env::var("APPLE_PRIVATE_KEY").ok(),

            // Email
            sendgrid_api_key: env::var("SENDGRID_API_KEY").unwrap_or_default(),
            sendgrid_from_email: env::var("SENDGRID_FROM_EMAIL")
                .unwrap_or_else(|_| "noreply@yourapp.com".to_string()),
            sendgrid_from_name: env::var("SENDGRID_FROM_NAME")
                .unwrap_or_else(|_| "Reddit Clone".to_string()),

            // SMS
            twilio_account_sid: env::var("TWILIO_ACCOUNT_SID").unwrap_or_default(),
            twilio_auth_token: env::var("TWILIO_AUTH_TOKEN").unwrap_or_default(),
            twilio_phone_number: env::var("TWILIO_PHONE_NUMBER").unwrap_or_default(),

            // App
            app_name: env::var("APP_NAME").unwrap_or_else(|_| "Reddit Clone".to_string()),
            base_url: env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string()),
        })
    }
}
