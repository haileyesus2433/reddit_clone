use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PostVote {
    pub id: Uuid,
    pub user_id: Uuid,
    pub post_id: Uuid,
    pub vote_type: i16, // -1 for downvote, 1 for upvote
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CommentVote {
    pub id: Uuid,
    pub user_id: Uuid,
    pub comment_id: Uuid,
    pub vote_type: i16, // -1 for downvote, 1 for upvote
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// Vote request
#[derive(Debug, Deserialize)]
pub struct VoteRequest {
    pub vote_type: i16, // -1 for downvote, 0 for remove vote, 1 for upvote
}

// Vote response
#[derive(Debug, Serialize)]
pub struct VoteResponse {
    pub user_vote: Option<i16>,
    pub upvotes: i32,
    pub downvotes: i32,
    pub score: i32,
}
