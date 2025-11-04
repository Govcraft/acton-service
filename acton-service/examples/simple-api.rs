//! Simple Versioned API Example - Zero Configuration
//!
//! This example demonstrates:
//! - IMPOSSIBLE-to-bypass versioning enforcement (type-safe at compile time)
//! - Automatic configuration loading from environment/files
//! - Automatic tracing/logging initialization
//! - Automatic health and readiness endpoints
//!
//! Run with: cargo run --example simple-api
//!
//! The service runs on port 8080 by default (configurable via ACTON_SERVICE_PORT env var)
//!
//! Test with:
//!   curl http://localhost:8080/health
//!   curl http://localhost:8080/ready
//!   curl http://localhost:8080/api/v1/hello
//!   curl http://localhost:8080/api/v2/hello

use acton_service::prelude::*;
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
struct Message {
    version: String,
    message: String,
}

// V1 Handler
async fn hello_v1() -> Json<Message> {
    Json(Message {
        version: "v1".to_string(),
        message: "Hello from API V1!".to_string(),
    })
}

// V2 Handler
async fn hello_v2() -> Json<Message> {
    Json(Message {
        version: "v2".to_string(),
        message: "Hello from API V2 with improvements!".to_string(),
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    // Build ENFORCED versioned routes with automatic health endpoints
    // The type system makes it IMPOSSIBLE to add unversioned routes
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/hello", get(hello_v1))
        })
        .add_version(ApiVersion::V2, |router| {
            router.route("/hello", get(hello_v2))
        })
        .build_routes(); // Returns VersionedRoutes with /health and /ready included!

    // Build and serve - ZERO manual configuration required!
    // ServiceBuilder automatically:
    // - Loads config from environment/files (or uses defaults)
    // - Initializes tracing/logging based on config
    // - Includes health and readiness endpoints from routes
    ServiceBuilder::new()
        .with_routes(routes) // ONLY accepts VersionedRoutes!
        .build() // Auto-loads config and initializes tracing!
        .serve()
        .await?;

    Ok(())
}

// ❌ TRY THIS - IT WON'T COMPILE!
// Uncommenting this code will result in a compile error
// because ServiceBuilder.with_routes() ONLY accepts VersionedRoutes
/*
fn try_to_bypass_versioning() {
    let bad_router = Router::new()
        .route("/unversioned", get(|| async { "bypassed!" }));

    ServiceBuilder::new()
        .with_routes(bad_router)  // ❌ ERROR: expected VersionedRoutes, found Router
        .build();
}
*/
