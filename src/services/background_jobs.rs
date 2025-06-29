use chrono::{Timelike, Utc};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::time::{Duration, interval};

use crate::{
    error::Result,
    redis::RedisClient,
    services::{
        email_service::EmailService, notification_service::NotificationService,
        sms_service::SmsService, typing_service::TypingService,
    },
};

#[derive(Clone)]
pub struct BackgroundJobsService {
    db: PgPool,
    redis: Arc<RedisClient>,
    notification_service: NotificationService,
    typing_service: TypingService,
    email_service: Arc<EmailService>,
    sms_service: Arc<SmsService>,
}

impl BackgroundJobsService {
    pub fn new(
        db: PgPool,
        redis: Arc<RedisClient>,
        email_service: Arc<EmailService>,
        sms_service: Arc<SmsService>,
    ) -> Self {
        let notification_service = NotificationService::new(
            db.clone(),
            redis.clone(),
            email_service.clone(),
            sms_service.clone(),
        );

        let typing_service = TypingService::new(db.clone(), redis.clone());

        Self {
            db,
            redis,
            notification_service,
            typing_service,
            email_service,
            sms_service,
        }
    }

    /// Start all background jobs
    pub async fn start_all_jobs(&self) {
        let jobs_service = self.clone();

        // Cleanup old typing indicators every 30 seconds
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                if let Err(e) = jobs_service.cleanup_typing_indicators().await {
                    tracing::error!("Failed to cleanup typing indicators: {}", e);
                }
            }
        });

        let jobs_service = self.clone();

        // Cleanup old notifications every hour
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(3600)); // 1 hour
            loop {
                interval.tick().await;
                if let Err(e) = jobs_service.cleanup_old_notifications().await {
                    tracing::error!("Failed to cleanup old notifications: {}", e);
                }
            }
        });

        let jobs_service = self.clone();

        // Send daily digest emails at 9 AM
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(3600)); // Check every hour
            loop {
                interval.tick().await;
                let now = Utc::now();
                if now.hour() == 9 && now.minute() < 5 {
                    // Send between 9:00-9:05 AM
                    if let Err(e) = jobs_service.send_daily_digests().await {
                        tracing::error!("Failed to send daily digests: {}", e);
                    }
                }
            }
        });

        let jobs_service = self.clone();

        // Update hot scores every 15 minutes
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(900)); // 15 minutes
            loop {
                interval.tick().await;
                if let Err(e) = jobs_service.update_hot_scores().await {
                    tracing::error!("Failed to update hot scores: {}", e);
                }
            }
        });

        tracing::info!("Background jobs started successfully");
    }

    /// Cleanup old typing indicators
    async fn cleanup_typing_indicators(&self) -> Result<()> {
        let cleaned = self.typing_service.cleanup_old_typing_indicators().await?;
        if cleaned > 0 {
            tracing::debug!("Cleaned up {} old typing indicators", cleaned);
        }
        Ok(())
    }

    /// Cleanup old notifications (older than 30 days)
    async fn cleanup_old_notifications(&self) -> Result<()> {
        let cleaned = self
            .notification_service
            .cleanup_old_notifications(30)
            .await?;
        if cleaned > 0 {
            tracing::info!("Cleaned up {} old notifications", cleaned);
        }
        Ok(())
    }

    /// Send daily digest emails to users who have it enabled
    async fn send_daily_digests(&self) -> Result<()> {
        let users_with_digest = sqlx::query!(
            r#"
            SELECT u.id, u.username, u.email
            FROM users u
            JOIN user_preferences up ON u.id = up.user_id
            WHERE up.email_notifications = true 
            AND u.email IS NOT NULL
            AND u.status = 'active'
            "#
        )
        .fetch_all(&self.db)
        .await?;

        for user in users_with_digest {
            // Get unread notifications from the last 24 hours
            let notifications = sqlx::query!(
                r#"
                SELECT n.title, n.content, n. notification_type as "notification_type: crate::models::NotificationType", n.created_at,
                       s.username as sender_username
                FROM notifications n
                LEFT JOIN users s ON n.sender_id = s.id
                WHERE n.recipient_id = $1 
                AND n.created_at > NOW() - INTERVAL '24 hours'
                ORDER BY n.created_at DESC
                LIMIT 10
                "#,
                user.id
            )
            .fetch_all(&self.db)
            .await?;

            if !notifications.is_empty() {
                let notification_responses: Vec<crate::models::NotificationResponse> =
                    notifications
                        .into_iter()
                        .map(|n| crate::models::NotificationResponse {
                            id: uuid::Uuid::new_v4(), // Placeholder
                            notification_type: n.notification_type,
                            title: n.title,
                            content: n.content,
                            is_read: false,
                            created_at: n.created_at.unwrap_or_default(),
                            sender: Some(crate::models::NotificationSender {
                                id: uuid::Uuid::new_v4(), // Placeholder
                                username: n.sender_username,
                                avatar_url: None,
                            }),
                            post: None,
                            comment: None,
                            community: None,
                        })
                        .collect();

                // Send digest email using the existing email service
                if let Some(email) = user.email {
                    let _ = self
                        .email_service
                        .send_digest_email(&email, &user.username, &notification_responses)
                        .await;
                }
            }
        }

        tracing::info!("Daily digest emails sent");
        Ok(())
    }

    /// Update hot scores for posts using Reddit's algorithm
    async fn update_hot_scores(&self) -> Result<()> {
        // Reddit's hot algorithm: log10(max(|score|, 1)) + (age_in_seconds / 45000)
        sqlx::query!(
            r#"
            UPDATE posts 
            SET hot_score = CASE 
                WHEN score > 0 THEN 
                    LOG(GREATEST(ABS(score), 1)) + (EXTRACT(EPOCH FROM (created_at - '1970-01-01'::timestamp)) / 45000.0)
                WHEN score < 0 THEN 
                    -LOG(GREATEST(ABS(score), 1)) + (EXTRACT(EPOCH FROM (created_at - '1970-01-01'::timestamp)) / 45000.0)
                ELSE 
                    (EXTRACT(EPOCH FROM (created_at - '1970-01-01'::timestamp)) / 45000.0)
            END
            WHERE status = 'active'
            AND updated_at > NOW() - INTERVAL '24 hours'
            "#
        )
        .execute(&self.db)
        .await?;

        tracing::debug!("Hot scores updated");
        Ok(())
    }

    /// Update community online user counts
    pub async fn update_community_online_counts(&self) -> Result<()> {
        // Get all communities
        let communities = sqlx::query!("SELECT id, name FROM communities WHERE status = 'active'")
            .fetch_all(&self.db)
            .await?;

        for community in communities {
            // Get online users for this community from Redis
            let online_key = format!("community_online:{}", community.id);
            let online_users = self.redis.cache_get(&online_key).await?;

            let online_count = if let Some(users_json) = online_users {
                serde_json::from_str::<Vec<uuid::Uuid>>(&users_json)
                    .map(|users| users.len())
                    .unwrap_or(0)
            } else {
                0
            };

            // Store in Redis with TTL
            let count_key = format!("community_online_count:{}", community.id);
            self.redis
                .cache_set(&count_key, &online_count.to_string(), 300)
                .await?; // 5 minutes TTL
        }

        Ok(())
    }

    /// Clean up expired sessions from Redis
    pub async fn cleanup_expired_sessions(&self) -> Result<()> {
        // This would be handled by Redis TTL automatically, but we can add cleanup logic here
        // For example, cleaning up database session records that are expired

        sqlx::query!("DELETE FROM user_sessions WHERE expires_at < NOW()")
            .execute(&self.db)
            .await?;

        tracing::debug!("Expired sessions cleaned up");
        Ok(())
    }

    /// Update user karma based on recent votes
    pub async fn update_user_karma(&self) -> Result<()> {
        // Update karma for users based on votes received in the last hour
        sqlx::query!(
            r#"
            WITH karma_changes AS (
                SELECT 
                    p.author_id as user_id,
                    SUM(pv.vote_type) as karma_change
                FROM post_votes pv
                JOIN posts p ON pv.post_id = p.id
                WHERE pv.created_at > NOW() - INTERVAL '1 hour'
                GROUP BY p.author_id
                
                UNION ALL
                
                SELECT 
                    c.author_id as user_id,
                    SUM(cv.vote_type) as karma_change
                FROM comment_votes cv
                JOIN comments c ON cv.comment_id = c.id
                WHERE cv.created_at > NOW() - INTERVAL '1 hour'
                GROUP BY c.author_id
            )
            UPDATE users 
            SET karma_points = karma_points + COALESCE(kc.total_karma, 0)
            FROM (
                SELECT user_id, SUM(karma_change) as total_karma
                FROM karma_changes
                GROUP BY user_id
            ) kc
            WHERE users.id = kc.user_id
            "#
        )
        .execute(&self.db)
        .await?;

        tracing::debug!("User karma updated");
        Ok(())
    }
}
