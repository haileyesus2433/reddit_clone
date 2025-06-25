use reqwest::Client;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use crate::{
    config::Config,
    error::{AppError, Result},
};

#[derive(Debug, Clone)]
pub struct EmailService {
    client: Client,
    api_key: String,
    from_email: String,
    from_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmailTemplate {
    pub subject: String,
    pub html_content: String,
    pub text_content: String,
}

#[derive(Debug, Serialize)]
struct SendGridEmail {
    personalizations: Vec<Personalization>,
    from: EmailAddress,
    subject: String,
    content: Vec<Content>,
}

#[derive(Debug, Serialize)]
struct Personalization {
    to: Vec<EmailAddress>,
    dynamic_template_data: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize)]
struct EmailAddress {
    email: String,
    name: Option<String>,
}

#[derive(Debug, Serialize)]
struct Content {
    #[serde(rename = "type")]
    content_type: String,
    value: String,
}

impl EmailService {
    pub fn new(config: &Config) -> Self {
        Self {
            client: Client::new(),
            api_key: config.sendgrid_api_key.clone(),
            from_email: config.sendgrid_from_email.clone(),
            from_name: config.sendgrid_from_name.clone(),
        }
    }

    pub async fn send_email(
        &self,
        to_email: &str,
        to_name: Option<&str>,
        subject: &str,
        html_content: &str,
        text_content: &str,
    ) -> Result<()> {
        tracing::info!("Sending email to {}", to_email);

        let email = SendGridEmail {
            personalizations: vec![Personalization {
                to: vec![EmailAddress {
                    email: to_email.to_string(),
                    name: to_name.map(|s| s.to_string()),
                }],
                dynamic_template_data: None,
            }],
            from: EmailAddress {
                email: self.from_email.clone(),
                name: Some(self.from_name.clone()),
            },
            subject: subject.to_string(),
            content: vec![
                Content {
                    content_type: "text/plain".to_string(),
                    value: text_content.to_string(),
                },
                Content {
                    content_type: "text/html".to_string(),
                    value: html_content.to_string(),
                },
            ],
        };

        let response = self
            .client
            .post("https://api.sendgrid.com/v3/mail/send")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&email)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            tracing::error!("SendGrid API error: {}", error_text);
            return Err(AppError::Internal(format!(
                "SendGrid API error: {}",
                error_text
            )));
        }

        tracing::info!("Email sent successfully to {}", to_email);

        Ok(())
    }

    pub async fn send_verification_email(
        &self,
        to_email: &str,
        username: &str,
        verification_token: &str,
        base_url: &str,
    ) -> Result<()> {
        let verification_url = format!("{}/verify-email?token={}", base_url, verification_token);

        let subject = "Verify your email address";
        let html_content = format!(
            r#"
            <!DOCTYPE html>
            <html>
            <head>
                <meta charset="utf-8">
                <title>Email Verification</title>
                <style>
                    body {{ font-family: Arial, sans-serif; line-height: 1.6; color: #333; }}
                    .container {{ max-width: 600px; margin: 0 auto; padding: 20px; }}
                    .header {{ background-color: #ff4500; color: white; padding: 20px; text-align: center; }}
                    .content {{ padding: 20px; background-color: #f9f9f9; }}
                    .button {{ display: inline-block; padding: 12px 24px; background-color: #ff4500; color: white; text-decoration: none; border-radius: 4px; margin: 20px 0; }}
                    .footer {{ padding: 20px; text-align: center; color: #666; font-size: 12px; }}
                </style>
            </head>
            <body>
                <div class="container">
                    <div class="header">
                        <h1>Welcome to Reddit Clone!</h1>
                    </div>
                    <div class="content">
                        <h2>Hi {}!</h2>
                        <p>Thank you for signing up! Please verify your email address to complete your registration.</p>
                        <p>Click the button below to verify your email:</p>
                        <a href="{}" class="button">Verify Email Address</a>
                        <p>Or copy and paste this link into your browser:</p>
                        <p><a href="{}">{}</a></p>
                        <p>This link will expire in 24 hours.</p>
                        <p>If you didn't create an account, you can safely ignore this email.</p>
                    </div>
                    <div class="footer">
                        <p>© 2024 Reddit Clone. All rights reserved.</p>
                    </div>
                </div>
            </body>
            </html>
            "#,
            username, verification_url, verification_url, verification_url
        );

        let text_content = format!(
            r#"
            Hi {}!

            Thank you for signing up for Reddit Clone! Please verify your email address to complete your registration.

            Click this link to verify your email: {}

            This link will expire in 24 hours.

            If you didn't create an account, you can safely ignore this email.

            © 2024 Reddit Clone. All rights reserved.
            "#,
            username, verification_url
        );

        self.send_email(
            to_email,
            Some(username),
            subject,
            &html_content,
            &text_content,
        )
        .await
    }

    pub async fn send_password_reset_email(
        &self,
        to_email: &str,
        username: &str,
        reset_token: &str,
        base_url: &str,
    ) -> Result<()> {
        let reset_url = format!("{}/reset-password?token={}", base_url, reset_token);

        let subject = "Reset your password";
        let html_content = format!(
            r#"
            <!DOCTYPE html>
            <html>
            <head>
                <meta charset="utf-8">
                <title>Password Reset</title>
                <style>
                    body {{ font-family: Arial, sans-serif; line-height: 1.6; color: #333; }}
                    .container {{ max-width: 600px; margin: 0 auto; padding: 20px; }}
                    .header {{ background-color: #ff4500; color: white; padding: 20px; text-align: center; }}
                    .content {{ padding: 20px; background-color: #f9f9f9; }}
                    .button {{ display: inline-block; padding: 12px 24px; background-color: #ff4500; color: white; text-decoration: none; border-radius: 4px; margin: 20px 0; }}
                    .footer {{ padding: 20px; text-align: center; color: #666; font-size: 12px; }}
                    .warning {{ background-color: #fff3cd; border: 1px solid #ffeaa7; padding: 15px; border-radius: 4px; margin: 15px 0; }}
                </style>
            </head>
            <body>
                <div class="container">
                    <div class="header">
                        <h1>Password Reset Request</h1>
                    </div>
                    <div class="content">
                        <h2>Hi {}!</h2>
                        <p>We received a request to reset your password for your Reddit Clone account.</p>
                        <p>Click the button below to reset your password:</p>
                        <a href="{}" class="button">Reset Password</a>
                        <p>Or copy and paste this link into your browser:</p>
                        <p><a href="{}">{}</a></p>
                        <div class="warning">
                            <strong>Important:</strong>
                            <ul>
                                <li>This link will expire in 1 hour</li>
                                <li>You can only use this link once</li>
                                <li>If you didn't request this reset, please ignore this email</li>
                            </ul>
                        </div>
                    </div>
                    <div class="footer">
                        <p>© 2024 Reddit Clone. All rights reserved.</p>
                    </div>
                </div>
            </body>
            </html>
            "#,
            username, reset_url, reset_url, reset_url
        );

        let text_content = format!(
            r#"
            Hi {}!

            We received a request to reset your password for your Reddit Clone account.

            Click this link to reset your password: {}

            Important:
            - This link will expire in 1 hour
            - You can only use this link once
            - If you didn't request this reset, please ignore this email

            © 2024 Reddit Clone. All rights reserved.
            "#,
            username, reset_url
        );

        self.send_email(
            to_email,
            Some(username),
            subject,
            &html_content,
            &text_content,
        )
        .await
    }

    pub async fn send_welcome_email(&self, to_email: &str, username: &str) -> Result<()> {
        let subject = "Welcome to Reddit Clone!";
        let html_content = format!(
            r#"
            <!DOCTYPE html>
            <html>
            <head>
                <meta charset="utf-8">
                <title>Welcome</title>
                <style>
                    body {{ font-family: Arial, sans-serif; line-height: 1.6; color: #333; }}
                    .container {{ max-width: 600px; margin: 0 auto; padding: 20px; }}
                    .header {{ background-color: #ff4500; color: white; padding: 20px; text-align: center; }}
                    .content {{ padding: 20px; background-color: #f9f9f9; }}
                    .tips {{ background-color: #e8f5e8; border: 1px solid #4caf50; padding: 15px; border-radius: 4px; margin: 15px 0; }}
                    .footer {{ padding: 20px; text-align: center; color: #666; font-size: 12px; }}
                </style>
            </head>
            <body>
                <div class="container">
                    <div class="header">
                        <h1>Welcome to Reddit Clone!</h1>
                    </div>
                    <div class="content">
                        <h2>Hi {}!</h2>
                        <p>Your email has been verified successfully! Welcome to our community.</p>
                        <div class="tips">
                            <h3>Getting Started:</h3>
                            <ul>
                                <li>Join communities that interest you</li>
                                <li>Create your first post</li>
                                <li>Engage with other users through comments</li>
                                <li>Upvote content you like</li>
                                <li>Customize your profile</li>
                            </ul>
                        </div>
                        <p>Happy browsing!</p>
                    </div>
                    <div class="footer">
                        <p>© 2024 Reddit Clone. All rights reserved.</p>
                    </div>
                </div>
            </body>
            </html>
            "#,
            username
        );

        let text_content = format!(
            r#"
            Hi {}!

            Your email has been verified successfully! Welcome to our Reddit Clone community.

            Getting Started:
            - Join communities that interest you
            - Create your first post
            - Engage with other users through comments
            - Upvote content you like
            - Customize your profile

            Happy browsing!

            © 2024 Reddit Clone. All rights reserved.
            "#,
            username
        );

        self.send_email(
            to_email,
            Some(username),
            subject,
            &html_content,
            &text_content,
        )
        .await
    }
}
