use crate::{
    AppState,
    error::{AppError, Result},
    models::User,
};
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

pub async fn send_password_reset_email(state: &AppState, user: &User, token: &str) -> Result<()> {
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
