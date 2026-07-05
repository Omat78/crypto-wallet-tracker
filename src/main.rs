mod alerts_worker;
mod config;
mod db;
mod handlers;
mod models;
mod rate_limit;
mod security;
mod services;

use axum::http::{HeaderValue, Method};
use axum::routing::{delete, get, post};
use axum::Router;
use config::Config;
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

pub struct AppState {
    pub pool: SqlitePool,
    pub http: reqwest::Client,
    pub config: Config,
    pub rate_limiter: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let config = Config::from_env();
    let pool = db::init_pool(&config.database_url).await?;
    let http = reqwest::Client::builder()
        .user_agent("crypto-wallet-tracker/0.1")
        .build()?;

    let state = Arc::new(AppState {
        pool,
        http,
        config: config.clone(),
        rate_limiter: Mutex::new(HashMap::new()),
    });

    // Background alerts worker (paid-tier real-time alerts).
    tokio::spawn(alerts_worker::run(state.clone()));

    let cors = if state.config.allowed_origins.is_empty() {
        tracing::warn!("ALLOWED_ORIGINS is not set — CORS is wide open. Set it to your frontend's origin(s) before going live.");
        CorsLayer::permissive()
    } else {
        let origins: Vec<HeaderValue> = state
            .config
            .allowed_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([Method::GET, Method::POST, Method::DELETE])
            .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::HeaderName::from_static("x-api-key")])
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/api/signup", post(handlers::auth::signup))
        .route("/api/recover", post(handlers::auth::recover))
        .route("/api/stripe/webhook", post(handlers::auth::stripe_webhook))
        .route("/api/checkout", post(handlers::payments::create_checkout))
        .route("/api/me", get(handlers::auth::me))
        .route("/api/wallet/:chain/:address/holdings", get(handlers::wallet::get_holdings))
        .route("/api/wallet/:chain/:address/transactions", get(handlers::wallet::get_transactions))
        .route("/api/wallet/:chain/:address/pnl", get(handlers::wallet::get_pnl))
        .route("/api/alerts", post(handlers::alerts::create_alert).get(handlers::alerts::list_alerts))
        .route("/api/alerts/:id", delete(handlers::alerts::delete_alert))
        .layer(axum::middleware::from_fn_with_state(state.clone(), rate_limit::rate_limit))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
        .fallback_service(ServeDir::new("static"));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.port)).await?;
    tracing::info!("listening on 0.0.0.0:{}", config.port);
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await?;

    Ok(())
}
