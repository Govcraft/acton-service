---
title: gRPC Guide
nextjs:
  metadata:
    title: gRPC Guide
    description: Complete guide to using gRPC with acton-service - protocol buffers, build setup, service implementation, and middleware integration.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See [Dual HTTP+gRPC](/docs/dual-protocol) for protocol multiplexing basics. Check the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

Complete guide to implementing gRPC services with acton-service, including protocol buffer setup, code generation, service implementation, and production features.

---

## Overview

acton-service provides first-class gRPC support with:
- **Automatic protocol buffers compilation** via build utilities
- **Middleware parity** - same middleware features as HTTP (auth, tracing, rate limiting)
- **Single or dual-port deployment** - run gRPC+HTTP on one port or separate them
- **Health checks and reflection** - standard gRPC features built-in
- **Type-safe service definitions** - compile-time verification

---

## Quick Start

### 1. Project Structure

```
my-service/
├── Cargo.toml
├── build.rs          # Protocol buffer compilation
├── proto/            # .proto files (convention)
│   └── my_service.proto
└── src/
    └── main.rs
```

### 2. Enable gRPC Feature

```toml
# Cargo.toml
[dependencies]
{% $dep.grpc %}
tonic = "0.12"
prost = "0.13"

[build-dependencies]
{% $dep.grpc %}
```

### 3. Create Protocol Buffer Definition

```protobuf
// proto/my_service.proto
syntax = "proto3";

package myservice.v1;

service MyService {
  rpc GetUser(GetUserRequest) returns (UserResponse);
  rpc ListUsers(ListUsersRequest) returns (stream UserResponse);
}

message GetUserRequest {
  int64 user_id = 1;
}

message ListUsersRequest {
  int32 page_size = 1;
  string page_token = 2;
}

message UserResponse {
  int64 id = 1;
  string name = 2;
  string email = 3;
}
```

### 4. Setup build.rs

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Automatically compiles all .proto files in proto/ directory
    acton_service::build_utils::compile_service_protos()?;
    Ok(())
}
```

### 5. Implement Service

```rust
// src/main.rs
use acton_service::prelude::*;
use acton_service::grpc::server::GrpcServicesBuilder;
use tonic::{Request, Response, Status};

// Include generated protobuf code
pub mod myservice {
    tonic::include_proto!("myservice.v1");

    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("my_service_descriptor");
}

use myservice::{
    my_service_server::{MyService, MyServiceServer},
    GetUserRequest, UserResponse,
};

// Service implementation
#[derive(Default)]
struct MyServiceImpl {}

#[tonic::async_trait]
impl MyService for MyServiceImpl {
    async fn get_user(
        &self,
        request: Request<GetUserRequest>,
    ) -> Result<Response<UserResponse>, Status> {
        let user_id = request.into_inner().user_id;

        // Your business logic here
        let user = UserResponse {
            id: user_id,
            name: "John Doe".to_string(),
            email: "john@example.com".to_string(),
        };

        Ok(Response::new(user))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Collect gRPC services into tonic routes
    let grpc_routes = GrpcServicesBuilder::new()
        .add_service(MyServiceServer::new(MyServiceImpl::default()))
        .build(None);

    // Serve on single port (HTTP + gRPC multiplexed)
    ServiceBuilder::new()
        .with_grpc_services(grpc_routes)
        .build()
        .serve()
        .await
}
```

`GrpcServicesBuilder` (from `acton_service::grpc::server`) collects every service you register and returns a `tonic::service::Routes`. `ServiceBuilder::with_grpc_services()` takes that single value — call `.add_service()` once per gRPC service rather than calling `with_grpc_services()` repeatedly.

The argument to `build()` is an optional `AppState`: pass `None` unless you enable the health service (see [Health Checks](#standard-grpc-health)), which needs the state to inspect your configured dependencies.

### 6. Build and Run

```bash
cargo build --features grpc
cargo run

# Test with grpcurl
grpcurl -plaintext -d '{"user_id":123}' \
  localhost:8080 myservice.v1.MyService/GetUser
```

---

## Protocol Buffer Compilation

### Build Utilities

acton-service provides three approaches for compiling protocol buffers:

#### 1. Convention-Based (Recommended)

Uses default `proto/` directory:

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    acton_service::build_utils::compile_service_protos()?;
    Ok(())
}
```

**Directory structure:**
```
proto/
├── users.proto
├── orders.proto
└── common/
    └── types.proto
```

All `.proto` files are discovered recursively and compiled together.

#### 2. Environment-Configured

Override proto location at build time:

```bash
# Use custom directory
ACTON_PROTO_DIR=../shared/protos cargo build

# Permanent override in .cargo/config.toml
[env]
ACTON_PROTO_DIR = "../shared/protos"
```

```rust
// build.rs - same code, respects ACTON_PROTO_DIR
fn main() -> Result<(), Box<dyn std::error::Error>> {
    acton_service::build_utils::compile_service_protos()?;
    Ok(())
}
```

#### 3. Explicit Directory

Specify directory in code:

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    acton_service::build_utils::compile_protos_from_dir("my-protos")?;
    Ok(())
}
```

#### 4. Advanced: Specific Files

For fine-grained control:

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    acton_service::build_utils::compile_specific_protos(
        &["proto/orders.proto", "proto/users.proto"],
        &["proto"],  // Include directories
        "my_descriptor.bin"
    )?;
    Ok(())
}
```

### What Gets Generated

During `cargo build`, proto compilation generates:

1. **Rust types** - message structs and service traits in `OUT_DIR`
2. **File descriptor set** - `{package_name}_descriptor.bin` for reflection
3. **Build warnings** - lists which protos were compiled

Example output:
```
warning: Using proto directory: proto
warning: Compiling 3 proto files from proto
warning:   - proto/users.proto
warning:   - proto/orders.proto
warning:   - proto/common/types.proto
warning: Generated descriptor: target/debug/build/.../my_service_descriptor.bin
```

---

## Including Generated Code

### Basic Include

```rust
// Include generated protobuf types
pub mod myservice {
    tonic::include_proto!("myservice.v1");
}

use myservice::{
    my_service_server::{MyService, MyServiceServer},
    my_service_client::MyServiceClient,
    GetUserRequest, UserResponse,
};
```

The `package` name in your `.proto` file determines the module path:

```protobuf
package myservice.v1;  // → tonic::include_proto!("myservice.v1")
package orders.api;    // → tonic::include_proto!("orders.api")
```

### Including File Descriptor Set

For gRPC reflection (required by `grpcurl`, gRPC UI tools):

```rust
pub mod myservice {
    tonic::include_proto!("myservice.v1");

    // File descriptor set (package name with underscores and _descriptor suffix)
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("my_service_descriptor");
}
```

**Naming convention:**
- Package: `myservice.v1` → Descriptor: `my_service_descriptor`
- Package: `orders.api` → Descriptor: `orders_descriptor`
- Rule: Replace dots with underscores, add `_descriptor`

---

## Service Implementation

### Basic Service

```rust
use tonic::{Request, Response, Status};

#[derive(Default)]
struct MyServiceImpl {}

#[tonic::async_trait]
impl MyService for MyServiceImpl {
    async fn get_user(
        &self,
        request: Request<GetUserRequest>,
    ) -> Result<Response<UserResponse>, Status> {
        // Extract request
        let req = request.into_inner();

        // Business logic
        let user = fetch_user_from_db(req.user_id).await
            .map_err(|e| Status::internal(format!("Database error: {}", e)))?;

        // Build response
        let response = UserResponse {
            id: user.id,
            name: user.name,
            email: user.email,
        };

        Ok(Response::new(response))
    }
}
```

### With Shared State

```rust
use std::sync::Arc;
use sqlx::PgPool;

struct MyServiceImpl {
    db: Arc<PgPool>,
}

impl MyServiceImpl {
    fn new(db: Arc<PgPool>) -> Self {
        Self { db }
    }
}

#[tonic::async_trait]
impl MyService for MyServiceImpl {
    async fn get_user(
        &self,
        request: Request<GetUserRequest>,
    ) -> Result<Response<UserResponse>, Status> {
        let user_id = request.into_inner().user_id;

        // Use shared database pool
        let user = sqlx::query_as!(
            User,
            "SELECT id, name, email FROM users WHERE id = $1",
            user_id
        )
        .fetch_one(&*self.db)
        .await
        .map_err(|e| Status::not_found(format!("User not found: {}", e)))?;

        Ok(Response::new(UserResponse {
            id: user.id,
            name: user.name,
            email: user.email,
        }))
    }
}

// In main():
let db = Arc::new(get_database_pool().await?);
let grpc_service = MyServiceServer::new(MyServiceImpl::new(db));
```

### Error Handling

Use tonic `Status` for errors:

```rust
use tonic::{Code, Status};

async fn get_user(&self, request: Request<GetUserRequest>)
    -> Result<Response<UserResponse>, Status>
{
    let user_id = request.into_inner().user_id;

    // Input validation
    if user_id <= 0 {
        return Err(Status::invalid_argument("user_id must be positive"));
    }

    // Business logic with error mapping
    let user = fetch_user(user_id).await
        .map_err(|e| match e {
            DbError::NotFound => Status::not_found("User not found"),
            DbError::ConnectionFailed => Status::unavailable("Database unavailable"),
            _ => Status::internal(format!("Internal error: {}", e)),
        })?;

    Ok(Response::new(user))
}
```

**Common status codes:**
- `Code::InvalidArgument` - Bad input
- `Code::NotFound` - Resource doesn't exist
- `Code::PermissionDenied` - No access
- `Code::Unauthenticated` - Not logged in
- `Code::Unavailable` - Service temporarily down
- `Code::Internal` - Unexpected server error

---

## Streaming

### Server Streaming

Service sends multiple responses:

```protobuf
service UserService {
  rpc ListUsers(ListUsersRequest) returns (stream UserResponse);
}
```

```rust
use tokio_stream::{Stream, StreamExt};
use std::pin::Pin;

type UserStream = Pin<Box<dyn Stream<Item = Result<UserResponse, Status>> + Send>>;

#[tonic::async_trait]
impl UserService for UserServiceImpl {
    type ListUsersStream = UserStream;

    async fn list_users(
        &self,
        request: Request<ListUsersRequest>,
    ) -> Result<Response<Self::ListUsersStream>, Status> {
        let page_size = request.into_inner().page_size;

        let stream = async_stream::try_stream! {
            let mut users = fetch_users_paginated(page_size).await?;

            while let Some(user) = users.next().await {
                yield UserResponse {
                    id: user.id,
                    name: user.name,
                    email: user.email,
                };
            }
        };

        Ok(Response::new(Box::pin(stream) as Self::ListUsersStream))
    }
}
```

### Client Streaming

Client sends multiple requests:

```protobuf
service BatchService {
  rpc BatchCreateUsers(stream CreateUserRequest) returns (BatchResponse);
}
```

```rust
#[tonic::async_trait]
impl BatchService for BatchServiceImpl {
    async fn batch_create_users(
        &self,
        request: Request<tonic::Streaming<CreateUserRequest>>,
    ) -> Result<Response<BatchResponse>, Status> {
        let mut stream = request.into_inner();
        let mut created_count = 0;

        while let Some(user_req) = stream.message().await? {
            create_user(user_req).await?;
            created_count += 1;
        }

        Ok(Response::new(BatchResponse { created_count }))
    }
}
```

### Bidirectional Streaming

Both send multiple messages:

```protobuf
service ChatService {
  rpc Chat(stream ChatMessage) returns (stream ChatMessage);
}
```

```rust
#[tonic::async_trait]
impl ChatService for ChatServiceImpl {
    type ChatStream = UserStream;

    async fn chat(
        &self,
        request: Request<tonic::Streaming<ChatMessage>>,
    ) -> Result<Response<Self::ChatStream>, Status> {
        let mut in_stream = request.into_inner();

        let out_stream = async_stream::try_stream! {
            while let Some(msg) = in_stream.message().await? {
                // Process and respond to each message
                let response = process_message(msg).await?;
                yield response;
            }
        };

        Ok(Response::new(Box::pin(out_stream) as Self::ChatStream))
    }
}
```

---

## Middleware and Interceptors

acton-service provides gRPC middleware with parity to HTTP features.

### Request ID Propagation

Automatically adds unique request IDs:

```rust
use acton_service::grpc::interceptors::request_id_interceptor;

let service = MyServiceServer::with_interceptor(
    service_impl,
    request_id_interceptor
);
```

Access in service:

```rust
use acton_service::grpc::RequestIdExtension;

async fn get_user(&self, request: Request<GetUserRequest>)
    -> Result<Response<UserResponse>, Status>
{
    // Get request ID from extensions
    if let Some(request_id) = request.extensions().get::<RequestIdExtension>() {
        tracing::info!(request_id = %request_id.0, "Processing request");
    }

    // ... business logic
}
```

### Token Authentication (PASETO/JWT)

When `[token]` is configured, token authentication is applied to all
registered gRPC services **automatically** — no interceptor wiring needed.
The `authorization` metadata is validated, `Claims` are injected into
request extensions, and failures return `UNAUTHENTICATED`. Health and
reflection services are exempt so infrastructure probes work without
credentials, and `public_paths` prefixes in the token config exempt
intentionally public methods:

```toml
[token]
format = "paseto"
version = "v4"
purpose = "local"
key_path = "./keys/paseto.key"
# Optional: methods that stay public, matched by prefix
public_paths = ["/hello.v1.PublicService/"]
```

With `[cedar]` also enabled, each method is additionally authorized against
Cedar policies as `Action::"/package.Service/Method"` — see
[Cedar Authorization](/docs/cedar-auth) and the runnable `cedar-grpc`
example.

For manually composed stacks you can still wire validation yourself, either
with the HTTP-level `GrpcTokenAuthLayer` (forwards `NamedService`, so the
wrapped service registers directly with `add_service`) or with tonic
interceptors. Note that manual wiring on top of a configured `[token]`
section validates the token twice — harmless, but redundant:

```rust
use acton_service::grpc::interceptors::paseto_auth_interceptor;
use acton_service::middleware::PasetoAuth;
use std::sync::Arc;

// Create PASETO validator (default)
let paseto_config = &config.token.as_paseto().unwrap();
let paseto_auth = Arc::new(PasetoAuth::new(paseto_config)?);

// Apply to service
let service = MyServiceServer::with_interceptor(
    service_impl,
    move |req| paseto_auth_interceptor(paseto_auth.clone())(req)
);
```

For JWT (requires `jwt` feature):
```rust
use acton_service::grpc::interceptors::jwt_auth_interceptor;
use acton_service::middleware::JwtAuth;

let jwt_auth = Arc::new(JwtAuth::new(&jwt_config)?);
let service = MyServiceServer::with_interceptor(
    service_impl,
    move |req| jwt_auth_interceptor(jwt_auth.clone())(req)
);
```

Tokens must be in metadata:
```bash
# grpcurl with Bearer token
grpcurl -H "authorization: Bearer <token>" \
  -plaintext localhost:8080 myservice.v1.MyService/GetUser
```

Access claims in service:

```rust
use acton_service::middleware::Claims;

async fn get_user(&self, request: Request<GetUserRequest>)
    -> Result<Response<UserResponse>, Status>
{
    // Extract claims from extensions
    let claims = request.extensions().get::<Claims>()
        .ok_or_else(|| Status::unauthenticated("Missing claims"))?;

    let user_id = claims.sub.parse::<i64>()
        .map_err(|_| Status::invalid_argument("Invalid user ID"))?;

    // ... business logic with authenticated user
}
```

### Tracing

OpenTelemetry tracing for gRPC:

```rust
use acton_service::grpc::middleware::GrpcTracingLayer;
use tonic::transport::Server;

Server::builder()
    .layer(GrpcTracingLayer)  // Add tracing
    .add_service(my_service)
    .serve(addr)
    .await?;
```

Or use acton-service's `ServiceBuilder` for automatic tracing:

```rust
let grpc_routes = GrpcServicesBuilder::new()
    .add_service(my_service)
    .build(None);

ServiceBuilder::new()
    .with_grpc_services(grpc_routes)  // Tracing applied automatically
    .build()
    .serve()
    .await?;
```

### Rate Limiting

Limit gRPC request rates:

`GrpcRateLimitLayer::new()` takes a `LocalRateLimitConfig` — the same struct the `[middleware.governor]` config section deserializes into:

```rust
use acton_service::grpc::middleware::GrpcRateLimitLayer;
use acton_service::config::LocalRateLimitConfig;

let rate_limit = LocalRateLimitConfig {
    enabled: true,
    requests_per_period: 100,
    period_secs: 1,        // 100 requests/second
    burst_size: 10,
};

Server::builder()
    .layer(GrpcRateLimitLayer::new(rate_limit))
    .add_service(my_service)
    .serve(addr)
    .await?;
```

Requires the `governor` feature.

### Combining Interceptors

Chain multiple interceptors:

```rust
let service = MyServiceServer::with_interceptor(
    service_impl,
    move |mut req| {
        // Request ID
        req = request_id_interceptor(req)?;

        // Token auth (PASETO)
        req = paseto_auth_interceptor(paseto_auth.clone())(req)?;

        // Custom logging
        tracing::info!("gRPC request received");

        Ok(req)
    }
);
```

---

## Health Checks

### Standard gRPC Health

acton-service implements the `grpc.health.v1.Health` protocol. Enable it with `.with_health()` — the builder constructs and registers the health service for you:

```rust
use acton_service::grpc::server::GrpcServicesBuilder;

let state = AppState::default();

let grpc_routes = GrpcServicesBuilder::new()
    .with_health()                        // Standard gRPC health
    .add_service(my_service)
    .build(Some(state.clone()));          // Health needs AppState

ServiceBuilder::new()
    .with_state(state)
    .with_grpc_services(grpc_routes)
    .build()
    .serve()
    .await?;
```

The health service reports on the dependencies configured in `AppState` (database, Redis, NATS). If you enable `.with_health()` but pass `None` to `build()`, the builder logs a warning and skips the health service.

Check health with grpcurl:

```bash
grpcurl -plaintext localhost:8080 grpc.health.v1.Health/Check
```

### Kubernetes Integration

Use gRPC health for readiness probes:

```yaml
readinessProbe:
  grpc:
    port: 8080
    service: grpc.health.v1.Health
  initialDelaySeconds: 5
  periodSeconds: 10
```

---

## gRPC Reflection

Enable service discovery for dynamic clients (grpcurl, gRPC UI). Call `.with_reflection()` and register the file descriptor set generated at build time by `tonic-build` — `GrpcServicesBuilder` assembles the reflection service itself:

```rust
use acton_service::grpc::server::GrpcServicesBuilder;

let grpc_routes = GrpcServicesBuilder::new()
    .with_reflection()
    .add_file_descriptor_set(myservice::FILE_DESCRIPTOR_SET)
    .add_service(my_service)
    .build(None);

ServiceBuilder::new()
    .with_grpc_services(grpc_routes)
    .build()
    .serve()
    .await?;
```

`.with_reflection()` requires at least one `.add_file_descriptor_set()` call. Register one descriptor set per proto package you want reflected.

Now you can use grpcurl without `.proto` files:

```bash
# List services
grpcurl -plaintext localhost:8080 list

# List methods
grpcurl -plaintext localhost:8080 list myservice.v1.MyService

# Describe method
grpcurl -plaintext localhost:8080 describe myservice.v1.MyService.GetUser

# Call method (without .proto file!)
grpcurl -plaintext -d '{"user_id":123}' \
  localhost:8080 myservice.v1.MyService/GetUser
```

---

## Deployment Modes

### Single Port (HTTP + gRPC)

Default mode - automatic protocol detection:

```rust
let grpc_routes = GrpcServicesBuilder::new()
    .add_service(grpc_service)
    .build(None);

ServiceBuilder::new()
    .with_routes(http_routes)          // HTTP routes
    .with_grpc_services(grpc_routes)   // gRPC services
    .build()
    .serve()  // Single port (8080)
    .await?;
```

Both protocols work on `localhost:8080`:
```bash
# HTTP
curl http://localhost:8080/api/v1/users

# gRPC
grpcurl -plaintext localhost:8080 myservice.v1.MyService/GetUser
```

### Separate Ports

Run gRPC on dedicated port:

```toml
# config.toml
[grpc]
enabled = true
use_separate_port = true
port = 9090
```

```rust
let grpc_routes = GrpcServicesBuilder::new()
    .add_service(grpc_service)
    .build(None);

ServiceBuilder::new()
    .with_routes(http_routes)          // Port 8080
    .with_grpc_services(grpc_routes)   // Port 9090
    .build()
    .serve()
    .await?;
```

### gRPC Only

Skip HTTP entirely:

```rust
let grpc_routes = GrpcServicesBuilder::new()
    .add_service(grpc_service)
    .build(None);

ServiceBuilder::new()
    .with_grpc_services(grpc_routes)
    .build()
    .serve()
    .await?;
```

---

## Configuration

```toml
# config.toml
[grpc]
# Enable gRPC server
enabled = true

# Use separate port for gRPC (false = single-port multiplexing)
use_separate_port = false

# gRPC port (only used if use_separate_port = true)
port = 9090

# Enable gRPC reflection
reflection_enabled = true

# Enable gRPC health check service
health_check_enabled = true

# Maximum message size in MB
max_message_size_mb = 4

# Connection timeout in seconds
connection_timeout_secs = 10

# Request timeout in seconds
timeout_secs = 30
```

Access in code:

```rust
let config = Config::load()?;

if let Some(grpc_config) = &config.grpc {
    let max_size = grpc_config.max_message_size_bytes();
    let timeout = grpc_config.timeout();
    // ...
}
```

---

## Complete Example

Full working example combining all features:

```rust
use acton_service::prelude::*;
use acton_service::grpc::{interceptors::*, middleware::*, server::GrpcServicesBuilder};
use acton_service::config::TokenConfig;
use std::sync::Arc;
use tonic::{Request, Response, Status};

// Include generated code
pub mod myservice {
    tonic::include_proto!("myservice.v1");
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("my_service_descriptor");
}

use myservice::{
    my_service_server::{MyService, MyServiceServer},
    GetUserRequest, UserResponse,
};

// Service implementation with state
struct MyServiceImpl {
    db: Arc<sqlx::PgPool>,
}

#[tonic::async_trait]
impl MyService for MyServiceImpl {
    async fn get_user(
        &self,
        request: Request<GetUserRequest>,
    ) -> Result<Response<UserResponse>, Status> {
        // Extract request ID
        if let Some(req_id) = request.extensions().get::<RequestIdExtension>() {
            tracing::info!(request_id = %req_id.0, "Processing GetUser");
        }

        // Extract token claims
        let claims = request.extensions().get::<Claims>()
            .ok_or_else(|| Status::unauthenticated("Missing auth"))?;

        let user_id = request.into_inner().user_id;

        // Fetch from database
        let user = sqlx::query_as!(
            User,
            "SELECT id, name, email FROM users WHERE id = $1",
            user_id
        )
        .fetch_one(&*self.db)
        .await
        .map_err(|_| Status::not_found("User not found"))?;

        Ok(Response::new(UserResponse {
            id: user.id,
            name: user.name,
            email: user.email,
        }))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load config
    let config = Config::load()?;

    // Setup database
    let db = Arc::new(get_db_pool(&config).await?);

    // Setup PASETO auth (default)
    let paseto_auth = match &config.token {
        Some(TokenConfig::Paseto(cfg)) => Arc::new(PasetoAuth::new(cfg)?),
        _ => panic!("Expected PASETO config"),
    };

    // Create service with interceptors
    let service_impl = MyServiceImpl { db };
    let grpc_service = MyServiceServer::with_interceptor(
        service_impl,
        move |req| {
            let req = request_id_interceptor(req)?;
            paseto_auth_interceptor(paseto_auth.clone())(req)
        }
    );

    // Build state so the health service can inspect dependencies
    let state = AppState::default();

    // Collect services, health, and reflection into tonic routes
    let grpc_routes = GrpcServicesBuilder::new()
        .with_health()
        .with_reflection()
        .add_file_descriptor_set(myservice::FILE_DESCRIPTOR_SET)
        .add_service(grpc_service)
        .build(Some(state.clone()));

    // Serve with acton-service
    ServiceBuilder::new()
        .with_config(config)
        .with_state(state)
        .with_grpc_services(grpc_routes)
        .build()
        .serve()
        .await
}
```

---

## Troubleshooting

### Proto Files Not Found

```
error: No .proto files found in directory: proto
```

**Solution:** Ensure `proto/` directory exists with `.proto` files, or set `ACTON_PROTO_DIR`.

### Descriptor Not Found

```
error: couldn't find `my_service_descriptor` in `OUT_DIR`
```

**Solution:** Descriptor name must match package name pattern:
- Package `myservice.v1` → descriptor `my_service_descriptor`
- Replace dots with underscores, add `_descriptor` suffix

### Build Fails Without ACTON_DATABASE_URL

SQLx compile-time verification requires database during build. acton-service automatically propagates `ACTON_DATABASE_URL` to SQLx.

**Solution:** Either set `ACTON_DATABASE_URL` or use SQLx offline mode:

```bash
# Option 1: Set environment variable
export ACTON_DATABASE_URL="postgres://localhost/dev_db"
cargo build

# Option 2: Use offline mode
cargo sqlx prepare  # Generate sqlx-data.json
export SQLX_OFFLINE=true
cargo build
```

### gRPC Service Not Responding

Check that:
1. `grpc` feature is enabled in Cargo.toml
2. Service is added via `GrpcServicesBuilder::add_service()` and the resulting routes passed to `ServiceBuilder::with_grpc_services()`
3. Correct port (default 8080, or check config)
4. Protocol detection working (use separate ports to debug)

---

## Next Steps

- Review [Dual HTTP+gRPC](/docs/dual-protocol) for protocol multiplexing
- Explore [Observability](/docs/observability) for gRPC tracing and metrics
- Check [Examples](/docs/examples) for complete working implementations
- See [Configuration](/docs/configuration) for gRPC-specific settings
- Read [Glossary](/docs/glossary) for protocol buffer and gRPC term definitions
