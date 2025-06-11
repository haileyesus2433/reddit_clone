use axum::{Extension, extract::State, http::StatusCode, response::Json};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use sqlx::Row;

use crate::{
    AppState,
    auth::{AuthUser, Claims, get_google_user_info, hash_password, verify_password},
    error::{AppError, Result},
    models::{
        AuthProvider, CreateUserRequest, PasswordResetToken, PhoneVerificationCode, User,
        UserStatus,
    },
    services::auth_service,
};

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterRequest {
    #[validate(length(min = 3, max = 50))]
    pub username: String,
    #[validate(email)]
    pub email: Option<String>,
    #[validate(length(min = 10, max = 20))]
    pub phone: Option<String>,
    #[validate(length(min = 8))]
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct LoginRequest {
    pub username_or_email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct GoogleOAuthRequest {
    pub access_token: String,
}

#[derive(Debug, Deserialize)]
pub struct AppleOAuthRequest {
    pub id_token: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ForgotPasswordRequest {
    #[validate(email)]
    pub email: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ResetPasswordRequest {
    pub token: String,
    #[validate(length(min = 8))]
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyEmailRequest {
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyPhoneRequest {
    pub phone: String,
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserResponse,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub username: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub karma_points: i32,
    pub is_verified: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            email: user.email,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            karma_points: user.karma_points,
            is_verified: user.is_verified,
            created_at: user.created_at,
        }
    }
}

pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<Value>)> {
    // Validate input
    payload.validate()?;

    // Check if email or phone is provided
    if payload.email.is_none() && payload.phone.is_none() {
        return Err(AppError::BadRequest(
            "Either email or phone must be provided".to_string(),
        ));
    }

    // Rate limiting
    let rate_limit_key = format!(
        "register_attempt:{}",
        payload.email.as_deref().unwrap_or(&payload.username)
    );
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 5, 3600)
        .await?
    {
        return Err(AppError::RateLimit);
    }

    // Check if username already exists
    let existing_user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE username = $1 AND status != 'deleted'",
    )
    .bind(&payload.username)
    .fetch_optional(&state.db)
    .await?;

    if existing_user.is_some() {
        return Err(AppError::Conflict("Username already exists".to_string()));
    }

    // Check if email already exists (if provided)
    if let Some(ref email) = payload.email {
        let existing_email = sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE email = $1 AND status != 'deleted'",
        )
        .bind(email)
        .fetch_optional(&state.db)
        .await?;

        if existing_email.is_some() {
            return Err(AppError::Conflict("Email already exists".to_string()));
        }
    }

    // Check if phone already exists (if provided)
    if let Some(ref phone) = payload.phone {
        let existing_phone = sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE phone = $1 AND status != 'deleted'",
        )
        .bind(phone)
        .fetch_optional(&state.db)
        .await?;

        if existing_phone.is_some() {
            return Err(AppError::Conflict(
                "Phone number already exists".to_string(),
            ));
        }
    }

    // Hash password
    let password_hash = hash_password(&payload.password)?;

    // Create user
    let user_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    let user = sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (
            id, username, email, phone, password_hash, 
            auth_provider, status, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING *
        "#,
    )
    .bind(user_id)
    .bind(&payload.username)
    .bind(&payload.email)
    .bind(&payload.phone)
    .bind(&password_hash)
    .bind(AuthProvider::Email)
    .bind(UserStatus::Active)
    .bind(now)
    .bind(now)
    .fetch_one(&state.db)
    .await?;

    // Create user preferences with defaults
    sqlx::query(
        r#"
        INSERT INTO user_preferences (
            id, user_id, email_notifications, push_notifications,
            comment_reply_notifications, post_reply_notifications,
            mention_notifications, upvote_notifications,
            community_notifications, nsfw_content,
            created_at, updated_at
        )
        VALUES ($1, $2, true, true, true, true, true, false, true, false, $3, $4)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(now)
    .bind(now)
    .execute(&state.db)
    .await?;

    // Generate JWT token
    let (token, claims) = Claims::new(user.id, user.username.clone(), &state.config.jwt_secret)?;

    // Store session in Redis
    state
        .redis
        .store_session(&claims.jti, &user.id.to_string(), 86400)
        .await?;

    // Send verification email if email provided
    if payload.email.is_some() {
        // TODO: Implement email verification
        // auth_service::send_verification_email(&state, &user).await?;
    }

    // Send verification SMS if phone provided
    if payload.phone.is_some() {
        // TODO: Implement phone verification
        // auth_service::send_verification_sms(&state, &user).await?;
    }

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "message": "User registered successfully",
            "token": token,
            "user": UserResponse::from(user)
        })),
    ))
}

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<(StatusCode, Json<Value>)> {
    // Rate limiting
    let rate_limit_key = format!("login_attempt:{}", payload.username_or_email);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 10, 900)
        .await?
    {
        return Err(AppError::RateLimit);
    }

    // Find user by username or email
    let user = sqlx::query_as::<_, User>(
        r#"
        SELECT * FROM users 
        WHERE (username = $1 OR email = $1) 
        AND status = 'active'
        AND auth_provider = 'email'
        "#,
    )
    .bind(&payload.username_or_email)
    .fetch_optional(&state.db)
    .await?;

    let user = user.ok_or_else(|| AppError::Authentication("Invalid credentials".to_string()))?;

    // Verify password
    let password_hash = user
        .password_hash
        .clone()
        .ok_or_else(|| AppError::Authentication("Invalid credentials".to_string()))?;

    if !verify_password(&payload.password, &password_hash)? {
        return Err(AppError::Authentication("Invalid credentials".to_string()));
    }

    // Update last login
    sqlx::query("UPDATE users SET last_login_at = $1 WHERE id = $2")
        .bind(chrono::Utc::now())
        .bind(user.id)
        .execute(&state.db)
        .await?;

    // Generate JWT token
    let (token, claims) = Claims::new(user.id, user.username.clone(), &state.config.jwt_secret)?;

    // Store session in Redis
    state
        .redis
        .store_session(&claims.jti, &user.id.to_string(), 86400)
        .await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "message": "Login successful",
            "token": token,
            "user": UserResponse::from(user)
        })),
    ))
}

pub async fn logout(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<(StatusCode, Json<Value>)> {
    // Remove session from Redis
    state.redis.delete_session(&auth_user.jti).await?;

    // Invalidate session in database
    sqlx::query("UPDATE user_sessions SET expires_at = NOW() WHERE token_jti = $1")
        .bind(&auth_user.jti)
        .execute(&state.db)
        .await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "message": "Logout successful"
        })),
    ))
}

pub async fn refresh_token(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<(StatusCode, Json<Value>)> {
    // Get user from database
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 AND status = 'active'")
        .bind(auth_user.user_id)
        .fetch_optional(&state.db)
        .await?;

    let user = user.ok_or_else(|| AppError::Authentication("User not found".to_string()))?;

    // Generate new JWT token
    let (token, claims) = Claims::new(user.id, user.username.clone(), &state.config.jwt_secret)?;

    // Remove old session and store new one
    state.redis.delete_session(&auth_user.jti).await?;
    state
        .redis
        .store_session(&claims.jti, &user.id.to_string(), 86400)
        .await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "token": token,
            "user": UserResponse::from(user)
        })),
    ))
}

pub async fn google_oauth(
    State(state): State<AppState>,
    Json(payload): Json<GoogleOAuthRequest>,
) -> Result<(StatusCode, Json<Value>)> {
    // Get user info from Google
    let google_user = get_google_user_info(&payload.access_token).await?;

    // Check if user already exists
    let existing_user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE oauth_id = $1 AND auth_provider = 'google' AND status != 'deleted'"
    )
    .bind(&google_user.id)
    .fetch_optional(&state.db)
    .await?;

    let user = if let Some(user) = existing_user {
        // Update last login
        sqlx::query("UPDATE users SET last_login_at = $1 WHERE id = $2")
            .bind(chrono::Utc::now())
            .bind(user.id)
            .execute(&state.db)
            .await?;
        user
    } else {
        // Create new user
        let user_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        // Generate unique username from Google name
        let base_username = google_user
            .name
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>();

        let username = auth_service::generate_unique_username(&state.db, &base_username).await?;

        let user = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (
                id, username, email, display_name, avatar_url,
                auth_provider, oauth_id, email_verified, status,
                created_at, updated_at, last_login_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING *
            "#,
        )
        .bind(user_id)
        .bind(&username)
        .bind(&google_user.email)
        .bind(&google_user.name)
        .bind(&google_user.picture)
        .bind(AuthProvider::Google)
        .bind(&google_user.id)
        .bind(google_user.verified_email)
        .bind(UserStatus::Active)
        .bind(now)
        .bind(now)
        .bind(now)
        .fetch_one(&state.db)
        .await?;

        // Create user preferences
        sqlx::query(
            r#"
            INSERT INTO user_preferences (
                id, user_id, email_notifications, push_notifications,
                comment_reply_notifications, post_reply_notifications,
                mention_notifications, upvote_notifications,
                community_notifications, nsfw_content,
                created_at, updated_at
            )
            VALUES ($1, $2, true, true, true, true, true, false, true, false, $3, $4)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(now)
        .bind(now)
        .execute(&state.db)
        .await?;

        user
    };

    // Generate JWT token
    let (token, claims) = Claims::new(user.id, user.username.clone(), &state.config.jwt_secret)?;

    // Store session in Redis
    state
        .redis
        .store_session(&claims.jti, &user.id.to_string(), 86400)
        .await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "message": "Google OAuth successful",
            "token": token,
            "user": UserResponse::from(user)
        })),
    ))
}

pub async fn apple_oauth(
    State(state): State<AppState>,
    Json(payload): Json<AppleOAuthRequest>,
) -> Result<(StatusCode, Json<Value>)> {
    // TODO: Implement Apple OAuth verification
    // This would involve verifying the Apple ID token
    // For now, return not implemented
    Err(AppError::Internal(
        "Apple OAuth not yet implemented".to_string(),
    ))
}

pub async fn forgot_password(
    State(state): State<AppState>,
    Json(payload): Json<ForgotPasswordRequest>,
) -> Result<(StatusCode, Json<Value>)> {
    payload.validate()?;

    // Rate limiting
    let rate_limit_key = format!("forgot_password:{}", payload.email);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 3, 3600)
        .await?
    {
        return Err(AppError::RateLimit);
    }

    // Find user by email
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE email = $1 AND status = 'active' AND auth_provider = 'email'",
    )
    .bind(&payload.email)
    .fetch_optional(&state.db)
    .await?;

    if let Some(user) = user {
        // Generate reset token
        let token = Uuid::new_v4().to_string();
        let expires_at = chrono::Utc::now() + chrono::Duration::hours(1);

        // Store reset token
        sqlx::query(
            r#"
            INSERT INTO password_reset_tokens (id, user_id, token, expires_at, created_at)
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

        // TODO: Send password reset email
        // auth_service::send_password_reset_email(&state, &user, &token).await?;
    }

    // Always return success to prevent email enumeration
    Ok((
        StatusCode::OK,
        Json(json!({
            "message": "If the email exists, a password reset link has been sent"
        })),
    ))
}

pub async fn reset_password(
    State(state): State<AppState>,
    Json(payload): Json<ResetPasswordRequest>,
) -> Result<(StatusCode, Json<Value>)> {
    payload.validate()?;

    // Find valid reset token
    let reset_token = sqlx::query_as::<_, PasswordResetToken>(
        r#"
        SELECT * FROM password_reset_tokens 
        WHERE token = $1 AND expires_at > NOW() AND used_at IS NULL
        "#,
    )
    .bind(&payload.token)
    .fetch_optional(&state.db)
    .await?;

    let reset_token = reset_token
        .ok_or_else(|| AppError::BadRequest("Invalid or expired reset token".to_string()))?;

    // Hash new password
    let password_hash = hash_password(&payload.new_password)?;

    // Update user password
    sqlx::query("UPDATE users SET password_hash = $1, updated_at = $2 WHERE id = $3")
        .bind(&password_hash)
        .bind(chrono::Utc::now())
        .bind(reset_token.user_id)
        .execute(&state.db)
        .await?;

    // Mark token as used
    sqlx::query("UPDATE password_reset_tokens SET used_at = $1 WHERE id = $2")
        .bind(chrono::Utc::now())
        .bind(reset_token.id)
        .execute(&state.db)
        .await?;

    // Invalidate all user sessions
    sqlx::query("UPDATE user_sessions SET expires_at = NOW() WHERE user_id = $1")
        .bind(reset_token.user_id)
        .execute(&state.db)
        .await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "message": "Password reset successful"
        })),
    ))
}

pub async fn verify_email(
    State(state): State<AppState>,
    Json(payload): Json<VerifyEmailRequest>,
) -> Result<(StatusCode, Json<Value>)> {
    // Find valid verification token
    let verification = sqlx::query(
        r#"
        SELECT user_id FROM email_verification_tokens 
        WHERE token = $1 AND expires_at > NOW() AND used_at IS NULL
        "#,
    )
    .bind(&payload.token)
    .fetch_optional(&state.db)
    .await?;

    let user_id: Uuid = verification
        .ok_or_else(|| AppError::BadRequest("Invalid or expired verification token".to_string()))?
        .get("user_id");

    // Update user as verified
    sqlx::query("UPDATE users SET email_verified = true, updated_at = $1 WHERE id = $2")
        .bind(chrono::Utc::now())
        .bind(user_id)
        .execute(&state.db)
        .await?;

    // Mark token as used
    sqlx::query("UPDATE email_verification_tokens SET used_at = $1 WHERE token = $2")
        .bind(chrono::Utc::now())
        .bind(&payload.token)
        .execute(&state.db)
        .await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "message": "Email verified successfully"
        })),
    ))
}

pub async fn verify_phone(
    State(state): State<AppState>,
    Json(payload): Json<VerifyPhoneRequest>,
) -> Result<(StatusCode, Json<Value>)> {
    // Find valid verification code
    let verification = sqlx::query_as::<_, PhoneVerificationCode>(
        r#"
        SELECT * FROM phone_verification_codes 
        WHERE phone = $1 AND code = $2 AND expires_at > NOW() AND used_at IS NULL
        "#,
    )
    .bind(&payload.phone)
    .bind(&payload.code)
    .fetch_optional(&state.db)
    .await?;

    let verification = verification
        .ok_or_else(|| AppError::BadRequest("Invalid or expired verification code".to_string()))?;

    // Update user as phone verified
    sqlx::query("UPDATE users SET phone_verified = true, updated_at = $1 WHERE phone = $2")
        .bind(chrono::Utc::now())
        .bind(&payload.phone)
        .execute(&state.db)
        .await?;

    // Mark code as used
    sqlx::query("UPDATE phone_verification_codes SET used_at = $1 WHERE id = $2")
        .bind(chrono::Utc::now())
        .bind(verification.id)
        .execute(&state.db)
        .await?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "message": "Phone verified successfully"
        })),
    ))
}
