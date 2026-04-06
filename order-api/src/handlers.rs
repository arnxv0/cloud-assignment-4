use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::Utc;
use hex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::models::{CreateItemRequest, CreateOrderRequest};
use crate::rate_limiter::RateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub rate_limiter: RateLimiter,
}

fn hash_body(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(hasher.finalize())
}

pub async fn health_check(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({"status": "ok", "db": "connected"})),
        ),
        Err(e) => {
            error!("Health check DB error: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"status": "error", "db": "disconnected"})),
            )
        }
    }
}

pub async fn create_item(
    State(state): State<AppState>,
    Json(req): Json<CreateItemRequest>,
) -> (StatusCode, Json<Value>) {
    let row = sqlx::query("INSERT INTO items (name, value) VALUES ($1, $2) RETURNING id")
        .bind(&req.name)
        .bind(req.value)
        .fetch_one(&state.db)
        .await;

    match row {
        Ok(r) => {
            let id: i32 = r.try_get("id").unwrap();
            info!("Created item id={}", id);
            (
                StatusCode::CREATED,
                Json(json!({"id": id, "name": req.name, "value": req.value})),
            )
        }
        Err(e) => {
            error!("Failed to create item: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal server error"})),
            )
        }
    }
}

pub async fn get_item(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<Value>) {
    let row = sqlx::query("SELECT id, name, value FROM items WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await;

    match row {
        Ok(Some(r)) => {
            let id: i32 = r.try_get("id").unwrap();
            let name: String = r.try_get("name").unwrap();
            let value: i32 = r.try_get("value").unwrap();
            (StatusCode::OK, Json(json!({"id": id, "name": name, "value": value})))
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "item not found"})),
        ),
        Err(e) => {
            error!("Failed to get item: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal server error"})),
            )
        }
    }
}

pub async fn create_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> (StatusCode, Json<Value>) {
    let idempotency_key = match headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
    {
        Some(k) => k.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Idempotency-Key header is required"})),
            )
        }
    };

    let req: CreateOrderRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid request body"})),
            )
        }
    };

    let request_hash = hash_body(&body);

    let existing = sqlx::query(
        "SELECT request_hash, response_body, status_code FROM idempotency_records WHERE idempotency_key = $1",
    )
    .bind(&idempotency_key)
    .fetch_optional(&state.db)
    .await;

    match existing {
        Ok(Some(row)) => {
            let stored_hash: String = row.try_get("request_hash").unwrap();
            let response_body: String = row.try_get("response_body").unwrap();
            let status_code: i32 = row.try_get("status_code").unwrap();

            if stored_hash != request_hash {
                warn!("Idempotency key conflict for key={}", idempotency_key);
                return (
                    StatusCode::CONFLICT,
                    Json(json!({"error": "idempotency key reused with different request body"})),
                );
            }

            let cached: Value = serde_json::from_str(&response_body).unwrap_or(json!({}));
            let status = StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::OK);
            return (status, Json(cached));
        }
        Ok(None) => {}
        Err(e) => {
            error!("DB error checking idempotency: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal server error"})),
            );
        }
    }

    let order_id = Uuid::new_v4().to_string();
    let ledger_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let amount = req.quantity * 100;

    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            error!("Failed to begin transaction: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal server error"})),
            );
        }
    };

    let res = sqlx::query(
        "INSERT INTO orders (order_id, customer_id, item_id, quantity, created_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&order_id)
    .bind(&req.customer_id)
    .bind(&req.item_id)
    .bind(req.quantity)
    .bind(now)
    .execute(&mut *tx)
    .await;

    if let Err(e) = res {
        error!("Failed to insert order: {}", e);
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "internal server error"})),
        );
    }

    let res = sqlx::query(
        "INSERT INTO ledger (ledger_id, order_id, customer_id, amount, created_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&ledger_id)
    .bind(&order_id)
    .bind(&req.customer_id)
    .bind(amount)
    .bind(now)
    .execute(&mut *tx)
    .await;

    if let Err(e) = res {
        error!("Failed to insert ledger entry: {}", e);
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "internal server error"})),
        );
    }

    let response_data = json!({
        "order_id": order_id,
        "customer_id": req.customer_id,
        "item_id": req.item_id,
        "quantity": req.quantity,
        "amount": amount,
        "created_at": now.to_rfc3339(),
    });

    let response_body = response_data.to_string();

    let res = sqlx::query(
        "INSERT INTO idempotency_records (idempotency_key, request_hash, response_body, status_code, created_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&idempotency_key)
    .bind(&request_hash)
    .bind(&response_body)
    .bind(201_i32)
    .bind(now)
    .execute(&mut *tx)
    .await;

    if let Err(e) = res {
        error!("Failed to store idempotency record: {}", e);
        let _ = tx.rollback().await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "internal server error"})),
        );
    }

    if let Err(e) = tx.commit().await {
        error!("Failed to commit transaction: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "internal server error"})),
        );
    }

    info!("Created order id={}", order_id);
    (StatusCode::CREATED, Json(response_data))
}

pub async fn get_order(
    State(state): State<AppState>,
    Path(order_id): Path<String>,
) -> (StatusCode, Json<Value>) {
    let row = sqlx::query(
        "SELECT order_id, customer_id, item_id, quantity, created_at FROM orders WHERE order_id = $1",
    )
    .bind(&order_id)
    .fetch_optional(&state.db)
    .await;

    match row {
        Ok(Some(r)) => {
            let oid: String = r.try_get("order_id").unwrap();
            let customer_id: String = r.try_get("customer_id").unwrap();
            let item_id: String = r.try_get("item_id").unwrap();
            let quantity: i32 = r.try_get("quantity").unwrap();
            let created_at: chrono::DateTime<Utc> = r.try_get("created_at").unwrap();
            (
                StatusCode::OK,
                Json(json!({
                    "order_id": oid,
                    "customer_id": customer_id,
                    "item_id": item_id,
                    "quantity": quantity,
                    "created_at": created_at.to_rfc3339(),
                })),
            )
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "order not found"})),
        ),
        Err(e) => {
            error!("Failed to get order: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "internal server error"})),
            )
        }
    }
}
