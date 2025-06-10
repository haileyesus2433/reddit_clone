use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::{Validate, ValidationError};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "community_type", rename_all = "lowercase")]
pub enum CommunityType {
    Public,
    Restricted,
    Private,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "community_status", rename_all = "lowercase")]
pub enum CommunityStatus {
    Active,
    Quarantined,
    Banned,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "membership_role", rename_all = "lowercase")]
pub enum MembershipRole {
    Member,
    Moderator,
    Admin,
    Owner,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Community {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub rules: Option<String>,
    pub icon_url: Option<String>,
    pub banner_url: Option<String>,
    pub community_type: CommunityType,
    pub status: CommunityStatus,
    pub is_nsfw: bool,
    pub subscriber_count: i32,
    pub post_count: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CommunityMembership {
    pub id: Uuid,
    pub user_id: Uuid,
    pub community_id: Uuid,
    pub role: MembershipRole,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CommunityRule {
    pub id: Uuid,
    pub community_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub rule_order: i32,
    pub created_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CommunityFlair {
    pub id: Uuid,
    pub community_id: Uuid,
    pub text: String,
    pub background_color: Option<String>,
    pub text_color: Option<String>,
    pub is_mod_only: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserCommunityFlair {
    pub id: Uuid,
    pub user_id: Uuid,
    pub community_id: Uuid,
    pub flair_id: Option<Uuid>,
    pub custom_text: Option<String>,
    pub created_at: DateTime<Utc>,
}

fn validate_community_name(name: &str) -> Result<(), ValidationError> {
    // Only allow alphanumeric characters, underscores, and hyphens
    // No spaces, must start with letter, 3-50 chars
    let valid = name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        && name.chars().next().map_or(false, |c| c.is_alphabetic())
        && name.len() >= 3
        && name.len() <= 50;

    if valid {
        Ok(())
    } else {
        Err(ValidationError::new(
            "Community name must be 3-50 characters, start with a letter, and contain only letters, numbers, underscores, or hyphens",
        ))
    }
}

// Create community request
#[derive(Debug, Validate, Deserialize)]
pub struct CreateCommunityRequest {
    #[validate(
        length(min = 3, max = 50),
        custom(function = "validate_community_name")
    )]
    pub name: String,
    #[validate(length(min = 1, max = 100))]
    pub display_name: String,
    #[validate(length(max = 1000))]
    pub description: Option<String>,
    pub community_type: CommunityType,
    pub is_nsfw: Option<bool>,
}
// Update community request
#[derive(Debug, Validate, Deserialize)]
pub struct UpdateCommunityRequest {
    #[validate(length(min = 1, max = 100))]
    pub display_name: Option<String>,
    #[validate(length(max = 1000))]
    pub description: Option<String>,
    pub rules: Option<String>,
    pub icon_url: Option<String>,
    pub banner_url: Option<String>,
    pub community_type: Option<CommunityType>,
    pub is_nsfw: Option<bool>,
}

// Community response with membership info
#[derive(Debug, Serialize)]
pub struct CommunityResponse {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub rules: Option<String>,
    pub icon_url: Option<String>,
    pub banner_url: Option<String>,
    pub community_type: CommunityType,
    pub status: CommunityStatus,
    pub is_nsfw: bool,
    pub subscriber_count: i32,
    pub post_count: i32,
    pub created_at: DateTime<Utc>,
    pub user_role: Option<MembershipRole>,
    pub is_member: bool,
}

#[derive(Debug, Serialize)]
pub struct CommunityListResponse {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub subscriber_count: i32,
    pub is_nsfw: bool,
    pub is_member: bool,
}
