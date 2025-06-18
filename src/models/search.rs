use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Decode, Type};
use uuid::Uuid;
use validator::Validate;

use crate::models::{CommunityType, PostType};

#[derive(Debug, Deserialize, Validate)]
pub struct SearchQuery {
    #[validate(length(min = 1, max = 200))]
    pub q: String, // search query
    pub search_type: Option<SearchType>,
    pub sort: Option<SearchSort>,
    pub time_range: Option<SearchTimeRange>,
    pub community: Option<String>, // community name
    pub author: Option<String>,    // username
    pub post_type: Option<PostType>,
    pub is_nsfw: Option<bool>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Type)]
#[sqlx(type_name = "search_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum SearchType {
    All,
    Posts,
    Comments,
    Communities,
    Users,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum SearchSort {
    Relevance,
    New,
    Top,
    Comments,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum SearchTimeRange {
    Hour,
    Day,
    Week,
    Month,
    Year,
    All,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub total_results: i64,
    pub search_time_ms: u128,
    pub results: SearchResults,
    pub suggestions: Vec<SearchSuggestion>,
    pub filters: SearchFilters,
}

#[derive(Debug, Serialize)]
pub struct SearchResults {
    pub posts: Vec<SearchPostResult>,
    pub comments: Vec<SearchCommentResult>,
    pub communities: Vec<SearchCommunityResult>,
    pub users: Vec<SearchUserResult>,
}

#[derive(Debug, Serialize)]
pub struct SearchPostResult {
    pub id: Uuid,
    pub title: String,
    pub content: Option<String>,
    pub post_type: PostType,
    pub score: i32,
    pub comment_count: i32,
    pub created_at: DateTime<Utc>,
    pub author: SearchAuthor,
    pub community: SearchCommunity,
    pub is_nsfw: bool,
    pub thumbnail_url: Option<String>,
    pub relevance_score: f32,
    pub highlight: Option<String>, // highlighted search terms
}

#[derive(Debug, Serialize)]
pub struct SearchCommentResult {
    pub id: Uuid,
    pub content: String,
    pub score: i32,
    pub created_at: DateTime<Utc>,
    pub author: SearchAuthor,
    pub post: SearchPost,
    pub relevance_score: f32,
    pub highlight: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchCommunityResult {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub subscriber_count: i32,
    pub post_count: i32,
    pub community_type: CommunityType,
    pub icon_url: Option<String>,
    pub is_nsfw: bool,
    pub relevance_score: f32,
    pub highlight: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchUserResult {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub karma_points: i32,
    pub avatar_url: Option<String>,
    pub is_verified: bool,
    pub created_at: DateTime<Utc>,
    pub relevance_score: f32,
    pub highlight: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchAuthor {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub is_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct SearchCommunity {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub icon_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SearchPost {
    pub id: Uuid,
    pub title: String,
    pub community: SearchCommunity,
}

#[derive(Debug, Serialize)]
pub struct SearchSuggestion {
    pub text: String,
    pub suggestion_type: SuggestionType,
    pub count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SuggestionType {
    Query,
    Community,
    User,
    Tag,
}

#[derive(Debug, Serialize)]
pub struct SearchFilters {
    pub available_communities: Vec<FilterOption>,
    pub available_authors: Vec<FilterOption>,
    pub post_types: Vec<FilterOption>,
    pub time_ranges: Vec<FilterOption>,
}

#[derive(Debug, Serialize)]
pub struct FilterOption {
    pub value: String,
    pub label: String,
    pub count: i64,
}

// Trending and discovery models
#[derive(Debug, Serialize)]
pub struct TrendingResponse {
    pub trending_posts: Vec<TrendingPost>,
    pub trending_communities: Vec<TrendingCommunity>,
    pub trending_topics: Vec<TrendingTopic>,
    pub rising_posts: Vec<TrendingPost>,
}

#[derive(Debug, Serialize)]
pub struct TrendingPost {
    pub id: Uuid,
    pub title: String,
    pub score: i32,
    pub comment_count: i32,
    pub growth_rate: f32, // percentage growth in last hour
    pub author: SearchAuthor,
    pub community: SearchCommunity,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct TrendingCommunity {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub subscriber_count: i32,
    pub growth_rate: f32,
    pub icon_url: Option<String>,
    pub recent_post_count: i32,
}

#[derive(Debug, Serialize)]
pub struct TrendingTopic {
    pub topic: String,
    pub mention_count: i64,
    pub growth_rate: f32,
    pub related_communities: Vec<String>,
}

// Search history and suggestions
#[derive(Debug, Serialize, Decode)]
pub struct SearchHistory {
    pub id: Uuid,
    pub user_id: Uuid,
    pub query: String,
    pub search_type: SearchType,
    pub results_count: Option<i32>,
    pub clicked_result_id: Option<Uuid>,
    pub clicked_result_type: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct AutocompleteQuery {
    #[validate(length(min = 1, max = 100))]
    pub q: String,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct AutocompleteResponse {
    pub suggestions: Vec<AutocompleteSuggestion>,
}

#[derive(Debug, Serialize)]
pub struct AutocompleteSuggestion {
    pub text: String,
    pub suggestion_type: SuggestionType,
    pub icon_url: Option<String>,
    pub subtitle: Option<String>, // e.g., "r/programming • 1.2M members"
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaginationParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}
