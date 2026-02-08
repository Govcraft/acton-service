//! Health check handlers

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;

use crate::{error::Error, state::AppState};

/// Health check response
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Service status
    pub status: String,

    /// Service name
    pub service: String,

    /// Version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Readiness check response with dependency status
#[derive(Debug, Serialize, Deserialize)]
pub struct ReadinessResponse {
    /// Overall readiness status
    pub ready: bool,

    /// Service name
    pub service: String,

    /// Dependency statuses
    pub dependencies: HashMap<String, DependencyStatus>,
}

/// Individual dependency status
#[derive(Debug, Serialize, Deserialize)]
pub struct DependencyStatus {
    /// Dependency is healthy
    pub healthy: bool,

    /// Optional message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Simple health check (liveness probe)
///
/// Always returns 200 OK if the service is running.
/// This is used by Kubernetes to determine if the pod should be restarted.
pub async fn health<T>(State(state): State<AppState<T>>) -> impl IntoResponse
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    let response = HealthResponse {
        status: "healthy".to_string(),
        service: state.config().service.name.clone(),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };

    (StatusCode::OK, Json(response))
}

/// Readiness check with dependency validation (readiness probe)
///
/// Returns 200 OK if the service and all dependencies are ready.
/// Returns 503 Service Unavailable if any dependency is unhealthy.
/// This is used by Kubernetes to determine if the pod should receive traffic.
pub async fn readiness<T>(State(state): State<AppState<T>>) -> Result<impl IntoResponse, Error>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    #[cfg_attr(not(any(feature = "database", feature = "cache", feature = "events", feature = "turso", feature = "surrealdb")), allow(unused_mut))]
    let mut dependencies = HashMap::new();
    #[cfg_attr(not(any(feature = "database", feature = "cache", feature = "events", feature = "turso", feature = "surrealdb")), allow(unused_mut))]
    let mut all_ready = true;

    // Check database connection
    #[cfg(feature = "database")]
    if state.config().database.is_some() {
        match state.db().await {
            Some(db_pool) => {
                match sqlx::query("SELECT 1").fetch_one(&db_pool).await {
                    Ok(_) => {
                        dependencies.insert(
                            "database".to_string(),
                            DependencyStatus {
                                healthy: true,
                                message: Some("Connected".to_string()),
                            },
                        );
                    }
                    Err(e) => {
                        tracing::error!("Database health check failed: {}", e);
                        let is_optional = state
                            .config()
                            .database
                            .as_ref()
                            .map(|db| db.optional)
                            .unwrap_or(false);

                        if !is_optional {
                            all_ready = false;
                        }

                        dependencies.insert(
                            "database".to_string(),
                            DependencyStatus {
                                healthy: false,
                                message: Some(format!("Connection failed: {}", e)),
                            },
                        );
                    }
                }
            }
            None => {
                // Database configured but not connected yet (lazy init in progress)
                let is_optional = state
                    .config()
                    .database
                    .as_ref()
                    .map(|db| db.optional)
                    .unwrap_or(false);

                let is_lazy = state
                    .config()
                    .database
                    .as_ref()
                    .map(|db| db.lazy_init)
                    .unwrap_or(false);

                if !is_optional {
                    all_ready = false;
                }

                let message = if is_lazy {
                    "Connection initializing (lazy mode)".to_string()
                } else {
                    "Not connected".to_string()
                };

                dependencies.insert(
                    "database".to_string(),
                    DependencyStatus {
                        healthy: false,
                        message: Some(message),
                    },
                );
            }
        }
    }

    // Check Redis connection
    #[cfg(feature = "cache")]
    if state.config().redis.is_some() {
        match state.redis().await {
            Some(redis_pool) => {
                match redis_pool.get().await {
                    Ok(mut conn) => {
                        use std::ops::DerefMut;
                        match redis::cmd("PING")
                            .query_async::<String>(conn.deref_mut())
                            .await
                        {
                            Ok(_) => {
                                dependencies.insert(
                                    "redis".to_string(),
                                    DependencyStatus {
                                        healthy: true,
                                        message: Some("Connected".to_string()),
                                    },
                                );
                            }
                            Err(e) => {
                                tracing::error!("Redis ping failed: {}", e);
                                let is_optional = state
                                    .config()
                                    .redis
                                    .as_ref()
                                    .map(|r| r.optional)
                                    .unwrap_or(false);

                                if !is_optional {
                                    all_ready = false;
                                }

                                dependencies.insert(
                                    "redis".to_string(),
                                    DependencyStatus {
                                        healthy: false,
                                        message: Some(format!("Ping failed: {}", e)),
                                    },
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to get Redis connection: {}", e);
                        let is_optional = state
                            .config()
                            .redis
                            .as_ref()
                            .map(|r| r.optional)
                            .unwrap_or(false);

                        if !is_optional {
                            all_ready = false;
                        }

                        dependencies.insert(
                            "redis".to_string(),
                            DependencyStatus {
                                healthy: false,
                                message: Some(format!("Connection pool error: {}", e)),
                            },
                        );
                    }
                }
            }
            None => {
                // Redis configured but not connected yet (lazy init in progress)
                let is_optional = state
                    .config()
                    .redis
                    .as_ref()
                    .map(|r| r.optional)
                    .unwrap_or(false);

                let is_lazy = state
                    .config()
                    .redis
                    .as_ref()
                    .map(|r| r.lazy_init)
                    .unwrap_or(false);

                if !is_optional {
                    all_ready = false;
                }

                let message = if is_lazy {
                    "Connection initializing (lazy mode)".to_string()
                } else {
                    "Not connected".to_string()
                };

                dependencies.insert(
                    "redis".to_string(),
                    DependencyStatus {
                        healthy: false,
                        message: Some(message),
                    },
                );
            }
        }
    }

    // Check NATS connection
    #[cfg(feature = "events")]
    if state.config().nats.is_some() {
        match state.nats().await {
            Some(nats_client) => {
                match nats_client.connection_state() {
                    async_nats::connection::State::Connected => {
                        dependencies.insert(
                            "nats".to_string(),
                            DependencyStatus {
                                healthy: true,
                                message: Some("Connected".to_string()),
                            },
                        );
                    }
                    conn_state => {
                        tracing::warn!("NATS connection state: {:?}", conn_state);
                        let is_optional = state
                            .config()
                            .nats
                            .as_ref()
                            .map(|n| n.optional)
                            .unwrap_or(false);

                        if !is_optional {
                            all_ready = false;
                        }

                        dependencies.insert(
                            "nats".to_string(),
                            DependencyStatus {
                                healthy: false,
                                message: Some(format!("Connection state: {:?}", conn_state)),
                            },
                        );
                    }
                }
            }
            None => {
                // NATS configured but not connected yet (lazy init in progress)
                let is_optional = state
                    .config()
                    .nats
                    .as_ref()
                    .map(|n| n.optional)
                    .unwrap_or(false);

                let is_lazy = state
                    .config()
                    .nats
                    .as_ref()
                    .map(|n| n.lazy_init)
                    .unwrap_or(false);

                if !is_optional {
                    all_ready = false;
                }

                let message = if is_lazy {
                    "Connection initializing (lazy mode)".to_string()
                } else {
                    "Not connected".to_string()
                };

                dependencies.insert(
                    "nats".to_string(),
                    DependencyStatus {
                        healthy: false,
                        message: Some(message),
                    },
                );
            }
        }
    }

    // Check Turso/libsql connection
    #[cfg(feature = "turso")]
    if state.config().turso.is_some() {
        match state.turso().await {
            Some(db) => {
                // Try to get a connection to verify the database is working
                match db.connect() {
                    Ok(conn) => {
                        // Try a simple query to verify connectivity
                        match conn.query("SELECT 1", ()).await {
                            Ok(_) => {
                                dependencies.insert(
                                    "turso".to_string(),
                                    DependencyStatus {
                                        healthy: true,
                                        message: Some("Connected".to_string()),
                                    },
                                );
                            }
                            Err(e) => {
                                tracing::error!("Turso query failed: {}", e);
                                let is_optional = state
                                    .config()
                                    .turso
                                    .as_ref()
                                    .map(|t| t.optional)
                                    .unwrap_or(false);

                                if !is_optional {
                                    all_ready = false;
                                }

                                dependencies.insert(
                                    "turso".to_string(),
                                    DependencyStatus {
                                        healthy: false,
                                        message: Some(format!("Query failed: {}", e)),
                                    },
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to get Turso connection: {}", e);
                        let is_optional = state
                            .config()
                            .turso
                            .as_ref()
                            .map(|t| t.optional)
                            .unwrap_or(false);

                        if !is_optional {
                            all_ready = false;
                        }

                        dependencies.insert(
                            "turso".to_string(),
                            DependencyStatus {
                                healthy: false,
                                message: Some(format!("Connection error: {}", e)),
                            },
                        );
                    }
                }
            }
            None => {
                // Turso configured but not connected yet (lazy init in progress)
                let is_optional = state
                    .config()
                    .turso
                    .as_ref()
                    .map(|t| t.optional)
                    .unwrap_or(false);

                let is_lazy = state
                    .config()
                    .turso
                    .as_ref()
                    .map(|t| t.lazy_init)
                    .unwrap_or(false);

                if !is_optional {
                    all_ready = false;
                }

                let message = if is_lazy {
                    "Connection initializing (lazy mode)".to_string()
                } else {
                    "Not connected".to_string()
                };

                dependencies.insert(
                    "turso".to_string(),
                    DependencyStatus {
                        healthy: false,
                        message: Some(message),
                    },
                );
            }
        }
    }

    // Check SurrealDB connection
    #[cfg(feature = "surrealdb")]
    if state.config().surrealdb.is_some() {
        match state.surrealdb().await {
            Some(client) => {
                // Try a simple query to verify connectivity
                match client.query("RETURN true").await {
                    Ok(_) => {
                        dependencies.insert(
                            "surrealdb".to_string(),
                            DependencyStatus {
                                healthy: true,
                                message: Some("Connected".to_string()),
                            },
                        );
                    }
                    Err(e) => {
                        tracing::error!("SurrealDB health check failed: {}", e);
                        let is_optional = state
                            .config()
                            .surrealdb
                            .as_ref()
                            .map(|s| s.optional)
                            .unwrap_or(false);

                        if !is_optional {
                            all_ready = false;
                        }

                        dependencies.insert(
                            "surrealdb".to_string(),
                            DependencyStatus {
                                healthy: false,
                                message: Some(format!("Query failed: {}", e)),
                            },
                        );
                    }
                }
            }
            None => {
                // SurrealDB configured but not connected yet (lazy init in progress)
                let is_optional = state
                    .config()
                    .surrealdb
                    .as_ref()
                    .map(|s| s.optional)
                    .unwrap_or(false);

                let is_lazy = state
                    .config()
                    .surrealdb
                    .as_ref()
                    .map(|s| s.lazy_init)
                    .unwrap_or(false);

                if !is_optional {
                    all_ready = false;
                }

                let message = if is_lazy {
                    "Connection initializing (lazy mode)".to_string()
                } else {
                    "Not connected".to_string()
                };

                dependencies.insert(
                    "surrealdb".to_string(),
                    DependencyStatus {
                        healthy: false,
                        message: Some(message),
                    },
                );
            }
        }
    }

    // Check gRPC status
    #[cfg(feature = "grpc")]
    if state.config().grpc.is_some() {
        let grpc_config = state.config().grpc.as_ref().unwrap();

        dependencies.insert(
            "grpc".to_string(),
            DependencyStatus {
                healthy: true,
                message: Some(if grpc_config.enabled {
                    format!(
                        "Enabled (health: {}, reflection: {})",
                        grpc_config.health_check_enabled,
                        grpc_config.reflection_enabled
                    )
                } else {
                    "Disabled".to_string()
                }),
            },
        );
    }

    let response = ReadinessResponse {
        ready: all_ready,
        service: state.config().service.name.clone(),
        dependencies,
    };

    let status = if all_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    Ok((status, Json(response)))
}

/// Pool health metrics endpoint
///
/// Returns detailed metrics about connection pool health including:
/// - Database pool: size, idle connections, utilization
/// - Redis pool: status, availability
/// - NATS client: connection state
///
/// This is useful for monitoring and capacity planning.
pub async fn pool_metrics<T>(State(state): State<AppState<T>>) -> impl IntoResponse
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    let health = state.pool_health().await;
    let status = if health.healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(health))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_response() {
        let response = HealthResponse {
            status: "healthy".to_string(),
            service: "test-service".to_string(),
            version: Some("1.0.0".to_string()),
        };

        assert_eq!(response.status, "healthy");
        assert_eq!(response.service, "test-service");
    }

    #[test]
    fn test_dependency_status() {
        let status = DependencyStatus {
            healthy: true,
            message: Some("OK".to_string()),
        };

        assert!(status.healthy);
        assert_eq!(status.message, Some("OK".to_string()));
    }
}
