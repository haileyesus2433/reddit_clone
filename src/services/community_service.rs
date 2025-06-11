use serde::Serialize;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    error::Result,
    models::{Community, CommunityListResponse, CommunityMembership, MembershipRole},
};

#[derive(Debug, Serialize)]
pub struct CommunityWithMembership {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub subscriber_count: i32,
    pub is_nsfw: bool,
    pub is_member: bool,
}
pub async fn get_community_by_name(db: &PgPool, name: &str) -> Result<Option<Community>> {
    let community = sqlx::query_as::<_, Community>(
        "SELECT * FROM communities WHERE name = $1 AND status = 'active'",
    )
    .bind(name)
    .fetch_optional(db)
    .await?;

    Ok(community)
}

pub async fn get_community_by_id(db: &PgPool, id: Uuid) -> Result<Option<Community>> {
    let community = sqlx::query_as::<_, Community>(
        "SELECT * FROM communities WHERE id = $1 AND status = 'active'",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;

    Ok(community)
}

pub async fn get_user_membership(
    db: &PgPool,
    user_id: Uuid,
    community_id: Uuid,
) -> Result<Option<CommunityMembership>> {
    let membership = sqlx::query_as::<_, CommunityMembership>(
        "SELECT * FROM community_memberships WHERE user_id = $1 AND community_id = $2",
    )
    .bind(user_id)
    .bind(community_id)
    .fetch_optional(db)
    .await?;

    Ok(membership)
}

pub async fn get_communities_with_membership(
    db: &PgPool,
    user_id: Option<Uuid>,
    limit: u32,
    offset: u32,
    sort: &str,
    search: Option<&str>,
) -> Result<Vec<CommunityListResponse>> {
    let order_clause = match sort {
        "new" => "c.created_at DESC",
        "top" => "c.subscriber_count DESC",
        _ => "c.subscriber_count DESC", // default to popular
    };

    let mut query = format!(
        r#"
        SELECT c.id, c.name, c.display_name, c.description, c.icon_url, 
               c.subscriber_count, c.is_nsfw,
               CASE WHEN cm.user_id IS NOT NULL THEN true ELSE false END as is_member
        FROM communities c
        LEFT JOIN community_memberships cm ON c.id = cm.community_id AND cm.user_id = $1
        WHERE c.status = 'active'
        "#
    );

    if let Some(_search_term) = search {
        query.push_str(
            " AND (c.name ILIKE $4 OR c.display_name ILIKE $4 OR c.description ILIKE $4)",
        );
    }

    query.push_str(&format!(" ORDER BY {} LIMIT $2 OFFSET $3", order_clause));

    let mut query_builder = sqlx::query(&query)
        .bind(user_id)
        .bind(limit as i64)
        .bind(offset as i64);

    if let Some(search_term) = search {
        let search_pattern = format!("%{}%", search_term);
        query_builder = query_builder.bind(search_pattern);
    }

    let rows = query_builder.fetch_all(db).await?;

    let communities: Vec<CommunityListResponse> = rows
        .into_iter()
        .map(|row| CommunityListResponse {
            id: row.get("id"),
            name: row.get("name"),
            display_name: row.get("display_name"),
            description: row.get("description"),
            icon_url: row.get("icon_url"),
            subscriber_count: row.get("subscriber_count"),
            is_nsfw: row.get("is_nsfw"),
            is_member: row.get("is_member"),
        })
        .collect();

    Ok(communities)
}

pub async fn get_communities_count(db: &PgPool, search: Option<&str>) -> Result<u32> {
    let mut query = "SELECT COUNT(*) as count FROM communities WHERE status = 'active'".to_string();

    let count = if let Some(search_term) = search {
        query.push_str(" AND (name ILIKE $1 OR display_name ILIKE $1 OR description ILIKE $1)");
        let search_pattern = format!("%{}%", search_term);
        sqlx::query(&query)
            .bind(search_pattern)
            .fetch_one(db)
            .await?
    } else {
        sqlx::query(&query).fetch_one(db).await?
    };

    Ok(count.get::<i64, _>("count") as u32)
}

pub async fn create_default_rules(db: &PgPool, community_id: Uuid) -> Result<()> {
    let default_rules = vec![
        (
            "Be respectful",
            Some("Treat all community members with respect and courtesy."),
        ),
        (
            "No spam",
            Some("Do not post repetitive content or advertisements."),
        ),
        (
            "Stay on topic",
            Some("Keep posts relevant to the community's purpose."),
        ),
        (
            "No harassment",
            Some("Harassment, bullying, or personal attacks are not tolerated."),
        ),
        (
            "Follow Reddit's content policy",
            Some("All posts must comply with Reddit's site-wide rules."),
        ),
    ];

    for (index, (title, description)) in default_rules.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO community_rules (id, community_id, title, description, rule_order, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#
        )
        .bind(Uuid::new_v4())
        .bind(community_id)
        .bind(title)
        .bind(description)
        .bind((index + 1) as i32)
        .bind(chrono::Utc::now())
        .execute(db)
        .await?;
    }

    Ok(())
}

pub async fn get_user_communities(
    db: &PgPool,
    user_id: Uuid,
    limit: u32,
    offset: u32,
) -> Result<Vec<CommunityListResponse>> {
    let communities = sqlx::query!(
        r#"
        SELECT c.id, c.name, c.display_name, c.description, c.icon_url, 
               c.subscriber_count, c.is_nsfw, cm.role as "role: MembershipRole"
        FROM communities c
        JOIN community_memberships cm ON c.id = cm.community_id
        WHERE cm.user_id = $1 AND c.status = 'active'
        ORDER BY cm.joined_at DESC
        LIMIT $2 OFFSET $3
        "#,
        user_id,
        limit as i64,
        offset as i64
    )
    .fetch_all(db)
    .await?;

    let result: Vec<CommunityListResponse> = communities
        .into_iter()
        .map(|row| CommunityListResponse {
            id: row.id,
            name: row.name,
            display_name: row.display_name,
            description: row.description,
            icon_url: row.icon_url,
            subscriber_count: row.subscriber_count.unwrap_or(0),
            is_nsfw: row.is_nsfw.unwrap_or(false),
            is_member: true, // Always true since we're querying user's communities
        })
        .collect();

    Ok(result)
}

pub async fn get_moderated_communities(
    db: &PgPool,
    user_id: Uuid,
) -> Result<Vec<CommunityListResponse>> {
    let communities = sqlx::query!(
        r#"
        SELECT c.id, c.name, c.display_name, c.description, c.icon_url, 
               c.subscriber_count, c.is_nsfw, cm.role as "role: MembershipRole"
        FROM communities c
        JOIN community_memberships cm ON c.id = cm.community_id
        WHERE cm.user_id = $1 AND c.status = 'active'
        AND cm.role IN ('owner', 'admin', 'moderator')
        ORDER BY cm.joined_at DESC
        "#,
        user_id
    )
    .fetch_all(db)
    .await?;

    let result: Vec<CommunityListResponse> = communities
        .into_iter()
        .map(|row| CommunityListResponse {
            id: row.id,
            name: row.name,
            display_name: row.display_name,
            description: row.description,
            icon_url: row.icon_url,
            subscriber_count: row.subscriber_count.unwrap_or(0),
            is_nsfw: row.is_nsfw.unwrap_or(false),
            is_member: true,
        })
        .collect();

    Ok(result)
}

pub async fn search_communities(
    db: &PgPool,
    query: &str,
    user_id: Option<Uuid>,
    limit: u32,
    offset: u32,
) -> Result<Vec<CommunityListResponse>> {
    let search_pattern = format!("%{}%", query);

    let communities = sqlx::query!(
        r#"
        SELECT c.id, c.name, c.display_name, c.description, c.icon_url, 
               c.subscriber_count, c.is_nsfw,
               CASE WHEN cm.user_id IS NOT NULL THEN true ELSE false END as is_member
        FROM communities c
        LEFT JOIN community_memberships cm ON c.id = cm.community_id AND cm.user_id = $1
        WHERE c.status = 'active'
        AND (c.name ILIKE $2 OR c.display_name ILIKE $2 OR c.description ILIKE $2)
        ORDER BY 
            CASE 
                WHEN c.name ILIKE $2 THEN 1
                WHEN c.display_name ILIKE $2 THEN 2
                ELSE 3
            END,
            c.subscriber_count DESC
        LIMIT $3 OFFSET $4
        "#,
        user_id,
        search_pattern,
        limit as i64,
        offset as i64
    )
    .fetch_all(db)
    .await?;

    let result: Vec<CommunityListResponse> = communities
        .into_iter()
        .map(|row| CommunityListResponse {
            id: row.id,
            name: row.name,
            display_name: row.display_name,
            description: row.description,
            icon_url: row.icon_url,
            subscriber_count: row.subscriber_count.unwrap_or(0),
            is_nsfw: row.is_nsfw.unwrap_or(false),
            is_member: row.is_member.unwrap_or(false),
        })
        .collect();

    Ok(result)
}

pub async fn get_community_stats(db: &PgPool, community_id: Uuid) -> Result<serde_json::Value> {
    let stats = sqlx::query!(
        r#"
        SELECT 
            (SELECT COUNT(*) FROM community_memberships WHERE community_id = $1) as member_count,
            (SELECT COUNT(*) FROM posts WHERE community_id = $1 AND status != 'deleted') as post_count,
            (SELECT COUNT(*) FROM comments c 
             JOIN posts p ON c.post_id = p.id 
             WHERE p.community_id = $1 AND c.status != 'deleted') as comment_count,
            (SELECT COUNT(*) FROM community_memberships 
             WHERE community_id = $1 AND role IN ('owner', 'admin', 'moderator')) as moderator_count
        "#,
        community_id
    )
    .fetch_one(db)
    .await?;

    Ok(serde_json::json!({
        "member_count": stats.member_count.unwrap_or(0),
        "post_count": stats.post_count.unwrap_or(0),
        "comment_count": stats.comment_count.unwrap_or(0),
        "moderator_count": stats.moderator_count.unwrap_or(0)
    }))
}

pub async fn is_community_member(db: &PgPool, user_id: Uuid, community_id: Uuid) -> Result<bool> {
    let exists = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM community_memberships WHERE user_id = $1 AND community_id = $2)",
        user_id,
        community_id
    )
    .fetch_one(db)
    .await?;

    Ok(exists.exists.unwrap_or(false))
}

pub async fn can_user_post_in_community(
    db: &PgPool,
    user_id: Uuid,
    community_id: Uuid,
) -> Result<bool> {
    let community = get_community_by_id(db, community_id).await?;

    let community = match community {
        Some(c) => c,
        None => return Ok(false),
    };

    match community.community_type {
        crate::models::CommunityType::Public => Ok(true),
        crate::models::CommunityType::Restricted | crate::models::CommunityType::Private => {
            is_community_member(db, user_id, community_id).await
        }
    }
}
