use acton_service::prelude::*;
use axum::{routing::get, Router};
use backend_service::{config::Config, grpc_service, handlers, AppState};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer, cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = Config::load()?;

    // Initialize tracing subscriber
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| config.service.log_level.clone().into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!(
        "Starting {} on port {}",
        config.service.name,
        config.service.port
    );

    // TODO: Initialize connection pools here
    // Example:
    // let db = PgPoolOptions::new()
    //     .max_connections(50)
    //     .connect(&config.database.url)
    //     .await?;

    // Create shared application state
    let state = AppState {
        service_name: config.service.name.clone(),
        // TODO: Add your pools here
        // db,
        // redis,
        // nats,
    };

    // Build HTTP router with middleware
    let app = create_http_router(state.clone(), &config);

    // HTTP server address
    let http_addr = SocketAddr::from(([0, 0, 0, 0], config.service.port));

    // gRPC server address (port + 1)
    let grpc_addr = SocketAddr::from(([0, 0, 0, 0], config.service.port + 1));

    tracing::info!("Backend service starting...");
    tracing::info!("HTTP server: http://{}", http_addr);
    tracing::info!("  POST /api/v1/users - Create user");
    tracing::info!("  GET  /api/v1/users - List users");
    tracing::info!("  GET  /api/v1/users/:id - Get user");
    tracing::info!("  GET  /health - Health check");
    tracing::info!("  GET  /ready - Readiness check");
    tracing::info!("gRPC server: http://{}", grpc_addr);
    tracing::info!("  CreateUser, GetUser, ListUsers");

    // Start both servers concurrently
    let http_server = async {
        let listener = TcpListener::bind(http_addr).await?;
        serve_with_shutdown(listener, app).await
    };

    let grpc_server = async {
        let grpc_service = grpc_service::create_grpc_service();
        tonic::transport::Server::builder()
            .add_service(grpc_service)
            .serve_with_shutdown(grpc_addr, shutdown_signal())
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    };

    // Run both servers
    tokio::try_join!(http_server, grpc_server)?;

    tracing::info!("Server stopped gracefully");
    Ok(())
}

/// Create HTTP router with all routes and middleware
fn create_http_router(state: AppState, config: &Config) -> Router {
    // Build versioned API routes with ENFORCED versioning
    // The type system makes it IMPOSSIBLE to create unversioned routes
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |routes| {
            routes
                .route(
                    "/users",
                    get(handlers::list_users).post(handlers::create_user),
                )
                .route("/users/{id}", get(handlers::get_user))
        })
        .build_routes();  // Returns VersionedRoutes (opaque, enforced)

    // Build router with health checks and versioned routes
    // The type system ensures routes can ONLY come from VersionedApiBuilder
    let app = Router::new()
        .route("/health", get(handlers::health))
        .route("/ready", get(handlers::readiness))
        .merge(routes)  // Only accepts VersionedRoutes!
        .with_state(state);

    app.layer(
        ServiceBuilder::new()
            // Outer layers (executed first on request, last on response)
            .layer(TraceLayer::new_for_http())
            .layer(TimeoutLayer::new(config.timeout()))
            .layer(CompressionLayer::new())
            .layer(CorsLayer::permissive()) // Configure CORS as needed
    )
}

/// Serve with graceful shutdown on SIGTERM/SIGINT
async fn serve_with_shutdown(
    listener: TcpListener,
    app: Router,
) -> Result<(), Box<dyn std::error::Error>> {
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Wait for shutdown signal (SIGTERM, SIGINT, or Ctrl+C)
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C signal");
        }
        _ = terminate => {
            tracing::info!("Received SIGTERM signal");
        }
    }
}
