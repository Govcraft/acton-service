use acton_service::prelude::*;
use api_gateway::{handlers, AppState, Config};
use axum::{routing::get, Router};
use std::net::SocketAddr;
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer, cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = Config::load()?;

    // Initialize tracing
    init_tracing(&config)?;

    tracing::info!(
        service = %config.service.name,
        port = config.service.port,
        "Starting service"
    );

    // Create shared application state
    let state = AppState {
        config: config.clone(),
    };

    // Build HTTP router with middleware stack
    let http_router = create_http_router(state);

    // Create socket address
    let addr = SocketAddr::from(([0, 0, 0, 0], config.service.port));

    tracing::info!("API Gateway listening on http://{}", addr);
    tracing::info!("Endpoints:");
    tracing::info!("  POST /api/v1/users - Create user (proxied to backend)");
    tracing::info!("  GET  /api/v1/users - List users (proxied to backend)");
    tracing::info!("  GET  /api/v1/users/:id - Get user (proxied to backend)");
    tracing::info!("  GET  /health - Health check");
    tracing::info!("  GET  /ready - Readiness check");
    tracing::info!("Backend service: http://localhost:8080");

    // Create server with graceful shutdown
    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(listener, http_router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("Server shutdown complete");

    Ok(())
}

fn create_http_router(state: AppState) -> Router {
    // Build middleware stack (outer to inner)
    let middleware_stack = ServiceBuilder::new()
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(std::time::Duration::from_secs(30)))
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive());

    // Build versioned API routes with ENFORCED versioning
    // The type system makes it IMPOSSIBLE to create unversioned routes
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |routes| {
            routes
                .route(
                    "/users",
                    get(handlers::list_users_proxy).post(handlers::create_user_proxy),
                )
                .route("/users/:id", get(handlers::get_user_proxy))
        })
        .build_routes();  // Returns VersionedRoutes (opaque, enforced)

    // Build router with health checks, versioned routes, and apply state
    // The type system ensures routes can ONLY come from VersionedApiBuilder
    Router::new()
        .route("/health", get(handlers::health))
        .route("/ready", get(handlers::readiness))
        .merge(routes)  // Only accepts VersionedRoutes!
        .with_state(state)
        .layer(middleware_stack)
}

fn init_tracing(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    // Simple tracing setup without OTLP for now
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| config.service.log_level.clone().into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            tracing::info!("Received termination signal");
        },
    }

    tracing::info!("Shutdown signal received, draining connections");
}
