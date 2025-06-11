use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Authorization error: {0}")]
    Authorization(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Internal server error: {0}")]
    Internal(String),

    #[error("JWT error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error("Bcrypt error: {0}")]
    Bcrypt(#[from] bcrypt::BcryptError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    // #[error("Image processing error: {0}")]
    // Image(#[from] image::ImageError),
    #[error("HTTP client error: {0}")]
    HttpClient(#[from] reqwest::Error),

    #[error("Rate limit exceeded")]
    RateLimit,

    #[error("Content too large")]
    ContentTooLarge,

    #[error("Unsupported media type")]
    UnsupportedMediaType,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AppError::Database(ref e) => {
                tracing::error!("Database error: {:?}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
            AppError::Redis(ref e) => {
                tracing::error!("Redis error: {:?}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
            AppError::Validation(ref message) => (StatusCode::BAD_REQUEST, message.as_str()),
            AppError::Authentication(ref message) => (StatusCode::UNAUTHORIZED, message.as_str()),
            AppError::Authorization(ref message) => (StatusCode::FORBIDDEN, message.as_str()),
            AppError::NotFound(ref message) => (StatusCode::NOT_FOUND, message.as_str()),
            AppError::Conflict(ref message) => (StatusCode::CONFLICT, message.as_str()),
            AppError::BadRequest(ref message) => (StatusCode::BAD_REQUEST, message.as_str()),
            AppError::Internal(ref message) => {
                tracing::error!("Internal error: {}", message);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
            AppError::Jwt(ref e) => {
                tracing::error!("JWT error: {:?}", e);
                (StatusCode::UNAUTHORIZED, "Invalid token")
            }
            AppError::Bcrypt(ref e) => {
                tracing::error!("Bcrypt error: {:?}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
            AppError::Io(ref e) => {
                tracing::error!("IO error: {:?}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
            // AppError::Image(ref e) => {
            //     tracing::error!("Image processing error: {:?}", e);
            //     (StatusCode::BAD_REQUEST, "Invalid image format")
            // }
            AppError::HttpClient(ref e) => {
                tracing::error!("HTTP client error: {:?}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "External service error")
            }
            AppError::RateLimit => (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded"),
            AppError::ContentTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "Content too large"),
            AppError::UnsupportedMediaType => {
                (StatusCode::UNSUPPORTED_MEDIA_TYPE, "Unsupported media type")
            }
        };

        let body = Json(json!({
            "error": error_message,
            "status": status.as_u16()
        }));

        (status, body).into_response()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

// Validation helper
impl From<validator::ValidationErrors> for AppError {
    fn from(errors: validator::ValidationErrors) -> Self {
        let error_messages: Vec<String> = errors
            .field_errors()
            .iter()
            .flat_map(|(field, errors)| {
                errors.iter().map(move |error| {
                    format!(
                        "{}: {}",
                        field,
                        error.message.as_ref().unwrap_or(&"Invalid value".into())
                    )
                })
            })
            .collect();

        AppError::Validation(error_messages.join(", "))
    }
}
