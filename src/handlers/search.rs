use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use validator::Validate;

use crate::{
    AppState,
    auth::{AuthUser, OptionalAuthUser},
    error::Result,
    models::{AutocompleteQuery, SearchQuery},
    services::search_service,
};

pub async fn search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
    user: OptionalAuthUser,
) -> Result<Json<crate::models::SearchResponse>> {
    // Validate query
    query.validate()?;

    let viewer_id = user.0.map(|u| u.user_id);

    let results = search_service::search(&state.db, &query, viewer_id).await?;

    Ok(Json(results))
}

pub async fn trending(
    State(state): State<AppState>,
) -> Result<Json<crate::models::TrendingResponse>> {
    let trending = search_service::get_trending(&state.db).await?;

    Ok(Json(trending))
}

pub async fn autocomplete(
    State(state): State<AppState>,
    Query(query): Query<AutocompleteQuery>,
) -> Result<Json<crate::models::AutocompleteResponse>> {
    // Validate query
    query.validate()?;

    let suggestions = search_service::autocomplete(&state.db, &query).await?;

    Ok(Json(suggestions))
}

pub async fn search_history(
    State(state): State<AppState>,
    user: AuthUser,
    Query(params): Query<crate::models::PaginationParams>,
) -> Result<Json<Vec<crate::models::SearchHistory>>> {
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0);

    let history = sqlx::query_as!(
        crate::models::SearchHistory,
        r#"
        SELECT 
            id, user_id, query, 
            search_type as "search_type: crate::models::SearchType",
            results_count, 
            clicked_result_id, 
            clicked_result_type, 
            created_at
        FROM search_history
        WHERE user_id = $1
        ORDER BY created_at DESC
        LIMIT $2 OFFSET $3
        "#,
        user.user_id,
        limit as i64,
        offset as i64
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(history))
}

pub async fn clear_search_history(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<StatusCode> {
    sqlx::query!(
        "DELETE FROM search_history WHERE user_id = $1",
        user.user_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn track_search_click(
    State(state): State<AppState>,
    user: AuthUser,
    Json(payload): Json<TrackSearchClickRequest>,
) -> Result<StatusCode> {
    sqlx::query!(
        r#"
        UPDATE search_history 
        SET clicked_result_id = $1, clicked_result_type = $2
        WHERE id = $3 AND user_id = $4
        "#,
        payload.result_id,
        payload.result_type,
        payload.search_history_id,
        user.user_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
pub struct TrackSearchClickRequest {
    pub search_history_id: uuid::Uuid,
    pub result_id: uuid::Uuid,
    pub result_type: String, // "post", "comment", "community", "user"
}
