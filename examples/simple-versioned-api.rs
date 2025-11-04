//! Simple Versioned API Example - Type-Safe Enforcement
//!
//! This example demonstrates the IMPOSSIBLE-to-bypass versioning enforcement.
//! Try to add an unversioned route - the compiler won't let you!

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
    // Simple configuration
    let config = Config::default();

    // Build ENFORCED versioned routes
    // The type system makes it IMPOSSIBLE to add unversioned routes
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/hello", get(hello_v1))
        })
        .add_version(ApiVersion::V2, |router| {
            router.route("/hello", get(hello_v2))
        })
        .build_routes();  // Returns VersionedRoutes (opaque)

    info!("Starting simple versioned API");
    info!("Try these endpoints:");
    info!("  GET  /health (automatic)");
    info!("  GET  /ready (automatic)");
    info!("  GET  /api/v1/hello");
    info!("  GET  /api/v2/hello");

    // Build and serve with ServiceBuilder
    // Health and readiness are AUTOMATIC (batteries-included)
    ServiceBuilder::new()
        .with_config(config)
        .with_routes(routes)  // ONLY accepts VersionedRoutes!
        .build()
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
