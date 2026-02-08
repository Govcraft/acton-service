//! Single-Port HTTP + gRPC Example
//!
//! This example demonstrates running both HTTP REST API and gRPC services
//! on a SINGLE PORT (8080) with automatic protocol detection.
//!
//! ## Features
//!
//! - HTTP REST API and gRPC on port 8080
//! - Automatic routing based on content-type header
//! - gRPC requests (application/grpc) route to tonic services
//! - All other requests route to axum HTTP handlers
//!
//! ## Running
//!
//! ```bash
//! cargo run --example single-port --features grpc
//! ```
//!
//! ## Testing
//!
//! ```bash
//! # Test HTTP endpoint
//! curl http://localhost:8080/api/v1/hello
//!
//! # Test gRPC endpoint (requires grpcurl)
//! grpcurl -plaintext -d '{"name":"World"}' localhost:8080 hello.v1.HelloService/SayHello
//!
//! # Check gRPC health
//! grpcurl -plaintext localhost:8080 grpc.health.v1.Health/Check
//! ```

use acton_service::prelude::*;
use axum::Json;
use serde::{Deserialize, Serialize};

// ============================================================================
// Protocol Buffers
// ============================================================================

pub mod hello {
    tonic::include_proto!("hello.v1");

    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("hello_descriptor");
}

use hello::{
    hello_service_server::{HelloService as HelloServiceTrait, HelloServiceServer},
    HelloRequest, HelloResponse,
};

// ============================================================================
// gRPC Service Implementation
// ============================================================================

#[derive(Debug, Default, Clone)]
struct HelloServiceImpl;

#[tonic::async_trait]
impl HelloServiceTrait for HelloServiceImpl {
    async fn say_hello(
        &self,
        request: tonic::Request<HelloRequest>,
    ) -> std::result::Result<tonic::Response<HelloResponse>, tonic::Status> {
        let name = request.into_inner().name;

        tracing::info!("gRPC request received: name={}", name);

        let response = HelloResponse {
            message: format!("Hello, {}! (via gRPC)", name),
        };

        Ok(tonic::Response::new(response))
    }
}

// ============================================================================
// HTTP Handlers
// ============================================================================

#[derive(Debug, Serialize)]
struct HelloHttpResponse {
    message: String,
}

async fn http_hello() -> Json<HelloHttpResponse> {
    tracing::info!("HTTP request received");

    Json(HelloHttpResponse {
        message: "Hello from HTTP!".to_string(),
    })
}

#[derive(Debug, Deserialize)]
struct NameQuery {
    name: Option<String>,
}

async fn http_hello_name(Query(query): Query<NameQuery>) -> Json<HelloHttpResponse> {
    let name = query.name.unwrap_or_else(|| "World".to_string());

    tracing::info!("HTTP request received: name={}", name);

    Json(HelloHttpResponse {
        message: format!("Hello, {}! (via HTTP)", name),
    })
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    tracing::info!("🚀 Starting Single-Port HTTP + gRPC Service");
    tracing::info!("   Port: 8080 (both HTTP and gRPC)");
    tracing::info!("");
    tracing::info!("Test commands:");
    tracing::info!("  HTTP: curl http://localhost:8080/api/v1/hello");
    tracing::info!("  HTTP with name: curl http://localhost:8080/api/v1/hello?name=Alice");
    tracing::info!("  gRPC: grpcurl -plaintext -d '{{\"name\":\"World\"}}' localhost:8080 hello.v1.HelloService/SayHello");
    tracing::info!("  Health: grpcurl -plaintext localhost:8080 grpc.health.v1.Health/Check");
    tracing::info!("");

    // Build HTTP routes
    let http_routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/hello", get(http_hello).post(http_hello_name))
        })
        .build_routes();

    // Build gRPC services
    let hello_service = HelloServiceImpl;

    let grpc_routes = acton_service::grpc::server::GrpcServicesBuilder::new()
        .with_health()
        .with_reflection()
        .add_file_descriptor_set(hello::FILE_DESCRIPTOR_SET)
        .add_service(HelloServiceServer::new(hello_service))
        .build(None);

    // Create config with single-port mode (default)
    let mut config = Config::default();
    config.service.port = 8080;

    // Enable gRPC and ensure single-port mode
    if let Some(ref mut grpc_config) = config.grpc {
        grpc_config.enabled = true;
        grpc_config.use_separate_port = false; // CRITICAL: This enables single-port mode
    }

    // Build and serve the combined service
    ServiceBuilder::new()
        .with_config(config)
        .with_routes(http_routes)
        .with_grpc_services(grpc_routes)
        .build()
        .serve()
        .await?;

    Ok(())
}
