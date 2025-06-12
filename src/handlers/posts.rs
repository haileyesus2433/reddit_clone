use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use crate::{
    AppState,
    auth::{AuthUser, OptionalAuthUser},
    error::{AppError, Result},
    models::{
        CreatePostRequest, Post, PostResponse, PostSort, PostStatus, PostType, TimeRange,
        UpdatePostRequest,
    },
    services::{community_service, post_service},
};

#[derive(Debug, Deserialize)]
pub struct GetPostsQuery {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub sort: Option<PostSort>,
    pub time: Option<TimeRange>,
    pub community: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VoteRequest {
    pub vote_type: i16, // -1 for downvote, 1 for upvote, 0 to remove vote
}

pub async fn create_post(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<CreatePostRequest>,
) -> Result<(StatusCode, Json<Value>)> {
    // Validate input
    payload.validate()?;

    // Rate limiting - limit post creation
    let rate_limit_key = format!("create_post:{}", auth_user.user_id);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 10, 3600)
        .await?
    {
        // 10 per hour
        return Err(AppError::RateLimit);
    }

    // Check if community exists and user can post
    let _community = community_service::get_community_by_id(&state.db, payload.community_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Community not found".to_string()))?;

    let can_post = community_service::can_user_post_in_community(
        &state.db,
        auth_user.user_id,
        payload.community_id,
    )
    .await?;

    if !can_post {
        return Err(AppError::Authorization(
            "Cannot post in this community".to_string(),
        ));
    }

    // Validate post content based on type
    match payload.post_type {
        PostType::Text => {
            if payload.content.is_none() || payload.content.as_ref().unwrap().trim().is_empty() {
                return Err(AppError::BadRequest(
                    "Text posts must have content".to_string(),
                ));
            }
        }
        PostType::Link => {
            if payload.url.is_none() {
                return Err(AppError::BadRequest(
                    "Link posts must have a URL".to_string(),
                ));
            }
        }
        PostType::Image | PostType::Video => {
            // Media will be handled separately via upload endpoints
        }
    }

    // Create post
    let post_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    let _post = sqlx::query_as::<_, Post>(
        r#"
        INSERT INTO posts (
            id, title, content, url, post_type, status, is_nsfw, is_spoiler,
            author_id, community_id, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        RETURNING *
        "#,
    )
    .bind(post_id)
    .bind(&payload.title)
    .bind(&payload.content)
    .bind(&payload.url)
    .bind(&payload.post_type)
    .bind(PostStatus::Active)
    .bind(payload.is_nsfw.unwrap_or(false))
    .bind(payload.is_spoiler.unwrap_or(false))
    .bind(auth_user.user_id)
    .bind(payload.community_id)
    .bind(now)
    .bind(now)
    .fetch_one(&state.db)
    .await?;

    // Update community post count
    sqlx::query("UPDATE communities SET post_count = post_count + 1 WHERE id = $1")
        .bind(payload.community_id)
        .execute(&state.db)
        .await?;

    // Calculate initial hot score
    post_service::update_post_hot_score(&state.db, post_id).await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "message": "Post created successfully",
            "post_id": post_id
        })),
    ))
}

pub async fn get_posts(
    State(state): State<AppState>,
    Query(params): Query<GetPostsQuery>,
    auth_user: OptionalAuthUser,
) -> Result<Json<Value>> {
    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(25).min(100); // Max 100 per page
    let offset = (page - 1) * limit;
    let sort = params.sort.unwrap_or(PostSort::Hot);
    let time_range = params.time;

    let user_id = auth_user.0.as_ref().map(|user| user.user_id);

    let posts = post_service::get_posts(
        &state.db,
        user_id,
        params.community.as_deref(),
        sort,
        &time_range,
        limit,
        offset,
    )
    .await?;

    let total_count =
        post_service::get_posts_count(&state.db, params.community.as_deref(), time_range).await?;

    Ok(Json(json!({
        "posts": posts,
        "pagination": {
            "page": page,
            "limit": limit,
            "total": total_count,
            "pages": (total_count + limit - 1) / limit
        }
    })))
}

pub async fn get_post(
    State(state): State<AppState>,
    Path(post_id): Path<Uuid>,
    auth_user: OptionalAuthUser,
) -> Result<Json<PostResponse>> {
    let user_id = auth_user.0.as_ref().map(|user| user.user_id);

    let post = post_service::get_post_by_id(&state.db, post_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Post not found".to_string()))?;

    // Record view if user is authenticated
    if let Some(user_id) = user_id {
        post_service::record_post_view(&state.db, post_id, Some(user_id), None).await?;
    }

    Ok(Json(post))
}

pub async fn update_post(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(post_id): Path<Uuid>,
    Json(payload): Json<UpdatePostRequest>,
) -> Result<Json<Value>> {
    payload.validate()?;

    // Check if post exists and user owns it
    let post = post_service::get_post_by_id_raw(&state.db, post_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Post not found".to_string()))?;

    if post.author_id != auth_user.user_id {
        return Err(AppError::Authorization(
            "Can only edit your own posts".to_string(),
        ));
    }

    // Update post
    sqlx::query(
        r#"
        UPDATE posts 
        SET title = COALESCE($1, title),
            content = COALESCE($2, content),
            is_nsfw = COALESCE($3, is_nsfw),
            is_spoiler = COALESCE($4, is_spoiler),
            updated_at = $5
        WHERE id = $6
        "#,
    )
    .bind(&payload.title)
    .bind(&payload.content)
    .bind(&payload.is_nsfw)
    .bind(&payload.is_spoiler)
    .bind(chrono::Utc::now())
    .bind(post_id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "Post updated successfully"
    })))
}

pub async fn delete_post(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(post_id): Path<Uuid>,
) -> Result<Json<Value>> {
    // Check if post exists and user owns it or is a moderator
    let post = post_service::get_post_by_id_raw(&state.db, post_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Post not found".to_string()))?;

    let can_delete = if post.author_id == auth_user.user_id {
        true
    } else {
        // Check if user is a moderator of the community
        let membership =
            community_service::get_user_membership(&state.db, auth_user.user_id, post.community_id)
                .await?;

        match membership {
            Some(membership) => matches!(
                membership.role,
                crate::models::MembershipRole::Owner
                    | crate::models::MembershipRole::Admin
                    | crate::models::MembershipRole::Moderator
            ),
            None => false,
        }
    };

    if !can_delete {
        return Err(AppError::Authorization(
            "Cannot delete this post".to_string(),
        ));
    }

    // Soft delete the post
    sqlx::query("UPDATE posts SET status = $1, updated_at = $2 WHERE id = $3")
        .bind(PostStatus::Deleted)
        .bind(chrono::Utc::now())
        .bind(post_id)
        .execute(&state.db)
        .await?;

    // Update community post count
    sqlx::query("UPDATE communities SET post_count = post_count - 1 WHERE id = $1")
        .bind(post.community_id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({
        "message": "Post deleted successfully"
    })))
}

pub async fn vote_post(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(post_id): Path<Uuid>,
    Json(payload): Json<VoteRequest>,
) -> Result<Json<Value>> {
    // Validate vote type
    if ![-1, 0, 1].contains(&payload.vote_type) {
        return Err(AppError::BadRequest("Invalid vote type".to_string()));
    }

    // Check if post exists
    let _post = post_service::get_post_by_id_raw(&state.db, post_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Post not found".to_string()))?;

    // Rate limiting for voting
    let rate_limit_key = format!("vote_post:{}", auth_user.user_id);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 100, 3600)
        .await?
    {
        // 100 votes per hour
        return Err(AppError::RateLimit);
    }

    // Handle vote
    if payload.vote_type == 0 {
        // Remove vote
        sqlx::query("DELETE FROM post_votes WHERE user_id = $1 AND post_id = $2")
            .bind(auth_user.user_id)
            .bind(post_id)
            .execute(&state.db)
            .await?;
    } else {
        // Insert or update vote
        sqlx::query(
            r#"
            INSERT INTO post_votes (id, user_id, post_id, vote_type, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (user_id, post_id)
            DO UPDATE SET vote_type = $4, updated_at = $6
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(auth_user.user_id)
        .bind(post_id)
        .bind(payload.vote_type as i16)
        .bind(chrono::Utc::now())
        .bind(chrono::Utc::now())
        .execute(&state.db)
        .await?;
    }

    // Update hot score
    post_service::update_post_hot_score(&state.db, post_id).await?;

    Ok(Json(json!({
        "message": "Vote recorded successfully"
    })))
}

pub async fn save_post(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(post_id): Path<Uuid>,
) -> Result<Json<Value>> {
    // Check if post exists
    let _post = post_service::get_post_by_id_raw(&state.db, post_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Post not found".to_string()))?;

    // Check if already saved
    let existing = sqlx::query!(
        "SELECT id FROM saved_posts WHERE user_id = $1 AND post_id = $2",
        auth_user.user_id,
        post_id
    )
    .fetch_optional(&state.db)
    .await?;

    if existing.is_some() {
        return Err(AppError::Conflict("Post already saved".to_string()));
    }

    // Save post
    sqlx::query(
        "INSERT INTO saved_posts (id, user_id, post_id, created_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(auth_user.user_id)
    .bind(post_id)
    .bind(chrono::Utc::now())
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "Post saved successfully"
    })))
}

pub async fn unsave_post(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(post_id): Path<Uuid>,
) -> Result<Json<Value>> {
    // Remove saved post
    let result = sqlx::query("DELETE FROM saved_posts WHERE user_id = $1 AND post_id = $2")
        .bind(auth_user.user_id)
        .bind(post_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Saved post not found".to_string()));
    }

    Ok(Json(json!({
        "message": "Post unsaved successfully"
    })))
}
pub async fn report_post(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(post_id): Path<Uuid>,
    Json(payload): Json<ReportPostRequest>,
) -> Result<Json<Value>> {
    payload.validate()?;

    // Check if post exists
    let _post = post_service::get_post_by_id_raw(&state.db, post_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Post not found".to_string()))?;

    // Check if user already reported this post
    let existing = sqlx::query!(
        "SELECT id FROM post_reports WHERE post_id = $1 AND reported_by = $2",
        post_id,
        auth_user.user_id
    )
    .fetch_optional(&state.db)
    .await?;

    if existing.is_some() {
        return Err(AppError::Conflict("Post already reported".to_string()));
    }

    // Create report
    sqlx::query(
        r#"
        INSERT INTO post_reports (id, post_id, reported_by, reason, description, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(post_id)
    .bind(auth_user.user_id)
    .bind(&payload.reason)
    .bind(&payload.description)
    .bind(chrono::Utc::now())
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "Post reported successfully"
    })))
}

pub async fn get_user_posts(
    State(state): State<AppState>,
    Path(username): Path<String>,
    Query(params): Query<GetPostsQuery>,
    auth_user: OptionalAuthUser,
) -> Result<Json<Value>> {
    // Get user by username
    let user = sqlx::query!(
        "SELECT id FROM users WHERE username = $1 AND status = 'active'",
        username
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(25).min(100);
    let offset = (page - 1) * limit;
    let sort = params.sort.unwrap_or(PostSort::New);

    let viewer_id = auth_user.0.as_ref().map(|user| user.user_id);

    let posts = post_service::get_user_posts(
        &state.db,
        user.id,
        viewer_id,
        sort,
        params.time,
        limit,
        offset,
    )
    .await?;

    let total_count = post_service::get_user_posts_count(&state.db, user.id).await?;

    Ok(Json(json!({
        "posts": posts,
        "pagination": {
            "page": page,
            "limit": limit,
            "total": total_count,
            "pages": (total_count + limit - 1) / limit
        }
    })))
}

pub async fn get_saved_posts(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(params): Query<GetPostsQuery>,
) -> Result<Json<Value>> {
    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(25).min(100);
    let offset = (page - 1) * limit;

    let posts = post_service::get_saved_posts(&state.db, auth_user.user_id, limit, offset).await?;

    let total_count = post_service::get_saved_posts_count(&state.db, auth_user.user_id).await?;

    Ok(Json(json!({
        "posts": posts,
        "pagination": {
            "page": page,
            "limit": limit,
            "total": total_count,
            "pages": (total_count + limit - 1) / limit
        }
    })))
}

#[derive(Debug, Validate, Deserialize)]
pub struct ReportPostRequest {
    #[validate(length(min = 1, max = 100))]
    pub reason: String,
    #[validate(length(max = 500))]
    pub description: Option<String>,
}
