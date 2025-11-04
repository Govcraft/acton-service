# API Gateway Service

API Gateway service built with acton-service framework.

## Features

- HTTP endpoints (Axum)
- gRPC services (Tonic) - ready for implementation
- Middleware stack: Tracing → Timeout → Compression → CORS
- Health and readiness endpoints
- Graceful shutdown
- OpenTelemetry observability
- Configuration via TOML and environment variables

## Getting Started

### Prerequisites

- Rust 1.70+
- OpenTelemetry collector (optional, for tracing)

### Building

```bash
cargo build
```

### Running

```bash
# Using config.toml
cargo run

# Or with environment variables
export ACTON_SERVICE_PORT=8081
export ACTON_SERVICE_LOG_LEVEL=debug
cargo run
```

### Testing

```bash
cargo test
```

## Configuration

Configuration is loaded from `config.toml` and can be overridden with environment variables prefixed with `ACTON_`.

Example:
```bash
export ACTON_SERVICE_PORT=9000
export ACTON_SERVICE_LOG_LEVEL=debug
```

See `.env.example` for all available environment variables.

## Endpoints

- `GET /health` - Health check (liveness probe)
- `GET /ready` - Readiness check (readiness probe)

## Development

### Adding HTTP Endpoints

1. Create handler in `src/handlers/`
2. Register route in `src/main.rs` router
3. Add business logic in `src/services/`

### Adding gRPC Services

1. Define protobuf in `proto/` directory
2. Update `build.rs` to compile proto
3. Implement service trait
4. Register with tonic server in `src/main.rs`

## Project Structure

```
api-gateway/
├── src/
│   ├── main.rs           # Server setup and middleware
│   ├── lib.rs            # Library exports
│   ├── config.rs         # Configuration management
│   ├── handlers/         # HTTP/gRPC handlers
│   │   ├── mod.rs
│   │   └── health.rs
│   ├── services/         # Business logic
│   │   └── mod.rs
│   └── models/           # Domain models
│       └── mod.rs
├── config.toml           # Default configuration
├── .env.example          # Environment variable template
├── build.rs              # Build script for protobuf
└── Cargo.toml            # Dependencies
```

## License

MIT
