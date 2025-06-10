use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::models::CommentMediaResponse;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PostMedia {
    pub id: Uuid,
    pub post_id: Uuid,
    pub media_url: String,
    pub thumbnail_url: Option<String>,
    pub media_type: String,
    pub file_size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration: Option<i32>,
    pub media_order: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CommentMedia {
    pub id: Uuid,
    pub comment_id: Uuid,
    pub media_url: String,
    pub thumbnail_url: Option<String>,
    pub media_type: String,
    pub file_size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration: Option<i32>,
    pub media_order: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PostMediaResponse {
    pub id: Uuid,
    pub media_url: String,
    pub thumbnail_url: Option<String>,
    pub media_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration: Option<i32>,
    pub media_order: i32,
}

impl From<PostMedia> for PostMediaResponse {
    fn from(media: PostMedia) -> Self {
        Self {
            id: media.id,
            media_url: media.media_url,
            thumbnail_url: media.thumbnail_url,
            media_type: media.media_type,
            width: media.width,
            height: media.height,
            duration: media.duration,
            media_order: media.media_order,
        }
    }
}

impl From<CommentMedia> for CommentMediaResponse {
    fn from(media: CommentMedia) -> Self {
        Self {
            id: media.id,
            media_url: media.media_url,
            thumbnail_url: media.thumbnail_url,
            media_type: media.media_type,
            width: media.width,
            height: media.height,
        }
    }
}

// Upload response
#[derive(Debug, Serialize)]
pub struct MediaUploadResponse {
    pub media_url: String,
    pub thumbnail_url: Option<String>,
    pub media_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub file_size: i64,
}
