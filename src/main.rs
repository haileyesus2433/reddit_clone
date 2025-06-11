use reddit_clone::config::Config;
use reddit_clone::database::create_pool;
use reddit_clone::redis::RedisClient;
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

    // Create application state
    let state = AppState {
        db,
        redis,
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
