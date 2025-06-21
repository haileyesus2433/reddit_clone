use crate::{
    AppState,
    error::{AppError, Result},
    models::User,
};
use oauth2::{
    AccessToken, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, TokenUrl, reqwest,
};
use oauth2::{basic::BasicClient, url};
use std::error::Error;

use sqlx::PgPool;
use uuid::Uuid;

pub async fn generate_unique_username(db: &PgPool, base_username: &str) -> Result<String> {
    let mut username = base_username.to_string();
    let mut counter = 0;

    loop {
        let existing =
            sqlx::query("SELECT id FROM users WHERE username = $1 AND status != 'deleted'")
                .bind(&username)
                .fetch_optional(db)
                .await?;

        if existing.is_none() {
            return Ok(username);
        }

        counter += 1;
        username = format!("{}{}", base_username, counter);

        // Prevent infinite loop
        if counter > 9999 {
            return Err(AppError::Internal(
                "Could not generate unique username".to_string(),
            ));
        }
    }
}

pub async fn send_verification_email(state: &AppState, user: &User) -> Result<()> {
    // Generate verification token
    let token = Uuid::new_v4().to_string();
    let expires_at = chrono::Utc::now() + chrono::Duration::hours(24);

    // Store verification token
    sqlx::query(
        r#"
        INSERT INTO email_verification_tokens (id, user_id, token, expires_at, created_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(user.id)
    .bind(&token)
    .bind(expires_at)
    .bind(chrono::Utc::now())
    .execute(&state.db)
    .await?;

    // TODO: Send actual email using SMTP
    // For now, just log the token (in production, remove this)
    tracing::info!("Email verification token for {}: {}", user.username, token);

    Ok(())
}

pub async fn send_verification_sms(state: &AppState, user: &User) -> Result<()> {
    if let Some(ref phone) = user.phone {
        // Generate verification code
        let code = generate_verification_code();
        let expires_at = chrono::Utc::now() + chrono::Duration::minutes(10);

        // Store verification code
        sqlx::query(
            r#"
            INSERT INTO phone_verification_codes (id, phone, code, expires_at, created_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(phone)
        .bind(&code)
        .bind(expires_at)
        .bind(chrono::Utc::now())
        .execute(&state.db)
        .await?;

        // TODO: Send actual SMS using Twilio
        // For now, just log the code (in production, remove this)
        tracing::info!("SMS verification code for {}: {}", phone, code);
    }

    Ok(())
}

pub async fn send_password_reset_email(_state: &AppState, user: &User, token: &str) -> Result<()> {
    // TODO: Send actual password reset email
    // For now, just log the token (in production, remove this)
    tracing::info!("Password reset token for {}: {}", user.username, token);

    Ok(())
}

fn generate_verification_code() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    format!("{:06}", rng.random_range(100000..999999))
}

type GoogleOAuthClient = oauth2::Client<
    oauth2::StandardErrorResponse<oauth2::basic::BasicErrorResponseType>,
    oauth2::StandardTokenResponse<oauth2::EmptyExtraTokenFields, oauth2::basic::BasicTokenType>,
    oauth2::StandardTokenIntrospectionResponse<
        oauth2::EmptyExtraTokenFields,
        oauth2::basic::BasicTokenType,
    >,
    oauth2::StandardRevocableToken,
    oauth2::StandardErrorResponse<oauth2::RevocationErrorResponseType>,
    oauth2::EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;

pub struct GoogleOAuthService {
    client: GoogleOAuthClient,
    http_client: reqwest::Client,
}

impl GoogleOAuthService {
    pub fn new(
        client_id: &str,
        client_secret: &str,
        redirect_uri: &str,
    ) -> std::result::Result<Self, Box<dyn Error>> {
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        let client = BasicClient::new(ClientId::new(client_id.to_string()))
            .set_client_secret(ClientSecret::new(client_secret.to_string()))
            .set_auth_uri(AuthUrl::new(
                "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            )?)
            .set_token_uri(TokenUrl::new(
                "https://www.googleapis.com/oauth2/v4/token".to_string(),
            )?)
            .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string())?);

        Ok(Self {
            client,
            http_client,
        })
    }

    pub fn get_authorization_url(&self) -> (url::Url, CsrfToken) {
        self.client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/userinfo.email".to_string(),
            ))
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/userinfo.profile".to_string(),
            ))
            .url()
    }

    pub async fn exchange_code_for_token(
        &self,
        code: &str,
    ) -> std::result::Result<AccessToken, Box<dyn Error>> {
        let token_result = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .request_async(&self.http_client)
            .await?;

        Ok(token_result.access_token().clone())
    }
}
