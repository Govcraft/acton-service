//! gRPC + Cedar Authorization Example
//!
//! Demonstrates the framework-managed authentication and authorization path
//! for gRPC: when `[token]` and `[cedar]` are configured, `ServiceBuilder`
//! automatically applies PASETO token authentication and Cedar policy
//! enforcement to every registered gRPC service. No per-service interceptor
//! wiring is required.
//!
//! Health (`grpc.health.v1.Health`) and reflection services are exempt, so
//! infrastructure probes keep working without credentials — reflection is
//! what lets the unauthenticated grpcurl commands below resolve descriptors.
//!
//! ## Running
//!
//! ```bash
//! cargo run --example cedar-grpc --features "grpc,cedar-authz,auth"
//! ```
//!
//! The example creates its policy and key files in
//! `~/.config/acton-service/cedar-grpc-example/` and prints ready-to-use
//! test tokens on startup.
//!
//! ## Testing
//!
//! ```bash
//! # Denied without a token (UNAUTHENTICATED)
//! grpcurl -plaintext -d '{"name":"World"}' localhost:8080 hello.v1.HelloService/SayHello
//!
//! # Allowed with the "user" role token printed at startup
//! grpcurl -plaintext -H "authorization: Bearer <USER_TOKEN>" \
//!     -d '{"name":"World"}' localhost:8080 hello.v1.HelloService/SayHello
//!
//! # Denied by policy with the role-less token (PERMISSION_DENIED)
//! grpcurl -plaintext -H "authorization: Bearer <GUEST_TOKEN>" \
//!     -d '{"name":"World"}' localhost:8080 hello.v1.HelloService/SayHello
//! ```

use acton_service::auth::{PasetoGenerator, TokenGenerator};
use acton_service::config::{GrpcConfig, PasetoConfig, TokenConfig};
use acton_service::middleware::Claims;
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

        Ok(tonic::Response::new(HelloResponse {
            message: format!("Hello, {}! (authorized by Cedar)", name),
        }))
    }
}

// ============================================================================
// Example Setup
// ============================================================================

/// Demo-only symmetric key. Never ship a fixed key in a real service.
const DEMO_KEY: &[u8; 32] = b"acton-cedar-grpc-example-demo-k!";

fn setup_example_files() -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let config_dir = std::path::Path::new(&std::env::var("HOME").expect("HOME must be set"))
        .join(".config/acton-service/cedar-grpc-example");
    std::fs::create_dir_all(&config_dir)?;

    // Copy the policy file (always overwrite to pick up changes)
    let policy_path = config_dir.join("policies.cedar");
    let policy_src =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/grpc/policies.cedar");
    std::fs::copy(&policy_src, &policy_path)?;

    // Write the demo PASETO key
    let key_path = config_dir.join("paseto.key");
    std::fs::write(&key_path, DEMO_KEY)?;

    Ok((policy_path, key_path))
}

fn demo_token(key_path: &std::path::Path, sub: &str, roles: Vec<String>) -> Result<String> {
    let paseto_config = acton_service::auth::PasetoGenerationConfig {
        version: "v4".to_string(),
        purpose: "local".to_string(),
        key_path: key_path.to_path_buf(),
        issuer: None,
        audience: None,
    };
    let token_config = acton_service::auth::TokenGenerationConfig::default();
    let generator = PasetoGenerator::new(&paseto_config, &token_config)?;

    let claims = Claims {
        sub: sub.to_string(),
        email: None,
        username: None,
        roles,
        perms: vec![],
        exp: 0, // set by the generator
        iat: None,
        jti: None,
        iss: None,
        aud: None,
        custom: Default::default(),
    };

    generator.generate_token(&claims)
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    let (policy_path, key_path) = setup_example_files()?;

    let user_token = demo_token(&key_path, "user:alice", vec!["user".to_string()])?;
    let guest_token = demo_token(&key_path, "user:guest", vec![])?;

    println!("🚀 gRPC + Cedar Authorization Example (port 8080)");
    println!();
    println!("Token with the \"user\" role (SayHello permitted):");
    println!("  {user_token}");
    println!();
    println!("Token without roles (SayHello denied by policy):");
    println!("  {guest_token}");
    println!();
    println!("Try it:");
    println!("  grpcurl -plaintext -d '{{\"name\":\"World\"}}' localhost:8080 hello.v1.HelloService/SayHello");
    println!("  grpcurl -plaintext -H \"authorization: Bearer <TOKEN>\" -d '{{\"name\":\"World\"}}' localhost:8080 hello.v1.HelloService/SayHello");
    println!();

    // Build gRPC services — no auth or Cedar wiring needed here; the
    // framework applies both to all gRPC routes because `[token]` and
    // `[cedar]` are configured below.
    let grpc_routes = acton_service::grpc::server::GrpcServicesBuilder::new()
        .with_reflection()
        .add_file_descriptor_set(hello::FILE_DESCRIPTOR_SET)
        .add_service(HelloServiceServer::new(HelloServiceImpl))
        .build(None);

    let mut config = Config::default();
    config.service.port = 8080;

    config.grpc = Some(GrpcConfig {
        enabled: true,
        use_separate_port: false, // single-port HTTP + gRPC
        bind: None,
        #[cfg(feature = "tls")]
        tls: None,
        port: 50051,
        reflection_enabled: true,
        health_check_enabled: true,
        max_message_size_mb: 4,
        connection_timeout_secs: 10,
        timeout_secs: 30,
        proto: Default::default(),
    });

    config.token = Some(TokenConfig::Paseto(PasetoConfig {
        version: "v4".to_string(),
        purpose: "local".to_string(),
        key_path,
        issuer: None,
        audience: None,
        public_paths: Vec::new(),
    }));

    config.cedar = Some(CedarConfig {
        enabled: true,
        policy_path,
        hot_reload: false,
        hot_reload_interval_secs: 60,
        cache_enabled: false,
        cache_ttl_secs: 300,
        fail_open: false,
    });

    // A tiny HTTP route so the same port also serves REST
    let http_routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/hello", get(|| async { "Hello from HTTP!" }))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_config(config)
        .with_routes(http_routes)
        .with_grpc_services(grpc_routes)
        .try_build()?
        .serve()
        .await?;

    Ok(())
}
