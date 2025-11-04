//! Users API Example Service
//!
//! Demonstrates usage of the acton-service framework

use acton_service::prelude::*;

mod handlers;
mod models;

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration
    let config = Config::load()?;

    // Initialize tracing
    init_tracing(&config)?;

    info!("Starting Users API service");

    // Build application state
    let state = AppState::builder()
        .config(config.clone())
        .build()
        .await?;

    // Build versioned API routes with ENFORCED versioning
    // The type system makes it IMPOSSIBLE to create unversioned routes
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |routes| {
            routes
                .route("/users", get(handlers::list_users).post(handlers::create_user))
                .route(
                    "/users/:id",
                    get(handlers::get_user)
                        .put(handlers::update_user)
                        .delete(handlers::delete_user),
                )
        })
        .build_routes();  // Returns VersionedRoutes (opaque, enforced)

    // Build router with versioned routes and health checks, then apply state
    // The type system ensures routes can ONLY come from VersionedApiBuilder
    let app = Router::new()
        .merge(routes)  // Only accepts VersionedRoutes!
        .route("/health", get(health))
        .route("/ready", get(readiness))
        .with_state(state);

    // Run server
    Server::new(config)
        .serve(app)
        .await?;

    Ok(())
}
