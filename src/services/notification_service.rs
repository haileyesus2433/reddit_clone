use chrono::Utc;
use regex::Regex;
use serde_json::json;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    error::{AppError, Result},
    models::{Notification, NotificationResponse, NotificationType, UserPreferences},
    redis::RedisClient,
    services::{email_service::EmailService, sms_service::SmsService},
};

#[derive(Clone)]
pub struct NotificationService {
    db: PgPool,
    redis: std::sync::Arc<RedisClient>,
    email_service: std::sync::Arc<EmailService>,
    sms_service: std::sync::Arc<SmsService>,
}

impl NotificationService {
    pub fn new(
        db: PgPool,
        redis: std::sync::Arc<RedisClient>,
        email_service: std::sync::Arc<EmailService>,
        sms_service: std::sync::Arc<SmsService>,
    ) -> Self {
        Self {
            db,
            redis,
            email_service,
            sms_service,
        }
    }

    /// Create a new notification
    pub async fn create_notification(
        &self,
        recipient_id: Uuid,
        sender_id: Option<Uuid>,
        notification_type: NotificationType,
        title: String,
        content: Option<String>,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
        community_id: Option<Uuid>,
    ) -> Result<Notification> {
        // Check if user wants this type of notification
        if !self
            .should_send_notification(recipient_id, &notification_type)
            .await?
        {
            return Err(AppError::BadRequest(
                "User has disabled this notification type".to_string(),
            ));
        }

        // Don't send notification to self
        if let Some(sender) = sender_id {
            if sender == recipient_id {
                return Err(AppError::BadRequest(
                    "Cannot send notification to self".to_string(),
                ));
            }
        }

        let notification_id = Uuid::new_v4();
        let now = Utc::now();

        let notification = sqlx::query_as!(
            Notification,
            r#"
            INSERT INTO notifications (
                id, recipient_id, sender_id, notification_type, title, content,
                post_id, comment_id, community_id, is_read, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING id, recipient_id, sender_id, 
                      notification_type as "notification_type: NotificationType",
                      title, content, is_read as "is_read!", post_id, comment_id, community_id, created_at as "created_at!"
            "#,
            notification_id,
            recipient_id,
            sender_id,
            notification_type as NotificationType,
            title,
            content,
            post_id,
            comment_id,
            community_id,
            false,
            now
        )
        .fetch_one(&self.db)
        .await?;

        // Send real-time notification
        self.send_realtime_notification(&notification).await?;

        // Send email/SMS if enabled
        self.send_external_notifications(&notification).await?;

        Ok(notification)
    }

    /// Get notifications for a user with pagination
    pub async fn get_user_notifications(
        &self,
        user_id: Uuid,
        limit: u32,
        offset: u32,
        unread_only: bool,
    ) -> Result<Vec<NotificationResponse>> {
        let mut query = r#"
            SELECT 
                n.id, n.notification_type, n.title, n.content, n.is_read, n.created_at,
                n.post_id, n.comment_id, n.community_id,
                -- Sender info
                s.id as sender_id, s.username as sender_username, s.avatar_url as sender_avatar,
                -- Post info
                p.title as post_title,
                -- Comment info  
                c.content as comment_content,
                -- Community info
                comm.id as community_id, comm.name as community_name, comm.display_name as community_display_name
            FROM notifications n
            LEFT JOIN users s ON n.sender_id = s.id
            LEFT JOIN posts p ON n.post_id = p.id
            LEFT JOIN comments c ON n.comment_id = c.id
            LEFT JOIN communities comm ON n.community_id = comm.id
            WHERE n.recipient_id = $1
        "#.to_string();

        if unread_only {
            query.push_str(" AND n.is_read = false");
        }

        query.push_str(" ORDER BY n.created_at DESC LIMIT $2 OFFSET $3");

        let rows = sqlx::query(&query)
            .bind(user_id)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.db)
            .await?;

        let mut notifications = Vec::new();
        for row in rows {
            let notification = NotificationResponse {
                id: row.get("id"),
                notification_type: row.get("notification_type"),
                title: row.get("title"),
                content: row.get("content"),
                is_read: row.get("is_read"),
                created_at: row.get("created_at"),
                sender: if row.get::<Option<Uuid>, _>("sender_id").is_some() {
                    Some(crate::models::NotificationSender {
                        id: row.get("sender_id"),
                        username: row.get("sender_username"),
                        avatar_url: row.get("sender_avatar"),
                    })
                } else {
                    None
                },
                post: if row.get::<Option<Uuid>, _>("post_id").is_some() {
                    Some(crate::models::NotificationPost {
                        id: row.get("post_id"),
                        title: row.get("post_title"),
                    })
                } else {
                    None
                },
                comment: if row.get::<Option<Uuid>, _>("comment_id").is_some() {
                    Some(crate::models::NotificationComment {
                        id: row.get("comment_id"),
                        content: row
                            .get::<String, _>("comment_content")
                            .chars()
                            .take(100)
                            .collect(),
                    })
                } else {
                    None
                },
                community: if row.get::<Option<Uuid>, _>("community_id").is_some() {
                    Some(crate::models::NotificationCommunity {
                        id: row.get("community_id"),
                        name: row.get("community_name"),
                        display_name: row.get("community_display_name"),
                    })
                } else {
                    None
                },
            };
            notifications.push(notification);
        }

        Ok(notifications)
    }

    /// Mark notifications as read
    pub async fn mark_notifications_read(
        &self,
        user_id: Uuid,
        notification_ids: Vec<Uuid>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE notifications 
            SET is_read = true 
            WHERE recipient_id = $1 AND id = ANY($2)
            "#,
            user_id,
            &notification_ids
        )
        .execute(&self.db)
        .await?;

        // Update real-time notification count
        self.update_notification_count(user_id).await?;

        Ok(())
    }

    /// Mark all notifications as read
    pub async fn mark_all_notifications_read(&self, user_id: Uuid) -> Result<()> {
        sqlx::query!(
            "UPDATE notifications SET is_read = true WHERE recipient_id = $1 AND is_read = false",
            user_id
        )
        .execute(&self.db)
        .await?;

        // Update real-time notification count
        self.update_notification_count(user_id).await?;

        Ok(())
    }

    /// Get unread notification count
    pub async fn get_unread_count(&self, user_id: Uuid) -> Result<i64> {
        let count = sqlx::query!(
            "SELECT COUNT(*) as count FROM notifications WHERE recipient_id = $1 AND is_read = false",
            user_id
        )
        .fetch_one(&self.db)
        .await?;

        Ok(count.count.unwrap_or(0))
    }

    /// Delete old notifications (cleanup job)
    pub async fn cleanup_old_notifications(&self, days: i32) -> Result<u64> {
        let cutoff_date = Utc::now() - chrono::Duration::days(days as i64);

        let result = sqlx::query!(
            "DELETE FROM notifications WHERE created_at < $1 AND is_read = true",
            cutoff_date
        )
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected())
    }

    /// Check if user wants this notification type
    async fn should_send_notification(
        &self,
        user_id: Uuid,
        notification_type: &NotificationType,
    ) -> Result<bool> {
        let preferences_row = sqlx::query(
            r#"
            SELECT 
            id,
            user_id,
            COALESCE(comment_reply_notifications, true) as comment_reply_notifications,
            COALESCE(post_reply_notifications, true) as post_reply_notifications,
            COALESCE(mention_notifications, true) as mention_notifications,
            COALESCE(upvote_notifications, true) as upvote_notifications,
            COALESCE(community_notifications, true) as community_notifications,
            COALESCE(email_notifications, false) as email_notifications,
            COALESCE(push_notifications, false) as push_notifications,
            COALESCE(nsfw_content, false) as nsfw_content,
            created_at,
            updated_at
            FROM user_preferences WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.db)
        .await?;

        let preferences = preferences_row.map(|row| UserPreferences {
            id: row.get("id"),
            user_id: row.get("user_id"),
            comment_reply_notifications: row.get("comment_reply_notifications"),
            post_reply_notifications: row.get("post_reply_notifications"),
            mention_notifications: row.get("mention_notifications"),
            upvote_notifications: row.get("upvote_notifications"),
            community_notifications: row.get("community_notifications"),
            email_notifications: row.get("email_notifications"),
            push_notifications: row.get("push_notifications"),
            nsfw_content: row.get("nsfw_content"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        });

        let prefs = match preferences {
            Some(p) => p,
            None => return Ok(true), // Default to allowing notifications
        };

        let should_send = match notification_type {
            NotificationType::CommentReply => prefs.comment_reply_notifications,
            NotificationType::PostReply => prefs.post_reply_notifications,
            NotificationType::Mention => prefs.mention_notifications,
            NotificationType::Upvote | NotificationType::Downvote => prefs.upvote_notifications,
            NotificationType::CommunityInvite | NotificationType::CommunityBan => {
                prefs.community_notifications
            }
            _ => true,
        };

        Ok(should_send)
    }

    /// Send real-time notification via WebSocket
    async fn send_realtime_notification(&self, notification: &Notification) -> Result<()> {
        let notification_data = json!({
            "type": "notification",
            "data": {
                "id": notification.id,
                "type": notification.notification_type,
                "title": notification.title,
                "content": notification.content,
                "created_at": notification.created_at,
                "is_read": notification.is_read
            }
        });

        // Publish to user's notification channel
        let channel = format!("user_notifications:{}", notification.recipient_id);
        self.redis
            .publish(&channel, &notification_data.to_string())
            .await?;

        // Update notification count
        self.update_notification_count(notification.recipient_id)
            .await?;

        Ok(())
    }

    /// Update real-time notification count
    async fn update_notification_count(&self, user_id: Uuid) -> Result<()> {
        let count = self.get_unread_count(user_id).await?;

        let count_data = json!({
            "type": "notification_count",
            "count": count
        });

        let channel = format!("user_notifications:{}", user_id);
        self.redis
            .publish(&channel, &count_data.to_string())
            .await?;

        Ok(())
    }

    /// Send email/SMS notifications if enabled
    async fn send_external_notifications(&self, notification: &Notification) -> Result<()> {
        // Get user preferences
        let preferences_row = sqlx::query(
            r#"
            SELECT 
            id,
            user_id,
            COALESCE(comment_reply_notifications, true) as comment_reply_notifications,
            COALESCE(post_reply_notifications, true) as post_reply_notifications,
            COALESCE(mention_notifications, true) as mention_notifications,
            COALESCE(upvote_notifications, true) as upvote_notifications,
            COALESCE(community_notifications, true) as community_notifications,
            COALESCE(email_notifications, false) as email_notifications,
            COALESCE(push_notifications, false) as push_notifications,
            COALESCE(nsfw_content, false) as nsfw_content,
            created_at,
            updated_at
            FROM user_preferences WHERE user_id = $1
            "#,
        )
        .bind(notification.recipient_id)
        .fetch_optional(&self.db)
        .await?;

        let preferences = preferences_row.map(|row| UserPreferences {
            id: row.get("id"),
            user_id: row.get("user_id"),
            comment_reply_notifications: row.get("comment_reply_notifications"),
            post_reply_notifications: row.get("post_reply_notifications"),
            mention_notifications: row.get("mention_notifications"),
            upvote_notifications: row.get("upvote_notifications"),
            community_notifications: row.get("community_notifications"),
            email_notifications: row.get("email_notifications"),
            push_notifications: row.get("push_notifications"),
            nsfw_content: row.get("nsfw_content"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        });

        let prefs = match preferences {
            Some(p) => p,
            None => return Ok(()), // No preferences, skip external notifications
        };

        // Get user info
        let user = sqlx::query!(
            "SELECT username, email, phone FROM users WHERE id = $1",
            notification.recipient_id
        )
        .fetch_optional(&self.db)
        .await?;

        let user = match user {
            Some(u) => u,
            None => return Ok(()),
        };

        // Send email notification
        if prefs.email_notifications {
            if let Some(email) = user.email {
                let _ = self
                    .send_email_notification(&email, &user.username, notification)
                    .await;
            }
        }

        // Send SMS notification for critical notifications
        if prefs.push_notifications
            && matches!(
                notification.notification_type,
                NotificationType::CommunityBan | NotificationType::PostRemoved
            )
        {
            if let Some(phone) = user.phone {
                let _ = self
                    .send_sms_notification(&phone, &user.username, notification)
                    .await;
            }
        }

        Ok(())
    }

    async fn send_email_notification(
        &self,
        email: &str,
        username: &str,
        notification: &Notification,
    ) -> Result<()> {
        let _subject = format!("Reddit Clone - {}", notification.title);
        let content = notification
            .content
            .as_deref()
            .unwrap_or("No additional details");

        self.email_service
            .send_notification_email(email, username, &notification.title, content)
            .await?;

        Ok(())
    }

    async fn send_sms_notification(
        &self,
        phone: &str,
        _username: &str,
        notification: &Notification,
    ) -> Result<()> {
        let message = format!("Reddit Clone: {}", notification.title);

        self.sms_service.send_sms(phone, &message).await?;

        Ok(())
    }

    // Specific notification creators for different events

    /// Create comment reply notification
    pub async fn notify_comment_reply(
        &self,
        comment_author_id: Uuid,
        replier_id: Uuid,
        comment_id: Uuid,
        post_id: Uuid,
        reply_content: &str,
    ) -> Result<()> {
        let replier = self.get_user_info(replier_id).await?;

        let title = format!("{} replied to your comment", replier.username);
        let content = Some(format!(
            "\"{}\"",
            reply_content.chars().take(100).collect::<String>()
        ));

        self.create_notification(
            comment_author_id,
            Some(replier_id),
            NotificationType::CommentReply,
            title,
            content,
            Some(post_id),
            Some(comment_id),
            None,
        )
        .await?;

        Ok(())
    }

    /// Create post reply notification
    pub async fn notify_post_reply(
        &self,
        post_author_id: Uuid,
        commenter_id: Uuid,
        post_id: Uuid,
        comment_id: Uuid,
        comment_content: &str,
    ) -> Result<()> {
        let commenter = self.get_user_info(commenter_id).await?;

        let title = format!("{} commented on your post", commenter.username);
        let content = Some(format!(
            "\"{}\"",
            comment_content.chars().take(100).collect::<String>()
        ));

        self.create_notification(
            post_author_id,
            Some(commenter_id),
            NotificationType::PostReply,
            title,
            content,
            Some(post_id),
            Some(comment_id),
            None,
        )
        .await?;

        Ok(())
    }

    /// Create mention notification
    pub async fn notify_mention(
        &self,
        mentioned_user_id: Uuid,
        mentioner_id: Uuid,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
        content: &str,
    ) -> Result<()> {
        let mentioner = self.get_user_info(mentioner_id).await?;

        let title = format!("{} mentioned you", mentioner.username);
        let notification_content = Some(format!(
            "\"{}\"",
            content.chars().take(100).collect::<String>()
        ));

        self.create_notification(
            mentioned_user_id,
            Some(mentioner_id),
            NotificationType::Mention,
            title,
            notification_content,
            post_id,
            comment_id,
            None,
        )
        .await?;

        Ok(())
    }

    /// Create upvote notification
    pub async fn notify_upvote(
        &self,
        content_author_id: Uuid,
        voter_id: Uuid,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
    ) -> Result<()> {
        let voter = self.get_user_info(voter_id).await?;

        let content_type = if post_id.is_some() { "post" } else { "comment" };
        let title = format!("{} upvoted your {}", voter.username, content_type);

        self.create_notification(
            content_author_id,
            Some(voter_id),
            NotificationType::Upvote,
            title,
            None,
            post_id,
            comment_id,
            None,
        )
        .await?;

        Ok(())
    }

    /// Create follow notification
    pub async fn notify_follow(&self, followed_user_id: Uuid, follower_id: Uuid) -> Result<()> {
        let follower = self.get_user_info(follower_id).await?;

        let title = format!("{} started following you", follower.username);

        self.create_notification(
            followed_user_id,
            Some(follower_id),
            NotificationType::Follow,
            title,
            None,
            None,
            None,
            None,
        )
        .await?;

        Ok(())
    }

    /// Create community invite notification
    pub async fn notify_community_invite(
        &self,
        invited_user_id: Uuid,
        inviter_id: Uuid,
        community_id: Uuid,
    ) -> Result<()> {
        let inviter = self.get_user_info(inviter_id).await?;
        let community = self.get_community_info(community_id).await?;

        let title = format!(
            "{} invited you to join r/{}",
            inviter.username, community.name
        );

        self.create_notification(
            invited_user_id,
            Some(inviter_id),
            NotificationType::CommunityInvite,
            title,
            None,
            None,
            None,
            Some(community_id),
        )
        .await?;

        Ok(())
    }

    /// Helper to get user info
    async fn get_user_info(&self, user_id: Uuid) -> Result<UserInfo> {
        let user = sqlx::query!(
            "SELECT username, avatar_url FROM users WHERE id = $1",
            user_id
        )
        .fetch_one(&self.db)
        .await?;

        Ok(UserInfo {
            username: user.username,
            avatar_url: user.avatar_url,
        })
    }

    /// Helper to get community info
    async fn get_community_info(&self, community_id: Uuid) -> Result<CommunityInfo> {
        let community = sqlx::query!(
            "SELECT name, display_name FROM communities WHERE id = $1",
            community_id
        )
        .fetch_one(&self.db)
        .await?;

        Ok(CommunityInfo {
            name: community.name,
            display_name: community.display_name,
        })
    }

    /// Extract mentions from text content
    pub fn extract_mentions(&self, content: &str) -> Vec<String> {
        let mention_regex = Regex::new(r"@(\w+)").unwrap();
        mention_regex
            .captures_iter(content)
            .map(|cap| cap[1].to_string())
            .collect()
    }

    /// Process mentions in content and send notifications
    pub async fn process_mentions(
        &self,
        content: &str,
        mentioner_id: Uuid,
        post_id: Option<Uuid>,
        comment_id: Option<Uuid>,
    ) -> Result<()> {
        let mentions = self.extract_mentions(content);

        for username in mentions {
            // Get user ID by username
            if let Ok(Some(user)) = sqlx::query!(
                "SELECT id FROM users WHERE username = $1 AND status = 'active'",
                username
            )
            .fetch_optional(&self.db)
            .await
            {
                let _ = self
                    .notify_mention(user.id, mentioner_id, post_id, comment_id, content)
                    .await;
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
struct UserInfo {
    username: String,
    avatar_url: Option<String>,
}

#[derive(Debug)]
struct CommunityInfo {
    name: String,
    display_name: String,
}
