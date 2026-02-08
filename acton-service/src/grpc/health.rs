//! gRPC health check service
//!
//! Implements the standard gRPC health checking protocol.
//! See: https://github.com/grpc/grpc/blob/master/doc/health-checking.md

use tonic::{Request, Response, Status};
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::{HealthCheckRequest, HealthCheckResponse};

use crate::state::AppState;

/// Health check service implementation
///
/// Provides health status for the service and its dependencies.
/// This implements the standard gRPC health checking protocol.
#[derive(Clone)]
pub struct HealthService {
    #[allow(dead_code)]
    state: AppState,
}

impl HealthService {
    /// Create a new health service with the given state
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    /// Check all dependencies and return overall health status
    async fn check_dependencies(&self) -> bool {
        #[allow(unused_mut)]
        let mut all_healthy = true;

        // Check database connection
        #[cfg(feature = "database")]
        if self.state.config().database.is_some() {
            match self.state.db().await {
                Some(db_pool) => {
                    if let Err(e) = sqlx::query("SELECT 1").fetch_one(&db_pool).await {
                        tracing::error!("Database health check failed: {}", e);
                        let is_optional = self
                            .state
                            .config()
                            .database
                            .as_ref()
                            .map(|db| db.optional)
                            .unwrap_or(false);
                        if !is_optional {
                            all_healthy = false;
                        }
                    }
                }
                None => {
                    let is_optional = self
                        .state
                        .config()
                        .database
                        .as_ref()
                        .map(|db| db.optional)
                        .unwrap_or(false);
                    if !is_optional {
                        all_healthy = false;
                    }
                }
            }
        }

        // Check Redis connection
        #[cfg(feature = "cache")]
        if self.state.config().redis.is_some() {
            match self.state.redis().await {
                Some(redis_pool) => match redis_pool.get().await {
                    Ok(mut conn) => {
                        use std::ops::DerefMut;
                        if let Err(e) = redis::cmd("PING")
                            .query_async::<String>(conn.deref_mut())
                            .await
                        {
                            tracing::error!("Redis ping failed: {}", e);
                            let is_optional = self
                                .state
                                .config()
                                .redis
                                .as_ref()
                                .map(|r| r.optional)
                                .unwrap_or(false);
                            if !is_optional {
                                all_healthy = false;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to get Redis connection: {}", e);
                        let is_optional = self
                            .state
                            .config()
                            .redis
                            .as_ref()
                            .map(|r| r.optional)
                            .unwrap_or(false);
                        if !is_optional {
                            all_healthy = false;
                        }
                    }
                },
                None => {
                    let is_optional = self
                        .state
                        .config()
                        .redis
                        .as_ref()
                        .map(|r| r.optional)
                        .unwrap_or(false);
                    if !is_optional {
                        all_healthy = false;
                    }
                }
            }
        }

        // Check NATS connection
        #[cfg(feature = "events")]
        if self.state.config().nats.is_some() {
            match self.state.nats().await {
                Some(nats_client) => {
                    if !matches!(
                        nats_client.connection_state(),
                        async_nats::connection::State::Connected
                    ) {
                        let is_optional = self
                            .state
                            .config()
                            .nats
                            .as_ref()
                            .map(|n| n.optional)
                            .unwrap_or(false);
                        if !is_optional {
                            all_healthy = false;
                        }
                    }
                }
                None => {
                    let is_optional = self
                        .state
                        .config()
                        .nats
                        .as_ref()
                        .map(|n| n.optional)
                        .unwrap_or(false);
                    if !is_optional {
                        all_healthy = false;
                    }
                }
            }
        }

        // Check SurrealDB connection
        #[cfg(feature = "surrealdb")]
        if self.state.config().surrealdb.is_some() {
            match self.state.surrealdb().await {
                Some(client) => {
                    if let Err(e) = client.query("RETURN true").await {
                        tracing::error!("SurrealDB health check failed: {}", e);
                        let is_optional = self
                            .state
                            .config()
                            .surrealdb
                            .as_ref()
                            .map(|s| s.optional)
                            .unwrap_or(false);
                        if !is_optional {
                            all_healthy = false;
                        }
                    }
                }
                None => {
                    let is_optional = self
                        .state
                        .config()
                        .surrealdb
                        .as_ref()
                        .map(|s| s.optional)
                        .unwrap_or(false);
                    if !is_optional {
                        all_healthy = false;
                    }
                }
            }
        }

        all_healthy
    }
}

#[tonic::async_trait]
impl tonic_health::pb::health_server::Health for HealthService {
    type WatchStream = std::pin::Pin<
        Box<dyn futures::Stream<Item = Result<HealthCheckResponse, Status>> + Send + 'static>,
    >;

    /// Check the health of the service
    ///
    /// This performs a comprehensive check of all dependencies (database, Redis, NATS)
    /// and returns SERVING if all required dependencies are healthy, or NOT_SERVING otherwise.
    async fn check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let service_name = request.into_inner().service;

        tracing::debug!("gRPC health check for service: {:?}", service_name);

        // Check all dependencies
        let all_healthy = self.check_dependencies().await;

        let status = if all_healthy {
            ServingStatus::Serving
        } else {
            ServingStatus::NotServing
        };

        tracing::debug!("gRPC health status: {:?}", status);

        Ok(Response::new(HealthCheckResponse {
            status: status as i32,
        }))
    }

    /// Watch the health of the service (streaming endpoint)
    ///
    /// This provides a stream of health status updates. Currently returns
    /// the current status and completes. Future enhancements could provide
    /// real-time updates when health status changes.
    async fn watch(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let service_name = request.into_inner().service;

        tracing::debug!("gRPC health watch for service: {:?}", service_name);

        // Check current health
        let all_healthy = self.check_dependencies().await;

        let status = if all_healthy {
            ServingStatus::Serving
        } else {
            ServingStatus::NotServing
        };

        // Create a stream that sends the current status and completes
        // In the future, this could be enhanced to send updates when health changes
        use tokio_stream::wrappers::ReceiverStream;
        let (tx, rx) = tokio::sync::mpsc::channel(1);

        // Send initial status
        tokio::spawn(async move {
            let response = HealthCheckResponse {
                status: status as i32,
            };
            let _ = tx.send(Ok(response)).await;
        });

        Ok(Response::new(
            Box::pin(ReceiverStream::new(rx)) as Self::WatchStream
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_service_creation() {
        // This test just verifies we can create a health service
        // Full integration tests would require a complete AppState setup
        use crate::config::Config;

        let config = Config::default();
        let state = AppState::new(config);
        let _health_service = HealthService::new(state);
    }
}
