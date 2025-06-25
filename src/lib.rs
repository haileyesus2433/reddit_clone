pub mod auth;
pub mod config;
pub mod database;
pub mod error;
pub mod handlers;
pub mod models;
pub mod redis;
pub mod services;
// pub mod utils;
// pub mod websocket;

use axum::{
    Router,
    http::{
        HeaderValue, Method,
        header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    },
    routing::{delete, get, post, put},
};
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{
    config::Config,
    redis::RedisClient,
    services::{
        apple_service::AppleOAuthService, auth_service::GoogleOAuthService,
        email_service::EmailService, sms_service::SmsService,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub google_service: Arc<GoogleOAuthService>,
    pub apple_service: Arc<AppleOAuthService>,
    pub redis: Arc<RedisClient>,
    pub config: Arc<Config>,
    pub email_service: Arc<EmailService>,
    pub sms_service: Arc<SmsService>,
}

pub fn create_app(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(
            state
                .config
                .allowed_origins
                .iter()
                .map(|origin| origin.parse::<HeaderValue>().unwrap())
                .collect::<Vec<_>>(),
        )
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::PATCH,
        ])
        .allow_headers([AUTHORIZATION, ACCEPT, CONTENT_TYPE]);

    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/api/auth/register", post(handlers::auth::register))
        .route("/api/auth/login", post(handlers::auth::login))
        .route(
            "/api/auth/forgot-password",
            post(handlers::auth::forgot_password),
        )
        .route(
            "/api/auth/reset-password",
            post(handlers::auth::reset_password),
        )
        .route("/api/auth/verify-email", post(handlers::auth::verify_email))
        .route("/api/auth/verify-phone", post(handlers::auth::verify_phone))
        .route(
            "/api/auth/oauth/google",
            get(handlers::auth::initiate_google_oauth),
        )
        .route(
            "/api/auth/oauth/google/callback",
            get(handlers::auth::google_oauth),
        )
        .route(
            "/api/auth/oauth/apple",
            get(handlers::auth::initiate_apple_oauth),
        )
        .route(
            "/api/auth/oauth/apple/callback",
            post(handlers::auth::apple_oauth),
        );

    // Protected routes
    let protected_routes = Router::new()
        .route("/api/auth/logout", post(handlers::auth::logout))
        .route("/api/auth/refresh", post(handlers::auth::refresh_token))
        // User routes
        .route("/api/users/me", get(handlers::users::get_current_user))
        .route("/api/users/me", put(handlers::users::update_current_user))
        .route(
            "/api/users/me/preferences",
            get(handlers::users::get_user_preferences),
        )
        .route(
            "/api/users/me/preferences",
            put(handlers::users::update_user_preferences),
        )
        .route(
            "/api/users/me/follow/{user_id}",
            post(handlers::users::follow_user),
        )
        .route(
            "/api/users/me/unfollow/{user_id}",
            delete(handlers::users::unfollow_user),
        )
        .route(
            "/api/users/me/block/{user_id}",
            post(handlers::users::block_user),
        )
        .route(
            "/api/users/me/unblock/{user_id}",
            delete(handlers::users::unblock_user),
        )
        .route(
            "/api/users/{username}",
            get(handlers::users::get_user_by_username),
        )
        // Community routes
        .route(
            "/api/communities",
            get(handlers::communities::get_communities),
        )
        .route(
            "/api/communities",
            post(handlers::communities::create_community),
        )
        .route(
            "/api/communities/{name}",
            get(handlers::communities::get_community),
        )
        .route(
            "/api/communities/{name}",
            put(handlers::communities::update_community),
        )
        .route(
            "/api/communities/{name}/join",
            post(handlers::communities::join_community),
        )
        .route(
            "/api/communities/{name}/leave",
            post(handlers::communities::leave_community),
        )
        .route(
            "/api/communities/{name}/members",
            get(handlers::communities::get_community_members),
        )
        .route(
            "/api/communities/{name}/members/{member_id}/role",
            put(handlers::communities::update_member_role),
        )
        .route(
            "/api/communities/{name}/members/{member_id}",
            delete(handlers::communities::remove_member),
        )
        .route(
            "/api/communities/{name}/rules",
            get(handlers::communities::get_community_rules),
        )
        .route(
            "/api/communities/{name}/rules",
            post(handlers::communities::create_community_rule),
        )
        .route(
            "/api/communities/{name}/flairs",
            get(handlers::communities::get_community_flairs),
        )
        .route(
            "/api/communities/{name}/flairs",
            post(handlers::communities::create_community_flair),
        )
        // Post routes
        .route("/api/posts", post(handlers::posts::create_post))
        .route("/api/posts/{post_id}", put(handlers::posts::update_post))
        .route("/api/posts/{post_id}", delete(handlers::posts::delete_post))
        .route(
            "/api/posts/{post_id}/vote",
            post(handlers::posts::vote_post),
        )
        .route(
            "/api/posts/{post_id}/save",
            post(handlers::posts::save_post),
        )
        .route(
            "/api/posts/{post_id}/save",
            delete(handlers::posts::unsave_post),
        )
        .route(
            "/api/posts/{post_id}/report",
            post(handlers::posts::report_post),
        )
        .route("/api/users/me/saved", get(handlers::posts::get_saved_posts))
        // Comment routes
        .route("/api/comments", post(handlers::comments::create_comment))
        .route(
            "/api/comments/{comment_id}",
            put(handlers::comments::update_comment),
        )
        .route(
            "/api/comments/{comment_id}",
            delete(handlers::comments::delete_comment),
        )
        .route(
            "/api/comments/{comment_id}/vote",
            post(handlers::comments::vote_comment),
        )
        .route(
            "/api/comments/{comment_id}/save",
            post(handlers::comments::save_comment),
        )
        .route(
            "/api/comments/{comment_id}/save",
            delete(handlers::comments::unsave_comment),
        )
        .route(
            "/api/comments/{comment_id}/report",
            post(handlers::comments::report_comment),
        )
        .route(
            "/api/users/me/comments/saved",
            get(handlers::comments::get_saved_comments),
        )
        // Search routes
        .route("/api/search", get(handlers::search::search))
        .route("/api/search/trending", get(handlers::search::trending))
        .route(
            "/api/search/autocomplete",
            get(handlers::search::autocomplete),
        )
        .route("/api/search/history", get(handlers::search::search_history))
        .route(
            "/api/search/history",
            delete(handlers::search::clear_search_history),
        )
        .route(
            "/api/search/track-click",
            post(handlers::search::track_search_click),
        );

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(cors),
        )
        .with_state(state)
}
