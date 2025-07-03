use std::str::FromStr;

use crate::models::PostMediaResponse;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "post_type", rename_all = "lowercase")]
pub enum PostType {
    Text,
    Link,
    Image,
    Video,
}

impl FromStr for PostType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" | "Text" => Ok(PostType::Text),
            "link" | "Link" => Ok(PostType::Link),
            "image" | "Image" => Ok(PostType::Image),
            "video" | "Video" => Ok(PostType::Video),
            _ => Err(format!("Unknown PostType: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, Eq)]
#[sqlx(type_name = "post_status", rename_all = "lowercase")]
pub enum PostStatus {
    Active,
    Removed,
    Deleted,
    Spam,
}

impl FromStr for PostStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" | "Active" => Ok(PostStatus::Active),
            "deleted" | "Deleted" => Ok(PostStatus::Deleted),
            "removed" | "Removed" => Ok(PostStatus::Removed),
            "spam" | "Spam" => Ok(PostStatus::Spam),
            _ => Err(format!("Unknown PostStatus: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, sqlx::Decode)]
pub struct Post {
    pub id: Uuid,
    pub title: String,
    pub content: Option<String>,
    pub url: Option<String>,
    pub post_type: PostType,
    pub status: PostStatus,
    pub is_nsfw: bool,
    pub is_spoiler: bool,
    pub is_locked: bool,
    pub is_pinned: bool,
    pub author_id: Uuid,
    pub community_id: Uuid,
    pub upvotes: i32,
    pub downvotes: i32,
    pub score: i32,
    pub comment_count: i32,
    pub view_count: i32,
    pub share_count: i32,
    pub hot_score: rust_decimal::Decimal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PostView {
    pub id: Uuid,
    pub post_id: Uuid,
    pub user_id: Option<Uuid>,
    pub ip_address: Option<std::net::IpAddr>,
    pub user_agent: Option<String>,
    pub viewed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PostReport {
    pub id: Uuid,
    pub post_id: Uuid,
    pub reported_by: Uuid,
    pub reason: String,
    pub description: Option<String>,
    pub status: String,
    pub reviewed_by: Option<Uuid>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PostShare {
    pub id: Uuid,
    pub post_id: Uuid,
    pub user_id: Option<Uuid>,
    pub ip_address: Option<std::net::IpAddr>,
    pub shared_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SavedPost {
    pub id: Uuid,
    pub user_id: Uuid,
    pub post_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PostFlair {
    pub id: Uuid,
    pub post_id: Uuid,
    pub flair_id: Uuid,
    pub created_at: DateTime<Utc>,
}

// Create post request
#[derive(Debug, Validate, Deserialize)]
pub struct CreatePostRequest {
    #[validate(length(min = 1, max = 300))]
    pub title: String,
    pub content: Option<String>,
    #[validate(url)]
    pub url: Option<String>,
    pub post_type: PostType,
    pub community_id: Uuid,
    pub is_nsfw: Option<bool>,
    pub is_spoiler: Option<bool>,
    pub flair_id: Option<Uuid>,
}

// Update post request
#[derive(Debug, Validate, Deserialize)]
pub struct UpdatePostRequest {
    #[validate(length(min = 1, max = 300))]
    pub title: Option<String>,
    pub content: Option<String>,
    pub is_nsfw: Option<bool>,
    pub is_spoiler: Option<bool>,
}

// Post response with additional info
#[derive(Debug, Serialize)]
pub struct PostResponse {
    pub id: Uuid,
    pub title: String,
    pub content: Option<String>,
    pub url: Option<String>,
    pub post_type: PostType,
    pub status: PostStatus,
    pub is_nsfw: bool,
    pub is_spoiler: bool,
    pub is_locked: bool,
    pub is_pinned: bool,
    pub author: PostAuthor,
    pub community: PostCommunity,
    pub upvotes: i32,
    pub downvotes: i32,
    pub score: i32,
    pub comment_count: i32,
    pub view_count: i32,
    pub share_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub user_vote: Option<i16>,
    pub is_saved: bool,
    pub media: Vec<PostMediaResponse>,
    pub flair: Option<PostFlairResponse>,
}

#[derive(Debug, Serialize)]
pub struct PostAuthor {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub is_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct PostCommunity {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub icon_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PostFlairResponse {
    pub id: Uuid,
    pub text: String,
    pub background_color: Option<String>,
    pub text_color: Option<String>,
}

// Post list response (for feeds)
#[derive(Debug, Serialize)]
pub struct PostListResponse {
    pub id: Uuid,
    pub title: String,
    pub post_type: PostType,
    pub is_nsfw: bool,
    pub is_spoiler: bool,
    pub author: PostAuthor,
    pub community: PostCommunity,
    pub score: i32,
    pub comment_count: i32,
    pub created_at: DateTime<Utc>,
    pub user_vote: Option<i16>,
    pub thumbnail_url: Option<String>,
    pub flair: Option<PostFlairResponse>,
}

// Sorting options for posts
#[derive(Debug, Deserialize)]
pub enum PostSort {
    Hot,
    New,
    Top,
    Rising,
}

// Time range for top posts
#[derive(Debug, Deserialize)]
pub enum TimeRange {
    Hour,
    Day,
    Week,
    Month,
    Year,
    All,
}
