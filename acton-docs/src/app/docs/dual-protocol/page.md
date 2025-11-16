---
title: HTTP + gRPC Support
nextjs:
  metadata:
    title: Dual-Protocol HTTP + gRPC
    description: Run HTTP REST APIs and gRPC services on the same port with automatic protocol detection
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


# HTTP + gRPC Support

Acton Service supports running HTTP REST APIs and gRPC services simultaneously, either on a single port with automatic protocol detection or on separate ports for isolated traffic management. This dual-protocol capability enables you to serve different client types from the same service deployment.

---

## Why Run Both Protocols?

Combining HTTP and gRPC in a single service provides several architectural benefits:

**Internal Service Communication**
- Use gRPC for efficient, type-safe service-to-service communication
- Leverage HTTP/2 streaming and reduced serialization overhead
- Automatic schema validation with Protocol Buffers

**External Client Access**
- Expose HTTP REST APIs for browser clients and third-party integrations
- Support clients without gRPC tooling or Protocol Buffer support
- Maintain backwards compatibility with existing REST consumers

**Operational Simplicity**
- Single deployment unit serving both protocols
- Optional single-port mode with automatic protocol detection
- Unified observability, logging, and metrics across both protocols
- Simplified service mesh integration

---

## Single-Port Mode (Default)

By default, Acton Service runs both HTTP and gRPC on the same port using automatic protocol detection. The framework inspects incoming connections to determine whether they're HTTP/1.1, HTTP/2 REST, or HTTP/2 gRPC requests.

### How Protocol Detection Works

- **gRPC requests**: Identified by the `content-type: application/grpc` header
- **HTTP/2 requests**: Standard HTTP/2 traffic routed to REST handlers
- **HTTP/1.1 requests**: Automatically upgraded or handled as REST

This detection happens transparently with no performance penalty, allowing clients to connect using their preferred protocol without coordination.

### Benefits of Single-Port Mode

- **Simplified networking**: One port to configure in firewalls and load balancers
- **Reduced resource usage**: Single listener, single connection pool
- **Easier service discovery**: Clients only need to know one endpoint
- **Cloud-friendly**: Fewer port allocations in containerized environments

---

## Separate-Port Mode

For scenarios requiring protocol isolation, you can configure separate ports for HTTP and gRPC traffic:

```toml
[grpc]
enabled = true
use_separate_port = true
port = 9090  # gRPC traffic only
```

```toml
[server]
port = 8080  # HTTP traffic only
```

### When to Use Separate Ports

**Network Policy Requirements**
- Different firewall rules for internal vs external traffic
- Separate ingress controllers for HTTP and gRPC
- Protocol-specific rate limiting or throttling

**Performance Tuning**
- Dedicated connection pools for each protocol
- Independent timeout configurations
- Separate load balancing strategies

**Monitoring and Observability**
- Protocol-specific metrics collection
- Isolated traffic analysis
- Granular network-level debugging

---

## Configuration Options

Configure gRPC behavior in your `config.toml`:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `grpc.enabled` | boolean | `true` | Enable or disable gRPC support |
| `grpc.use_separate_port` | boolean | `false` | Use dedicated port for gRPC traffic |
| `grpc.port` | integer | `9090` | Port for gRPC when `use_separate_port = true` |
| `server.port` | integer | `8080` | Primary HTTP port (also serves gRPC in single-port mode) |

### Single-Port Configuration (Default)

```toml
[grpc]
enabled = true
use_separate_port = false  # Automatic protocol detection on server.port
```

### Separate-Port Configuration

```toml
[server]
port = 8080  # HTTP REST traffic

[grpc]
enabled = true
use_separate_port = true
port = 9090  # gRPC traffic only
```

---

## Complete Example

Here's a complete service implementing both HTTP REST and gRPC:

```rust
use acton_service::{ServiceBuilder, VersionedApiBuilder, ApiVersion};
use axum::{routing::get, Json};
use tonic::{Request, Response, Status};

// HTTP handler
async fn list_users() -> Json<Vec<String>> {
    Json(vec!["alice".to_string(), "bob".to_string()])
}

// Define HTTP routes with versioning
let http_routes = VersionedApiBuilder::new()
    .add_version(ApiVersion::V1, |router| {
        router.route("/users", get(list_users))
    })
    .build_routes();

// Define gRPC service
#[derive(Default)]
struct MyGrpcService;

#[tonic::async_trait]
impl my_service::MyService for MyGrpcService {
    async fn my_method(&self, req: Request<MyRequest>)
        -> Result<Response<MyResponse>, Status> {
        let user_id = req.into_inner().user_id;

        Ok(Response::new(MyResponse {
            message: format!("Hello from gRPC, user {}", user_id),
        }))
    }
}

// Serve both protocols on the same port (automatic protocol detection)
ServiceBuilder::new()
    .with_routes(http_routes)
    .with_grpc_service(my_service::MyServiceServer::new(MyGrpcService))
    .build()
    .serve()
    .await?;
```

### Client Examples

**HTTP REST client** (any HTTP library, curl, browser):
```bash
curl http://localhost:8080/v1/users
```

**gRPC client** (using Tonic or any gRPC client):
```rust
let mut client = my_service::MyServiceClient::connect("http://localhost:8080").await?;
let response = client.my_method(MyRequest { user_id: 123 }).await?;
```

Both clients connect to the same port, with the framework automatically routing traffic to the appropriate handler.

---

## Advanced Features

### Reflection Support

Enable gRPC reflection for dynamic client discovery:

```rust
use tonic_reflection::server::Builder;

let reflection_service = Builder::configure()
    .register_encoded_file_descriptor_set(my_service::FILE_DESCRIPTOR_SET)
    .build()?;

ServiceBuilder::new()
    .with_grpc_service(my_service::MyServiceServer::new(MyGrpcService))
    .with_grpc_service(reflection_service)
    .build()
    .serve()
    .await?;
```

### Health Checks

Both protocols share the same health check endpoint:

- **HTTP**: `GET /health`
- **gRPC**: Standard `grpc.health.v1.Health` service

---

## Next Steps

- See [Examples](/docs/examples) for complete dual-protocol service implementations
- Learn about [Configuration](/docs/configuration) options for fine-tuning both protocols
- Explore [gRPC Guide](/docs/grpc-guide) for protocol buffers, build setup, and implementation details
- Review [Observability](/docs/observability) for protocol-specific metrics and tracing
