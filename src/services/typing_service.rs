use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{error::Result, redis::RedisClient};

#[derive(Clone)]
pub struct TypingService {
    db: PgPool,
    redis: std::sync::Arc<RedisClient>,
}

impl TypingService {
    pub fn new(db: PgPool, redis: std::sync::Arc<RedisClient>) -> Self {
        Self { db, redis }
    }

    /// Start typing indicator
    pub async fn start_typing(
        &self,
        user_id: Uuid,
        post_id: Uuid,
        parent_comment_id: Option<Uuid>,
    ) -> Result<()> {
        let now = Utc::now();

        // Update or insert typing indicator
        sqlx::query!(
            r#"
            INSERT INTO comment_typing_indicators (
                id, user_id, post_id, parent_comment_id, started_typing_at, last_activity_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (user_id, post_id, parent_comment_id)
            DO UPDATE SET last_activity_at = $6
            "#,
            Uuid::new_v4(),
            user_id,
            post_id,
            parent_comment_id,
            now,
            now
        )
        .execute(&self.db)
        .await?;

        // Broadcast typing indicator
        self.broadcast_typing_update(post_id, parent_comment_id)
            .await?;

        Ok(())
    }

    /// Stop typing indicator
    pub async fn stop_typing(
        &self,
        user_id: Uuid,
        post_id: Uuid,
        parent_comment_id: Option<Uuid>,
    ) -> Result<()> {
        sqlx::query!(
            "DELETE FROM comment_typing_indicators WHERE user_id = $1 AND post_id = $2 AND parent_comment_id IS NOT DISTINCT FROM $3",
            user_id,
            post_id,
            parent_comment_id
        )
        .execute(&self.db)
        .await?;

        // Broadcast typing update
        self.broadcast_typing_update(post_id, parent_comment_id)
            .await?;

        Ok(())
    }

    /// Get current typing users
    pub async fn get_typing_users(
        &self,
        post_id: Uuid,
        parent_comment_id: Option<Uuid>,
    ) -> Result<Vec<TypingUser>> {
        let typing_users = sqlx::query!(
            r#"
            SELECT cti.user_id, u.username, u.avatar_url, cti.started_typing_at
            FROM comment_typing_indicators cti
            JOIN users u ON cti.user_id = u.id
            WHERE cti.post_id = $1 
            AND cti.parent_comment_id IS NOT DISTINCT FROM $2
            AND cti.last_activity_at > NOW() - INTERVAL '30 seconds'
            ORDER BY cti.started_typing_at ASC
            "#,
            post_id,
            parent_comment_id
        )
        .fetch_all(&self.db)
        .await?;

        Ok(typing_users
            .into_iter()
            .map(|row| TypingUser {
                user_id: row.user_id,
                username: row.username,
                avatar_url: row.avatar_url,
                started_typing_at: row.started_typing_at,
            })
            .collect())
    }

    /// Broadcast typing update to all users viewing the post/comment
    async fn broadcast_typing_update(
        &self,
        post_id: Uuid,
        parent_comment_id: Option<Uuid>,
    ) -> Result<()> {
        let typing_users = self.get_typing_users(post_id, parent_comment_id).await?;

        let typing_data = json!({
            "type": "typing_update",
            "post_id": post_id,
            "parent_comment_id": parent_comment_id,
            "typing_users": typing_users,
            "count": typing_users.len()
        });

        // Broadcast to post channel
        let channel = if let Some(parent_id) = parent_comment_id {
            format!("comment_typing:{}:{}", post_id, parent_id)
        } else {
            format!("post_typing:{}", post_id)
        };

        self.redis
            .publish(&channel, &typing_data.to_string())
            .await?;

        Ok(())
    }

    /// Cleanup old typing indicators (background job)
    pub async fn cleanup_old_typing_indicators(&self) -> Result<u64> {
        let result = sqlx::query!(
            "DELETE FROM comment_typing_indicators WHERE last_activity_at < NOW() - INTERVAL '30 seconds'"
        )
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected())
    }

    /// Update typing activity (heartbeat)
    pub async fn update_typing_activity(
        &self,
        user_id: Uuid,
        post_id: Uuid,
        parent_comment_id: Option<Uuid>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE comment_typing_indicators 
            SET last_activity_at = NOW()
            WHERE user_id = $1 AND post_id = $2 AND parent_comment_id IS NOT DISTINCT FROM $3
            "#,
            user_id,
            post_id,
            parent_comment_id
        )
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Cleanup all typing indicators for a specific user (when they disconnect)
    pub async fn cleanup_user_typing_indicators(&self, user_id: Uuid) -> Result<u64> {
        let result = sqlx::query!(
            "DELETE FROM comment_typing_indicators WHERE user_id = $1",
            user_id
        )
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected())
    }

    /// Get all typing indicators for a specific user
    pub async fn get_user_typing_indicators(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<(Uuid, Option<Uuid>)>> {
        let indicators = sqlx::query!(
            "SELECT post_id, parent_comment_id FROM comment_typing_indicators WHERE user_id = $1",
            user_id
        )
        .fetch_all(&self.db)
        .await?;

        Ok(indicators
            .into_iter()
            .map(|row| (row.post_id, row.parent_comment_id))
            .collect())
    }

    /// Cleanup typing indicators for a specific post/comment combination
    pub async fn cleanup_typing_indicators_for_context(
        &self,
        post_id: Uuid,
        parent_comment_id: Option<Uuid>,
    ) -> Result<u64> {
        let result = sqlx::query!(
            "DELETE FROM comment_typing_indicators WHERE post_id = $1 AND parent_comment_id IS NOT DISTINCT FROM $2",
            post_id,
            parent_comment_id
        )
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected())
    }

    /// Check if a user is currently typing in a specific context
    pub async fn is_user_typing(
        &self,
        user_id: Uuid,
        post_id: Uuid,
        parent_comment_id: Option<Uuid>,
    ) -> Result<bool> {
        let exists = sqlx::query!(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM comment_typing_indicators 
                WHERE user_id = $1 AND post_id = $2 AND parent_comment_id IS NOT DISTINCT FROM $3
                AND last_activity_at > NOW() - INTERVAL '30 seconds'
            ) as exists
            "#,
            user_id,
            post_id,
            parent_comment_id
        )
        .fetch_one(&self.db)
        .await?;

        Ok(exists.exists.unwrap_or(false))
    }
}

#[derive(Debug, serde::Serialize)]
pub struct TypingUser {
    pub user_id: Uuid,
    pub username: String,
    pub avatar_url: Option<String>,
    pub started_typing_at: Option<DateTime<Utc>>,
}
