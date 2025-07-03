use sqlx::{PgPool, Row, types::ipnetwork};
use std::net::IpAddr;
use uuid::Uuid;

use crate::{
    error::{AppError, Result},
    models::{
        Post, PostAuthor, PostCommunity, PostFlairResponse, PostListResponse, PostMediaResponse,
        PostResponse, PostSort, TimeRange,
    },
};

pub async fn get_post_by_id_raw(db: &PgPool, post_id: Uuid) -> Result<Option<Post>> {
    let post = sqlx::query_as::<_, Post>("SELECT * FROM posts WHERE id = $1")
        .bind(post_id)
        .fetch_optional(db)
        .await?;

    Ok(post)
}

pub async fn get_post_by_id(
    db: &PgPool,
    post_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<Option<PostResponse>> {
    let post_data = sqlx::query!(
        r#"
        SELECT 
            p.id, p.title, p.content, p.url, p.author_id, p.community_id,
            p.is_nsfw, p.is_spoiler, p.is_locked, p.is_pinned,
            p.upvotes, p.downvotes, p.score, p.comment_count,
            p.view_count, p.share_count, p.created_at, p.updated_at,
            p.post_type::TEXT as post_type_str,
            p.status::TEXT as status_str,
            u.username, u.display_name as user_display_name, u.avatar_url, u.is_verified,
            c.name as community_name, c.display_name as community_display_name, c.icon_url as community_icon,
            CASE WHEN pv.vote_type IS NOT NULL THEN pv.vote_type ELSE NULL END as user_vote,
            CASE WHEN sp.id IS NOT NULL THEN true ELSE false END as is_saved
        FROM posts p
        JOIN users u ON p.author_id = u.id
        JOIN communities c ON p.community_id = c.id
        LEFT JOIN post_votes pv ON p.id = pv.post_id AND pv.user_id = $2
        LEFT JOIN saved_posts sp ON p.id = sp.post_id AND sp.user_id = $2
        WHERE p.id = $1 AND p.status != 'deleted'
        "#,
        post_id,
        user_id
    )
    .fetch_optional(db)
    .await?;

    let Some(row) = post_data else {
        return Ok(None);
    };

    // Get media for this post
    let media = get_post_media(db, post_id).await?;

    // Get flair for this post
    let flair = get_post_flair(db, post_id).await?;

    let post_response = PostResponse {
        id: row.id,
        title: row.title,
        content: row.content,
        url: row.url,
        post_type: row
            .post_type_str
            .as_ref()
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| AppError::Internal(format!("Invalid post_type: {}", e)))?
            .ok_or_else(|| AppError::Internal("Missing post_type_str".to_string()))?,
        status: row
            .status_str
            .as_ref()
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| AppError::Internal(format!("Invalid status: {}", e)))?
            .ok_or_else(|| AppError::Internal("Missing status_str".to_string()))?,
        is_nsfw: row.is_nsfw.unwrap_or_default(),
        is_spoiler: row.is_spoiler.unwrap_or_default(),
        is_locked: row.is_locked.unwrap_or_default(),
        is_pinned: row.is_pinned.unwrap_or_default(),
        author: PostAuthor {
            id: row.author_id,
            username: row.username,
            display_name: row.user_display_name,
            avatar_url: row.avatar_url,
            is_verified: row.is_verified.unwrap_or_default(),
        },
        community: PostCommunity {
            id: row.community_id,
            name: row.community_name,
            display_name: row.community_display_name,
            icon_url: row.community_icon,
        },
        upvotes: row.upvotes.unwrap_or_default(),
        downvotes: row.downvotes.unwrap_or_default(),
        score: row.score.unwrap_or_default(),
        comment_count: row.comment_count.unwrap_or_default(),
        view_count: row.view_count.unwrap_or_default(),
        share_count: row.share_count.unwrap_or_default(),
        created_at: row.created_at.unwrap_or_default(),
        updated_at: row.updated_at.unwrap_or_default(),
        user_vote: row.user_vote,
        is_saved: row.is_saved.unwrap_or_default(),
        media,
        flair,
    };

    Ok(Some(post_response))
}

pub async fn get_posts(
    db: &PgPool,
    user_id: Option<Uuid>,
    community_name: Option<&str>,
    sort: PostSort,
    time_range: &Option<TimeRange>,
    limit: u32,
    offset: u32,
) -> Result<Vec<PostListResponse>> {
    let mut query = r#"
        SELECT 
            p.id, p.title, p.post_type, p.is_nsfw, p.is_spoiler, p.score, 
            p.comment_count, p.created_at,
            u.id as author_id, u.username, u.display_name as user_display_name, 
            u.avatar_url, u.is_verified,
            c.id as community_id, c.name as community_name, 
            c.display_name as community_display_name, c.icon_url as community_icon,
            CASE WHEN pv.vote_type IS NOT NULL THEN pv.vote_type ELSE NULL END as user_vote,
            -- Get thumbnail from media variants
            COALESCE(mv.cdn_url, mv.file_path) as thumbnail_url
        FROM posts p
        JOIN users u ON p.author_id = u.id
        JOIN communities c ON p.community_id = c.id
        LEFT JOIN post_votes pv ON p.id = pv.post_id AND pv.user_id = $1
        LEFT JOIN post_media pm ON p.id = pm.post_id AND pm.media_order = 1
        LEFT JOIN media_files mf ON pm.media_file_id = mf.id
        LEFT JOIN media_variants mv ON mf.id = mv.media_file_id AND mv.variant_type = 'thumbnail'
        WHERE p.status = 'active'
    "#
    .to_string();

    let mut param_count = 1;

    // Add community filter
    if let Some(_community) = community_name {
        param_count += 1;
        query.push_str(&format!(" AND c.name = ${}", param_count));
    }

    // Add time range filter
    if let Some(time) = time_range {
        param_count += 1;
        let time_filter = match time {
            TimeRange::Hour => "p.created_at >= NOW() - INTERVAL '1 hour'",
            TimeRange::Day => "p.created_at >= NOW() - INTERVAL '1 day'",
            TimeRange::Week => "p.created_at >= NOW() - INTERVAL '1 week'",
            TimeRange::Month => "p.created_at >= NOW() - INTERVAL '1 month'",
            TimeRange::Year => "p.created_at >= NOW() - INTERVAL '1 year'",
            TimeRange::All => "",
        };
        if !time_filter.is_empty() {
            query.push_str(&format!(" AND {}", time_filter));
        }
    }

    // Add sorting
    let order_clause = match sort {
        PostSort::Hot => "p.hot_score DESC, p.created_at DESC",
        PostSort::New => "p.created_at DESC",
        PostSort::Top => "p.score DESC, p.created_at DESC",
        PostSort::Rising => "p.score DESC, p.created_at DESC", // Simplified rising algorithm
    };

    query.push_str(&format!(
        " ORDER BY {} LIMIT ${} OFFSET ${}",
        order_clause,
        param_count + 1,
        param_count + 2
    ));

    let mut query_builder = sqlx::query(&query).bind(user_id);

    if let Some(community) = community_name {
        query_builder = query_builder.bind(community);
    }

    query_builder = query_builder.bind(limit as i64).bind(offset as i64);

    let rows = query_builder.fetch_all(db).await?;

    let mut posts = Vec::new();
    for row in rows {
        let flair = get_post_flair(db, row.get("id")).await?;

        posts.push(PostListResponse {
            id: row.get("id"),
            title: row.get("title"),
            post_type: row.get("post_type"),
            is_nsfw: row.get("is_nsfw"),
            is_spoiler: row.get("is_spoiler"),
            author: PostAuthor {
                id: row.get("author_id"),
                username: row.get("username"),
                display_name: row.get("user_display_name"),
                avatar_url: row.get("avatar_url"),
                is_verified: row.get("is_verified"),
            },
            community: PostCommunity {
                id: row.get("community_id"),
                name: row.get("community_name"),
                display_name: row.get("community_display_name"),
                icon_url: row.get("community_icon"),
            },
            score: row.get("score"),
            comment_count: row.get("comment_count"),
            created_at: row.get("created_at"),
            user_vote: row.get("user_vote"),
            thumbnail_url: row.get("thumbnail_url"),
            flair,
        });
    }

    Ok(posts)
}
pub async fn get_posts_count(
    db: &PgPool,
    community_name: Option<&str>,
    time_range: Option<TimeRange>,
) -> Result<u32> {
    let mut query = "SELECT COUNT(*) as count FROM posts p".to_string();

    if community_name.is_some() {
        query.push_str(" JOIN communities c ON p.community_id = c.id");
    }

    query.push_str(" WHERE p.status = 'active'");

    let mut param_count = 0;

    if let Some(_community) = community_name {
        param_count += 1;
        query.push_str(&format!(" AND c.name = ${}", param_count));
    }

    if let Some(time) = time_range {
        let time_filter = match time {
            TimeRange::Hour => "p.created_at >= NOW() - INTERVAL '1 hour'",
            TimeRange::Day => "p.created_at >= NOW() - INTERVAL '1 day'",
            TimeRange::Week => "p.created_at >= NOW() - INTERVAL '1 week'",
            TimeRange::Month => "p.created_at >= NOW() - INTERVAL '1 month'",
            TimeRange::Year => "p.created_at >= NOW() - INTERVAL '1 year'",
            TimeRange::All => "",
        };
        if !time_filter.is_empty() {
            query.push_str(&format!(" AND {}", time_filter));
        }
    }

    let mut query_builder = sqlx::query(&query);

    if let Some(community) = community_name {
        query_builder = query_builder.bind(community);
    }

    let row = query_builder.fetch_one(db).await?;
    Ok(row.get::<i64, _>("count") as u32)
}
pub async fn get_user_posts(
    db: &PgPool,
    author_id: Uuid,
    viewer_id: Option<Uuid>,
    sort: PostSort,
    time_range: Option<TimeRange>,
    limit: u32,
    offset: u32,
) -> Result<Vec<PostListResponse>> {
    let mut query = r#"
        SELECT 
            p.id, p.title, p.post_type, p.is_nsfw, p.is_spoiler, p.score, 
            p.comment_count, p.created_at,
            u.id as author_id, u.username, u.display_name as user_display_name, 
            u.avatar_url, u.is_verified,
            c.id as community_id, c.name as community_name, 
            c.display_name as community_display_name, c.icon_url as community_icon,
            CASE WHEN pv.vote_type IS NOT NULL THEN pv.vote_type ELSE NULL END as user_vote,
            -- Get thumbnail from media variants
            COALESCE(mv.cdn_url, mv.file_path) as thumbnail_url
        FROM posts p
        JOIN users u ON p.author_id = u.id
        JOIN communities c ON p.community_id = c.id
        LEFT JOIN post_votes pv ON p.id = pv.post_id AND pv.user_id = $1
        LEFT JOIN post_media pm ON p.id = pm.post_id AND pm.media_order = 1
        LEFT JOIN media_files mf ON pm.media_file_id = mf.id
        LEFT JOIN media_variants mv ON mf.id = mv.media_file_id AND mv.variant_type = 'thumbnail'
        WHERE p.status = 'active' AND p.author_id = $2
    "#
    .to_string();

    // Add time range filter
    if let Some(time) = time_range {
        let time_filter = match time {
            TimeRange::Hour => "p.created_at >= NOW() - INTERVAL '1 hour'",
            TimeRange::Day => "p.created_at >= NOW() - INTERVAL '1 day'",
            TimeRange::Week => "p.created_at >= NOW() - INTERVAL '1 week'",
            TimeRange::Month => "p.created_at >= NOW() - INTERVAL '1 month'",
            TimeRange::Year => "p.created_at >= NOW() - INTERVAL '1 year'",
            TimeRange::All => "",
        };
        if !time_filter.is_empty() {
            query.push_str(&format!(" AND {}", time_filter));
        }
    }

    // Add sorting
    let order_clause = match sort {
        PostSort::Hot => "p.hot_score DESC, p.created_at DESC",
        PostSort::New => "p.created_at DESC",
        PostSort::Top => "p.score DESC, p.created_at DESC",
        PostSort::Rising => "p.score DESC, p.created_at DESC",
    };

    query.push_str(&format!(" ORDER BY {} LIMIT $3 OFFSET $4", order_clause));

    let rows = sqlx::query(&query)
        .bind(viewer_id)
        .bind(author_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(db)
        .await?;

    let mut posts = Vec::new();
    for row in rows {
        let flair = get_post_flair(db, row.get("id")).await?;

        posts.push(PostListResponse {
            id: row.get("id"),
            title: row.get("title"),
            post_type: row.get("post_type"),
            is_nsfw: row.get("is_nsfw"),
            is_spoiler: row.get("is_spoiler"),
            author: PostAuthor {
                id: row.get("author_id"),
                username: row.get("username"),
                display_name: row.get("user_display_name"),
                avatar_url: row.get("avatar_url"),
                is_verified: row.get("is_verified"),
            },
            community: PostCommunity {
                id: row.get("community_id"),
                name: row.get("community_name"),
                display_name: row.get("community_display_name"),
                icon_url: row.get("community_icon"),
            },
            score: row.get("score"),
            comment_count: row.get("comment_count"),
            created_at: row.get("created_at"),
            user_vote: row.get("user_vote"),
            thumbnail_url: row.get("thumbnail_url"),
            flair,
        });
    }

    Ok(posts)
}

pub async fn get_user_posts_count(db: &PgPool, author_id: Uuid) -> Result<u32> {
    let row = sqlx::query!(
        "SELECT COUNT(*) as count FROM posts WHERE author_id = $1 AND status = 'active'",
        author_id
    )
    .fetch_one(db)
    .await?;

    Ok(row.count.unwrap_or(0) as u32)
}

pub async fn get_saved_posts(
    db: &PgPool,
    user_id: Uuid,
    limit: u32,
    offset: u32,
) -> Result<Vec<PostListResponse>> {
    let query = r#"
        SELECT 
            p.id, p.title, p.post_type, 
            p.is_nsfw, p.is_spoiler, p.score, p.comment_count, p.created_at,
            u.id as author_id, u.username, u.display_name as user_display_name, 
            u.avatar_url, u.is_verified,
            c.id as community_id, c.name as community_name, 
            c.display_name as community_display_name, c.icon_url as community_icon,
            pv.vote_type as user_vote,
            COALESCE(mv.cdn_url, mv.file_path) as thumbnail_url
        FROM saved_posts sp
        JOIN posts p ON sp.post_id = p.id
        JOIN users u ON p.author_id = u.id
        JOIN communities c ON p.community_id = c.id
        LEFT JOIN post_votes pv ON p.id = pv.post_id AND pv.user_id = $1
        LEFT JOIN post_media pm ON p.id = pm.post_id AND pm.media_order = 1
        LEFT JOIN media_files mf ON pm.media_file_id = mf.id
        LEFT JOIN media_variants mv ON mf.id = mv.media_file_id AND mv.variant_type = 'thumbnail'
        WHERE sp.user_id = $1 AND p.status = 'active'
        ORDER BY sp.created_at DESC
        LIMIT $2 OFFSET $3
    "#;

    let rows = sqlx::query(query)
        .bind(user_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(db)
        .await?;

    let mut posts = Vec::new();
    for row in rows {
        let flair = get_post_flair(db, row.get("id")).await?;

        posts.push(PostListResponse {
            id: row.get("id"),
            title: row.get("title"),
            post_type: row.get("post_type"),
            is_nsfw: row.get("is_nsfw"),
            is_spoiler: row.get("is_spoiler"),
            author: PostAuthor {
                id: row.get("author_id"),
                username: row.get("username"),
                display_name: row.get("user_display_name"),
                avatar_url: row.get("avatar_url"),
                is_verified: row.get("is_verified"),
            },
            community: PostCommunity {
                id: row.get("community_id"),
                name: row.get("community_name"),
                display_name: row.get("community_display_name"),
                icon_url: row.get("community_icon"),
            },
            score: row.get("score"),
            comment_count: row.get("comment_count"),
            created_at: row.get("created_at"),
            user_vote: row.get("user_vote"),
            thumbnail_url: row.get("thumbnail_url"),
            flair,
        });
    }

    Ok(posts)
}

pub async fn get_saved_posts_count(db: &PgPool, user_id: Uuid) -> Result<u32> {
    let row = sqlx::query!(
        r#"
        SELECT COUNT(*) as count 
        FROM saved_posts sp
        JOIN posts p ON sp.post_id = p.id
        WHERE sp.user_id = $1 AND p.status = 'active'
        "#,
        user_id
    )
    .fetch_one(db)
    .await?;

    Ok(row.count.unwrap_or(0) as u32)
}

pub async fn record_post_view(
    db: &PgPool,
    post_id: Uuid,
    user_id: Option<Uuid>,
    ip_address: Option<IpAddr>,
) -> Result<()> {
    // Check if view already exists for this user/IP in the last hour to avoid spam
    let existing = if let Some(uid) = user_id {
        sqlx::query(
            r#"
            SELECT id FROM post_views 
            WHERE post_id = $1 AND user_id = $2 
            AND viewed_at > NOW() - INTERVAL '1 hour'
            "#,
        )
        .bind(post_id)
        .bind(uid)
        .fetch_optional(db)
        .await?
    } else if let Some(ip) = ip_address {
        // Convert Option<IpAddr> to Option<IpNetwork>
        let ip_network = ipnetwork::IpNetwork::from(ip);

        sqlx::query(
            r#"
            SELECT id FROM post_views 
            WHERE post_id = $1 AND ip_address = $2 
            AND viewed_at > NOW() - INTERVAL '1 hour'
            "#,
        )
        .bind(post_id)
        .bind(ip_network)
        .fetch_optional(db)
        .await?
    } else {
        None
    };

    if existing.is_none() {
        // Record new view
        // Convert Option<IpAddr> to Option<IpNetwork>
        let ip_network = ip_address.map(|ip| ipnetwork::IpNetwork::from(ip));

        sqlx::query!(
            r#"
            INSERT INTO post_views (id, post_id, user_id, ip_address, viewed_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
            Uuid::new_v4(),
            post_id,
            user_id,
            ip_network,
            chrono::Utc::now()
        )
        .execute(db)
        .await?;

        // Update post view count
        sqlx::query!(
            "UPDATE posts SET view_count = view_count + 1 WHERE id = $1",
            post_id
        )
        .execute(db)
        .await?;
    }

    Ok(())
}

pub async fn update_post_hot_score(db: &PgPool, post_id: Uuid) -> Result<()> {
    // Reddit's hot algorithm: log10(max(|score|, 1)) + (age_in_seconds / 45000)
    // Simplified version for our implementation
    sqlx::query!(
        r#"
        UPDATE posts 
        SET hot_score = CASE 
            WHEN score > 0 THEN 
                LOG(GREATEST(ABS(score), 1)) + (EXTRACT(EPOCH FROM (NOW() - created_at)) / 45000.0)
            ELSE 
                -LOG(GREATEST(ABS(score), 1)) + (EXTRACT(EPOCH FROM (NOW() - created_at)) / 45000.0)
        END
        WHERE id = $1
        "#,
        post_id
    )
    .execute(db)
    .await?;

    Ok(())
}

async fn get_post_media(db: &PgPool, post_id: Uuid) -> Result<Vec<PostMediaResponse>> {
    let media = sqlx::query!(
        r#"
        SELECT 
            pm.id,
            pm.media_order,
            mf.id as media_file_id,
            mf.original_name,
            mf.file_path,
            mf.cdn_url,
            mf.file_type,
            mf.mime_type,
            mf.file_size,
            mf.width,
            mf.height,
            mf.duration,
            -- Get thumbnail variant
            (
                SELECT COALESCE(mv.cdn_url, mv.file_path)
                FROM media_variants mv 
                WHERE mv.media_file_id = mf.id 
                AND mv.variant_type = 'thumbnail'
                LIMIT 1
            ) as thumbnail_url
        FROM post_media pm
        JOIN media_files mf ON pm.media_file_id = mf.id
        WHERE pm.post_id = $1 AND mf.status = 'completed'
        ORDER BY pm.media_order
        "#,
        post_id
    )
    .fetch_all(db)
    .await?;

    Ok(media
        .into_iter()
        .map(|row| PostMediaResponse {
            id: row.id,
            media_url: row.cdn_url.unwrap_or(row.file_path),
            thumbnail_url: row.thumbnail_url,
            media_type: row.mime_type,
            file_size: Some(row.file_size),
            width: row.width,
            height: row.height,
            duration: row.duration,
            media_order: row.media_order.unwrap_or(0),
        })
        .collect())
}

async fn get_post_flair(db: &PgPool, post_id: Uuid) -> Result<Option<PostFlairResponse>> {
    let flair = sqlx::query!(
        r#"
        SELECT cf.id, cf.text, cf.background_color, cf.text_color
        FROM post_flairs pf
        JOIN community_flairs cf ON pf.flair_id = cf.id
        WHERE pf.post_id = $1
        "#,
        post_id
    )
    .fetch_optional(db)
    .await?;

    Ok(flair.map(|row| PostFlairResponse {
        id: row.id,
        text: row.text,
        background_color: row.background_color,
        text_color: row.text_color,
    }))
}
