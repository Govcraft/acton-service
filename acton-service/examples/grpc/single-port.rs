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
//! # Test HTTP endpoint (`name` is optional and defaults to "World")
//! curl http://localhost:8080/api/v1/hello
//! curl 'http://localhost:8080/api/v1/hello?name=Alice'
//!
//! # Test gRPC endpoint (requires grpcurl)
//! grpcurl -plaintext -d '{"name":"World"}' localhost:8080 hello.v1.HelloService/SayHello
//!
//! # Check gRPC health
//! grpcurl -plaintext localhost:8080 grpc.health.v1.Health/Check
//! ```

use acton_service::config::GrpcConfig;
use acton_service::prelude::*;

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

#[derive(Debug, Deserialize)]
struct NameQuery {
    name: Option<String>,
}

/// Handles both `/hello` and `/hello?name=...`, so every documented `curl`
/// above behaves as advertised.
async fn http_hello(Query(query): Query<NameQuery>) -> Json<HelloHttpResponse> {
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
    tracing::info!("  HTTP with name: curl 'http://localhost:8080/api/v1/hello?name=Alice'");
    tracing::info!("  gRPC: grpcurl -plaintext -d '{{\"name\":\"World\"}}' localhost:8080 hello.v1.HelloService/SayHello");
    tracing::info!("  Health: grpcurl -plaintext localhost:8080 grpc.health.v1.Health/Check");
    tracing::info!("");

    // Build HTTP routes
    let http_routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/hello", get(http_hello))
        })
        .build_routes();

    // Enable gRPC in single-port mode.
    //
    // `Config::default()` leaves every optional section as `None`, so the
    // section must be *assigned*, not mutated in place — mutating through an
    // `if let Some(..)` would never run its body and the build would be
    // refused for registering gRPC services that no listener can expose.
    let mut config = Config::default();
    config.service.port = 8080;
    config.grpc = Some(GrpcConfig {
        enabled: true,
        use_separate_port: false, // single-port HTTP + gRPC
        ..Default::default()
    });

    // The health service needs an `AppState` to probe dependencies; passing
    // `None` would skip it with only a warning and the documented
    // `grpc.health.v1.Health/Check` call would fail. This config declares no
    // database, cache, or event bus, so the check has nothing to probe and
    // reports SERVING without any external service running.
    let state = AppState::builder().config(config.clone()).build().await?;

    // Build gRPC services
    let grpc_routes = acton_service::grpc::server::GrpcServicesBuilder::new()
        .with_health()
        .with_reflection()
        .add_file_descriptor_set(hello::FILE_DESCRIPTOR_SET)
        .add_service(HelloServiceServer::new(HelloServiceImpl))
        .build(Some(state));

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
