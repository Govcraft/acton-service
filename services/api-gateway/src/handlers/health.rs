use axum::{
    extract::State,
    http::StatusCode,
};

use crate::AppState;

/// Health check endpoint
///
/// Returns "ok" if the service is running.
/// Used by Kubernetes liveness probe.
pub async fn health() -> &'static str {
    "ok"
}

/// Readiness check endpoint
///
/// Returns "ready" if the service can handle requests.
/// Used by Kubernetes readiness probe.
///
/// Currently performs basic validation. Add additional checks
/// as needed (e.g., database connectivity, external service health).
pub async fn readiness(
    State(_state): State<AppState>,
) -> Result<&'static str, StatusCode> {
    // TODO: Add connectivity checks for external dependencies
    // Example:
    // if let Some(ref db) = state.db {
    //     sqlx::query("SELECT 1")
    //         .execute(db)
    //         .await
    //         .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    // }

    Ok("ready")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, JwtConfig, OtlpConfig, RateLimitConfig, ServiceConfig};

    #[tokio::test]
    async fn test_health() {
        let response = health().await;
        assert_eq!(response, "ok");
    }

    #[tokio::test]
    async fn test_readiness() {
        let config = Config {
            service: ServiceConfig {
                name: "api-gateway".to_string(),
                port: 8081,
                log_level: "info".to_string(),
            },
            jwt: JwtConfig {
                public_key_path: "./keys/jwt-public.pem".to_string(),
                algorithm: "RS256".to_string(),
            },
            rate_limit: RateLimitConfig {
                per_user_rpm: 200,
                per_client_rpm: 1000,
            },
            otlp: OtlpConfig {
                endpoint: "http://localhost:4317".to_string(),
            },
        };

        let state = AppState { config };
        let response = readiness(State(state)).await;
        assert!(response.is_ok());
        assert_eq!(response.unwrap(), "ready");
    }
}
