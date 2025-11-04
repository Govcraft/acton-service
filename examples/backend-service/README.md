# backend-service

A production-ready microservice built with acton-service framework.

## Features

- **HTTP Server**: Powered by axum 0.8.6
- **gRPC Server**: Ready for tonic 0.14 (add .proto files to enable)
- **Dual-Protocol Routing**: HTTP and gRPC on same port (port 8080)
- **Middleware Stack**: Tracing, timeout, compression, CORS
- **Health Checks**: `/health` and `/ready` endpoints
- **Graceful Shutdown**: Handles SIGTERM/SIGINT
- **Configuration**: TOML files + environment variables via figment
- **Observability**: OpenTelemetry tracing ready

## Quick Start

1. **Install dependencies**:
   ```bash
   cd services/backend-service
   cargo build
   ```

2. **Configure the service**:
   ```bash
   cp .env.example .env
   # Edit .env and config.toml as needed
   ```

3. **Run the service**:
   ```bash
   cargo run
   ```

4. **Test health endpoints**:
   ```bash
   curl http://localhost:8080/health
   curl http://localhost:8080/ready
   ```

## Project Structure

```
backend-service/
├── src/
│   ├── main.rs              # Server setup, middleware, dual-protocol routing
│   ├── lib.rs               # Library exports
│   ├── config.rs            # Configuration loading and validation
│   ├── handlers/            # HTTP endpoint handlers
│   │   ├── mod.rs
│   │   └── health.rs        # Health check endpoints
│   ├── services/            # Business logic services
│   │   └── mod.rs
│   └── models/              # Domain models
│       └── mod.rs
├── config.toml              # Default configuration
├── .env.example             # Environment variable template
├── Cargo.toml               # Dependencies
└── build.rs                 # Build script for gRPC (proto compilation)
```

## Configuration

Configuration is loaded via [figment](https://docs.rs/figment) from:
1. `config.toml` (base configuration)
2. Environment variables prefixed with `ACTON_` (overrides)

Example:
- `ACTON_SERVICE_PORT=9000` overrides `service.port` in config.toml
- `ACTON_SERVICE_LOG_LEVEL=debug` overrides `service.log_level`

## Adding Features

### Add Database Support

1. Add to `Cargo.toml`:
   ```toml
   sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "uuid", "chrono"] }
   ```

2. Add to `config.toml`:
   ```toml
   [database]
   url = "postgres://user:pass@localhost:5433/db"
   max_connections = 50
   ```

3. Update `AppState` in `src/main.rs`:
   ```rust
   pub struct AppState {
       pub db: PgPool,
       // ...
   }
   ```

### Add Redis Cache

1. Add to `Cargo.toml`:
   ```toml
   deadpool-redis = { version = "0.19", features = ["rt_tokio_1"] }
   redis = { version = "0.27", features = ["tokio-comp"] }
   ```

2. Add to `config.toml`:
   ```toml
   [redis]
   url = "redis://localhost:6379"
   max_connections = 20
   ```

3. Update `AppState` in `src/main.rs`:
   ```rust
   pub struct AppState {
       pub redis: RedisPool,
       // ...
   }
   ```

### Add NATS Events

1. Add to `Cargo.toml`:
   ```toml
   async-nats = "0.38"
   ```

2. Add to `config.toml`:
   ```toml
   [nats]
   url = "nats://localhost:4222"
   ```

3. Update `AppState` in `src/main.rs`:
   ```rust
   pub struct AppState {
       pub nats: NatsClient,
       // ...
   }
   ```

### Add gRPC Support

1. Create `proto/` directory and add your `.proto` files

2. Update `build.rs`:
   ```rust
   fn main() -> Result<(), Box<dyn std::error::Error>> {
       tonic_build::compile_protos("proto/service.proto")?;
       Ok(())
   }
   ```

3. Uncomment gRPC sections in `src/main.rs`

4. Implement your gRPC service

## Development

### Run with auto-reload
```bash
cargo install cargo-watch
cargo watch -x run
```

### Run tests
```bash
cargo test
```

### Check code
```bash
cargo clippy
cargo fmt
```

## Middleware Stack

Middleware is executed in order from outer to inner:

1. **TraceLayer** - Request ID and distributed tracing
2. **TimeoutLayer** - 30-second timeout
3. **CompressionLayer** - gzip/brotli compression
4. **CorsLayer** - CORS headers
5. **Business Logic** - Your handlers

When you add authentication/rate limiting, they go between CORS and handlers.

## Health Checks

### Liveness Probe
- **Endpoint**: `GET /health`
- **Purpose**: Service is running
- **Returns**: `200 OK` with `{"status": "ok", "service": "backend-service"}`

### Readiness Probe
- **Endpoint**: `GET /ready`
- **Purpose**: Service can handle requests
- **Returns**: `200 OK` when ready, `503 Service Unavailable` when not ready

## Graceful Shutdown

The service handles shutdown signals gracefully:
- Listens for SIGTERM (Kubernetes) and SIGINT (Ctrl+C)
- Stops accepting new connections
- Waits for in-flight requests to complete
- Closes connection pools
- Exits cleanly

## Next Steps

1. Add your HTTP routes in `src/handlers/`
2. Implement business logic in `src/services/`
3. Define domain models in `src/models/`
4. Add database migrations (if using database)
5. Add gRPC .proto definitions (if using gRPC)
6. Configure middleware (auth, rate limiting) as needed
7. Write tests in `tests/` directory

## Production Deployment

### Docker
Create a `Dockerfile`:
```dockerfile
FROM rust:1.83 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/backend-service /usr/local/bin/
CMD ["backend-service"]
```

### Kubernetes
Example deployment:
```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: backend-service
spec:
  replicas: 3
  template:
    spec:
      containers:
      - name: backend-service
        image: backend-service:latest
        ports:
        - containerPort: 8080
        env:
        - name: ACTON_SERVICE_PORT
          value: "8080"
        livenessProbe:
          httpGet:
            path: /health
            port: 8080
        readinessProbe:
          httpGet:
            path: /ready
            port: 8080
```

## License

MIT
