//! Ping-Pong HTTP to gRPC Example
//!
//! This example demonstrates:
//! - HTTP REST API that forwards requests to a gRPC backend
//! - Running both HTTP (port 8080) and gRPC (port 9090) services
//! - Using generated protobuf types
//!
//! Run with: cargo run --example ping-pong --features grpc
//!
//! Test with:
//!   # Via HTTP REST API:
//!   curl -X POST http://localhost:8080/api/v1/ping \
//!     -H "Content-Type: application/json" \
//!     -d '{"message":"Hello"}'
//!
//!   # Directly via gRPC (if you have grpcurl):
//!   grpcurl -plaintext -d '{"message":"Direct"}' localhost:9090 ping.v1.PingService/Ping

use acton_service::prelude::*;
use axum::Json;
use serde::{Deserialize, Serialize};
use tonic::{Request, Response, Status};

// Include generated protobuf code
pub mod ping {
    tonic::include_proto!("ping.v1");

    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("ping_descriptor");
}

use ping::{
    ping_service_client::PingServiceClient, ping_service_server::{PingService, PingServiceServer},
    PingRequest, PongResponse,
};

// ============================================================================
// gRPC Service Implementation
// ============================================================================

#[derive(Default)]
struct PingServiceImpl {}

#[tonic::async_trait]
impl PingService for PingServiceImpl {
    async fn ping(
        &self,
        request: Request<PingRequest>,
    ) -> std::result::Result<Response<PongResponse>, Status> {
        let req = request.into_inner();

        tracing::info!(message = %req.message, "gRPC: Received ping");

        let response = PongResponse {
            message: format!("pong: {}", req.message),
            timestamp: chrono::Utc::now().timestamp(),
        };

        Ok(Response::new(response))
    }
}

// ============================================================================
// HTTP Handlers
// ============================================================================

#[derive(Debug, Deserialize)]
struct HttpPingRequest {
    message: String,
}

#[derive(Debug, Serialize)]
struct HttpPongResponse {
    message: String,
    timestamp: i64,
}

async fn http_ping_handler(Json(req): Json<HttpPingRequest>) -> std::result::Result<Json<HttpPongResponse>, (axum::http::StatusCode, String)> {
    tracing::info!(message = %req.message, "HTTP: Forwarding ping to gRPC backend");

    // Connect to gRPC backend
    let mut client = PingServiceClient::connect("http://localhost:9090")
        .await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("gRPC connection failed: {}", e)))?;

    let grpc_request = PingRequest {
        message: req.message,
    };

    let response = client
        .ping(grpc_request)
        .await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("gRPC call failed: {}", e)))?;

    let response = response.into_inner();

    Ok(Json(HttpPongResponse {
        message: response.message,
        timestamp: response.timestamp,
    }))
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    let grpc_addr = "0.0.0.0:9090".parse().unwrap();

    tracing::info!("🚀 Starting Ping-Pong Service");
    tracing::info!("   gRPC backend: {}", grpc_addr);
    tracing::info!("   HTTP gateway: http://0.0.0.0:8080");

    // Start gRPC backend server
    let grpc_task = tokio::spawn(async move {
        tracing::info!("✓ gRPC service listening on {}", grpc_addr);

        let ping_service = PingServiceImpl::default();

        // Build gRPC server with health and reflection
        let router = acton_service::grpc::server::GrpcServicesBuilder::new()
            .with_health()
            .with_reflection()
            .add_file_descriptor_set(ping::FILE_DESCRIPTOR_SET)
            .add_service(PingServiceServer::new(ping_service))
            .build(None);

        if let Some(router) = router {
            router
                .serve(grpc_addr)
                .await
                .expect("gRPC server failed");
        }
    });

    // Wait for gRPC server to start
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    tracing::info!("✓ gRPC backend ready");

    // Build HTTP gateway routes
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/ping", post(http_ping_handler))
        })
        .build_routes();

    // Start HTTP gateway server using ServiceBuilder
    let http_task = tokio::spawn(async move {
        tracing::info!("✓ HTTP gateway listening on http://0.0.0.0:8080");

        ServiceBuilder::new()
            .with_routes(routes)
            .build()
            .serve()
            .await
            .expect("HTTP server failed");
    });

    tracing::info!("");
    tracing::info!("✨ Services are running!");
    tracing::info!("");
    tracing::info!("Try these commands:");
    tracing::info!("  # Send ping via HTTP → gRPC:");
    tracing::info!(r#"  curl -X POST http://localhost:8080/api/v1/ping -H "Content-Type: application/json" -d '{{"message":"Hello"}}'"#);
    tracing::info!("");
    tracing::info!("  # Check health:");
    tracing::info!("  curl http://localhost:8080/health");
    tracing::info!("  curl http://localhost:8080/ready");
    tracing::info!("");
    tracing::info!("  # Call gRPC directly (with grpcurl):");
    tracing::info!(r#"  grpcurl -plaintext -d '{{"message":"Direct"}}' localhost:9090 ping.v1.PingService/Ping"#);
    tracing::info!(r#"  grpcurl -plaintext localhost:9090 list"#);
    tracing::info!("");

    // Wait for both servers
    tokio::select! {
        _ = grpc_task => tracing::error!("gRPC server stopped"),
        _ = http_task => tracing::error!("HTTP server stopped"),
    }

    Ok(())
}
