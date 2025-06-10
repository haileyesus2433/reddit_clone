use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "comment_status", rename_all = "lowercase")]
pub enum CommentStatus {
    Active,
    Removed,
    Deleted,
    Spam,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Comment {
    pub id: Uuid,
    pub content: String,
    pub post_id: Uuid,
    pub author_id: Uuid,
    pub parent_comment_id: Option<Uuid>,
    pub status: CommentStatus,
    pub is_edited: bool,
    pub upvotes: i32,
    pub downvotes: i32,
    pub score: i32,
    pub reply_count: i32,
    pub depth: i32,
    pub path: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CommentReport {
    pub id: Uuid,
    pub comment_id: Uuid,
    pub reported_by: Uuid,
    pub reason: String,
    pub description: Option<String>,
    pub status: String,
    pub reviewed_by: Option<Uuid>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SavedComment {
    pub id: Uuid,
    pub user_id: Uuid,
    pub comment_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CommentTypingIndicator {
    pub id: Uuid,
    pub user_id: Uuid,
    pub post_id: Uuid,
    pub parent_comment_id: Option<Uuid>,
    pub started_typing_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

// Create comment request
#[derive(Debug, Validate, Deserialize)]
pub struct CreateCommentRequest {
    #[validate(length(min = 1, max = 10000))]
    pub content: String,
    pub post_id: Uuid,
    pub parent_comment_id: Option<Uuid>,
}

// Update comment request
#[derive(Debug, Validate, Deserialize)]
pub struct UpdateCommentRequest {
    #[validate(length(min = 1, max = 10000))]
    pub content: String,
}

// Comment response with nested structure
#[derive(Debug, Serialize)]
pub struct CommentResponse {
    pub id: Uuid,
    pub content: String,
    pub post_id: Uuid,
    pub parent_comment_id: Option<Uuid>,
    pub status: CommentStatus,
    pub is_edited: bool,
    pub upvotes: i32,
    pub downvotes: i32,
    pub score: i32,
    pub reply_count: i32,
    pub depth: i32,
    pub author: CommentAuthor,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
    pub user_vote: Option<i16>,
    pub is_saved: bool,
    pub replies: Vec<CommentResponse>,
    pub media: Vec<CommentMediaResponse>,
}

#[derive(Debug, Serialize)]
pub struct CommentAuthor {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub is_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct CommentMediaResponse {
    pub id: Uuid,
    pub media_url: String,
    pub thumbnail_url: Option<String>,
    pub media_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

// Comment sorting options
#[derive(Debug, Deserialize)]
pub enum CommentSort {
    Best,
    Top,
    New,
    Controversial,
    Old,
}
