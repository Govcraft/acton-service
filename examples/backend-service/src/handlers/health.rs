use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    status: String,
    service: String,
}

/// Health check endpoint for Kubernetes liveness probe
///
/// Returns 200 OK if the service is running.
pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
        service: state.service_name.clone(),
    })
}

/// Readiness check endpoint for Kubernetes readiness probe
///
/// Returns 200 OK if the service is ready to handle requests.
/// Currently returns ready immediately, but you can add checks for:
/// - Database connectivity
/// - Redis connectivity
/// - NATS connectivity
/// - External service dependencies
pub async fn readiness(State(state): State<AppState>) -> Result<Json<HealthResponse>, StatusCode> {
    // TODO: Add connectivity checks here when you add database/redis/nats
    // Example:
    // if let Some(ref db) = state.db {
    //     sqlx::query("SELECT 1")
    //         .execute(db)
    //         .await
    //         .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    // }

    Ok(Json(HealthResponse {
        status: "ready".to_string(),
        service: state.service_name.clone(),
    }))
}
