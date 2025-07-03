use chrono::Utc;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    error::{AppError, Result},
    models::{
        Comment, CommentAuthor, CommentMediaResponse, CommentResponse, CommentSort, CommentStatus,
        CreateCommentRequest, MembershipRole, UpdateCommentRequest, VoteResponse,
    },
};

pub async fn get_comment_by_id_raw(db: &PgPool, comment_id: Uuid) -> Result<Option<Comment>> {
    let comment = sqlx::query_as::<_, Comment>("SELECT * FROM comments WHERE id = $1")
        .bind(comment_id)
        .fetch_optional(db)
        .await?;

    Ok(comment)
}

pub async fn get_comment_by_id(
    db: &PgPool,
    comment_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<Option<CommentResponse>> {
    let comment_data = sqlx::query!(
        r#"
        SELECT 
            c.id, c.content, c.post_id, c.author_id, c.parent_comment_id, 
            c.status as "status: CommentStatus", c.is_edited, c.upvotes, c.downvotes, 
            c.score, c.reply_count, c.depth, c.created_at, c.updated_at, c.edited_at,
            u.username, u.display_name as user_display_name, u.avatar_url, u.is_verified,
            CASE WHEN cv.vote_type IS NOT NULL THEN cv.vote_type ELSE NULL END as user_vote,
            CASE WHEN sc.id IS NOT NULL THEN true ELSE false END as is_saved
        FROM comments c
        JOIN users u ON c.author_id = u.id
        LEFT JOIN comment_votes cv ON c.id = cv.comment_id AND cv.user_id = $2
        LEFT JOIN saved_comments sc ON c.id = sc.comment_id AND sc.user_id = $2
        WHERE c.id = $1 AND c.status != 'deleted'
        "#,
        comment_id,
        viewer_id
    )
    .fetch_optional(db)
    .await?;

    let Some(row) = comment_data else {
        return Ok(None);
    };

    // Get media for this comment
    let media = get_comment_media(db, comment_id).await?;

    // Get replies for this comment (limited depth to avoid infinite recursion)
    let replies = if row.depth < Some(10) {
        get_comment_replies(db, comment_id, viewer_id, 5, 0).await?
    } else {
        Vec::new()
    };

    let comment = CommentResponse {
        id: row.id,
        content: row.content,
        post_id: row.post_id,
        parent_comment_id: row.parent_comment_id,
        status: serde_json::from_value(
            serde_json::to_value(row.status)
                .map_err(|_e| AppError::Internal("Missing status".to_string()))?,
        )
        .map_err(|e| AppError::Internal(e.to_string()))?,
        is_edited: row.is_edited.unwrap_or_default(),
        upvotes: row.upvotes.unwrap_or_default(),
        downvotes: row.downvotes.unwrap_or_default(),
        score: row.score.unwrap_or_default(),
        reply_count: row.reply_count.unwrap_or_default(),
        depth: row.depth.unwrap_or_default(),
        author: CommentAuthor {
            id: row.author_id,
            username: row.username,
            display_name: row.user_display_name,
            avatar_url: row.avatar_url,
            is_verified: row.is_verified.unwrap_or_default(),
        },
        created_at: row.created_at.unwrap_or_default(),
        updated_at: row.updated_at.unwrap_or_default(),
        edited_at: row.edited_at,
        user_vote: row.user_vote,
        is_saved: row.is_saved.unwrap_or_default(),
        replies,
        media,
    };

    Ok(Some(comment))
}
pub async fn create_comment(
    db: &PgPool,
    author_id: Uuid,
    request: &CreateCommentRequest,
) -> Result<CommentResponse> {
    let comment_id = Uuid::new_v4();
    let now = Utc::now();

    // Insert comment (triggers will handle depth and path)
    sqlx::query!(
        r#"
        INSERT INTO comments (
            id, content, post_id, author_id, parent_comment_id, 
            status, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, 'active', $6, $7)
        "#,
        comment_id,
        &request.content,
        request.post_id,
        author_id,
        request.parent_comment_id,
        now,
        now
    )
    .execute(db)
    .await?;

    // Get the created comment with all details
    get_comment_by_id(db, comment_id, Some(author_id))
        .await?
        .ok_or_else(|| AppError::Internal("Failed to retrieve created comment".to_string()))
}

pub async fn update_comment(
    db: &PgPool,
    comment_id: Uuid,
    request: &UpdateCommentRequest,
) -> Result<CommentResponse> {
    let now = Utc::now();

    sqlx::query!(
        r#"
        UPDATE comments 
        SET content = $1, is_edited = true, edited_at = $2, updated_at = $3
        WHERE id = $4
        "#,
        &request.content,
        now,
        now,
        comment_id
    )
    .execute(db)
    .await?;

    // Get the updated comment
    get_comment_by_id(db, comment_id, None)
        .await?
        .ok_or_else(|| AppError::Internal("Failed to retrieve updated comment".to_string()))
}

pub async fn delete_comment(db: &PgPool, comment_id: Uuid) -> Result<()> {
    // Soft delete - mark as deleted but keep for thread structure
    sqlx::query!(
        r#"
        UPDATE comments 
        SET status = 'deleted', content = '[deleted]', updated_at = NOW()
        WHERE id = $1
        "#,
        comment_id
    )
    .execute(db)
    .await?;

    Ok(())
}

pub async fn get_post_comments(
    db: &PgPool,
    post_id: Uuid,
    viewer_id: Option<Uuid>,
    sort: CommentSort,
    limit: u32,
    offset: u32,
) -> Result<Vec<CommentResponse>> {
    let order_clause = match sort {
        CommentSort::Best => "c.score DESC, c.created_at ASC",
        CommentSort::Top => "c.score DESC, c.created_at ASC",
        CommentSort::New => "c.created_at DESC",
        CommentSort::Old => "c.created_at ASC",
        CommentSort::Controversial => "ABS(c.upvotes - c.downvotes) ASC, c.score DESC",
    };

    let query = format!(
        r#"
        SELECT 
            c.id, c.content, c.post_id, c.author_id, c.parent_comment_id, 
            c.status, c.is_edited, c.upvotes, c.downvotes, 
            c.score, c.reply_count, c.depth, c.created_at, c.updated_at, c.edited_at,
            u.username, u.display_name as user_display_name, u.avatar_url, u.is_verified,
            CASE WHEN cv.vote_type IS NOT NULL THEN cv.vote_type ELSE NULL END as user_vote,
            CASE WHEN sc.id IS NOT NULL THEN true ELSE false END as is_saved
        FROM comments c
        JOIN users u ON c.author_id = u.id
        LEFT JOIN comment_votes cv ON c.id = cv.comment_id AND cv.user_id = $1
        LEFT JOIN saved_comments sc ON c.id = sc.comment_id AND sc.user_id = $1
        WHERE c.post_id = $2 AND c.parent_comment_id IS NULL AND c.status = 'active'
        ORDER BY {}
        LIMIT $3 OFFSET $4
        "#,
        order_clause
    );

    let rows = sqlx::query(&query)
        .bind(viewer_id)
        .bind(post_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(db)
        .await?;

    let mut comments = Vec::new();
    for row in rows {
        let comment_id: Uuid = row.get("id");
        let depth: i32 = row.get("depth");

        // Get media for this comment
        let media = get_comment_media(db, comment_id).await?;

        // Get replies (limited depth)
        let replies = if depth < 10 {
            get_comment_replies(db, comment_id, viewer_id, 10, 0).await?
        } else {
            Vec::new()
        };

        let comment = CommentResponse {
            id: comment_id,
            content: row.get("content"),
            post_id: row.get("post_id"),
            parent_comment_id: row.get("parent_comment_id"),
            status: row.get("status"),
            is_edited: row.get("is_edited"),
            upvotes: row.get("upvotes"),
            downvotes: row.get("downvotes"),
            score: row.get("score"),
            reply_count: row.get("reply_count"),
            depth: row.get("depth"),
            author: CommentAuthor {
                id: row.get("author_id"),
                username: row.get("username"),
                display_name: row.get("user_display_name"),
                avatar_url: row.get("avatar_url"),
                is_verified: row.get("is_verified"),
            },
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            edited_at: row.get("edited_at"),
            user_vote: row.get("user_vote"),
            is_saved: row.get("is_saved"),
            replies,
            media,
        };

        comments.push(comment);
    }

    Ok(comments)
}

use std::future::Future;
use std::pin::Pin;

pub fn get_comment_replies(
    db: &PgPool,
    parent_comment_id: Uuid,
    viewer_id: Option<Uuid>,
    limit: u32,
    offset: u32,
) -> Pin<Box<dyn Future<Output = Result<Vec<CommentResponse>>> + Send + '_>> {
    Box::pin(async move {
        let rows = sqlx::query!(
            r#"
            SELECT 
                c.id, c.content, c.post_id, c.author_id, c.parent_comment_id, 
                c.status as "status: CommentStatus", c.is_edited, c.upvotes, c.downvotes, 
                c.score, c.reply_count, c.depth, c.created_at, c.updated_at, c.edited_at,
                u.username, u.display_name as user_display_name, u.avatar_url, u.is_verified,
                CASE WHEN cv.vote_type IS NOT NULL THEN cv.vote_type ELSE NULL END as user_vote,
                CASE WHEN sc.id IS NOT NULL THEN true ELSE false END as is_saved
            FROM comments c
            JOIN users u ON c.author_id = u.id
            LEFT JOIN comment_votes cv ON c.id = cv.comment_id AND cv.user_id = $1
            LEFT JOIN saved_comments sc ON c.id = sc.comment_id AND sc.user_id = $1
            WHERE c.parent_comment_id = $2 AND c.status = 'active'
            ORDER BY c.score DESC, c.created_at ASC
            LIMIT $3 OFFSET $4
            "#,
            viewer_id,
            parent_comment_id,
            limit as i64,
            offset as i64
        )
        .fetch_all(db)
        .await?;

        let mut replies = Vec::new();
        for row in rows {
            let comment_id = row.id;
            let depth = row.depth;

            // Get media for this comment
            let media = get_comment_media(db, comment_id).await?;

            // Recursively get nested replies (with depth limit)
            let nested_replies = if depth < Some(10) {
                get_comment_replies(db, comment_id, viewer_id, 5, 0).await?
            } else {
                Vec::new()
            };

            let reply = CommentResponse {
                id: comment_id,
                content: row.content,
                post_id: row.post_id,
                parent_comment_id: row.parent_comment_id,
                status: serde_json::from_value(
                    serde_json::to_value(row.status)
                        .map_err(|_e| AppError::Internal("Missing status".to_string()))?,
                )
                .map_err(|e| AppError::Internal(e.to_string()))?,
                is_edited: row.is_edited.unwrap_or_default(),
                upvotes: row.upvotes.unwrap_or_default(),
                downvotes: row.downvotes.unwrap_or_default(),
                score: row.score.unwrap_or_default(),
                reply_count: row.reply_count.unwrap_or_default(),
                depth: row.depth.unwrap_or_default(),
                author: CommentAuthor {
                    id: row.author_id,
                    username: row.username,
                    display_name: row.user_display_name,
                    avatar_url: row.avatar_url,
                    is_verified: row.is_verified.unwrap_or_default(),
                },
                created_at: row.created_at.unwrap_or_default(),
                updated_at: row.updated_at.unwrap_or_default(),
                edited_at: row.edited_at,
                user_vote: row.user_vote,
                is_saved: row.is_saved.unwrap_or_default(),
                replies: nested_replies,
                media,
            };

            replies.push(reply);
        }

        Ok(replies)
    })
}

pub async fn vote_comment(
    db: &PgPool,
    user_id: Uuid,
    comment_id: Uuid,
    vote_type: i16,
) -> Result<VoteResponse> {
    let mut tx = db.begin().await?;

    if vote_type == 0 {
        // Remove vote
        sqlx::query!(
            "DELETE FROM comment_votes WHERE user_id = $1 AND comment_id = $2",
            user_id,
            comment_id
        )
        .execute(&mut *tx)
        .await?;
    } else {
        // Insert or update vote
        sqlx::query!(
            r#"
            INSERT INTO comment_votes (id, user_id, comment_id, vote_type, created_at, updated_at)
            VALUES ($1, $2, $3, $4, NOW(), NOW())
            ON CONFLICT (user_id, comment_id)
            DO UPDATE SET vote_type = $4, updated_at = NOW()
            "#,
            Uuid::new_v4(),
            user_id,
            comment_id,
            vote_type
        )
        .execute(&mut *tx)
        .await?;
    }

    // Get updated vote counts
    let vote_data = sqlx::query!(
        r#"
        SELECT upvotes, downvotes, score,
               CASE WHEN cv.vote_type IS NOT NULL THEN cv.vote_type ELSE NULL END as user_vote
        FROM comments c
        LEFT JOIN comment_votes cv ON c.id = cv.comment_id AND cv.user_id = $1
        WHERE c.id = $2
        "#,
        user_id,
        comment_id
    )
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(VoteResponse {
        user_vote: vote_data.user_vote,
        upvotes: vote_data.upvotes.unwrap_or_default(),
        downvotes: vote_data.downvotes.unwrap_or_default(),
        score: vote_data.score.unwrap_or_default(),
    })
}

pub async fn save_comment(db: &PgPool, user_id: Uuid, comment_id: Uuid) -> Result<()> {
    sqlx::query!(
        r#"
        INSERT INTO saved_comments (id, user_id, comment_id, created_at)
        VALUES ($1, $2, $3, NOW())
        ON CONFLICT (user_id, comment_id) DO NOTHING
        "#,
        Uuid::new_v4(),
        user_id,
        comment_id
    )
    .execute(db)
    .await?;

    Ok(())
}

pub async fn unsave_comment(db: &PgPool, user_id: Uuid, comment_id: Uuid) -> Result<()> {
    sqlx::query!(
        "DELETE FROM saved_comments WHERE user_id = $1 AND comment_id = $2",
        user_id,
        comment_id
    )
    .execute(db)
    .await?;

    Ok(())
}

pub async fn get_user_comments(
    db: &PgPool,
    user_id: Uuid,
    viewer_id: Option<Uuid>,
    sort: CommentSort,
    limit: u32,
    offset: u32,
) -> Result<Vec<CommentResponse>> {
    let order_clause = match sort {
        CommentSort::Best => "c.score DESC, c.created_at DESC",
        CommentSort::Top => "c.score DESC, c.created_at DESC",
        CommentSort::New => "c.created_at DESC",
        CommentSort::Old => "c.created_at ASC",
        CommentSort::Controversial => "ABS(c.upvotes - c.downvotes) ASC, c.score DESC",
    };

    let query = format!(
        r#"
        SELECT 
            c.id, c.content, c.post_id, c.author_id, c.parent_comment_id, 
            c.status as "status: CommentStatus", c.is_edited, c.upvotes, c.downvotes, 
            c.score, c.reply_count, c.depth, c.created_at, c.updated_at, c.edited_at,
            u.username, u.display_name as user_display_name, u.avatar_url, u.is_verified,
            CASE WHEN cv.vote_type IS NOT NULL THEN cv.vote_type ELSE NULL END as user_vote,
            CASE WHEN sc.id IS NOT NULL THEN true ELSE false END as is_saved
        FROM comments c
        JOIN users u ON c.author_id = u.id
        LEFT JOIN comment_votes cv ON c.id = cv.comment_id AND cv.user_id = $1
        LEFT JOIN saved_comments sc ON c.id = sc.comment_id AND sc.user_id = $1
        WHERE c.author_id = $2 AND c.status::TEXT as status_str != 'deleted'
        ORDER BY {}
        LIMIT $3 OFFSET $4
        "#,
        order_clause
    );

    let rows = sqlx::query(&query)
        .bind(viewer_id)
        .bind(user_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(db)
        .await?;

    let mut comments = Vec::new();
    for row in rows {
        let comment_id: Uuid = row.get("id");

        // Get media for this comment
        let media = get_comment_media(db, comment_id).await?;

        let comment = CommentResponse {
            id: comment_id,
            content: row.get("content"),
            post_id: row.get("post_id"),
            parent_comment_id: row.get("parent_comment_id"),
            status: serde_json::from_value(
                serde_json::to_value(row.get::<serde_json::Value, _>("status"))
                    .map_err(|_e| AppError::Internal("Missing status".to_string()))?,
            )
            .map_err(|e| AppError::Internal(e.to_string()))?,

            is_edited: row.get("is_edited"),
            upvotes: row.get("upvotes"),
            downvotes: row.get("downvotes"),
            score: row.get("score"),
            reply_count: row.get("reply_count"),
            depth: row.get("depth"),
            author: CommentAuthor {
                id: row.get("author_id"),
                username: row.get("username"),
                display_name: row.get("user_display_name"),
                avatar_url: row.get("avatar_url"),
                is_verified: row.get("is_verified"),
            },
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            edited_at: row.get("edited_at"),
            user_vote: row.get("user_vote"),
            is_saved: row.get("is_saved"),
            replies: Vec::new(), // Don't load replies for user comment lists
            media,
        };

        comments.push(comment);
    }

    Ok(comments)
}

pub async fn get_saved_comments(
    db: &PgPool,
    user_id: Uuid,
    limit: u32,
    offset: u32,
) -> Result<Vec<CommentResponse>> {
    let rows = sqlx::query!(
        r#"
        SELECT 
            c.id, c.content, c.post_id, c.author_id, c.parent_comment_id, 
            c.status as "status: CommentStatus", c.is_edited, c.upvotes, c.downvotes, 
            c.score, c.reply_count, c.depth, c.created_at, c.updated_at, c.edited_at,
            u.username, u.display_name as user_display_name, u.avatar_url, u.is_verified,
            CASE WHEN cv.vote_type IS NOT NULL THEN cv.vote_type ELSE NULL END as user_vote
        FROM saved_comments sc
        JOIN comments c ON sc.comment_id = c.id
        JOIN users u ON c.author_id = u.id
        LEFT JOIN comment_votes cv ON c.id = cv.comment_id AND cv.user_id = $1
        WHERE sc.user_id = $1 AND c.status = 'active'
        ORDER BY sc.created_at DESC
        LIMIT $2 OFFSET $3
        "#,
        user_id,
        limit as i64,
        offset as i64
    )
    .fetch_all(db)
    .await?;

    let mut comments = Vec::new();
    for row in rows {
        let comment_id = row.id;

        // Get media for this comment
        let media = get_comment_media(db, comment_id).await?;

        let comment = CommentResponse {
            id: comment_id,
            content: row.content,
            post_id: row.post_id,
            parent_comment_id: row.parent_comment_id,
            status: serde_json::from_value(
                serde_json::to_value(row.status)
                    .map_err(|_e| AppError::Internal("Missing status".to_string()))?,
            )
            .map_err(|e| AppError::Internal(e.to_string()))?,
            is_edited: row.is_edited.unwrap_or_default(),
            upvotes: row.upvotes.unwrap_or_default(),
            downvotes: row.downvotes.unwrap_or_default(),
            score: row.score.unwrap_or_default(),
            reply_count: row.reply_count.unwrap_or_default(),
            depth: row.depth.unwrap_or_default(),
            author: CommentAuthor {
                id: row.author_id,
                username: row.username,
                display_name: row.user_display_name,
                avatar_url: row.avatar_url,
                is_verified: row.is_verified.unwrap_or_default(),
            },
            created_at: row.created_at.unwrap_or_default(),
            updated_at: row.updated_at.unwrap_or_default(),
            edited_at: row.edited_at,
            user_vote: row.user_vote,
            is_saved: true,      // Always true for saved comments
            replies: Vec::new(), // Don't load replies for saved comment lists
            media,
        };

        comments.push(comment);
    }

    Ok(comments)
}

pub async fn can_user_moderate_comment(
    db: &PgPool,
    user_id: Uuid,
    comment_id: Uuid,
) -> Result<bool> {
    // Get the comment's post and community
    let comment_info = sqlx::query!(
        r#"
        SELECT p.community_id
        FROM comments c
        JOIN posts p ON c.post_id = p.id
        WHERE c.id = $1
        "#,
        comment_id
    )
    .fetch_optional(db)
    .await?;

    let Some(info) = comment_info else {
        return Ok(false);
    };

    // Check if user is a moderator or admin of the community
    let membership = sqlx::query!(
        r#"
        SELECT role as "role: MembershipRole" FROM community_memberships 
        WHERE user_id = $1 AND community_id = $2 
        AND role IN ('moderator', 'admin', 'owner')
        "#,
        user_id,
        info.community_id
    )
    .fetch_optional(db)
    .await?;

    Ok(membership.is_some())
}

async fn get_comment_media(db: &PgPool, comment_id: Uuid) -> Result<Vec<CommentMediaResponse>> {
    let media_rows = sqlx::query!(
        r#"
        SELECT 
            cm.id,
            mf.cdn_url as media_url,
            -- Get the first thumbnail variant if exists
            (
                SELECT mv.cdn_url FROM media_variants mv 
                WHERE mv.media_file_id = mf.id AND mv.variant_type = 'thumbnail'
                ORDER BY mv.created_at ASC LIMIT 1
            ) as thumbnail_url,
            mf.file_type as media_type,
            mf.width,
            mf.height
        FROM comment_media cm
        JOIN media_files mf ON cm.media_file_id = mf.id
        WHERE cm.comment_id = $1
        ORDER BY cm.media_order ASC
        "#,
        comment_id
    )
    .fetch_all(db)
    .await?;

    let media = media_rows
        .into_iter()
        .map(|row| CommentMediaResponse {
            id: row.id,
            media_url: row.media_url.unwrap_or_default(),
            thumbnail_url: row.thumbnail_url,
            media_type: row.media_type,
            width: row.width,
            height: row.height,
        })
        .collect();

    Ok(media)
}
