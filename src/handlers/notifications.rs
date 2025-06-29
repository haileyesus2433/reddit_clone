use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    AppState, auth::AuthUser, error::Result, models::MarkNotificationsReadRequest,
    services::notification_service::NotificationService,
};

#[derive(Debug, Deserialize)]
pub struct NotificationQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub unread_only: Option<bool>,
}

pub async fn get_notifications(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(params): Query<NotificationQuery>,
) -> Result<Json<Value>> {
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0);
    let unread_only = params.unread_only.unwrap_or(false);

    let notification_service = NotificationService::new(
        state.db.clone(),
        state.redis.clone(),
        state.email_service.clone(),
        state.sms_service.clone(),
    );

    let notifications = notification_service
        .get_user_notifications(auth_user.user_id, limit, offset, unread_only)
        .await?;

    let unread_count = notification_service
        .get_unread_count(auth_user.user_id)
        .await?;

    Ok(Json(json!({
        "notifications": notifications,
        "unread_count": unread_count,
        "pagination": {
            "limit": limit,
            "offset": offset,
            "has_more": notifications.len() == limit as usize
        }
    })))
}

pub async fn get_unread_count(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<Value>> {
    let notification_service = NotificationService::new(
        state.db.clone(),
        state.redis.clone(),
        state.email_service.clone(),
        state.sms_service.clone(),
    );

    let count = notification_service
        .get_unread_count(auth_user.user_id)
        .await?;

    Ok(Json(json!({
        "unread_count": count
    })))
}

pub async fn mark_notifications_read(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<MarkNotificationsReadRequest>,
) -> Result<Json<Value>> {
    let notification_service = NotificationService::new(
        state.db.clone(),
        state.redis.clone(),
        state.email_service,
        state.sms_service,
    );

    notification_service
        .mark_notifications_read(auth_user.user_id, payload.notification_ids)
        .await?;

    Ok(Json(json!({
        "message": "Notifications marked as read"
    })))
}

pub async fn mark_all_notifications_read(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<Value>> {
    let notification_service = NotificationService::new(
        state.db.clone(),
        state.redis.clone(),
        state.email_service.clone(),
        state.sms_service.clone(),
    );

    notification_service
        .mark_all_notifications_read(auth_user.user_id)
        .await?;

    Ok(Json(json!({
        "message": "All notifications marked as read"
    })))
}

pub async fn delete_notification(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(notification_id): Path<Uuid>,
) -> Result<StatusCode> {
    sqlx::query!(
        "DELETE FROM notifications WHERE id = $1 AND recipient_id = $2",
        notification_id,
        auth_user.user_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

// WebSocket endpoint for real-time notifications
pub async fn websocket_handler(
    ws: axum::extract::WebSocketUpgrade,
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> axum::response::Response {
    let typing_service =
        crate::services::typing_service::TypingService::new(state.db.clone(), state.redis.clone());

    let websocket_service = crate::services::websocket_service::WebSocketService::new(
        typing_service,
        state.redis.clone(),
    );
    // Handle the WebSocket connection
    Arc::new(websocket_service).handle_websocket(ws, auth_user.user_id)
}

// Typing indicator endpoints
pub async fn start_typing(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<TypingRequest>,
) -> Result<StatusCode> {
    let typing_service =
        crate::services::typing_service::TypingService::new(state.db.clone(), state.redis.clone());

    typing_service
        .start_typing(
            auth_user.user_id,
            payload.post_id,
            payload.parent_comment_id,
        )
        .await?;

    Ok(StatusCode::OK)
}

pub async fn stop_typing(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<TypingRequest>,
) -> Result<StatusCode> {
    let typing_service =
        crate::services::typing_service::TypingService::new(state.db.clone(), state.redis.clone());

    typing_service
        .stop_typing(
            auth_user.user_id,
            payload.post_id,
            payload.parent_comment_id,
        )
        .await?;

    Ok(StatusCode::OK)
}

pub async fn get_typing_users(
    State(state): State<AppState>,
    Query(params): Query<TypingQuery>,
) -> Result<Json<Value>> {
    let typing_service =
        crate::services::typing_service::TypingService::new(state.db.clone(), state.redis.clone());

    let typing_users = typing_service
        .get_typing_users(params.post_id, params.parent_comment_id)
        .await?;

    Ok(Json(json!({
        "typing_users": typing_users,
        "count": typing_users.len()
    })))
}

#[derive(Debug, Deserialize)]
pub struct TypingRequest {
    pub post_id: Uuid,
    pub parent_comment_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct TypingQuery {
    pub post_id: Uuid,
    pub parent_comment_id: Option<Uuid>,
}
