use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use crate::{
    AppState,
    auth::{AuthUser, OptionalAuthUser},
    error::{AppError, Result},
    models::{
        CommentResponse, CommentSort, CreateCommentRequest, UpdateCommentRequest, VoteRequest,
        VoteResponse,
    },
    services::{comment_service, post_service},
};

#[derive(Debug, Deserialize)]
pub struct GetCommentsQuery {
    pub sort: Option<CommentSort>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

pub async fn create_comment(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<CreateCommentRequest>,
) -> Result<Json<CommentResponse>> {
    payload.validate()?;

    // Check rate limiting
    let rate_limit_key = format!("comment_create:user:{}", auth_user.user_id);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 10, 60)
        .await?
    {
        return Err(AppError::RateLimit);
    }

    // Verify post exists and is not locked
    let post = post_service::get_post_by_id_raw(&state.db, payload.post_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Post not found".to_string()))?;

    if post.is_locked {
        return Err(AppError::BadRequest(
            "Post is locked for comments".to_string(),
        ));
    }

    if post.status != crate::models::PostStatus::Active {
        return Err(AppError::BadRequest(
            "Cannot comment on inactive post".to_string(),
        ));
    }

    // If replying to a comment, verify parent comment exists
    if let Some(parent_id) = payload.parent_comment_id {
        let parent_comment = comment_service::get_comment_by_id_raw(&state.db, parent_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Parent comment not found".to_string()))?;

        if parent_comment.post_id != payload.post_id {
            return Err(AppError::BadRequest(
                "Parent comment is not on the same post".to_string(),
            ));
        }

        if parent_comment.status != crate::models::CommentStatus::Active {
            return Err(AppError::BadRequest(
                "Cannot reply to inactive comment".to_string(),
            ));
        }
    }

    let comment = comment_service::create_comment(&state.db, auth_user.user_id, &payload).await?;

    Ok(Json(comment))
}

pub async fn get_post_comments(
    State(state): State<AppState>,
    Path(post_id): Path<Uuid>,
    Query(params): Query<GetCommentsQuery>,
    auth_user: OptionalAuthUser,
) -> Result<Json<Value>> {
    // Verify post exists
    let _post = post_service::get_post_by_id_raw(&state.db, post_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Post not found".to_string()))?;

    let sort = params.sort.unwrap_or(CommentSort::Best);
    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);

    let viewer_id = auth_user.0.as_ref().map(|user| user.user_id);

    let comments =
        comment_service::get_post_comments(&state.db, post_id, viewer_id, sort, limit, offset)
            .await?;

    Ok(Json(json!({
        "comments": comments,
        "post_id": post_id
    })))
}

pub async fn get_comment(
    State(state): State<AppState>,
    Path(comment_id): Path<Uuid>,
    auth_user: OptionalAuthUser,
) -> Result<Json<CommentResponse>> {
    let viewer_id = auth_user.0.as_ref().map(|user| user.user_id);

    let comment = comment_service::get_comment_by_id(&state.db, comment_id, viewer_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Comment not found".to_string()))?;

    Ok(Json(comment))
}

pub async fn update_comment(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(comment_id): Path<Uuid>,
    Json(payload): Json<UpdateCommentRequest>,
) -> Result<Json<CommentResponse>> {
    payload.validate()?;

    // Get existing comment
    let existing_comment = comment_service::get_comment_by_id_raw(&state.db, comment_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Comment not found".to_string()))?;

    // Check ownership
    if existing_comment.author_id != auth_user.user_id {
        return Err(AppError::Authorization(
            "You can only edit your own comments".to_string(),
        ));
    }

    // Check if comment is still editable
    if existing_comment.status != crate::models::CommentStatus::Active {
        return Err(AppError::BadRequest(
            "Cannot edit inactive comment".to_string(),
        ));
    }

    let updated_comment = comment_service::update_comment(&state.db, comment_id, &payload).await?;

    Ok(Json(updated_comment))
}

pub async fn delete_comment(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(comment_id): Path<Uuid>,
) -> Result<Json<Value>> {
    // Get existing comment
    let existing_comment = comment_service::get_comment_by_id_raw(&state.db, comment_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Comment not found".to_string()))?;

    // Check ownership or moderation permissions
    let can_delete = existing_comment.author_id == auth_user.user_id
        || comment_service::can_user_moderate_comment(&state.db, auth_user.user_id, comment_id)
            .await?;

    if !can_delete {
        return Err(AppError::Authorization(
            "You cannot delete this comment".to_string(),
        ));
    }

    comment_service::delete_comment(&state.db, comment_id).await?;

    Ok(Json(json!({
        "message": "Comment deleted successfully"
    })))
}

pub async fn vote_comment(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(comment_id): Path<Uuid>,
    Json(payload): Json<VoteRequest>,
) -> Result<Json<VoteResponse>> {
    // Validate vote type
    if ![-1, 0, 1].contains(&payload.vote_type) {
        return Err(AppError::BadRequest("Invalid vote type".to_string()));
    }

    // Check rate limiting
    let rate_limit_key = format!("comment_vote:user:{}", auth_user.user_id);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 30, 60)
        .await?
    {
        return Err(AppError::RateLimit);
    }

    // Verify comment exists
    let comment = comment_service::get_comment_by_id_raw(&state.db, comment_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Comment not found".to_string()))?;

    // Prevent self-voting
    if comment.author_id == auth_user.user_id {
        return Err(AppError::BadRequest(
            "Cannot vote on your own comment".to_string(),
        ));
    }

    let vote_response =
        comment_service::vote_comment(&state.db, auth_user.user_id, comment_id, payload.vote_type)
            .await?;

    Ok(Json(vote_response))
}

pub async fn save_comment(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(comment_id): Path<Uuid>,
) -> Result<Json<Value>> {
    // Verify comment exists
    let _comment = comment_service::get_comment_by_id_raw(&state.db, comment_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Comment not found".to_string()))?;

    comment_service::save_comment(&state.db, auth_user.user_id, comment_id).await?;

    Ok(Json(json!({
        "message": "Comment saved successfully"
    })))
}

pub async fn unsave_comment(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(comment_id): Path<Uuid>,
) -> Result<Json<Value>> {
    comment_service::unsave_comment(&state.db, auth_user.user_id, comment_id).await?;

    Ok(Json(json!({
        "message": "Comment unsaved successfully"
    })))
}

pub async fn report_comment(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(comment_id): Path<Uuid>,
    Json(payload): Json<ReportCommentRequest>,
) -> Result<Json<Value>> {
    payload.validate()?;

    // Verify comment exists
    let _comment = comment_service::get_comment_by_id_raw(&state.db, comment_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Comment not found".to_string()))?;

    // Check if user already reported this comment
    let existing = sqlx::query!(
        "SELECT id FROM comment_reports WHERE comment_id = $1 AND reported_by = $2",
        comment_id,
        auth_user.user_id
    )
    .fetch_optional(&state.db)
    .await?;

    if existing.is_some() {
        return Err(AppError::Conflict("Comment already reported".to_string()));
    }

    // Create report
    sqlx::query!(
        r#"
        INSERT INTO comment_reports (id, comment_id, reported_by, reason, description, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        Uuid::new_v4(),
        comment_id,
        auth_user.user_id,
        &payload.reason,
        &payload.description.unwrap_or_default(),
        chrono::Utc::now()
    )
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "Comment reported successfully"
    })))
}

pub async fn get_user_comments(
    State(state): State<AppState>,
    Path(username): Path<String>,
    Query(params): Query<GetCommentsQuery>,
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

    let sort = params.sort.unwrap_or(CommentSort::New);
    let limit = params.limit.unwrap_or(25).min(100);
    let offset = params.offset.unwrap_or(0);

    let viewer_id = auth_user.0.as_ref().map(|user| user.user_id);

    let comments =
        comment_service::get_user_comments(&state.db, user.id, viewer_id, sort, limit, offset)
            .await?;

    Ok(Json(json!({
        "comments": comments,
        "username": username
    })))
}

pub async fn get_saved_comments(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(params): Query<GetCommentsQuery>,
) -> Result<Json<Value>> {
    let limit = params.limit.unwrap_or(25).min(100);
    let offset = params.offset.unwrap_or(0);

    let comments =
        comment_service::get_saved_comments(&state.db, auth_user.user_id, limit, offset).await?;

    Ok(Json(json!({
        "comments": comments
    })))
}

#[derive(Debug, Validate, Deserialize)]
pub struct ReportCommentRequest {
    #[validate(length(min = 1, max = 100))]
    pub reason: String,
    #[validate(length(max = 500))]
    pub description: Option<String>,
}
