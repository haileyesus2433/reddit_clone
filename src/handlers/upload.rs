use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use crate::{
    AppState,
    auth::AuthUser,
    error::{AppError, Result},
    models::{
        InitiateUploadRequest, InitiateUploadResponse, MediaUploadResponse, UploadStatusResponse,
        UploadType,
    },
    services::upload_service::UploadService,
};

#[derive(Debug, Deserialize)]
pub struct ChunkUploadQuery {
    pub upload_id: Uuid,
    pub session_token: String,
    pub chunk_index: usize,
}

#[derive(Debug, Deserialize)]
pub struct CompleteUploadRequest {
    pub upload_id: Uuid,
    pub session_token: String,
}

pub async fn initiate_upload(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<InitiateUploadRequest>,
) -> Result<(StatusCode, Json<InitiateUploadResponse>)> {
    payload.validate()?;

    // Rate limiting
    let rate_limit_key = format!("upload_initiate:user:{}", auth_user.user_id);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 10, 300)
        .await?
    {
        return Err(AppError::RateLimit);
    }

    let upload_service = UploadService::new(state.config.upload_config.clone());
    let response = upload_service
        .initiate_upload(&state.db, auth_user.user_id, payload)
        .await?;

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn upload_chunk(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(params): Query<ChunkUploadQuery>,
    mut multipart: Multipart,
) -> Result<Json<Value>> {
    // Rate limiting
    let rate_limit_key = format!("upload_chunk:user:{}", auth_user.user_id);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 100, 300)
        .await?
    {
        return Err(AppError::RateLimit);
    }

    // Extract chunk data from multipart
    let mut chunk_data = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Internal(format!("{:?}", e)))?
    {
        if field.name() == Some("chunk") {
            chunk_data = field
                .bytes()
                .await
                .map_err(|e| AppError::Internal(format!("{:?}", e)))?
                .to_vec();
            break;
        }
    }

    if chunk_data.is_empty() {
        return Err(AppError::BadRequest("No chunk data provided".to_string()));
    }

    let upload_service = UploadService::new(state.config.upload_config.clone());
    upload_service
        .upload_chunk(
            &state.db,
            auth_user.user_id,
            params.upload_id,
            &params.session_token,
            chunk_data,
            params.chunk_index,
        )
        .await?;

    Ok(Json(json!({
        "message": "Chunk uploaded successfully",
        "chunk_index": params.chunk_index
    })))
}

pub async fn complete_upload(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<CompleteUploadRequest>,
) -> Result<Json<MediaUploadResponse>> {
    let upload_service = UploadService::new(state.config.upload_config.clone());
    let media_file = upload_service
        .complete_upload(
            &state.db,
            auth_user.user_id,
            payload.upload_id,
            &payload.session_token,
        )
        .await?;

    Ok(Json(MediaUploadResponse::from(media_file)))
}

pub async fn get_upload_status(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(upload_id): Path<Uuid>,
) -> Result<Json<UploadStatusResponse>> {
    let upload_service = UploadService::new(state.config.upload_config.clone());
    let status = upload_service
        .get_upload_status(&state.db, auth_user.user_id, upload_id)
        .await?;

    Ok(Json(status))
}

pub async fn cancel_upload(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(upload_id): Path<Uuid>,
) -> Result<Json<Value>> {
    let upload_service = UploadService::new(state.config.upload_config.clone());
    upload_service
        .cancel_upload(&state.db, auth_user.user_id, upload_id)
        .await?;

    Ok(Json(json!({
        "message": "Upload cancelled successfully"
    })))
}

// Direct upload endpoints for smaller files
pub async fn upload_avatar(
    State(state): State<AppState>,
    auth_user: AuthUser,
    mut multipart: Multipart,
) -> Result<Json<MediaUploadResponse>> {
    // Rate limiting
    let rate_limit_key = format!("upload_avatar:user:{}", auth_user.user_id);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 5, 300)
        .await?
    {
        return Err(AppError::RateLimit);
    }

    tracing::debug!("Starting avatar upload for user {}", auth_user.user_id);

    // let (filename, file_data) = extract_file_from_multipart(&mut multipart).await?;

    let (filename, file_data) = extract_file_from_multipart(&mut multipart).await?;

    tracing::debug!("Extracted file: {} ({} bytes)", filename, file_data.len());

    let upload_service = UploadService::new(state.config.upload_config.clone());
    let media_file = upload_service
        .direct_upload(
            &state.db,
            auth_user.user_id,
            filename,
            file_data,
            UploadType::Avatar,
        )
        .await?;

    // Update user avatar URL
    sqlx::query!(
        "UPDATE users SET avatar_url = $1, updated_at = $2 WHERE id = $3",
        media_file.file_path,
        chrono::Utc::now(),
        auth_user.user_id
    )
    .execute(&state.db)
    .await?;

    Ok(Json(MediaUploadResponse::from(media_file)))
}

pub async fn upload_banner(
    State(state): State<AppState>,
    auth_user: AuthUser,
    mut multipart: Multipart,
) -> Result<Json<MediaUploadResponse>> {
    // Rate limiting
    let rate_limit_key = format!("upload_banner:user:{}", auth_user.user_id);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 5, 300)
        .await?
    {
        return Err(AppError::RateLimit);
    }

    let (filename, file_data) = extract_file_from_multipart(&mut multipart).await?;

    let upload_service = UploadService::new(state.config.upload_config.clone());
    let media_file = upload_service
        .direct_upload(
            &state.db,
            auth_user.user_id,
            filename,
            file_data,
            UploadType::Banner,
        )
        .await?;

    // Update user banner URL
    sqlx::query!(
        "UPDATE users SET banner_url = $1, updated_at = $2 WHERE id = $3",
        media_file.file_path,
        chrono::Utc::now(),
        auth_user.user_id
    )
    .execute(&state.db)
    .await?;

    Ok(Json(MediaUploadResponse::from(media_file)))
}

pub async fn upload_post_image(
    State(state): State<AppState>,
    auth_user: AuthUser,
    mut multipart: Multipart,
) -> Result<Json<MediaUploadResponse>> {
    // Rate limiting
    let rate_limit_key = format!("upload_post_image:user:{}", auth_user.user_id);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 20, 300)
        .await?
    {
        return Err(AppError::RateLimit);
    }

    let (filename, file_data) = extract_file_from_multipart(&mut multipart).await?;

    let upload_service = UploadService::new(state.config.upload_config.clone());
    let media_file = upload_service
        .direct_upload(
            &state.db,
            auth_user.user_id,
            filename,
            file_data,
            UploadType::PostImage,
        )
        .await?;

    Ok(Json(MediaUploadResponse::from(media_file)))
}

pub async fn upload_comment_image(
    State(state): State<AppState>,
    auth_user: AuthUser,
    mut multipart: Multipart,
) -> Result<Json<MediaUploadResponse>> {
    // Rate limiting
    let rate_limit_key = format!("upload_comment_image:user:{}", auth_user.user_id);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 30, 300)
        .await?
    {
        return Err(AppError::RateLimit);
    }

    let (filename, file_data) = extract_file_from_multipart(&mut multipart).await?;

    let upload_service = UploadService::new(state.config.upload_config.clone());
    let media_file = upload_service
        .direct_upload(
            &state.db,
            auth_user.user_id,
            filename,
            file_data,
            UploadType::CommentImage,
        )
        .await?;

    Ok(Json(MediaUploadResponse::from(media_file)))
}

pub async fn delete_media(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(media_id): Path<Uuid>,
) -> Result<Json<Value>> {
    let upload_service = UploadService::new(state.config.upload_config.clone());
    upload_service
        .delete_media_file(&state.db, auth_user.user_id, media_id)
        .await?;

    Ok(Json(json!({
        "message": "Media file deleted successfully"
    })))
}

pub async fn get_user_media(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Query(params): Query<MediaQuery>,
) -> Result<Json<Value>> {
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0);
    let upload_type = params.upload_type;

    let mut query = r#"
        SELECT id, original_name, file_path, cdn_url, file_type, file_size,
               mime_type, width, height, duration,
               upload_type as "upload_type: UploadType", user_id,
               status as "status: MediaStatus", metadata_json, created_at, processed_at
        FROM media_files
        WHERE user_id = $1 AND status = 'completed'
    "#
    .to_string();

    let mut bind_count = 1;
    if upload_type.is_some() {
        bind_count += 1;
        query.push_str(&format!(" AND upload_type = ${}", bind_count));
    }

    query.push_str(&format!(
        " ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
        bind_count + 1,
        bind_count + 2
    ));

    let mut query_builder = sqlx::query_as(&query).bind(auth_user.user_id);

    if let Some(ut) = upload_type {
        query_builder = query_builder.bind(ut);
    }

    let media_files: Vec<crate::models::MediaFile> = query_builder
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&state.db)
        .await?;

    let media_responses: Vec<MediaUploadResponse> = media_files
        .into_iter()
        .map(MediaUploadResponse::from)
        .collect();

    Ok(Json(json!({
        "media": media_responses,
        "pagination": {
            "limit": limit,
            "offset": offset,
            "has_more": media_responses.len() == limit as usize
        }
    })))
}

async fn extract_file_from_multipart(multipart: &mut Multipart) -> Result<(String, Vec<u8>)> {
    let mut filename = String::new();
    let mut file_data = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Internal(format!("extract file error {:?}", e)))?
    {
        match field.name() {
            Some("file") => {
                filename = field.file_name().unwrap_or("unknown").to_string();
                file_data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::Internal(format!("{:?}", e)))?
                    .to_vec();
            }
            _ => continue,
        }
    }

    if filename.is_empty() || file_data.is_empty() {
        return Err(AppError::BadRequest("No file provided".to_string()));
    }

    Ok((filename, file_data))
}

#[derive(Debug, Deserialize)]
pub struct MediaQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub upload_type: Option<UploadType>,
}
