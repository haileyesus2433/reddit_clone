use sqlx::{PgPool, Row};
use std::time::Instant;
use uuid::Uuid;

use crate::{
    error::{AppError, Result},
    models::{
        AutocompleteQuery, AutocompleteResponse, AutocompleteSuggestion, FilterOption,
        SearchAuthor, SearchCommentResult, SearchCommunity, SearchCommunityResult, SearchFilters,
        SearchPost, SearchPostResult, SearchQuery, SearchResponse, SearchResults, SearchSort,
        SearchSuggestion, SearchTimeRange, SearchType, SearchUserResult, SuggestionType,
        TrendingCommunity, TrendingPost, TrendingResponse, TrendingTopic,
    },
};

pub async fn search(
    db: &PgPool,
    query: &SearchQuery,
    viewer_id: Option<Uuid>,
) -> Result<SearchResponse> {
    let start_time = Instant::now();

    // Sanitize and prepare search query
    let search_terms = sanitize_search_query(&query.q);
    let ts_query = build_tsquery(&search_terms);

    let limit = query.limit.unwrap_or(25).min(100);
    let offset = query.offset.unwrap_or(0);

    // Build time range filter
    let time_filter = build_time_filter(query.time_range.as_ref());

    let mut results = SearchResults {
        posts: Vec::new(),
        comments: Vec::new(),
        communities: Vec::new(),
        users: Vec::new(),
    };

    let mut total_results = 0i64;

    // Search based on type
    match query.search_type.as_ref().unwrap_or(&SearchType::All) {
        SearchType::All => {
            results.posts = search_posts(
                db,
                &ts_query,
                query,
                &time_filter,
                viewer_id,
                limit / 4,
                offset,
            )
            .await?;
            results.comments = search_comments(
                db,
                &ts_query,
                query,
                &time_filter,
                viewer_id,
                limit / 4,
                offset,
            )
            .await?;
            results.communities =
                search_communities(db, &ts_query, query, limit / 4, offset).await?;
            results.users = search_users(db, &ts_query, query, limit / 4, offset).await?;

            total_results = (results.posts.len()
                + results.comments.len()
                + results.communities.len()
                + results.users.len()) as i64;
        }
        SearchType::Posts => {
            results.posts =
                search_posts(db, &ts_query, query, &time_filter, viewer_id, limit, offset).await?;
            total_results = results.posts.len() as i64;
        }
        SearchType::Comments => {
            results.comments =
                search_comments(db, &ts_query, query, &time_filter, viewer_id, limit, offset)
                    .await?;
            total_results = results.comments.len() as i64;
        }
        SearchType::Communities => {
            results.communities = search_communities(db, &ts_query, query, limit, offset).await?;
            total_results = results.communities.len() as i64;
        }
        SearchType::Users => {
            results.users = search_users(db, &ts_query, query, limit, offset).await?;
            total_results = results.users.len() as i64;
        }
    }

    // Get search suggestions
    let suggestions = get_search_suggestions(db, &search_terms).await?;

    // Get available filters
    let filters = get_search_filters(db, &ts_query, query).await?;

    let search_time = start_time.elapsed().as_millis();

    // Save search to history if user is logged in
    if let Some(user_id) = viewer_id {
        save_search_history(db, user_id, query, total_results as i32).await?;
    }

    Ok(SearchResponse {
        query: query.q.clone(),
        total_results,
        search_time_ms: search_time,
        results,
        suggestions,
        filters,
    })
}

async fn search_posts(
    db: &PgPool,
    ts_query: &str,
    query: &SearchQuery,
    time_filter: &str,
    _viewer_id: Option<Uuid>,
    limit: u32,
    offset: u32,
) -> Result<Vec<SearchPostResult>> {
    let sort_clause = match query.sort.as_ref().unwrap_or(&SearchSort::Relevance) {
        SearchSort::Relevance => {
            "ts_rank(p.search_vector, to_tsquery('english', $1)) DESC, p.score DESC"
        }
        SearchSort::New => "p.created_at DESC",
        SearchSort::Top => "p.score DESC, p.created_at DESC",
        SearchSort::Comments => "p.comment_count DESC, p.created_at DESC",
    };

    let mut where_conditions = vec![
        "p.status = 'active'".to_string(),
        "to_tsquery('english', $1) @@ p.search_vector".to_string(),
    ];

    if !time_filter.is_empty() {
        where_conditions.push(time_filter.to_string());
    }

    if let Some(_community) = &query.community {
        where_conditions.push("c.name = $community".to_string());
    }

    if let Some(_author) = &query.author {
        where_conditions.push("u.username = $author".to_string());
    }

    if let Some(post_type) = &query.post_type {
        where_conditions.push(format!("p.post_type = '{:?}'", post_type).to_lowercase());
    }

    if let Some(is_nsfw) = query.is_nsfw {
        where_conditions.push(format!("p.is_nsfw = {}", is_nsfw));
    }

    let where_clause = where_conditions.join(" AND ");

    let sql = format!(
        r#"
        SELECT 
            p.id, p.title, p.content, p.post_type, p.score, p.comment_count, 
            p.created_at, p.is_nsfw,
            u.id as author_id, u.username, u.display_name as author_display_name, 
            u.avatar_url as author_avatar, u.is_verified,
            c.id as community_id, c.name as community_name, c.display_name as community_display_name,
            c.icon_url as community_icon,
            ts_rank(p.search_vector, to_tsquery('english', $1)) as relevance_score,
            ts_headline('english', COALESCE(p.content, p.title), to_tsquery('english', $1)) as highlight,
            pm.thumbnail_url
        FROM posts p
        JOIN users u ON p.author_id = u.id
        JOIN communities c ON p.community_id = c.id
        LEFT JOIN post_media pm ON p.id = pm.post_id AND pm.media_order = 1
        WHERE {}
        ORDER BY {}
        LIMIT $2 OFFSET $3
        "#,
        where_clause, sort_clause
    );

    let rows = sqlx::query(&sql)
        .bind(ts_query)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(db)
        .await?;

    let mut posts = Vec::new();
    for row in rows {
        let post = SearchPostResult {
            id: row.get("id"),
            title: row.get("title"),
            content: row.get("content"),
            post_type: serde_json::from_value(
                serde_json::to_value(row.get::<serde_json::Value, _>("post_type"))
                    .map_err(|e| AppError::Internal(e.to_string()))?,
            )
            .map_err(|e| AppError::Internal(e.to_string()))?,
            score: row.get("score"),
            comment_count: row.get("comment_count"),
            created_at: row.get("created_at"),
            author: SearchAuthor {
                id: row.get("author_id"),
                username: row.get("username"),
                display_name: row.get("author_display_name"),
                avatar_url: row.get("author_avatar"),
                is_verified: row.get("is_verified"),
            },
            community: SearchCommunity {
                id: row.get("community_id"),
                name: row.get("community_name"),
                display_name: row.get("community_display_name"),
                icon_url: row.get("community_icon"),
            },
            is_nsfw: row.get("is_nsfw"),
            thumbnail_url: row.get("thumbnail_url"),
            relevance_score: row.get::<f32, _>("relevance_score"),
            highlight: row.get("highlight"),
        };
        posts.push(post);
    }

    Ok(posts)
}

async fn search_comments(
    db: &PgPool,
    ts_query: &str,
    query: &SearchQuery,
    time_filter: &str,
    _viewer_id: Option<Uuid>,
    limit: u32,
    offset: u32,
) -> Result<Vec<SearchCommentResult>> {
    let sort_clause = match query.sort.as_ref().unwrap_or(&SearchSort::Relevance) {
        SearchSort::Relevance => {
            "ts_rank(to_tsvector('english', c.content), to_tsquery('english', $1)) DESC, c.score DESC"
        }
        SearchSort::New => "c.created_at DESC",
        SearchSort::Top => "c.score DESC, c.created_at DESC",
        SearchSort::Comments => "c.created_at DESC", // Not applicable for comments
    };

    let mut where_conditions = vec![
        "c.status = 'active'".to_string(),
        "to_tsquery('english', $1) @@ to_tsvector('english', c.content)".to_string(),
    ];

    if !time_filter.is_empty() {
        where_conditions.push(time_filter.replace("p.", "c."));
    }

    if let Some(_community) = &query.community {
        where_conditions.push("comm.name = $community".to_string());
    }

    if let Some(_author) = &query.author {
        where_conditions.push("u.username = $author".to_string());
    }

    let where_clause = where_conditions.join(" AND ");

    let sql = format!(
        r#"
        SELECT 
            c.id, c.content, c.score, c.created_at,
            u.id as author_id, u.username, u.display_name as author_display_name,
            u.avatar_url as author_avatar, u.is_verified,
            p.id as post_id, p.title as post_title,
            comm.id as community_id, comm.name as community_name, 
            comm.display_name as community_display_name, comm.icon_url as community_icon,
            ts_rank(to_tsvector('english', c.content), to_tsquery('english', $1)) as relevance_score,
            ts_headline('english', c.content, to_tsquery('english', $1)) as highlight
        FROM comments c
        JOIN users u ON c.author_id = u.id
        JOIN posts p ON c.post_id = p.id
        JOIN communities comm ON p.community_id = comm.id
        WHERE {}
        ORDER BY {}
        LIMIT $2 OFFSET $3
        "#,
        where_clause, sort_clause
    );

    let rows = sqlx::query(&sql)
        .bind(ts_query)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(db)
        .await?;

    let mut comments = Vec::new();
    for row in rows {
        let comment = SearchCommentResult {
            id: row.get("id"),
            content: row.get("content"),
            score: row.get("score"),
            created_at: row.get("created_at"),
            author: SearchAuthor {
                id: row.get("author_id"),
                username: row.get("username"),
                display_name: row.get("author_display_name"),
                avatar_url: row.get("author_avatar"),
                is_verified: row.get("is_verified"),
            },
            post: SearchPost {
                id: row.get("post_id"),
                title: row.get("post_title"),
                community: SearchCommunity {
                    id: row.get("community_id"),
                    name: row.get("community_name"),
                    display_name: row.get("community_display_name"),
                    icon_url: row.get("community_icon"),
                },
            },
            relevance_score: row.get::<f32, _>("relevance_score"),
            highlight: row.get("highlight"),
        };
        comments.push(comment);
    }

    Ok(comments)
}

async fn search_communities(
    db: &PgPool,
    ts_query: &str,
    query: &SearchQuery,
    limit: u32,
    offset: u32,
) -> Result<Vec<SearchCommunityResult>> {
    let sort_clause = match query.sort.as_ref().unwrap_or(&SearchSort::Relevance) {
        SearchSort::Relevance => {
            "ts_rank(c.search_vector, to_tsquery('english', $1)) DESC, c.subscriber_count DESC"
        }
        SearchSort::New => "c.created_at DESC",
        SearchSort::Top => "c.subscriber_count DESC, c.created_at DESC",
        SearchSort::Comments => "c.post_count DESC, c.created_at DESC",
    };

    let sql = format!(
        r#"
        SELECT 
            c.id, c.name, c.display_name, c.description, c.subscriber_count, 
            c.post_count, c.community_type, c.icon_url, c.is_nsfw,
            ts_rank(c.search_vector, to_tsquery('english', $1)) as relevance_score,
            ts_headline('english', COALESCE(c.description, c.display_name), to_tsquery('english', $1)) as highlight
        FROM communities c
        WHERE c.status = 'active' 
        AND to_tsquery('english', $1) @@ c.search_vector
        ORDER BY {}
        LIMIT $2 OFFSET $3
        "#,
        sort_clause
    );

    let rows = sqlx::query(&sql)
        .bind(ts_query)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(db)
        .await?;

    let mut communities = Vec::new();
    for row in rows {
        let community = SearchCommunityResult {
            id: row.get("id"),
            name: row.get("name"),
            display_name: row.get("display_name"),
            description: row.get("description"),
            subscriber_count: row.get("subscriber_count"),
            post_count: row.get("post_count"),
            community_type: serde_json::from_value(
                serde_json::to_value(row.get::<serde_json::Value, _>("community_type"))
                    .map_err(|e| AppError::Internal(e.to_string()))?,
            )
            .map_err(|e| AppError::Internal(e.to_string()))?,
            icon_url: row.get("icon_url"),
            is_nsfw: row.get("is_nsfw"),
            relevance_score: row.get::<f32, _>("relevance_score"),
            highlight: row.get("highlight"),
        };
        communities.push(community);
    }

    Ok(communities)
}

async fn search_users(
    db: &PgPool,
    ts_query: &str,
    query: &SearchQuery,
    limit: u32,
    offset: u32,
) -> Result<Vec<SearchUserResult>> {
    let sort_clause = match query.sort.as_ref().unwrap_or(&SearchSort::Relevance) {
        SearchSort::Relevance => {
            "ts_rank(u.search_vector, to_tsquery('english', $1)) DESC, u.karma_points DESC"
        }
        SearchSort::New => "u.created_at DESC",
        SearchSort::Top => "u.karma_points DESC, u.created_at DESC",
        SearchSort::Comments => "u.karma_points DESC, u.created_at DESC",
    };

    let sql = format!(
        r#"
        SELECT 
            u.id, u.username, u.display_name, u.bio, u.karma_points, 
            u.avatar_url, u.is_verified, u.created_at,
            ts_rank(u.search_vector, to_tsquery('english', $1)) as relevance_score,
            ts_headline('english', COALESCE(u.bio, u.display_name, u.username), to_tsquery('english', $1)) as highlight
        FROM users u
        WHERE u.status = 'active' 
        AND to_tsquery('english', $1) @@ u.search_vector
        ORDER BY {}
        LIMIT $2 OFFSET $3
        "#,
        sort_clause
    );

    let rows = sqlx::query(&sql)
        .bind(ts_query)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(db)
        .await?;

    let mut users = Vec::new();
    for row in rows {
        let user = SearchUserResult {
            id: row.get("id"),
            username: row.get("username"),
            display_name: row.get("display_name"),
            bio: row.get("bio"),
            karma_points: row.get("karma_points"),
            avatar_url: row.get("avatar_url"),
            is_verified: row.get("is_verified"),
            created_at: row.get("created_at"),
            relevance_score: row.get::<f32, _>("relevance_score"),
            highlight: row.get("highlight"),
        };
        users.push(user);
    }

    Ok(users)
}

async fn get_search_suggestions(db: &PgPool, search_terms: &str) -> Result<Vec<SearchSuggestion>> {
    let mut suggestions = Vec::new();

    // Get popular search queries similar to current query
    let query_suggestions = sqlx::query!(
        r#"
        SELECT query, COUNT(*) as count
        FROM search_history 
        WHERE query ILIKE $1 AND query != $2
        GROUP BY query
        ORDER BY count DESC
        LIMIT 3
        "#,
        format!("%{}%", search_terms),
        search_terms
    )
    .fetch_all(db)
    .await?;

    for row in query_suggestions {
        suggestions.push(SearchSuggestion {
            text: row.query,
            suggestion_type: SuggestionType::Query,
            count: row.count.unwrap_or(0),
        });
    }

    // Get community suggestions
    let community_suggestions = sqlx::query!(
        r#"
        SELECT name, display_name, subscriber_count
        FROM communities
        WHERE (name ILIKE $1 OR display_name ILIKE $1) 
        AND status = 'active'
        ORDER BY subscriber_count DESC
        LIMIT 3
        "#,
        format!("%{}%", search_terms)
    )
    .fetch_all(db)
    .await?;

    for row in community_suggestions {
        suggestions.push(SearchSuggestion {
            text: format!("r/{}", row.name),
            suggestion_type: SuggestionType::Community,
            count: row.subscriber_count.unwrap_or_default() as i64,
        });
    }

    // Get user suggestions
    let user_suggestions = sqlx::query!(
        r#"
        SELECT username, karma_points
        FROM users
        WHERE username ILIKE $1 AND status = 'active'
        ORDER BY karma_points DESC
        LIMIT 3
        "#,
        format!("%{}%", search_terms)
    )
    .fetch_all(db)
    .await?;

    for row in user_suggestions {
        suggestions.push(SearchSuggestion {
            text: format!("u/{}", row.username),
            suggestion_type: SuggestionType::User,
            count: row.karma_points.unwrap_or_default() as i64,
        });
    }

    Ok(suggestions)
}

async fn get_search_filters(
    db: &PgPool,
    ts_query: &str,
    _query: &SearchQuery,
) -> Result<SearchFilters> {
    // Get available communities from search results
    let communities = sqlx::query!(
        r#"
        SELECT c.name, c.display_name, COUNT(*) as post_count
        FROM posts p
        JOIN communities c ON p.community_id = c.id
        WHERE p.status = 'active' 
        AND to_tsquery('english', $1) @@ to_tsvector('english', p.title || ' ' || COALESCE(p.content, ''))
        GROUP BY c.id, c.name, c.display_name
        ORDER BY post_count DESC
        LIMIT 10
        "#,
        ts_query
    )
    .fetch_all(db)
    .await?;

    let available_communities = communities
        .into_iter()
        .map(|row| FilterOption {
            value: row.name,
            label: row.display_name,
            count: row.post_count.unwrap_or(0),
        })
        .collect();

    // Get available authors
    let authors = sqlx::query!(
        r#"
        SELECT u.username, u.display_name, COUNT(*) as post_count
        FROM posts p
        JOIN users u ON p.author_id = u.id
        WHERE p.status = 'active' 
        AND to_tsquery('english', $1) @@ to_tsvector('english', p.title || ' ' || COALESCE(p.content, ''))
        GROUP BY u.id, u.username, u.display_name
        ORDER BY post_count DESC
        LIMIT 10
        "#,
        ts_query
    )
    .fetch_all(db)
    .await?;

    let available_authors = authors
        .into_iter()
        .map(|row| FilterOption {
            value: row.username.clone(),
            label: row.display_name.unwrap_or(row.username),
            count: row.post_count.unwrap_or(0),
        })
        .collect();

    // Static filter options
    let post_types = vec![
        FilterOption {
            value: "text".to_string(),
            label: "Text".to_string(),
            count: 0,
        },
        FilterOption {
            value: "link".to_string(),
            label: "Link".to_string(),
            count: 0,
        },
        FilterOption {
            value: "image".to_string(),
            label: "Image".to_string(),
            count: 0,
        },
        FilterOption {
            value: "video".to_string(),
            label: "Video".to_string(),
            count: 0,
        },
    ];

    let time_ranges = vec![
        FilterOption {
            value: "hour".to_string(),
            label: "Past Hour".to_string(),
            count: 0,
        },
        FilterOption {
            value: "day".to_string(),
            label: "Past 24 Hours".to_string(),
            count: 0,
        },
        FilterOption {
            value: "week".to_string(),
            label: "Past Week".to_string(),
            count: 0,
        },
        FilterOption {
            value: "month".to_string(),
            label: "Past Month".to_string(),
            count: 0,
        },
        FilterOption {
            value: "year".to_string(),
            label: "Past Year".to_string(),
            count: 0,
        },
        FilterOption {
            value: "all".to_string(),
            label: "All Time".to_string(),
            count: 0,
        },
    ];

    Ok(SearchFilters {
        available_communities,
        available_authors,
        post_types,
        time_ranges,
    })
}

async fn save_search_history(
    db: &PgPool,
    user_id: Uuid,
    query: &SearchQuery,
    results_count: i32,
) -> Result<()> {
    sqlx::query!(
        r#"
    INSERT INTO search_history (user_id, query, search_type, results_count)
    VALUES ($1, $2, $3, $4)
    "#,
        user_id,
        query.q,
        query.search_type.as_ref().unwrap_or(&SearchType::All) as &SearchType,
        results_count
    )
    .execute(db)
    .await?;

    Ok(())
}

pub async fn get_trending(db: &PgPool) -> Result<TrendingResponse> {
    let trending_posts = get_trending_posts(db).await?;
    let trending_communities = get_trending_communities(db).await?;
    let trending_topics = get_trending_topics(db).await?;
    let rising_posts = get_rising_posts(db).await?;

    Ok(TrendingResponse {
        trending_posts,
        trending_communities,
        trending_topics,
        rising_posts,
    })
}

async fn get_trending_posts(db: &PgPool) -> Result<Vec<TrendingPost>> {
    let posts = sqlx::query!(
        r#"
        WITH post_stats AS (
            SELECT 
                p.id,
                p.title,
                p.score,
                p.comment_count,
                p.created_at,
                u.id as author_id,
                u.username,
                u.display_name as author_display_name,
                u.avatar_url as author_avatar,
                u.is_verified,
                c.id as community_id,
                c.name as community_name,
                c.display_name as community_display_name,
                c.icon_url as community_icon,
                -- Calculate growth rate based on votes in last hour vs previous hour
                COALESCE(
                    (SELECT COUNT(*) FROM post_votes pv 
                     WHERE pv.post_id = p.id 
                     AND pv.created_at > NOW() - INTERVAL '1 hour') * 100.0 / 
                    NULLIF((SELECT COUNT(*) FROM post_votes pv2 
                            WHERE pv2.post_id = p.id 
                            AND pv2.created_at BETWEEN NOW() - INTERVAL '2 hours' 
                            AND NOW() - INTERVAL '1 hour'), 0),
                    0
                ) as growth_rate
            FROM posts p
            JOIN users u ON p.author_id = u.id
            JOIN communities c ON p.community_id = c.id
            WHERE p.status = 'active'
            AND p.created_at > NOW() - INTERVAL '24 hours'
        )
        SELECT *
        FROM post_stats
        WHERE growth_rate > 50 -- At least 50% growth
        ORDER BY growth_rate DESC, score DESC
        LIMIT 20
        "#
    )
    .fetch_all(db)
    .await?;

    let trending_posts = posts
        .into_iter()
        .map(|row| TrendingPost {
            id: row.id,
            title: row.title,
            score: row.score.unwrap_or_default(),
            comment_count: row.comment_count.unwrap_or_default(),
            growth_rate: row
                .growth_rate
                .and_then(|d| rust_decimal::prelude::ToPrimitive::to_f32(&d))
                .unwrap_or(0.0),
            author: SearchAuthor {
                id: row.author_id,
                username: row.username,
                display_name: row.author_display_name,
                avatar_url: row.author_avatar,
                is_verified: row.is_verified.unwrap_or_default(),
            },
            community: SearchCommunity {
                id: row.community_id,
                name: row.community_name,
                display_name: row.community_display_name,
                icon_url: row.community_icon,
            },
            created_at: row.created_at.unwrap_or_default(),
        })
        .collect();

    Ok(trending_posts)
}

async fn get_trending_communities(db: &PgPool) -> Result<Vec<TrendingCommunity>> {
    let communities = sqlx::query!(
        r#"
        WITH community_stats AS (
            SELECT 
                c.id,
                c.name,
                c.display_name,
                c.subscriber_count,
                c.icon_url,
                -- Calculate growth rate based on new members in last 24h vs previous 24h
                COALESCE(
                    (SELECT COUNT(*) FROM community_memberships cm 
                     WHERE cm.community_id = c.id 
                     AND cm.joined_at > NOW() - INTERVAL '24 hours') * 100.0 / 
                    NULLIF((SELECT COUNT(*) FROM community_memberships cm2 
                            WHERE cm2.community_id = c.id 
                            AND cm2.joined_at BETWEEN NOW() - INTERVAL '48 hours' 
                            AND NOW() - INTERVAL '24 hours'), 0),
                    0
                ) as growth_rate,
                (SELECT COUNT(*) FROM posts p 
                 WHERE p.community_id = c.id 
                 AND p.created_at > NOW() - INTERVAL '24 hours') as recent_post_count
            FROM communities c
            WHERE c.status = 'active'
        )
        SELECT *
        FROM community_stats
        WHERE growth_rate > 20 -- At least 20% growth
        ORDER BY growth_rate DESC, subscriber_count DESC
        LIMIT 10
        "#
    )
    .fetch_all(db)
    .await?;

    let trending_communities = communities
        .into_iter()
        .map(|row| TrendingCommunity {
            id: row.id,
            name: row.name,
            display_name: row.display_name,
            subscriber_count: row.subscriber_count.unwrap_or_default(),
            growth_rate: row
                .growth_rate
                .and_then(|d| rust_decimal::prelude::ToPrimitive::to_f32(&d))
                .unwrap_or(0.0),
            icon_url: row.icon_url,
            recent_post_count: row.recent_post_count.unwrap_or(0) as i32,
        })
        .collect();

    Ok(trending_communities)
}

async fn get_trending_topics(db: &PgPool) -> Result<Vec<TrendingTopic>> {
    // Extract trending topics from post titles and content
    let topics = sqlx::query!(
        r#"
        WITH topic_mentions AS (
            SELECT 
                words.word as topic,
                p.created_at,
                c.name as community_name
            FROM posts p
            JOIN communities c ON p.community_id = c.id
            CROSS JOIN LATERAL unnest(
                string_to_array(
                    lower(regexp_replace(p.title || ' ' || COALESCE(p.content, ''), '[^a-zA-Z0-9\s]', ' ', 'g')), 
                    ' '
                )
            ) AS words(word)
            WHERE p.status = 'active'
            AND p.created_at > NOW() - INTERVAL '24 hours'
            AND length(words.word) > 3
        ),
        topic_stats AS (
            SELECT 
                topic,
                COUNT(*) as mention_count,
                COUNT(CASE WHEN created_at > NOW() - INTERVAL '12 hours' THEN 1 END) as recent_mentions,
                COUNT(CASE WHEN created_at BETWEEN NOW() - INTERVAL '24 hours' AND NOW() - INTERVAL '12 hours' THEN 1 END) as older_mentions,
                array_agg(DISTINCT community_name) as communities
            FROM topic_mentions
            WHERE topic NOT IN ('the', 'and', 'for', 'are', 'but', 'not', 'you', 'all', 'can', 'had', 'her', 'was', 'one', 'our', 'out', 'day', 'get', 'has', 'him', 'his', 'how', 'its', 'may', 'new', 'now', 'old', 'see', 'two', 'who', 'boy', 'did', 'man', 'men', 'put', 'say', 'she', 'too', 'use')
            GROUP BY topic
            HAVING COUNT(*) >= 5
        )
        SELECT 
            topic,
            mention_count,
            CASE 
                WHEN older_mentions = 0 THEN 100.0
                ELSE (recent_mentions * 100.0 / older_mentions)
            END as growth_rate,
            communities
        FROM topic_stats
        ORDER BY growth_rate DESC, mention_count DESC
        LIMIT 10
        "#
    )
    .fetch_all(db)
    .await?;

    let trending_topics = topics
        .into_iter()
        .map(|row| TrendingTopic {
            topic: row.topic.unwrap_or_default(),
            mention_count: row.mention_count.unwrap_or(0),
            growth_rate: row
                .growth_rate
                .and_then(|d| rust_decimal::prelude::ToPrimitive::to_f32(&d))
                .unwrap_or(0.0),
            related_communities: row.communities.unwrap_or_default(),
        })
        .collect();

    Ok(trending_topics)
}

async fn get_rising_posts(db: &PgPool) -> Result<Vec<TrendingPost>> {
    let posts = sqlx::query!(
        r#"
        WITH rising_posts AS (
            SELECT 
                p.id,
                p.title,
                p.score,
                p.comment_count,
                p.created_at,
                u.id as author_id,
                u.username,
                u.display_name as author_display_name,
                u.avatar_url as author_avatar,
                u.is_verified,
                c.id as community_id,
                c.name as community_name,
                c.display_name as community_display_name,
                c.icon_url as community_icon,
                -- Rising score: recent activity with good engagement
                (p.score * 0.7 + p.comment_count * 0.3) / 
                EXTRACT(EPOCH FROM (NOW() - p.created_at)) * 3600 as rising_score
            FROM posts p
            JOIN users u ON p.author_id = u.id
            JOIN communities c ON p.community_id = c.id
            WHERE p.status = 'active'
            AND p.created_at > NOW() - INTERVAL '6 hours'
            AND p.score > 0
        )
        SELECT *, rising_score as growth_rate
        FROM rising_posts
        ORDER BY rising_score DESC
        LIMIT 20
        "#
    )
    .fetch_all(db)
    .await?;

    let rising_posts = posts
        .into_iter()
        .map(|row| TrendingPost {
            id: row.id,
            title: row.title,
            score: row.score.unwrap_or_default(),
            comment_count: row.comment_count.unwrap_or_default(),
            growth_rate: row
                .growth_rate
                .and_then(|d| rust_decimal::prelude::ToPrimitive::to_f32(&d))
                .unwrap_or(0.0),
            author: SearchAuthor {
                id: row.author_id,
                username: row.username,
                display_name: row.author_display_name,
                avatar_url: row.author_avatar,
                is_verified: row.is_verified.unwrap_or_default(),
            },
            community: SearchCommunity {
                id: row.community_id,
                name: row.community_name,
                display_name: row.community_display_name,
                icon_url: row.community_icon,
            },
            created_at: row.created_at.unwrap_or_default(),
        })
        .collect();

    Ok(rising_posts)
}

pub async fn autocomplete(db: &PgPool, query: &AutocompleteQuery) -> Result<AutocompleteResponse> {
    let search_term = query.q.trim().to_lowercase();
    let limit = query.limit.unwrap_or(10).min(20);

    let mut suggestions = Vec::new();

    // Community suggestions
    let communities = sqlx::query!(
        r#"
        SELECT name, display_name, icon_url, subscriber_count
        FROM communities
        WHERE (name ILIKE $1 OR display_name ILIKE $1)
        AND status = 'active'
        ORDER BY subscriber_count DESC
        LIMIT $2
        "#,
        format!("{}%", search_term),
        limit as i64 / 3
    )
    .fetch_all(db)
    .await?;

    for community in communities {
        suggestions.push(AutocompleteSuggestion {
            text: community.name.clone(),
            suggestion_type: SuggestionType::Community,
            icon_url: community.icon_url,
            subtitle: Some(format!(
                "r/{} • {} members",
                community.name,
                format_number(community.subscriber_count.unwrap_or_default())
            )),
        });
    }

    // User suggestions
    let users = sqlx::query!(
        r#"
        SELECT username, display_name, avatar_url, karma_points, is_verified
        FROM users
        WHERE username ILIKE $1 AND status = 'active'
        ORDER BY karma_points DESC
        LIMIT $2
        "#,
        format!("{}%", search_term),
        limit as i64 / 3
    )
    .fetch_all(db)
    .await?;

    for user in users {
        let verified_badge = if user.is_verified.unwrap_or_default() {
            " ✓"
        } else {
            ""
        };
        suggestions.push(AutocompleteSuggestion {
            text: user.username.clone(),
            suggestion_type: SuggestionType::User,
            icon_url: user.avatar_url,
            subtitle: Some(format!(
                "u/{}{} • {} karma",
                user.username,
                verified_badge,
                format_number(user.karma_points.unwrap_or_default())
            )),
        });
    }

    // Popular search queries
    let queries = sqlx::query!(
        r#"
        SELECT query, COUNT(*) as search_count
        FROM search_history
        WHERE query ILIKE $1
        GROUP BY query
        ORDER BY search_count DESC
        LIMIT $2
        "#,
        format!("{}%", search_term),
        limit as i64 / 3
    )
    .fetch_all(db)
    .await?;

    for query_row in queries {
        suggestions.push(AutocompleteSuggestion {
            text: query_row.query,
            suggestion_type: SuggestionType::Query,
            icon_url: None,
            subtitle: Some(format!(
                "{} searches",
                format_number(query_row.search_count.unwrap_or(0) as i32)
            )),
        });
    }

    // Sort by relevance and limit
    suggestions.sort_by(|a, b| {
        // Prioritize exact matches, then by type (communities, users, queries)
        let a_exact = a.text.to_lowercase().starts_with(&search_term);
        let b_exact = b.text.to_lowercase().starts_with(&search_term);

        match (a_exact, b_exact) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a
                .suggestion_type
                .to_string()
                .cmp(&b.suggestion_type.to_string()),
        }
    });

    suggestions.truncate(limit as usize);

    Ok(AutocompleteResponse { suggestions })
}

// Helper functions
fn sanitize_search_query(query: &str) -> String {
    query
        .trim()
        .replace(['(', ')', '&', '|', '!', '<', '>', ':', '"'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_tsquery(terms: &str) -> String {
    terms
        .split_whitespace()
        .map(|term| format!("{}:*", term))
        .collect::<Vec<_>>()
        .join(" & ")
}

fn build_time_filter(time_range: Option<&SearchTimeRange>) -> String {
    match time_range {
        Some(SearchTimeRange::Hour) => "p.created_at > NOW() - INTERVAL '1 hour'".to_string(),
        Some(SearchTimeRange::Day) => "p.created_at > NOW() - INTERVAL '1 day'".to_string(),
        Some(SearchTimeRange::Week) => "p.created_at > NOW() - INTERVAL '1 week'".to_string(),
        Some(SearchTimeRange::Month) => "p.created_at > NOW() - INTERVAL '1 month'".to_string(),
        Some(SearchTimeRange::Year) => "p.created_at > NOW() - INTERVAL '1 year'".to_string(),
        Some(SearchTimeRange::All) | None => "".to_string(),
    }
}

fn format_number(num: i32) -> String {
    if num >= 1_000_000 {
        format!("{:.1}M", num as f32 / 1_000_000.0)
    } else if num >= 1_000 {
        format!("{:.1}K", num as f32 / 1_000.0)
    } else {
        num.to_string()
    }
}

impl ToString for SuggestionType {
    fn to_string(&self) -> String {
        match self {
            SuggestionType::Community => "1".to_string(), // Highest priority
            SuggestionType::User => "2".to_string(),
            SuggestionType::Query => "3".to_string(),
            SuggestionType::Tag => "4".to_string(),
        }
    }
}
