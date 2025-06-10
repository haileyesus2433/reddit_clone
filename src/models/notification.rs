use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "notification_type", rename_all = "snake_case")]
pub enum NotificationType {
    CommentReply,
    PostReply,
    Mention,
    Upvote,
    Downvote,
    CommunityInvite,
    CommunityBan,
    PostRemoved,
    CommentRemoved,
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
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct NotificationCommunity {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
}

// Mark notifications as read request
#[derive(Debug, Deserialize)]
pub struct MarkNotificationsReadRequest {
    pub notification_ids: Vec<Uuid>,
}
