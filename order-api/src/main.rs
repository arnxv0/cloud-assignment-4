mod db;
mod handlers;
mod models;
mod rate_limiter;

use handlers::AppState;
use rate_limiter::RateLimiter;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::net::SocketAddr;
use tracing::info;

async fn rate_limit_middleware(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| addr.ip().to_string());

    if state.rate_limiter.is_allowed(&ip) {
        let remaining = state.rate_limiter.remaining(&ip);
        let mut response = next.run(req).await;

        let limit_str = state.rate_limiter.limit.to_string();
        let remaining_str = remaining.to_string();
        if let Ok(v) = HeaderValue::from_str(&limit_str) {
            response.headers_mut().insert("X-RateLimit-Limit", v);
        }
        if let Ok(v) = HeaderValue::from_str(&remaining_str) {
            response.headers_mut().insert("X-RateLimit-Remaining", v);
        }
        response
    } else {
        let window = state.rate_limiter.window.as_secs();
        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "Too Many Requests",
                "message": format!(
                    "Rate limit of {} req/{}s exceeded. Retry after {}s.",
                    state.rate_limiter.limit, window, window
                )
            })),
        )
            .into_response();

        let window_str = window.to_string();
        let limit_str = state.rate_limiter.limit.to_string();
        if let Ok(v) = HeaderValue::from_str(&window_str) {
            response.headers_mut().insert("Retry-After", v);
        }
        if let Ok(v) = HeaderValue::from_str(&limit_str) {
            response.headers_mut().insert("X-RateLimit-Limit", v);
        }
        response
            .headers_mut()
            .insert("X-RateLimit-Remaining", HeaderValue::from_static("0"));
        response
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("order_api=info".parse().unwrap()),
        )
        .init();

    info!("Starting Orders API Server...");

    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        let host = std::env::var("DB_HOST").expect("DB_HOST or DATABASE_URL must be set");
        let port = std::env::var("DB_PORT").unwrap_or_else(|_| "5432".to_string());
        let name = std::env::var("DB_NAME").unwrap_or_else(|_| "orders".to_string());
        let user = std::env::var("DB_USER").expect("DB_USER must be set");
        let pass = std::env::var("DB_PASSWORD").expect("DB_PASSWORD must be set");
        format!(
            "postgres://{}:{}@{}:{}/{}?sslmode=require",
            user, pass, host, port, name
        )
    });

    let rate_limit_max: usize = std::env::var("RATE_LIMIT_MAX_REQUESTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);

    let rate_limit_window: u64 = std::env::var("RATE_LIMIT_WINDOW_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);

    info!(
        "Rate limiter: {} requests per {}s per IP",
        rate_limit_max, rate_limit_window
    );

    let pool = db::connect(&database_url).await;
    db::migrate(&pool).await;

    info!("Database connected and migrations applied.");

    let state = AppState {
        db: pool,
        rate_limiter: RateLimiter::new(rate_limit_max, rate_limit_window),
    };

    let app = Router::new()
        .route("/health", get(handlers::health_check))
        .route("/orders", post(handlers::create_order))
        .route("/orders/{order_id}", get(handlers::get_order))
        .route("/items", post(handlers::create_item))
        .route("/items/{id}", get(handlers::get_item))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit_middleware,
        ))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    info!("Listening on http://0.0.0.0:8080");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}
