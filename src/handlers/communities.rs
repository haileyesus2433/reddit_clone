use axum::{
    Extension,
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
        Community, CommunityFlair, CommunityResponse, CommunityRule, CommunityStatus,
        CommunityType, CreateCommunityRequest, MembershipRole, UpdateCommunityRequest,
    },
    services::community_service,
};

#[derive(Debug, Deserialize)]
pub struct GetCommunitiesQuery {
    pub page: Option<u32>,
    pub limit: Option<u32>,
    pub sort: Option<String>, // popular, new, top
    pub search: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct JoinCommunityRequest {
    pub community_id: Uuid,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateRuleRequest {
    #[validate(length(min = 1, max = 100))]
    pub title: String,
    #[validate(length(max = 500))]
    pub description: Option<String>,
    pub rule_order: i32,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateFlairRequest {
    #[validate(length(min = 1, max = 50))]
    pub text: String,
    pub background_color: Option<String>,
    pub text_color: Option<String>,
    pub is_mod_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMemberRoleRequest {
    pub role: MembershipRole,
}

pub async fn create_community(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(payload): Json<CreateCommunityRequest>,
) -> Result<(StatusCode, Json<Value>)> {
    // Validate input
    payload.validate()?;

    // Rate limiting - limit community creation
    let rate_limit_key = format!("create_community:{}", auth_user.user_id);
    if !state
        .redis
        .check_rate_limit(&rate_limit_key, 5, 86400)
        .await?
    {
        // 5 per day
        return Err(AppError::RateLimit);
    }

    // Check if community name already exists
    let existing_community = sqlx::query_as::<_, Community>(
        "SELECT * FROM communities WHERE name = $1 AND status != 'banned'",
    )
    .bind(&payload.name)
    .fetch_optional(&state.db)
    .await?;

    if existing_community.is_some() {
        return Err(AppError::Conflict(
            "Community name already exists".to_string(),
        ));
    }

    // Create community
    let community_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    let community = sqlx::query_as::<_, Community>(
        r#"
        INSERT INTO communities (
            id, name, display_name, description, community_type,
            status, is_nsfw, created_by, created_at, updated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        RETURNING *
        "#,
    )
    .bind(community_id)
    .bind(&payload.name)
    .bind(&payload.display_name)
    .bind(&payload.description)
    .bind(&payload.community_type)
    .bind(CommunityStatus::Active)
    .bind(payload.is_nsfw.unwrap_or(false))
    .bind(auth_user.user_id)
    .bind(now)
    .bind(now)
    .fetch_one(&state.db)
    .await?;

    // Add creator as owner
    sqlx::query(
        r#"
        INSERT INTO community_memberships (id, user_id, community_id, role, joined_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(auth_user.user_id)
    .bind(community_id)
    .bind(MembershipRole::Owner)
    .bind(now)
    .execute(&state.db)
    .await?;

    // Create default community rules
    community_service::create_default_rules(&state.db, community_id).await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "message": "Community created successfully",
            "community": CommunityResponse {
                id: community.id,
                name: community.name,
                display_name: community.display_name,
                description: community.description,
                rules: community.rules,
                icon_url: community.icon_url,
                banner_url: community.banner_url,
                community_type: community.community_type,
                status: community.status,
                is_nsfw: community.is_nsfw,
                subscriber_count: community.subscriber_count,
                post_count: community.post_count,
                created_at: community.created_at,
                user_role: Some(MembershipRole::Owner),
                is_member: true,
            }
        })),
    ))
}

pub async fn get_communities(
    State(state): State<AppState>,
    Query(params): Query<GetCommunitiesQuery>,
    auth_user: OptionalAuthUser,
) -> Result<Json<Value>> {
    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(20).min(100); // Max 100 per page
    let offset = (page - 1) * limit;
    let sort = params.sort.as_deref().unwrap_or("popular");

    let order_clause = match sort {
        "new" => "c.created_at DESC",
        "top" => "c.subscriber_count DESC",
        _ => "c.subscriber_count DESC", // default to popular
    };

    let mut query = format!(
        r#"
        SELECT c.*, 
               CASE WHEN cm.user_id IS NOT NULL THEN true ELSE false END as is_member
        FROM communities c
        LEFT JOIN community_memberships cm ON c.id = cm.community_id AND cm.user_id = $1
        WHERE c.status = 'active'
        "#
    );

    let mut bind_params: Vec<Box<dyn sqlx::Encode<'_, sqlx::Postgres> + Send + Sync>> = vec![];
    let mut param_count = 1;

    // Add user_id parameter (can be null for non-authenticated users)
    let user_id = auth_user.0.as_ref().map(|user| user.user_id);
    bind_params.push(Box::new(user_id));

    // Add search filter if provided
    if let Some(search) = &params.search {
        param_count += 1;
        query.push_str(&format!(
            " AND (c.name ILIKE ${} OR c.display_name ILIKE ${} OR c.description ILIKE ${})",
            param_count, param_count, param_count
        ));
        let search_pattern = format!("%{}%", search);
        bind_params.push(Box::new(search_pattern));
    }

    query.push_str(&format!(
        " ORDER BY {} LIMIT ${} OFFSET ${}",
        order_clause,
        param_count + 1,
        param_count + 2
    ));

    bind_params.push(Box::new(limit as i64));
    bind_params.push(Box::new(offset as i64));

    // This is a simplified version - in practice, you'd use a query builder or raw SQL with proper binding
    let communities = community_service::get_communities_with_membership(
        &state.db,
        user_id,
        limit,
        offset,
        sort,
        params.search.as_deref(),
    )
    .await?;

    let total_count =
        community_service::get_communities_count(&state.db, params.search.as_deref()).await?;

    Ok(Json(json!({
        "communities": communities,
        "pagination": {
            "page": page,
            "limit": limit,
            "total": total_count,
            "pages": (total_count + limit - 1) / limit
        }
    })))
}

pub async fn get_community(
    State(state): State<AppState>,
    Path(name): Path<String>,
    auth_user: OptionalAuthUser,
) -> Result<Json<CommunityResponse>> {
    let community = community_service::get_community_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound("Community not found".to_string()))?;

    let (user_role, is_member) = if let Some(auth_user) = auth_user.0.as_ref() {
        let membership =
            community_service::get_user_membership(&state.db, auth_user.user_id, community.id)
                .await?;

        match membership {
            Some(membership) => (Some(membership.role), true),
            None => (None, false),
        }
    } else {
        (None, false)
    };

    Ok(Json(CommunityResponse {
        id: community.id,
        name: community.name,
        display_name: community.display_name,
        description: community.description,
        rules: community.rules,
        icon_url: community.icon_url,
        banner_url: community.banner_url,
        community_type: community.community_type,
        status: community.status,
        is_nsfw: community.is_nsfw,
        subscriber_count: community.subscriber_count,
        post_count: community.post_count,
        created_at: community.created_at,
        user_role,
        is_member,
    }))
}

pub async fn update_community(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(name): Path<String>,
    Json(payload): Json<UpdateCommunityRequest>,
) -> Result<Json<Value>> {
    payload.validate()?;

    let community = community_service::get_community_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound("Community not found".to_string()))?;

    // Check if user has permission to update community
    let membership =
        community_service::get_user_membership(&state.db, auth_user.user_id, community.id).await?;

    let can_update = match membership {
        Some(membership) => matches!(
            membership.role,
            MembershipRole::Owner | MembershipRole::Admin | MembershipRole::Moderator
        ),
        None => false,
    };

    if !can_update {
        return Err(AppError::Authorization(
            "Insufficient permissions to update community".to_string(),
        ));
    }

    // Update community
    sqlx::query(
        r#"
        UPDATE communities 
        SET display_name = COALESCE($1, display_name),
            description = COALESCE($2, description),
            rules = COALESCE($3, rules),
            icon_url = COALESCE($4, icon_url),
            banner_url = COALESCE($5, banner_url),
            community_type = COALESCE($6, community_type),
            is_nsfw = COALESCE($7, is_nsfw),
            updated_at = $8
        WHERE id = $9
        "#,
    )
    .bind(&payload.display_name)
    .bind(&payload.description)
    .bind(&payload.rules)
    .bind(&payload.icon_url)
    .bind(&payload.banner_url)
    .bind(&payload.community_type)
    .bind(&payload.is_nsfw)
    .bind(chrono::Utc::now())
    .bind(community.id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "Community updated successfully"
    })))
}

pub async fn join_community(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(name): Path<String>,
) -> Result<Json<Value>> {
    let community = community_service::get_community_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound("Community not found".to_string()))?;

    // Check if already a member
    let existing_membership =
        community_service::get_user_membership(&state.db, auth_user.user_id, community.id).await?;

    if existing_membership.is_some() {
        return Err(AppError::Conflict(
            "Already a member of this community".to_string(),
        ));
    }

    // Check community type restrictions
    match community.community_type {
        CommunityType::Private => {
            return Err(AppError::Authorization(
                "Cannot join private community without invitation".to_string(),
            ));
        }
        CommunityType::Restricted => {
            // For restricted communities, we might want to create a join request instead
            // For now, we'll allow joining but could add approval workflow later
        }
        CommunityType::Public => {
            // Anyone can join public communities
        }
    }

    // Add user to community
    sqlx::query(
        r#"
        INSERT INTO community_memberships (id, user_id, community_id, role, joined_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(auth_user.user_id)
    .bind(community.id)
    .bind(MembershipRole::Member)
    .bind(chrono::Utc::now())
    .execute(&state.db)
    .await?;

    // Update subscriber count
    sqlx::query("UPDATE communities SET subscriber_count = subscriber_count + 1 WHERE id = $1")
        .bind(community.id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({
        "message": "Successfully joined community"
    })))
}

pub async fn leave_community(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(name): Path<String>,
) -> Result<Json<Value>> {
    let community = community_service::get_community_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound("Community not found".to_string()))?;

    // Check if user is a member
    let membership =
        community_service::get_user_membership(&state.db, auth_user.user_id, community.id).await?;

    let membership = membership
        .ok_or_else(|| AppError::NotFound("Not a member of this community".to_string()))?;

    // Owners cannot leave their own community
    if matches!(membership.role, MembershipRole::Owner) {
        return Err(AppError::BadRequest(
            "Community owners cannot leave their community".to_string(),
        ));
    }

    // Remove user from community
    sqlx::query("DELETE FROM community_memberships WHERE user_id = $1 AND community_id = $2")
        .bind(auth_user.user_id)
        .bind(community.id)
        .execute(&state.db)
        .await?;

    // Update subscriber count
    sqlx::query("UPDATE communities SET subscriber_count = subscriber_count - 1 WHERE id = $1")
        .bind(community.id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({
        "message": "Successfully left community"
    })))
}

pub async fn get_community_members(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<GetCommunitiesQuery>,
    auth_user: AuthUser,
) -> Result<Json<Value>> {
    let community = community_service::get_community_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound("Community not found".to_string()))?;

    // Check if user has permission to view members
    let membership =
        community_service::get_user_membership(&state.db, auth_user.user_id, community.id).await?;

    let can_view_members = match community.community_type {
        CommunityType::Public => true,
        CommunityType::Restricted | CommunityType::Private => membership.is_some(),
    };

    if !can_view_members {
        return Err(AppError::Authorization(
            "Cannot view members of this community".to_string(),
        ));
    }

    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = (page - 1) * limit;

    let members = sqlx::query!(
        r#"
        SELECT u.id, u.username, u.display_name, u.avatar_url, 
               cm.role as "role: MembershipRole", cm.joined_at
        FROM community_memberships cm
        JOIN users u ON cm.user_id = u.id
        WHERE cm.community_id = $1 AND u.status = 'active'
        ORDER BY 
            CASE cm.role
                WHEN 'owner' THEN 1
                WHEN 'admin' THEN 2
                WHEN 'moderator' THEN 3
                WHEN 'member' THEN 4
            END,
            cm.joined_at ASC
        LIMIT $2 OFFSET $3
        "#,
        community.id,
        limit as i64,
        offset as i64
    )
    .fetch_all(&state.db)
    .await?;

    let total_count = sqlx::query!(
        "SELECT COUNT(*) as count FROM community_memberships WHERE community_id = $1",
        community.id
    )
    .fetch_one(&state.db)
    .await?;

    let member_list: Vec<serde_json::Value> = members
        .into_iter()
        .map(|member| {
            json!({
                "id": member.id,
                "username": member.username,
                "display_name": member.display_name,
                "avatar_url": member.avatar_url,
                "role": member.role,
                "joined_at": member.joined_at
            })
        })
        .collect();

    Ok(Json(json!({
        "members": member_list,
        "pagination": {
            "page": page,
            "limit": limit,
            "total": total_count.count.unwrap_or(0),
            "pages": (total_count.count.unwrap_or(0) + limit as i64 - 1) / limit as i64
        }
    })))
}

pub async fn update_member_role(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path((name, member_id)): Path<(String, Uuid)>,
    Json(payload): Json<UpdateMemberRoleRequest>,
) -> Result<Json<Value>> {
    let community = community_service::get_community_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound("Community not found".to_string()))?;

    // Check if requesting user has permission
    let requester_membership =
        community_service::get_user_membership(&state.db, auth_user.user_id, community.id).await?;

    let can_update_roles = match requester_membership.clone() {
        Some(membership) => matches!(
            membership.role,
            MembershipRole::Owner | MembershipRole::Admin
        ),
        None => false,
    };

    if !can_update_roles {
        return Err(AppError::Authorization(
            "Insufficient permissions to update member roles".to_string(),
        ));
    }

    // Check if target user is a member
    let target_membership =
        community_service::get_user_membership(&state.db, member_id, community.id).await?;

    let target_membership = target_membership
        .ok_or_else(|| AppError::NotFound("User is not a member of this community".to_string()))?;

    // Owners cannot have their role changed
    if matches!(target_membership.role, MembershipRole::Owner) {
        return Err(AppError::BadRequest("Cannot change owner role".to_string()));
    }

    // Only owners can promote to admin
    if matches!(payload.role, MembershipRole::Admin | MembershipRole::Owner) {
        let is_owner = matches!(requester_membership.unwrap().role, MembershipRole::Owner);
        if !is_owner {
            return Err(AppError::Authorization(
                "Only owners can promote to admin or owner".to_string(),
            ));
        }
    }

    // Update member role
    sqlx::query(
        "UPDATE community_memberships SET role = $1 WHERE user_id = $2 AND community_id = $3",
    )
    .bind(&payload.role)
    .bind(member_id)
    .bind(community.id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "Member role updated successfully"
    })))
}

pub async fn remove_member(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path((name, member_id)): Path<(String, Uuid)>,
) -> Result<Json<Value>> {
    let community = community_service::get_community_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound("Community not found".to_string()))?;

    // Check if requesting user has permission
    let requester_membership =
        community_service::get_user_membership(&state.db, auth_user.user_id, community.id).await?;

    let can_remove_members = match requester_membership.clone() {
        Some(membership) => matches!(
            membership.role,
            MembershipRole::Owner | MembershipRole::Admin | MembershipRole::Moderator
        ),
        None => false,
    };

    if !can_remove_members {
        return Err(AppError::Authorization(
            "Insufficient permissions to remove members".to_string(),
        ));
    }

    // Check if target user is a member
    let target_membership =
        community_service::get_user_membership(&state.db, member_id, community.id).await?;

    let target_membership = target_membership
        .ok_or_else(|| AppError::NotFound("User is not a member of this community".to_string()))?;

    // Cannot remove owners or higher-ranked members
    let requester_role = requester_membership.unwrap().role;
    let target_role = target_membership.role;

    let can_remove = match (requester_role, target_role) {
        (MembershipRole::Owner, MembershipRole::Owner) => false, // Owners can't remove themselves
        (MembershipRole::Owner, _) => true,                      // Owners can remove anyone else
        (MembershipRole::Admin, MembershipRole::Owner | MembershipRole::Admin) => false,
        (MembershipRole::Admin, _) => true, // Admins can remove moderators and members
        (MembershipRole::Moderator, MembershipRole::Member) => true, // Moderators can only remove members
        _ => false,
    };

    if !can_remove {
        return Err(AppError::Authorization(
            "Cannot remove this member".to_string(),
        ));
    }

    // Remove member
    sqlx::query("DELETE FROM community_memberships WHERE user_id = $1 AND community_id = $2")
        .bind(member_id)
        .bind(community.id)
        .execute(&state.db)
        .await?;

    // Update subscriber count
    sqlx::query("UPDATE communities SET subscriber_count = subscriber_count - 1 WHERE id = $1")
        .bind(community.id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({
        "message": "Member removed successfully"
    })))
}

pub async fn get_community_rules(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Vec<CommunityRule>>> {
    let community = community_service::get_community_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound("Community not found".to_string()))?;

    let rules = sqlx::query_as::<_, CommunityRule>(
        "SELECT * FROM community_rules WHERE community_id = $1 ORDER BY rule_order ASC",
    )
    .bind(community.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rules))
}

pub async fn create_community_rule(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(name): Path<String>,
    Json(payload): Json<CreateRuleRequest>,
) -> Result<(StatusCode, Json<CommunityRule>)> {
    payload.validate()?;

    let community = community_service::get_community_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound("Community not found".to_string()))?;

    // Check permissions
    let membership =
        community_service::get_user_membership(&state.db, auth_user.user_id, community.id).await?;

    let can_create_rules = match membership {
        Some(membership) => matches!(
            membership.role,
            MembershipRole::Owner | MembershipRole::Admin | MembershipRole::Moderator
        ),
        None => false,
    };

    if !can_create_rules {
        return Err(AppError::Authorization(
            "Insufficient permissions to create rules".to_string(),
        ));
    }

    // Create rule
    let rule = sqlx::query_as::<_, CommunityRule>(
        r#"
        INSERT INTO community_rules (id, community_id, title, description, rule_order, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(community.id)
    .bind(&payload.title)
    .bind(&payload.description)
    .bind(payload.rule_order)
    .bind(chrono::Utc::now())
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(rule)))
}

pub async fn get_community_flairs(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Vec<CommunityFlair>>> {
    let community = community_service::get_community_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound("Community not found".to_string()))?;

    let flairs = sqlx::query_as::<_, CommunityFlair>(
        "SELECT * FROM community_flairs WHERE community_id = $1 ORDER BY created_at ASC",
    )
    .bind(community.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(flairs))
}

pub async fn create_community_flair(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(name): Path<String>,
    Json(payload): Json<CreateFlairRequest>,
) -> Result<(StatusCode, Json<CommunityFlair>)> {
    payload.validate()?;

    let community = community_service::get_community_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| AppError::NotFound("Community not found".to_string()))?;

    // Check permissions
    let membership =
        community_service::get_user_membership(&state.db, auth_user.user_id, community.id).await?;

    let can_create_flairs = match membership {
        Some(membership) => matches!(
            membership.role,
            MembershipRole::Owner | MembershipRole::Admin | MembershipRole::Moderator
        ),
        None => false,
    };

    if !can_create_flairs {
        return Err(AppError::Authorization(
            "Insufficient permissions to create flairs".to_string(),
        ));
    }

    // Create flair
    let flair = sqlx::query_as::<_, CommunityFlair>(
        r#"
        INSERT INTO community_flairs (
            id, community_id, text, background_color, text_color, 
            is_mod_only, created_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING *
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(community.id)
    .bind(&payload.text)
    .bind(&payload.background_color)
    .bind(&payload.text_color)
    .bind(payload.is_mod_only.unwrap_or(false))
    .bind(chrono::Utc::now())
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(flair)))
}
