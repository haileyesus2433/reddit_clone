use axum::{RequestPartsExt, extract::FromRequestParts, http::request::Parts};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Bearer},
};
use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppState,
    error::{AppError, Result},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user_id
    pub username: String,
    pub exp: i64,
    pub iat: i64,
    pub jti: String, // JWT ID for session management
}

impl Claims {
    pub fn new(user_id: Uuid, username: String, jwt_secret: &str) -> Result<(String, Self)> {
        let now = Utc::now();
        let exp = now + Duration::hours(24);
        let jti = Uuid::new_v4().to_string();

        let claims = Self {
            sub: user_id.to_string(),
            username,
            exp: exp.timestamp(),
            iat: now.timestamp(),
            jti: jti.clone(),
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(jwt_secret.as_ref()),
        )?;

        Ok((token, claims))
    }

    pub fn verify(token: &str, jwt_secret: &str) -> Result<Self> {
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(jwt_secret.as_ref()),
            &Validation::default(),
        )?;

        Ok(token_data.claims)
    }
}

#[derive(Debug)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub username: String,
    pub jti: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| AppError::Authentication("Missing authorization header".to_string()))?;

        let claims = Claims::verify(bearer.token(), &state.config.jwt_secret)?;

        // Check if session is still valid in Redis
        if let Some(stored_user_id) = state.redis.get_session(&claims.jti).await? {
            if stored_user_id != claims.sub {
                return Err(AppError::Authentication("Invalid session".to_string()));
            }
        } else {
            return Err(AppError::Authentication("Session expired".to_string()));
        }

        let user_id = Uuid::parse_str(&claims.sub)
            .map_err(|_| AppError::Authentication("Invalid user ID in token".to_string()))?;

        Ok(AuthUser {
            user_id,
            username: claims.username,
            jti: claims.jti,
        })
    }
}

// Optional auth user (for endpoints that work with or without auth)
#[derive(Debug)]
pub struct OptionalAuthUser(pub Option<AuthUser>);

impl FromRequestParts<AppState> for OptionalAuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(OptionalAuthUser(Some(user))),
            Err(_) => Ok(OptionalAuthUser(None)),
        }
    }
}

// Password hashing utilities
pub fn hash_password(password: &str) -> Result<String> {
    let cost = 12;
    bcrypt::hash(password, cost).map_err(AppError::from)
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
    bcrypt::verify(password, hash).map_err(AppError::from)
}

// OAuth providers
#[derive(Debug, Deserialize)]
pub struct GoogleUserInfo {
    pub id: String,
    pub email: String,
    pub name: String,
    pub picture: String,
    pub verified_email: bool,
}

#[derive(Debug, Deserialize)]
pub struct AppleUserInfo {
    pub sub: String,
    pub email: String,
    pub email_verified: bool,
}

pub async fn get_google_user_info(access_token: &str) -> Result<GoogleUserInfo> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(access_token)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(AppError::Authentication("Invalid Google token".to_string()));
    }

    let user_info: GoogleUserInfo = response.json().await?;
    Ok(user_info)
}
