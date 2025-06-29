use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "notification_type", rename_all = "snake_case")]
pub enum NotificationType {
    CommentReply,
    PostReply,
    Mention,
    Upvote,
    Downvote,
    Follow,
    CommunityInvite,
    CommunityBan,
    PostRemoved,
    CommentRemoved,
    ModeratorInvite,
    AwardReceived,
    PostTrending,
    SystemAnnouncement,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Notification {
    pub id: Uuid,
    pub recipient_id: Uuid,
    pub sender_id: Option<Uuid>,
    pub notification_type: NotificationType,
    pub title: String,
    pub content: Option<String>,
    pub is_read: bool,
    pub post_id: Option<Uuid>,
    pub comment_id: Option<Uuid>,
    pub community_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

// Notification response with sender info
#[derive(Debug, Serialize)]
pub struct NotificationResponse {
    pub id: Uuid,
    pub notification_type: NotificationType,
    pub title: String,
    pub content: Option<String>,
    pub is_read: bool,
    pub created_at: DateTime<Utc>,
    pub sender: Option<NotificationSender>,
    pub post: Option<NotificationPost>,
    pub comment: Option<NotificationComment>,
    pub community: Option<NotificationCommunity>,
}

#[derive(Debug, Serialize)]
pub struct NotificationSender {
    pub id: Uuid,
    pub username: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NotificationPost {
    pub id: Uuid,
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct NotificationComment {
    pub id: Uuid,
    pub content: String, // Truncated content
}

#[derive(Debug, Serialize)]
pub struct NotificationCommunity {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
}

// Mark notifications as read request
#[derive(Debug, Deserialize, Validate)]
pub struct MarkNotificationsReadRequest {
    #[validate(length(min = 1, max = 100))]
    pub notification_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct NotificationSettings {
    pub email_notifications: bool,
    pub push_notifications: bool,
    pub comment_reply_notifications: bool,
    pub post_reply_notifications: bool,
    pub mention_notifications: bool,
    pub upvote_notifications: bool,
    pub community_notifications: bool,
    pub digest_frequency: DigestFrequency,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "digest_frequency", rename_all = "lowercase")]
pub enum DigestFrequency {
    Never,
    Daily,
    Weekly,
    Monthly,
}

// Real-time notification models
#[derive(Debug, Serialize)]
pub struct RealtimeNotification {
    pub id: Uuid,
    pub notification_type: NotificationType,
    pub title: String,
    pub content: Option<String>,
    pub created_at: DateTime<Utc>,
    pub sender: Option<NotificationSender>,
}

#[derive(Debug, Serialize)]
pub struct NotificationCount {
    pub total: i64,
    pub unread: i64,
    pub by_type: std::collections::HashMap<NotificationType, i64>,
}
