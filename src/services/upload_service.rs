use chrono::{Duration, Utc};
use image::ImageFormat;
use sqlx::PgPool;

use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    error::{AppError, Result},
    models::{
        InitiateUploadRequest, InitiateUploadResponse, MediaFile, MediaMetadata, MediaStatus,
        UploadConfig, UploadSession, UploadStatusResponse, UploadType,
    },
};

#[derive(Debug, Clone)]
pub struct UploadService {
    config: UploadConfig,
}

impl UploadService {
    pub fn new(config: UploadConfig) -> Self {
        Self { config }
    }

    pub async fn initiate_upload(
        &self,
        db: &PgPool,
        user_id: Uuid,
        request: InitiateUploadRequest,
    ) -> Result<InitiateUploadResponse> {
        // Validate file size
        let max_size = self
            .config
            .max_file_sizes
            .get(&request.upload_type)
            .ok_or_else(|| AppError::BadRequest("Invalid upload type".to_string()))?;

        if request.file_size > *max_size {
            return Err(AppError::BadRequest(format!(
                "File size {} exceeds maximum allowed size {}",
                request.file_size, max_size
            )));
        }

        // Validate MIME type
        let allowed_types = self
            .config
            .allowed_mime_types
            .get(&request.upload_type)
            .ok_or_else(|| AppError::BadRequest("Invalid upload type".to_string()))?;

        if !allowed_types.contains(&request.mime_type) {
            return Err(AppError::BadRequest(format!(
                "MIME type {} not allowed for upload type {:?}",
                request.mime_type, request.upload_type
            )));
        }

        // Create upload session
        let upload_id = Uuid::new_v4();
        let session_token = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + Duration::minutes(self.config.session_timeout_minutes);

        sqlx::query!(
            r#"
            INSERT INTO upload_sessions (
                id, user_id, session_token, original_filename, total_size,
                upload_type, expires_at, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            upload_id,
            user_id,
            session_token,
            request.filename,
            request.file_size,
            request.upload_type as UploadType,
            expires_at,
            Utc::now()
        )
        .execute(db)
        .await?;

        Ok(InitiateUploadResponse {
            upload_id,
            session_token,
            chunk_size: self.config.chunk_size,
            expires_at,
        })
    }

    pub async fn upload_chunk(
        &self,
        db: &PgPool,
        user_id: Uuid,
        upload_id: Uuid,
        session_token: &str,
        chunk_data: Vec<u8>,
        chunk_index: usize,
    ) -> Result<()> {
        // Validate session
        let row = sqlx::query!(
            r#"
            SELECT id, user_id, session_token, original_filename, total_size,
               COALESCE(uploaded_size, 0) as uploaded_size,
               COALESCE(chunk_count, 0) as chunk_count,
               upload_type as "upload_type: UploadType", status, expires_at, created_at
            FROM upload_sessions
            WHERE id = $1 AND user_id = $2 AND session_token = $3 AND expires_at > NOW()
            "#,
            upload_id,
            user_id,
            session_token
        )
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AppError::NotFound("Upload session not found or expired".to_string()))?;

        let session = UploadSession {
            id: row.id,
            user_id: row.user_id,
            session_token: row.session_token,
            original_filename: row.original_filename,
            total_size: row.total_size,
            uploaded_size: row.uploaded_size.unwrap_or(0),
            chunk_count: row.chunk_count.unwrap_or(0),
            upload_type: row.upload_type as UploadType,
            status: row.status.unwrap_or_else(|| "active".to_string()),
            expires_at: row.expires_at,
            created_at: row.created_at.unwrap_or_else(|| chrono::Utc::now()),
        };

        if session.status != "active" {
            return Err(AppError::BadRequest(
                "Upload session is not active".to_string(),
            ));
        }

        // Create upload directory if it doesn't exist
        let upload_dir = Path::new(&self.config.upload_dir);
        fs::create_dir_all(upload_dir).await?;

        // Create temp file path
        let temp_file_path = upload_dir.join(format!("temp_{}_{}", upload_id, chunk_index));

        // Write chunk to temporary file
        let mut file = fs::File::create(&temp_file_path).await?;
        file.write_all(&chunk_data).await?;
        file.flush().await?;

        // Update session
        sqlx::query!(
            r#"
            UPDATE upload_sessions 
            SET uploaded_size = uploaded_size + $1, chunk_count = chunk_count + 1
            WHERE id = $2
            "#,
            chunk_data.len() as i64,
            upload_id
        )
        .execute(db)
        .await?;

        Ok(())
    }
    pub async fn complete_upload(
        &self,
        db: &PgPool,
        user_id: Uuid,
        upload_id: Uuid,
        session_token: &str,
    ) -> Result<MediaFile> {
        // Get session
        let row = sqlx::query!(
            r#"
            SELECT id, user_id, session_token, original_filename, total_size,
               COALESCE(uploaded_size, 0) as uploaded_size,
               COALESCE(chunk_count, 0) as chunk_count,
               upload_type as "upload_type: UploadType", status, expires_at, created_at
            FROM upload_sessions
            WHERE id = $1 AND user_id = $2 AND session_token = $3
            "#,
            upload_id,
            user_id,
            session_token
        )
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AppError::NotFound("Upload session not found".to_string()))?;

        let session = UploadSession {
            id: row.id,
            user_id: row.user_id,
            session_token: row.session_token,
            original_filename: row.original_filename,
            total_size: row.total_size,
            uploaded_size: row.uploaded_size.unwrap_or(0),
            chunk_count: row.chunk_count.unwrap_or(0),
            upload_type: row.upload_type as UploadType,
            status: row.status.unwrap_or_default(),
            expires_at: row.expires_at,
            created_at: row.created_at.unwrap_or_else(|| chrono::Utc::now()),
        };

        // Combine chunks into final file
        let final_file_path = self.combine_chunks(upload_id, &session).await?;

        // Process the file (get metadata, create thumbnails, etc.)
        let (width, height, duration, mime_type) = self
            .process_file(&final_file_path, &session.upload_type)
            .await?;

        // Create media file record
        let media_file_id = Uuid::new_v4();
        let file_type = self.get_file_type(&mime_type);

        let media_file_row = sqlx::query!(
            r#"
            INSERT INTO media_files (
            id, original_name, file_path, file_type, file_size, mime_type,
            width, height, duration, upload_type, user_id, status, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            RETURNING id, original_name, file_path, cdn_url, file_type, file_size,
                  mime_type, width, height, duration, 
                  upload_type as "upload_type: UploadType", user_id,
                  status as "status: MediaStatus", metadata_json, created_at, processed_at
            "#,
            media_file_id,
            session.original_filename.clone(),
            final_file_path.to_string_lossy().to_string(),
            file_type,
            session.total_size,
            mime_type,
            width.unwrap_or(0),
            height.unwrap_or(0),
            duration.unwrap_or(0),
            session.upload_type.clone() as UploadType,
            user_id,
            MediaStatus::Processing as MediaStatus,
            Utc::now()
        )
        .fetch_one(db)
        .await?;

        let media_file = MediaFile {
            id: media_file_row.id,
            original_name: media_file_row.original_name,
            file_path: media_file_row.file_path,
            cdn_url: media_file_row.cdn_url,
            file_type: media_file_row.file_type,
            file_size: media_file_row.file_size,
            mime_type: media_file_row.mime_type,
            width: media_file_row.width,
            height: media_file_row.height,
            duration: media_file_row.duration,
            upload_type: session.upload_type.clone(),
            user_id: media_file_row.user_id,
            status: media_file_row.status.unwrap_or(MediaStatus::Processing),
            metadata_json: media_file_row.metadata_json,
            created_at: media_file_row
                .created_at
                .unwrap_or_else(|| chrono::Utc::now()),
            processed_at: media_file_row.processed_at,
        };

        // Clean up session and temp files
        self.cleanup_upload_session(db, upload_id).await?;

        // Start background processing (thumbnails, etc.)
        tokio::spawn(async move {
            // This would be handled by a background job queue in production
            // For now, we'll do basic processing inline
        });

        Ok(media_file)
    }

    pub async fn get_upload_status(
        &self,
        db: &PgPool,
        user_id: Uuid,
        upload_id: Uuid,
    ) -> Result<UploadStatusResponse> {
        // Try to get from upload sessions first
        let session_row = sqlx::query!(
            r#"
            SELECT id, user_id, session_token, original_filename, total_size,
                   uploaded_size, chunk_count, upload_type as "upload_type: UploadType",
                   status, expires_at, created_at
            FROM upload_sessions
            WHERE id = $1 AND user_id = $2
            "#,
            upload_id,
            user_id
        )
        .fetch_optional(db)
        .await?;

        if let Some(row) = session_row {
            let session = UploadSession {
                id: row.id,
                user_id: row.user_id,
                session_token: row.session_token,
                original_filename: row.original_filename,
                total_size: row.total_size,
                uploaded_size: row.uploaded_size.unwrap_or(0),
                chunk_count: row.chunk_count.unwrap_or(0),
                upload_type: row.upload_type,
                status: row.status.unwrap_or_else(|| "active".to_string()),
                expires_at: row.expires_at,
                created_at: row.created_at.unwrap_or_else(|| chrono::Utc::now()),
            };

            let progress = if session.total_size > 0 {
                (session.uploaded_size as f64 / session.total_size as f64) * 100.0
            } else {
                0.0
            };

            return Ok(UploadStatusResponse {
                upload_id,
                status: session.status,
                uploaded_size: session.uploaded_size,
                total_size: session.total_size,
                progress,
                file_url: None,
                thumbnail_url: None,
                metadata: None,
            });
        }

        // Check if it's a completed media file
        let media_row = sqlx::query!(
            r#"
            SELECT id, original_name, file_path, cdn_url, file_type, file_size,
                   mime_type, width, height, duration,
                   upload_type as "upload_type: UploadType", user_id,
                   status as "status: MediaStatus", metadata_json, created_at, processed_at
            FROM media_files
            WHERE id = $1 AND user_id = $2
            "#,
            upload_id,
            user_id
        )
        .fetch_optional(db)
        .await?;

        if let Some(row) = media_row {
            let media_file = MediaFile {
                id: row.id,
                original_name: row.original_name,
                file_path: row.file_path,
                cdn_url: row.cdn_url,
                file_type: row.file_type,
                file_size: row.file_size,
                mime_type: row.mime_type,
                width: row.width,
                height: row.height,
                duration: row.duration,
                upload_type: row.upload_type,
                user_id: row.user_id,
                status: row.status.unwrap_or(MediaStatus::Processing),
                metadata_json: row.metadata_json,
                created_at: row.created_at.unwrap_or_else(|| chrono::Utc::now()),
                processed_at: row.processed_at,
            };

            let status = match media_file.status {
                MediaStatus::Uploading => "uploading",
                MediaStatus::Processing => "processing",
                MediaStatus::Completed => "completed",
                MediaStatus::Failed => "failed",
                MediaStatus::Deleted => "deleted",
            };

            return Ok(UploadStatusResponse {
                upload_id,
                status: status.to_string(),
                uploaded_size: media_file.file_size,
                total_size: media_file.file_size,
                progress: 100.0,
                file_url: Some(media_file.file_path),
                thumbnail_url: media_file.cdn_url,
                metadata: Some(MediaMetadata {
                    width: media_file.width,
                    height: media_file.height,
                    duration: media_file.duration,
                    file_size: media_file.file_size,
                    mime_type: media_file.mime_type,
                }),
            });
        }

        Err(AppError::NotFound("Upload not found".to_string()))
    }

    async fn combine_chunks(&self, upload_id: Uuid, session: &UploadSession) -> Result<PathBuf> {
        let upload_dir = Path::new(&self.config.upload_dir);
        let final_file_path =
            upload_dir.join(format!("{}_{}", upload_id, session.original_filename));

        let mut final_file = fs::File::create(&final_file_path).await?;

        // Combine chunks in order
        for chunk_index in 0..session.chunk_count {
            let chunk_path = upload_dir.join(format!("temp_{}_{}", upload_id, chunk_index));
            if chunk_path.exists() {
                let chunk_data = fs::read(&chunk_path).await?;
                final_file.write_all(&chunk_data).await?;
                fs::remove_file(&chunk_path).await?; // Clean up chunk
            }
        }

        final_file.flush().await?;
        Ok(final_file_path)
    }

    async fn process_file(
        &self,
        file_path: &Path,
        upload_type: &UploadType,
    ) -> Result<(Option<i32>, Option<i32>, Option<i32>, String)> {
        let mime_type = mime_guess::from_path(file_path)
            .first_or_octet_stream()
            .to_string();

        match upload_type {
            UploadType::Avatar
            | UploadType::Banner
            | UploadType::PostImage
            | UploadType::CommentImage => {
                // Process image
                let img = image::open(file_path)
                    .map_err(|e| AppError::Internal(format!("Failed to open image: {}", e)))?;

                let height = img.height();
                let width = img.width();
                Ok((Some(width as i32), Some(height as i32), None, mime_type))
            }
            UploadType::PostVideo | UploadType::VideoReply => {
                // For now, we'll just return basic info
                // In production, you'd use FFmpeg to get video metadata
                Ok((None, None, None, mime_type))
            }
            UploadType::VoiceReply => {
                // For now, we'll just return basic info
                // In production, you'd use FFmpeg to get audio duration
                Ok((None, None, None, mime_type))
            }
        }
    }

    fn get_file_type(&self, mime_type: &str) -> String {
        if mime_type.starts_with("image/") {
            "image".to_string()
        } else if mime_type.starts_with("video/") {
            "video".to_string()
        } else if mime_type.starts_with("audio/") {
            "audio".to_string()
        } else {
            "unknown".to_string()
        }
    }

    async fn cleanup_upload_session(&self, db: &PgPool, upload_id: Uuid) -> Result<()> {
        // Delete session
        sqlx::query!("DELETE FROM upload_sessions WHERE id = $1", upload_id)
            .execute(db)
            .await?;

        // Clean up any remaining temp files
        let upload_dir = Path::new(&self.config.upload_dir);
        let mut entries = fs::read_dir(upload_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let file_name = entry.file_name();
            if let Some(name_str) = file_name.to_str() {
                if name_str.starts_with(&format!("temp_{}", upload_id)) {
                    let _ = fs::remove_file(entry.path()).await;
                }
            }
        }

        Ok(())
    }

    pub async fn cancel_upload(&self, db: &PgPool, user_id: Uuid, upload_id: Uuid) -> Result<()> {
        // Update session status
        sqlx::query!(
            "UPDATE upload_sessions SET status = 'cancelled' WHERE id = $1 AND user_id = $2",
            upload_id,
            user_id
        )
        .execute(db)
        .await?;

        // Clean up
        self.cleanup_upload_session(db, upload_id).await?;

        Ok(())
    }

    pub async fn create_thumbnail(
        &self,
        db: &PgPool,
        media_file_id: Uuid,
        source_path: &Path,
        max_width: u32,
        max_height: u32,
    ) -> Result<String> {
        let img = image::open(source_path)
            .map_err(|e| AppError::Internal(format!("Failed to open image: {}", e)))?;

        let thumbnail = img.thumbnail(max_width, max_height);
        let thumb_width = thumbnail.width();
        let thumb_height = thumbnail.height();

        // Generate thumbnail path
        let upload_dir = Path::new(&self.config.upload_dir);
        let thumb_filename = format!("thumb_{}_{}.webp", media_file_id, max_width);
        let thumb_path = upload_dir.join(&thumb_filename);

        // Save thumbnail as WebP for better compression
        thumbnail
            .save_with_format(&thumb_path, ImageFormat::WebP)
            .map_err(|e| AppError::Internal(format!("Failed to save thumbnail: {}", e)))?;

        // Get file size
        let thumb_size = fs::metadata(&thumb_path).await?.len() as i64;

        // Save variant to database
        sqlx::query!(
            r#"
            INSERT INTO media_variants (
                id, media_file_id, variant_type, file_path, width, height, file_size, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            Uuid::new_v4(),
            media_file_id,
            "thumbnail",
            thumb_path.to_string_lossy().to_string(),
            thumb_width as i32,
            thumb_height as i32,
            thumb_size,
            Utc::now()
        )
        .execute(db)
        .await?;

        Ok(thumb_path.to_string_lossy().to_string())
    }

    pub async fn direct_upload(
        &self,
        db: &PgPool,
        user_id: Uuid,
        filename: String,
        file_data: Vec<u8>,
        upload_type: UploadType,
    ) -> Result<MediaFile> {
        // Validate file size
        let max_size = self
            .config
            .max_file_sizes
            .get(&upload_type)
            .ok_or_else(|| AppError::BadRequest("Invalid upload type".to_string()))?;

        if file_data.len() as i64 > *max_size {
            return Err(AppError::BadRequest(format!(
                "File size {} exceeds maximum allowed size {}",
                file_data.len(),
                max_size
            )));
        }

        // Detect MIME type from file content
        let mime_type = self.detect_mime_type(&file_data, &filename)?;

        // Validate MIME type
        let allowed_types = self
            .config
            .allowed_mime_types
            .get(&upload_type)
            .ok_or_else(|| AppError::BadRequest("Invalid upload type".to_string()))?;

        if !allowed_types.contains(&mime_type) {
            return Err(AppError::BadRequest(format!(
                "MIME type {} not allowed for upload type {:?}",
                mime_type, upload_type
            )));
        }

        // Create upload directory if it doesn't exist
        let upload_dir = Path::new(&self.config.upload_dir);
        fs::create_dir_all(upload_dir).await?;

        // Generate unique filename
        let media_file_id = Uuid::new_v4();
        let file_extension = Path::new(&filename)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("bin");
        let final_filename = format!(
            "{}_{}.{}",
            upload_type_to_prefix(&upload_type),
            media_file_id,
            file_extension
        );
        let final_file_path = upload_dir.join(&final_filename);

        // Write file
        fs::write(&final_file_path, &file_data).await?;

        // Process file to get metadata
        let (width, height, duration, _) =
            self.process_file(&final_file_path, &upload_type).await?;
        let file_type = self.get_file_type(&mime_type);

        // Create media file record
        let media_file_row = sqlx::query!(
            r#"
            INSERT INTO media_files (
            id, original_name, file_path, file_type, file_size, mime_type,
            width, height, duration, upload_type, user_id, status, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            RETURNING id, original_name, file_path, cdn_url, file_type, file_size,
                  mime_type, width, height, duration,
                  upload_type as "upload_type: UploadType", user_id,
                  status as "status: MediaStatus", metadata_json, created_at, processed_at
            "#,
            media_file_id,
            filename,
            final_file_path.to_string_lossy().to_string(),
            file_type,
            file_data.len() as i64,
            mime_type,
            width.unwrap_or(0),
            height.unwrap_or(0),
            duration.unwrap_or(0),
            upload_type as UploadType,
            user_id,
            MediaStatus::Processing as MediaStatus,
            Utc::now()
        )
        .fetch_one(db)
        .await?;

        let media_file = MediaFile {
            id: media_file_row.id,
            original_name: media_file_row.original_name,
            file_path: media_file_row.file_path,
            cdn_url: media_file_row.cdn_url,
            file_type: media_file_row.file_type,
            file_size: media_file_row.file_size,
            mime_type: media_file_row.mime_type,
            width: media_file_row.width,
            height: media_file_row.height,
            duration: media_file_row.duration,
            upload_type: media_file_row.upload_type,
            user_id: media_file_row.user_id,
            status: media_file_row.status.unwrap_or(MediaStatus::Processing),
            metadata_json: media_file_row.metadata_json,
            created_at: media_file_row
                .created_at
                .unwrap_or_else(|| chrono::Utc::now()),
            processed_at: media_file_row.processed_at,
        };

        // Create thumbnail for images
        if file_type == "image" {
            let _ = self
                .create_thumbnail(db, media_file_id, &final_file_path, 300, 300)
                .await;
        }

        // Mark as completed
        sqlx::query!(
            "UPDATE media_files SET status = $1, processed_at = $2 WHERE id = $3",
            MediaStatus::Completed as MediaStatus,
            Utc::now(),
            media_file_id
        )
        .execute(db)
        .await?;

        Ok(media_file)
    }

    fn detect_mime_type(&self, file_data: &[u8], filename: &str) -> Result<String> {
        // Check magic bytes for common file types
        if file_data.len() >= 4 {
            match &file_data[0..4] {
                [0xFF, 0xD8, 0xFF, _] => return Ok("image/jpeg".to_string()),
                [0x89, 0x50, 0x4E, 0x47] => return Ok("image/png".to_string()),
                [0x47, 0x49, 0x46, 0x38] => return Ok("image/gif".to_string()),
                [0x52, 0x49, 0x46, 0x46]
                    if file_data.len() >= 12 && &file_data[8..12] == b"WEBP" =>
                {
                    return Ok("image/webp".to_string());
                }
                _ => {}
            }
        }

        // Fall back to filename-based detection
        Ok(mime_guess::from_path(filename)
            .first_or_octet_stream()
            .to_string())
    }

    pub async fn delete_media_file(
        &self,
        db: &PgPool,
        user_id: Uuid,
        media_file_id: Uuid,
    ) -> Result<()> {
        // Get media file
        let row = sqlx::query!(
            r#"
            SELECT id, original_name, file_path, cdn_url, file_type, file_size,
               mime_type, width, height, duration,
               upload_type as "upload_type: UploadType", user_id,
               status as "status: MediaStatus", metadata_json, created_at, processed_at
            FROM media_files
            WHERE id = $1 AND user_id = $2
            "#,
            media_file_id,
            user_id
        )
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AppError::NotFound("Media file not found".to_string()))?;

        let media_file = MediaFile {
            id: row.id,
            original_name: row.original_name,
            file_path: row.file_path,
            cdn_url: row.cdn_url,
            file_type: row.file_type,
            file_size: row.file_size,
            mime_type: row.mime_type,
            width: row.width,
            height: row.height,
            duration: row.duration,
            upload_type: row.upload_type,
            user_id: row.user_id,
            status: row.status.unwrap_or(MediaStatus::Deleted),
            metadata_json: row.metadata_json,
            created_at: row.created_at.unwrap_or_else(|| chrono::Utc::now()),
            processed_at: row.processed_at,
        };

        // Delete physical file
        let file_path = Path::new(&media_file.file_path);
        if file_path.exists() {
            fs::remove_file(file_path).await?;
        }

        // Delete variants
        let variants = sqlx::query!(
            "SELECT file_path FROM media_variants WHERE media_file_id = $1",
            media_file_id
        )
        .fetch_all(db)
        .await?;

        for variant in variants {
            let variant_path = Path::new(&variant.file_path);
            if variant_path.exists() {
                let _ = fs::remove_file(variant_path).await;
            }
        }

        // Mark as deleted in database
        sqlx::query!(
            "UPDATE media_files SET status = $1 WHERE id = $2",
            MediaStatus::Deleted as MediaStatus,
            media_file_id
        )
        .execute(db)
        .await?;

        Ok(())
    }
}

fn upload_type_to_prefix(upload_type: &UploadType) -> &'static str {
    match upload_type {
        UploadType::Avatar => "avatar",
        UploadType::Banner => "banner",
        UploadType::PostImage => "post_img",
        UploadType::PostVideo => "post_vid",
        UploadType::CommentImage => "comment_img",
        UploadType::VoiceReply => "voice",
        UploadType::VideoReply => "video_reply",
    }
}
