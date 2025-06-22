use reddit_clone::config::Config;
use reddit_clone::database::create_pool;
use reddit_clone::redis::RedisClient;
use reddit_clone::services::apple_service::AppleOAuthService;
use reddit_clone::services::auth_service::GoogleOAuthService;
use reddit_clone::{AppState, create_app};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env file
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "reddit_clone=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config = Config::from_env().expect("Failed to load configuration");
    tracing::info!("Configuration loaded successfully");

    // Create database connection pool
    let db = create_pool(&config.database_url).await?;
    tracing::info!("Database connection pool created");

    // Run migrations
    sqlx::migrate!("./migrations").run(&db).await?;
    tracing::info!("Database migrations completed");

    // Create Redis client
    let redis = Arc::new(RedisClient::new(&config.redis_url)?);
    tracing::info!("Redis client created");

    // Create Google oauth service
    let google_service = Arc::new(GoogleOAuthService::new(
        &config.google_client_id.clone().unwrap_or_default(),
        &config.google_client_secret.clone().unwrap_or_default(),
        &format!(
            "http://{}:{}/api/auth/oauth/google/callback",
            config.host, config.port
        ),
    )?);

    // Create Apple oauth service
    let apple_service = Arc::new(AppleOAuthService::new(
        &config.apple_client_id.clone().unwrap_or_default(),
        &config.apple_team_id.clone().unwrap_or_default(),
        &config.apple_key_id.clone().unwrap_or_default(),
        &config.apple_private_key.clone().unwrap_or_default(),
        &format!(
            "http://{}:{}/api/auth/oauth/apple/callback",
            config.host, config.port
        ),
    )?);

    // Create application state
    let state = AppState {
        db,
        redis,
        google_service,
        apple_service,
        config: Arc::new(config.clone()),
    };

    // Create application
    let app = create_app(state);

    // Create listener
    let listener = TcpListener::bind(format!("{}:{}", config.host, config.port)).await?;
    tracing::info!("Server listening on {}:{}", config.host, config.port);

    // Start server
    axum::serve(listener, app).await?;

    Ok(())
}
