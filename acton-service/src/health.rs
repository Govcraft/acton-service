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
/// Returns 200 OK while the process should keep running. When the service has
/// registered app-defined liveness checks (see
/// [`ServiceBuilder::with_liveness_check`](crate::service_builder::ServiceBuilder::with_liveness_check)),
/// any check answering `Unready` turns this into 503 — the signal an
/// orchestrator restarts on. With no registered checks the endpoint cannot
/// fail, exactly as before.
pub async fn health<T>(State(state): State<AppState<T>>) -> impl IntoResponse
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    let alive = state.health_checks().liveness_ok().await;
    let response = HealthResponse {
        status: if alive { "healthy" } else { "unhealthy" }.to_string(),
        service: state.config().service.name.clone(),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };

    let status = if alive {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(response))
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
    // Unconditionally `mut`: app-defined checks below can update both even
    // when no backend feature is compiled in.
    let mut dependencies = HashMap::new();
    let mut all_ready = true;

    // Check database connection
    #[cfg(feature = "database")]
    if state.config().database.is_some() {
        match state.db().await {
            Some(db_pool) => match sqlx::query("SELECT 1").fetch_one(&db_pool).await {
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
            },
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
            Some(redis_pool) => match redis_pool.get().await {
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
            },
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
            Some(nats_client) => match nats_client.connection_state() {
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
            },
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

    // Check ClickHouse connection
    #[cfg(feature = "clickhouse")]
    if state.config().clickhouse.is_some() {
        match state.clickhouse().await {
            Some(client) => match client.query("SELECT 1").execute().await {
                Ok(_) => {
                    dependencies.insert(
                        "clickhouse".to_string(),
                        DependencyStatus {
                            healthy: true,
                            message: Some("Connected".to_string()),
                        },
                    );
                }
                Err(e) => {
                    tracing::error!("ClickHouse health check failed: {}", e);
                    let is_optional = state
                        .config()
                        .clickhouse
                        .as_ref()
                        .map(|c| c.optional)
                        .unwrap_or(false);

                    if !is_optional {
                        all_ready = false;
                    }

                    dependencies.insert(
                        "clickhouse".to_string(),
                        DependencyStatus {
                            healthy: false,
                            message: Some(format!("Query failed: {}", e)),
                        },
                    );
                }
            },
            None => {
                let is_optional = state
                    .config()
                    .clickhouse
                    .as_ref()
                    .map(|c| c.optional)
                    .unwrap_or(false);

                let is_lazy = state
                    .config()
                    .clickhouse
                    .as_ref()
                    .map(|c| c.lazy_init)
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
                    "clickhouse".to_string(),
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
                        grpc_config.health_check_enabled, grpc_config.reflection_enabled
                    )
                } else {
                    "Disabled".to_string()
                }),
            },
        );
    }

    // App-defined checks run after the built-in backend probes, concurrently
    // under one shared deadline. `Unready` flips overall readiness; `Degraded`
    // renders the dependency unhealthy without flipping it — visible to the
    // operator, invisible to the load balancer.
    for (name, outcome) in state.health_checks().readiness_outcomes().await {
        let status = match outcome {
            crate::checks::CheckOutcome::Ready => DependencyStatus {
                healthy: true,
                message: None,
            },
            crate::checks::CheckOutcome::Degraded(message) => DependencyStatus {
                healthy: false,
                message: Some(message),
            },
            crate::checks::CheckOutcome::Unready(message) => {
                all_ready = false;
                DependencyStatus {
                    healthy: false,
                    message: Some(message),
                }
            }
        };
        dependencies.insert(name, status);
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

    mod app_defined_checks {
        use super::super::*;
        use crate::checks::{CheckOutcome, HealthChecks, RegisteredCheck};
        use std::time::Duration;

        fn state_with(liveness: Vec<RegisteredCheck>, readiness: Vec<RegisteredCheck>) -> AppState {
            let mut state = AppState::<()>::default();
            state.set_health_checks(HealthChecks::new(
                liveness,
                readiness,
                Duration::from_millis(200),
            ));
            state
        }

        async fn body_json(response: axum::response::Response) -> serde_json::Value {
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body");
            serde_json::from_slice(&bytes).expect("json")
        }

        #[tokio::test]
        async fn health_without_checks_stays_200() {
            let response = health(State(state_with(Vec::new(), Vec::new())))
                .await
                .into_response();
            assert_eq!(response.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn unready_liveness_check_fails_health() {
            let state = state_with(
                vec![RegisteredCheck::new("writer", || async {
                    CheckOutcome::Unready("writer task exited".to_string())
                })],
                Vec::new(),
            );
            let response = health(State(state)).await.into_response();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body = body_json(response).await;
            assert_eq!(body["status"], "unhealthy");
        }

        #[tokio::test]
        async fn unready_readiness_check_fails_ready_with_message() {
            let state = state_with(
                Vec::new(),
                vec![RegisteredCheck::new("quorum", || async {
                    CheckOutcome::Unready("no quorum".to_string())
                })],
            );
            let response = readiness(State(state)).await.expect("ok").into_response();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body = body_json(response).await;
            assert_eq!(body["ready"], false);
            assert_eq!(body["dependencies"]["quorum"]["healthy"], false);
            assert_eq!(body["dependencies"]["quorum"]["message"], "no quorum");
        }

        #[tokio::test]
        async fn degraded_readiness_check_is_visible_but_ready() {
            let state = state_with(
                Vec::new(),
                vec![
                    RegisteredCheck::new("signing", || async {
                        CheckOutcome::Degraded("sidecar unreachable; 3 mints pending".to_string())
                    }),
                    RegisteredCheck::new("journal", || async { CheckOutcome::Ready }),
                ],
            );
            let response = readiness(State(state)).await.expect("ok").into_response();
            assert_eq!(response.status(), StatusCode::OK);
            let body = body_json(response).await;
            assert_eq!(body["ready"], true);
            assert_eq!(body["dependencies"]["signing"]["healthy"], false);
            assert_eq!(body["dependencies"]["journal"]["healthy"], true);
        }

        #[tokio::test]
        async fn timed_out_readiness_check_reports_unready() {
            let state = state_with(
                Vec::new(),
                vec![RegisteredCheck::new("stuck", || async {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    CheckOutcome::Ready
                })],
            );
            let response = readiness(State(state)).await.expect("ok").into_response();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            let body = body_json(response).await;
            assert_eq!(body["dependencies"]["stuck"]["message"], "check timed out");
        }
    }
}
