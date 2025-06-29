use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    error::{AppError, Result},
};

#[derive(Debug, Clone)]
pub struct SmsService {
    client: Client,
    account_sid: String,
    auth_token: String,
    from_phone: String,
}

#[derive(Debug, Serialize)]
struct TwilioSmsRequest {
    #[serde(rename = "To")]
    to: String,
    #[serde(rename = "From")]
    from: String,
    #[serde(rename = "Body")]
    body: String,
}

#[derive(Debug, Deserialize)]
struct TwilioSmsResponse {
    sid: String,
    status: String,
    error_code: Option<String>,
    error_message: Option<String>,
}

impl SmsService {
    pub fn new(config: &Config) -> Self {
        Self {
            client: Client::new(),
            account_sid: config.twilio_account_sid.clone(),
            auth_token: config.twilio_auth_token.clone(),
            from_phone: config.twilio_phone_number.clone(),
        }
    }

    pub async fn send_sms(&self, to_phone: &str, message: &str) -> Result<String> {
        tracing::info!("Sending SMS to {}", to_phone);
        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
            self.account_sid
        );

        let _auth_header = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", self.account_sid, self.auth_token));

        let form_data = [
            ("To", to_phone),
            ("From", &self.from_phone),
            ("Body", message),
        ];

        let response = self
            .client
            .post(&url)
            .basic_auth(&self.account_sid, Some(&self.auth_token))
            .form(&form_data)
            .send()
            .await?;

        if response.status().is_success() {
            let sms_response: TwilioSmsResponse = response.json().await?;
            tracing::info!("SMS sent to {}", to_phone);
            Ok(sms_response.sid)
        } else {
            let error_text = response.text().await.unwrap_or_default();
            tracing::error!("Twilio API error: {}", error_text);
            Err(AppError::Internal(format!(
                "Twilio API error: {}",
                error_text
            )))
        }
    }

    pub async fn send_verification_code(
        &self,
        to_phone: &str,
        code: &str,
        app_name: &str,
    ) -> Result<String> {
        let message = format!(
            "Your {} verification code is: {}. This code will expire in 10 minutes. Do not share this code with anyone.",
            app_name, code
        );

        self.send_sms(to_phone, &message).await
    }

    pub async fn send_login_code(
        &self,
        to_phone: &str,
        code: &str,
        app_name: &str,
    ) -> Result<String> {
        let message = format!(
            "Your {} login code is: {}. This code will expire in 10 minutes. If you didn't request this, please ignore this message.",
            app_name, code
        );

        self.send_sms(to_phone, &message).await
    }

    pub async fn send_password_reset_code(
        &self,
        to_phone: &str,
        code: &str,
        app_name: &str,
    ) -> Result<String> {
        let message = format!(
            "Your {} password reset code is: {}. This code will expire in 10 minutes. If you didn't request this, please ignore this message.",
            app_name, code
        );

        self.send_sms(to_phone, &message).await
    }

    pub async fn send_security_alert(
        &self,
        to_phone: &str,
        app_name: &str,
        action: &str,
    ) -> Result<String> {
        let message = format!(
            "{} Security Alert: {} was performed on your account. If this wasn't you, please secure your account immediately.",
            app_name, action
        );

        self.send_sms(to_phone, &message).await
    }

    pub async fn send_notification_sms(
        &self,
        to_phone: &str,
        username: &str,
        title: &str,
    ) -> Result<String> {
        let message = format!(
            "Reddit Clone: {} - {}. Check your notifications for details.",
            title, username
        );

        self.send_sms(to_phone, &message).await
    }

    pub async fn send_mention_notification(
        &self,
        to_phone: &str,
        mentioner_username: &str,
    ) -> Result<String> {
        let message = format!(
            "Reddit Clone: {} mentioned you in a comment. Check the app to see what they said!",
            mentioner_username
        );

        self.send_sms(to_phone, &message).await
    }

    pub async fn send_urgent_notification(&self, to_phone: &str, title: &str) -> Result<String> {
        let message = format!(
            "Reddit Clone URGENT: {}. Please check your account immediately.",
            title
        );

        self.send_sms(to_phone, &message).await
    }
}
