use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    AppState,
    error::{AppError, Result},
    models::{NotificationType, User},
};

#[derive(Debug, Serialize)]
pub struct UserStats {
    pub post_count: i32,
    pub comment_count: i32,
    pub follower_count: i32,
    pub following_count: i32,
}

pub async fn get_user_by_id(db: &PgPool, user_id: Uuid) -> Result<Option<User>> {
    let user =
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 AND status != 'deleted'")
            .bind(user_id)
            .fetch_optional(db)
            .await?;

    Ok(user)
}

pub async fn get_user_by_username(db: &PgPool, username: &str) -> Result<Option<User>> {
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE username = $1 AND status != 'deleted'",
    )
    .bind(username)
    .fetch_optional(db)
    .await?;

    Ok(user)
}

pub async fn get_user_stats(db: &PgPool, user_id: Uuid) -> Result<UserStats> {
    let stats = sqlx::query!(
        r#"
        SELECT 
            (SELECT COUNT(*)::int FROM posts WHERE author_id = $1 AND status != 'deleted') as post_count,
            (SELECT COUNT(*)::int FROM comments WHERE author_id = $1 AND status != 'deleted') as comment_count,
            (SELECT COUNT(*)::int FROM user_follows WHERE following_id = $1) as follower_count,
            (SELECT COUNT(*)::int FROM user_follows WHERE follower_id = $1) as following_count
        "#,
        user_id
    )
    .fetch_one(db)
    .await?;

    Ok(UserStats {
        post_count: stats.post_count.unwrap_or(0),
        comment_count: stats.comment_count.unwrap_or(0),
        follower_count: stats.follower_count.unwrap_or(0),
        following_count: stats.following_count.unwrap_or(0),
    })
}

pub async fn is_following(db: &PgPool, follower_id: Uuid, following_id: Uuid) -> Result<bool> {
    let exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM user_follows WHERE follower_id = $1 AND following_id = $2)",
        follower_id,
        following_id
    )
    .fetch_one(db)
    .await?;

    Ok(exists.exists.unwrap_or(false))
}

pub async fn is_blocked(db: &PgPool, blocker_id: Uuid, blocked_id: Uuid) -> Result<bool> {
    let exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM user_blocks WHERE blocker_id = $1 AND blocked_id = $2)",
        blocker_id,
        blocked_id
    )
    .fetch_one(db)
    .await?;

    Ok(exists.exists.unwrap_or(false))
}

pub async fn create_follow_notification(
    state: &AppState,
    follower_id: Uuid,
    following_id: Uuid,
) -> Result<()> {
    // Get follower info
    let follower = get_user_by_id(&state.db, follower_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Follower not found".to_string()))?;

    // Create notification
    sqlx::query(
        r#"
        INSERT INTO notifications (
            id, user_id, notification_type, title, message, 
            related_user_id, created_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(following_id)
    .bind(NotificationType::Follow)
    .bind("New Follower")
    .bind(format!("{} started following you", follower.username))
    .bind(follower_id)
    .bind(chrono::Utc::now())
    .execute(&state.db)
    .await?;

    // Send real-time notification
    let notification_data = serde_json::json!({
        "type": "follow",
        "follower": {
            "id": follower.id,
            "username": follower.username,
            "avatar_url": follower.avatar_url
        }
    });

    state
        .redis
        .publish(
            &format!("user_notifications:{}", following_id),
            &notification_data.to_string(),
        )
        .await?;

    Ok(())
}
