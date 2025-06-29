use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, Eq, Hash)]
#[sqlx(type_name = "upload_type", rename_all = "lowercase")]
pub enum UploadType {
    Avatar,
    Banner,
    PostImage,
    PostVideo,
    CommentImage,
    VoiceReply,
    VideoReply,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "media_status", rename_all = "lowercase")]
pub enum MediaStatus {
    Uploading,
    Processing,
    Completed,
    Failed,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MediaFile {
    pub id: Uuid,
    pub original_name: String,
    pub file_path: String,
    pub cdn_url: Option<String>,
    pub file_type: String, // image, video, audio
    pub file_size: i64,
    pub mime_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration: Option<i32>, // in seconds
    pub upload_type: UploadType,
    pub user_id: Uuid,
    pub status: MediaStatus,
    pub metadata_json: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MediaVariant {
    pub id: Uuid,
    pub media_file_id: Uuid,
    pub variant_type: String, // thumbnail, small, medium, large
    pub file_path: String,
    pub cdn_url: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub file_size: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UploadSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub session_token: String,
    pub original_filename: String,
    pub total_size: i64,
    pub uploaded_size: i64,
    pub chunk_count: i32,
    pub upload_type: UploadType,
    pub status: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PostMedia {
    pub id: Uuid,
    pub post_id: Uuid,
    pub media_file_id: Uuid, // Reference to MediaFile
    pub media_order: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CommentMedia {
    pub id: Uuid,
    pub comment_id: Uuid,
    pub media_file_id: Uuid, // Reference to MediaFile
    pub media_order: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Validate, Deserialize)]
pub struct InitiateUploadRequest {
    pub filename: String,
    pub file_size: i64,
    pub mime_type: String,
    pub upload_type: UploadType,
    #[validate(length(max = 500))]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InitiateUploadResponse {
    pub upload_id: Uuid,
    pub session_token: String,
    pub chunk_size: usize,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct UploadStatusResponse {
    pub upload_id: Uuid,
    pub status: String,
    pub uploaded_size: i64,
    pub total_size: i64,
    pub progress: f64,
    pub file_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub metadata: Option<MediaMetadata>,
}

#[derive(Debug, Serialize)]
pub struct MediaMetadata {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration: Option<i32>,
    pub file_size: i64,
    pub mime_type: String,
}

#[derive(Debug, Serialize)]
pub struct MediaUploadResponse {
    pub id: Uuid,
    pub file_url: String,
    pub thumbnail_url: Option<String>,
    pub media_type: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub file_size: i64,
    pub duration: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct PostMediaResponse {
    pub id: Uuid,
    pub media_url: String,
    pub thumbnail_url: Option<String>,
    pub media_type: String,
    pub file_size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration: Option<i32>,
    pub media_order: i32,
}

// #[derive(Debug, Serialize)]
// pub struct CommentMediaResponse {
//     pub id: Uuid,
//     pub media_url: String,
//     pub thumbnail_url: Option<String>,
//     pub media_type: String,
//     pub width: Option<i32>,
//     pub height: Option<i32>,
// }

#[derive(Debug, Clone)]
pub struct UploadConfig {
    pub max_file_sizes: std::collections::HashMap<UploadType, i64>,
    pub allowed_mime_types: std::collections::HashMap<UploadType, Vec<String>>,
    pub upload_dir: String,
    pub chunk_size: usize,
    pub session_timeout_minutes: i64,
}

impl Default for UploadConfig {
    fn default() -> Self {
        use std::collections::HashMap;

        let mut max_file_sizes = HashMap::new();
        max_file_sizes.insert(UploadType::Avatar, 1_048_576); // 1MB
        max_file_sizes.insert(UploadType::Banner, 2_097_152); // 2MB
        max_file_sizes.insert(UploadType::PostImage, 10_485_760); // 10MB
        max_file_sizes.insert(UploadType::PostVideo, 104_857_600); // 100MB
        max_file_sizes.insert(UploadType::CommentImage, 5_242_880); // 5MB
        max_file_sizes.insert(UploadType::VoiceReply, 5_242_880); // 5MB
        max_file_sizes.insert(UploadType::VideoReply, 52_428_800); // 50MB

        let mut allowed_mime_types = HashMap::new();
        allowed_mime_types.insert(
            UploadType::Avatar,
            vec![
                "image/jpeg".to_string(),
                "image/png".to_string(),
                "image/webp".to_string(),
            ],
        );
        allowed_mime_types.insert(
            UploadType::Banner,
            vec![
                "image/jpeg".to_string(),
                "image/png".to_string(),
                "image/webp".to_string(),
            ],
        );
        allowed_mime_types.insert(
            UploadType::PostImage,
            vec![
                "image/jpeg".to_string(),
                "image/png".to_string(),
                "image/webp".to_string(),
                "image/gif".to_string(),
            ],
        );
        allowed_mime_types.insert(
            UploadType::PostVideo,
            vec![
                "video/mp4".to_string(),
                "video/webm".to_string(),
                "video/quicktime".to_string(),
            ],
        );
        allowed_mime_types.insert(
            UploadType::CommentImage,
            vec![
                "image/jpeg".to_string(),
                "image/png".to_string(),
                "image/webp".to_string(),
                "image/gif".to_string(),
            ],
        );
        allowed_mime_types.insert(
            UploadType::VoiceReply,
            vec![
                "audio/mpeg".to_string(),
                "audio/wav".to_string(),
                "audio/webm".to_string(),
                "audio/ogg".to_string(),
            ],
        );
        allowed_mime_types.insert(
            UploadType::VideoReply,
            vec!["video/mp4".to_string(), "video/webm".to_string()],
        );

        Self {
            max_file_sizes,
            allowed_mime_types,
            upload_dir: "./uploads".to_string(),
            chunk_size: 5_242_880, // 5MB chunks
            session_timeout_minutes: 60,
        }
    }
}

impl From<MediaFile> for MediaUploadResponse {
    fn from(media: MediaFile) -> Self {
        Self {
            id: media.id,
            file_url: media.file_path,
            thumbnail_url: None, // Will be populated from variants
            media_type: media.file_type,
            width: media.width,
            height: media.height,
            file_size: media.file_size,
            duration: media.duration,
        }
    }
}
