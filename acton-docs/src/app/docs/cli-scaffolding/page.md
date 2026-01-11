---
title: Scaffolding Services
nextjs:
  metadata:
    title: Scaffolding Services
    description: Create production-ready backend services with the Acton CLI scaffolding tool
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


Use the Acton CLI to scaffold complete, production-ready backend services with all the features you need.

## Creating a New Service

The `acton service new` command creates a fully configured service with your chosen features. Choose from interactive prompts or fast command-line flags.

### Interactive Mode (Beginner-Friendly)

Interactive mode walks you through all options with helpful prompts:

```bash
acton service new my-service
```

This will prompt you for:
- Service type (HTTP/gRPC/Both)
- Database support (PostgreSQL, MySQL, SQLite)
- Caching support (Redis)
- Event streaming (NATS)
- Observability features (OpenTelemetry)
- Additional features (auth, rate limiting, etc.)

### Non-Interactive Mode (Fast)

Specify all options as command-line flags for instant scaffolding:

```bash
acton service new user-service \
    --http \
    --database postgres \
    --cache redis \
    --events nats \
    --observability
```

This creates a service with:
- HTTP REST API using Axum
- PostgreSQL database with SQLx
- Redis caching
- NATS event streaming
- OpenTelemetry tracing and metrics

### Quick Start (Minimal)

Accept all defaults and create a minimal HTTP service:

```bash
acton service new my-api --yes
```

This is perfect for:
- Quick prototyping
- Learning the framework
- Starting with minimal dependencies

## Common Service Patterns

### Simple HTTP API

Create a basic REST API service:

```bash
acton service new todo-api --yes
cd todo-api
cargo run
```

Default features:
- HTTP server on port 8080
- Health check endpoint
- Configuration management
- Structured logging
- Graceful shutdown

### Full-Stack Service

Create a comprehensive service with all features:

```bash
acton service new user-service \
    --http \
    --grpc \
    --database postgres \
    --cache redis \
    --events nats \
    --auth jwt \
    --observability \
    --resilience
```

Includes:
- Dual HTTP and gRPC protocols
- PostgreSQL with connection pooling
- Redis caching layer
- NATS event streaming
- JWT authentication
- OpenTelemetry tracing
- Circuit breaker and retry logic

### HTTP + gRPC Dual Protocol

Create a service that speaks both protocols:

```bash
acton service new gateway \
    --full \
    --database postgres
```

The `--full` flag enables both HTTP and gRPC automatically.

### Event-Driven Microservice

Create a service focused on event processing:

```bash
acton service new event-processor \
    --http \
    --events nats \
    --database postgres \
    --observability
```

Perfect for:
- Background job processing
- Event stream consumers
- Asynchronous workflows

## Available Options

### Service Type
- `--http` - Enable HTTP REST API (default)
- `--grpc` - Enable gRPC service
- `--full` - Enable both HTTP and gRPC

### Data Layer
- `--database <TYPE>` - Add database support
  - `postgres` - PostgreSQL with SQLx
  - `mysql` - MySQL support (planned)
  - `sqlite` - SQLite support (planned)
- `--cache <TYPE>` - Add caching layer
  - `redis` - Redis with connection pooling

### Event Streaming
- `--events <TYPE>` - Add event streaming
  - `nats` - NATS JetStream

### Authentication & Security
- `--auth <TYPE>` - Add authentication
  - `jwt` - JWT token authentication with Cedar authorization
- `--rate-limit` - Enable rate limiting per-user and per-client

### Observability
- `--observability` - Enable OpenTelemetry tracing and metrics
  - Distributed tracing with context propagation
  - Prometheus metrics endpoint
  - Health check endpoints

### Resilience
- `--resilience` - Enable resilience patterns
  - Circuit breaker for downstream services
  - Retry logic with exponential backoff
  - Bulkhead isolation
  - Timeout controls

### API Documentation
- `--openapi` - Generate OpenAPI/Swagger documentation
  - Auto-generated from code
  - Serves Swagger UI

### Additional Options
- `--template <NAME>` - Use organization template (planned)
- `--path <DIR>` - Create service in specific directory
- `--no-git` - Skip git initialization
- `-i, --interactive` - Force interactive mode
- `-y, --yes` - Accept all defaults
- `--dry-run` - Show what would be generated without creating files

## Generated Project Structure

A generated service has this structure:

```text
my-service/
├── Cargo.toml                 # Dependencies with correct features
├── config.toml                # Complete configuration
├── Dockerfile                 # Multi-stage build
├── .dockerignore
├── .gitignore
├── README.md                  # Generated documentation
├── build.rs                   # Proto compilation (if gRPC)
├── proto/                     # Proto files (if gRPC)
│   └── service.proto
└── src/
    ├── main.rs               # Service entry point
    └── handlers.rs           # HTTP handlers (if HTTP)
```

### Key Files

**Cargo.toml**: Contains all necessary dependencies with the correct feature flags. Uses workspace versions for consistency.

**config.toml**: Complete configuration template with:
- Server settings (host, port)
- Database connection strings
- Cache configuration
- Event stream settings
- Observability endpoints
- Feature-specific settings

**Dockerfile**: Multi-stage build optimized for:
- Fast builds with layer caching
- Small final image size
- Security best practices
- Production readiness

**src/main.rs**: Service entry point with:
- Configuration loading
- Dependency injection setup
- Server initialization
- Graceful shutdown handling
- Health check registration

**src/handlers.rs**: HTTP handler templates with:
- Request/response types
- Error handling
- Validation examples
- Documentation comments

## What Gets Generated

### HTTP Service

When you enable `--http`, you get:

**Router Setup**: Axum router with:
- Health check endpoint (`/health`)
- Example CRUD endpoints
- Middleware stack configuration
- Error handling

**Handler Functions**: Template handlers showing:
- Request extraction
- Response formatting
- Error handling patterns
- Database integration (if enabled)

**Configuration**: HTTP server settings:
```toml
[http]
host = "127.0.0.1"
port = 8080
```

### gRPC Service

When you enable `--grpc`, you get:

**Proto Files**: Example service definition:
```protobuf
syntax = "proto3";
package myservice.v1;

service MyService {
  rpc GetItem(GetItemRequest) returns (GetItemResponse);
}
```

**Build Script**: Compiles protos to Rust code

**Server Implementation**: Tonic service with:
- Request handlers
- Error conversion
- Metadata handling

### Database Integration

When you enable `--database postgres`, you get:

**Database Module**: SQLx integration with:
- Connection pool setup
- Migration support
- Example queries with compile-time checking

**Configuration**:
```toml
[database]
url = "postgresql://user:pass@localhost/db"
max_connections = 10
min_connections = 2
```

**Example Queries**: Type-safe query examples

### Caching Layer

When you enable `--cache redis`, you get:

**Cache Module**: Redis integration with:
- Connection pool management
- Common operations (get, set, delete)
- TTL handling

**Configuration**:
```toml
[cache]
url = "redis://localhost:6379"
pool_size = 10
```

### Event Streaming

When you enable `--events nats`, you get:

**Event Module**: NATS integration with:
- JetStream setup
- Publisher functions
- Subscriber handlers
- Stream configuration

**Configuration**:
```toml
[events]
url = "nats://localhost:4222"
```

### Authentication

When you enable `--auth jwt`, you get:

**Auth Module**: JWT validation with:
- Token verification
- Claims extraction
- Cedar policy engine integration
- Middleware setup

**Configuration**:
```toml
[auth]
jwt_secret = "your-secret-key"
jwt_expiration = 3600
```

### Observability

When you enable `--observability`, you get:

**Telemetry Module**: OpenTelemetry setup with:
- Tracer initialization
- Span creation helpers
- Metrics collection
- Export configuration

**Configuration**:
```toml
[telemetry]
service_name = "my-service"
otlp_endpoint = "http://localhost:4317"
```

## Next Steps After Generation

After generating your service:

1. **Review Configuration**: Update `config.toml` with your settings
2. **Add Dependencies**: Run database migrations if needed
3. **Implement Handlers**: Replace placeholder code with your logic
4. **Add Endpoints**: Use `acton service add endpoint` to add more routes
5. **Add Workers**: Use `acton service add worker` for background jobs
6. **Test**: Run `cargo test` to verify the service
7. **Run**: Start the service with `cargo run`

See [CLI Commands](/docs/cli-commands) for more ways to extend your service.
