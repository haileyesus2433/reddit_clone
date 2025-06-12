use axum::{
    extract::{Path, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use crate::{
    AppState,
    auth::{AuthUser, OptionalAuthUser},
    error::{AppError, Result},
    models::UserPreferences,
    services::user_service,
};

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateUserRequest {
    #[validate(length(min = 1, max = 100))]
    pub display_name: Option<String>,
    #[validate(length(max = 500))]
    pub bio: Option<String>,
    #[validate(url)]
    pub avatar_url: Option<String>,
    #[validate(url)]
    pub banner_url: Option<String>,
    pub location: Option<String>,
    pub website: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePreferencesRequest {
    pub email_notifications: Option<bool>,
    pub push_notifications: Option<bool>,
    pub comment_reply_notifications: Option<bool>,
    pub post_reply_notifications: Option<bool>,
    pub mention_notifications: Option<bool>,
    pub upvote_notifications: Option<bool>,
    pub community_notifications: Option<bool>,
    pub nsfw_content: Option<bool>,
    pub autoplay_videos: Option<bool>,
    pub theme: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UserProfileResponse {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    // pub location: Option<String>,
    // pub website: Option<String>,
    pub karma_points: i32,
    pub post_count: i32,
    pub comment_count: i32,
    pub follower_count: i32,
    pub following_count: i32,
    pub is_verified: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub is_following: Option<bool>,
    pub is_blocked: Option<bool>,
}

pub async fn get_current_user(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<UserProfileResponse>> {
    let user = user_service::get_user_by_id(&state.db, auth_user.user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    let stats = user_service::get_user_stats(&state.db, auth_user.user_id).await?;

    Ok(Json(UserProfileResponse {
        id: user.id,
        username: user.username,
        display_name: user.display_name,
        bio: user.bio,
        avatar_url: user.avatar_url,
        banner_url: user.banner_url,
        // location: user.location,
        // website: user.website,
        karma_points: user.karma_points,
        post_count: stats.post_count,
        comment_count: stats.comment_count,
        follower_count: stats.follower_count,
        following_count: stats.following_count,
        is_verified: user.is_verified,
        created_at: user.created_at,
        is_following: None,
        is_blocked: None,
    }))
}

pub async fn update_current_user(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<UpdateUserRequest>,
) -> Result<Json<Value>> {
    payload.validate()?;

    // Update user
    sqlx::query(
        r#"
        UPDATE users 
        SET display_name = COALESCE($1, display_name),
            bio = COALESCE($2, bio),
            avatar_url = COALESCE($3, avatar_url),
            banner_url = COALESCE($4, banner_url),
            location = COALESCE($5, location),
            website = COALESCE($6, website),
            updated_at = $7
        WHERE id = $8
        "#,
    )
    .bind(&payload.display_name)
    .bind(&payload.bio)
    .bind(&payload.avatar_url)
    .bind(&payload.banner_url)
    .bind(&payload.location)
    .bind(&payload.website)
    .bind(chrono::Utc::now())
    .bind(auth_user.user_id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "User updated successfully"
    })))
}

pub async fn get_user_by_username(
    State(state): State<AppState>,
    auth_user: OptionalAuthUser,
    Path(username): Path<String>,
) -> Result<Json<UserProfileResponse>> {
    let user = user_service::get_user_by_username(&state.db, &username)
        .await?
        .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    let stats = user_service::get_user_stats(&state.db, user.id).await?;

    let (is_following, is_blocked) = if let Some(auth_user) = auth_user.0 {
        let is_following =
            user_service::is_following(&state.db, auth_user.user_id, user.id).await?;
        let is_blocked = user_service::is_blocked(&state.db, auth_user.user_id, user.id).await?;
        (Some(is_following), Some(is_blocked))
    } else {
        (None, None)
    };

    Ok(Json(UserProfileResponse {
        id: user.id,
        username: user.username,
        display_name: user.display_name,
        bio: user.bio,
        avatar_url: user.avatar_url,
        banner_url: user.banner_url,
        // location: user.location,
        // website: user.website,
        karma_points: user.karma_points,
        post_count: stats.post_count,
        comment_count: stats.comment_count,
        follower_count: stats.follower_count,
        following_count: stats.following_count,
        is_verified: user.is_verified,
        created_at: user.created_at,
        is_following,
        is_blocked,
    }))
}

pub async fn get_user_preferences(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<UserPreferences>> {
    let preferences =
        sqlx::query_as::<_, UserPreferences>("SELECT * FROM user_preferences WHERE user_id = $1")
            .bind(auth_user.user_id)
            .fetch_optional(&state.db)
            .await?;

    let preferences =
        preferences.ok_or_else(|| AppError::NotFound("User preferences not found".to_string()))?;

    Ok(Json(preferences))
}

pub async fn update_user_preferences(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<UpdatePreferencesRequest>,
) -> Result<Json<Value>> {
    sqlx::query(
        r#"
        UPDATE user_preferences 
        SET email_notifications = COALESCE($1, email_notifications),
            push_notifications = COALESCE($2, push_notifications),
            comment_reply_notifications = COALESCE($3, comment_reply_notifications),
            post_reply_notifications = COALESCE($4, post_reply_notifications),
            mention_notifications = COALESCE($5, mention_notifications),
            upvote_notifications = COALESCE($6, upvote_notifications),
            community_notifications = COALESCE($7, community_notifications),
            nsfw_content = COALESCE($8, nsfw_content),
            autoplay_videos = COALESCE($9, autoplay_videos),
            theme = COALESCE($10, theme),
            language = COALESCE($11, language),
            updated_at = $12
        WHERE user_id = $13
        "#,
    )
    .bind(payload.email_notifications)
    .bind(payload.push_notifications)
    .bind(payload.comment_reply_notifications)
    .bind(payload.post_reply_notifications)
    .bind(payload.mention_notifications)
    .bind(payload.upvote_notifications)
    .bind(payload.community_notifications)
    .bind(payload.nsfw_content)
    .bind(payload.autoplay_videos)
    .bind(&payload.theme)
    .bind(&payload.language)
    .bind(chrono::Utc::now())
    .bind(auth_user.user_id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "Preferences updated successfully"
    })))
}

pub async fn follow_user(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<Value>> {
    // Check if user exists
    let _target_user = user_service::get_user_by_id(&state.db, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    // Can't follow yourself
    if auth_user.user_id == user_id {
        return Err(AppError::BadRequest("Cannot follow yourself".to_string()));
    }

    // Check if already following
    let existing_follow =
        sqlx::query("SELECT id FROM user_follows WHERE follower_id = $1 AND following_id = $2")
            .bind(auth_user.user_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;

    if existing_follow.is_some() {
        return Err(AppError::Conflict(
            "Already following this user".to_string(),
        ));
    }

    // Check if blocked
    let is_blocked = user_service::is_blocked(&state.db, user_id, auth_user.user_id).await?;
    if is_blocked {
        return Err(AppError::Authorization(
            "Cannot follow this user".to_string(),
        ));
    }

    // Create follow relationship
    sqlx::query(
        r#"
        INSERT INTO user_follows (id, follower_id, following_id, created_at)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(auth_user.user_id)
    .bind(user_id)
    .bind(chrono::Utc::now())
    .execute(&state.db)
    .await?;

    // Create notification
    user_service::create_follow_notification(&state, auth_user.user_id, user_id).await?;

    Ok(Json(json!({
        "message": "User followed successfully"
    })))
}

pub async fn unfollow_user(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<Value>> {
    // Remove follow relationship
    let result =
        sqlx::query("DELETE FROM user_follows WHERE follower_id = $1 AND following_id = $2")
            .bind(auth_user.user_id)
            .bind(user_id)
            .execute(&state.db)
            .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(
            "Follow relationship not found".to_string(),
        ));
    }

    Ok(Json(json!({
        "message": "User unfollowed successfully"
    })))
}

pub async fn block_user(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<Value>> {
    // Check if user exists
    let _target_user = user_service::get_user_by_id(&state.db, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    // Can't block yourself
    if auth_user.user_id == user_id {
        return Err(AppError::BadRequest("Cannot block yourself".to_string()));
    }

    // Check if already blocked
    let existing_block =
        sqlx::query("SELECT id FROM user_blocks WHERE blocker_id = $1 AND blocked_id = $2")
            .bind(auth_user.user_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;

    if existing_block.is_some() {
        return Err(AppError::Conflict("User already blocked".to_string()));
    }

    // Create block relationship
    sqlx::query(
        r#"
        INSERT INTO user_blocks (id, blocker_id, blocked_id, created_at)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(auth_user.user_id)
    .bind(user_id)
    .bind(chrono::Utc::now())
    .execute(&state.db)
    .await?;

    // Remove any existing follow relationships
    sqlx::query(
        "DELETE FROM user_follows WHERE (follower_id = $1 AND following_id = $2) OR (follower_id = $2 AND following_id = $1)"
    )
    .bind(auth_user.user_id)
    .bind(user_id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "User blocked successfully"
    })))
}

pub async fn unblock_user(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<Value>> {
    // Remove block relationship
    let result = sqlx::query("DELETE FROM user_blocks WHERE blocker_id = $1 AND blocked_id = $2")
        .bind(auth_user.user_id)
        .bind(user_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound(
            "Block relationship not found".to_string(),
        ));
    }

    Ok(Json(json!({
        "message": "User unblocked successfully"
    })))
}
