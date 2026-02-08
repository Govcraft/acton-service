//! Users API Example - Demonstrating Enforced API Versioning
//!
//! This example shows how to use VersionedApiBuilder to enforce API versioning
//! across your microservice. It demonstrates:
//!
//! - Multiple API versions (V1, V2, V3)
//! - Automatic deprecation headers
//! - API evolution and breaking changes
//! - Type-safe version routing
//! - Zero manual configuration (automatic config loading and tracing)
//!
//! Run with: cargo run --example users-api
//!
//! The service runs on port 8080 by default (configurable via ACTON_SERVICE_PORT env var)
//!
//! Test with:
//!   curl http://localhost:8080/health
//!   curl http://localhost:8080/ready
//!   curl http://localhost:8080/api/v1/users
//!   curl http://localhost:8080/api/v1/users/1
//!   curl -I http://localhost:8080/api/v1/users  # See deprecation headers
//!   curl http://localhost:8080/api/v2/users
//!   curl http://localhost:8080/api/v3/users
//!   curl http://localhost:8080/api/v3/users/550e8400-e29b-41d4-a716-446655440000

use acton_service::prelude::*;
use axum::{extract::Path, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct UserV1 {
    id: u64,
    username: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserV2 {
    id: u64,
    username: String,
    email: String, // New field in V2
}

#[derive(Debug, Serialize, Deserialize)]
struct UserV3 {
    id: String, // Breaking change: ID is now a string (UUID)
    username: String,
    email: String,
    created_at: String, // New field in V3
}

// V1 Handlers
async fn list_users_v1() -> Json<Vec<UserV1>> {
    Json(vec![UserV1 {
        id: 1,
        username: "alice".to_string(),
    }])
}

async fn get_user_v1(Path(id): Path<u64>) -> Json<UserV1> {
    Json(UserV1 {
        id,
        username: format!("user{}", id),
    })
}

// V2 Handlers (adds email field)
async fn list_users_v2() -> Json<Vec<UserV2>> {
    Json(vec![UserV2 {
        id: 1,
        username: "alice".to_string(),
        email: "alice@example.com".to_string(),
    }])
}

async fn get_user_v2(Path(id): Path<u64>) -> Json<UserV2> {
    Json(UserV2 {
        id,
        username: format!("user{}", id),
        email: format!("user{}@example.com", id),
    })
}

// V3 Handlers (breaking change: string IDs + timestamps)
async fn list_users_v3() -> Json<Vec<UserV3>> {
    Json(vec![UserV3 {
        id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        username: "alice".to_string(),
        email: "alice@example.com".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
    }])
}

async fn get_user_v3(Path(id): Path<String>) -> Json<UserV3> {
    Json(UserV3 {
        id: id.clone(),
        username: "alice".to_string(),
        email: "alice@example.com".to_string(),
        created_at: "2025-01-01T00:00:00Z".to_string(),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    // Create versioned API with ENFORCED versioning
    // The type system makes it IMPOSSIBLE to accidentally create unversioned routes
    // Health and readiness endpoints are AUTOMATICALLY included by build_routes()
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        // V1: Deprecated, will be removed in December 2025
        .add_version_deprecated(
            ApiVersion::V1,
            |routes| {
                routes
                    .route("/users", get(list_users_v1))
                    .route("/users/{id}", get(get_user_v1))
            },
            DeprecationInfo::new(ApiVersion::V1, ApiVersion::V3)
                .with_sunset_date("2025-12-31T23:59:59Z")
                .with_message("V1 is deprecated. Please migrate to V3 for UUID support."),
        )
        // V2: Deprecated, sunset in June 2026
        .add_version_deprecated(
            ApiVersion::V2,
            |routes| {
                routes
                    .route("/users", get(list_users_v2))
                    .route("/users/{id}", get(get_user_v2))
            },
            DeprecationInfo::new(ApiVersion::V2, ApiVersion::V3)
                .with_sunset_date("2026-06-30T23:59:59Z")
                .with_message("V2 is deprecated. Migrate to V3 for improved ID handling."),
        )
        // V3: Current stable version
        .add_version(ApiVersion::V3, |routes| {
            routes
                .route("/users", get(list_users_v3))
                .route("/users/{id}", get(get_user_v3))
        })
        .build_routes(); // Returns VersionedRoutes with /health and /ready included!

    // Build and serve - ZERO manual configuration required!
    // ServiceBuilder automatically:
    // - Loads config from environment/files (or uses defaults)
    // - Initializes tracing/logging based on config
    // - Includes health and readiness endpoints from routes
    // ServiceBuilder only accepts VersionedRoutes - can't bypass versioning!
    ServiceBuilder::new()
        .with_routes(routes) // Only accepts VersionedRoutes!
        .build() // Auto-loads config and initializes tracing!
        .serve()
        .await?;

    Ok(())
}
