# Basic Examples

Simple getting-started examples demonstrating core acton-service functionality.

## Examples

### simple-api.rs

**Zero-configuration versioned API**

Demonstrates:
- Automatic configuration loading from environment/files
- Automatic tracing/logging initialization
- Type-safe API versioning (impossible to bypass at compile time)
- Automatic health (`/health`) and readiness (`/ready`) endpoints
- Multiple API versions on a single service

Run with:
```bash
cargo run --manifest-path=../../Cargo.toml --example simple-api
```

Test:
```bash
curl http://localhost:8080/health
curl http://localhost:8080/ready
curl http://localhost:8080/api/v1/hello
curl http://localhost:8080/api/v2/hello
```

### users-api.rs

**Multi-version API evolution**

Demonstrates:
- Multiple API versions (V1, V2, V3)
- Automatic deprecation headers
- API evolution and breaking changes
- Type-safe version routing

Run with:
```bash
cargo run --manifest-path=../../Cargo.toml --example users-api
```

Test:
```bash
curl -i http://localhost:8080/api/v1/users
curl -i http://localhost:8080/api/v2/users
curl -i http://localhost:8080/api/v3/users
```

Note the deprecation warnings in V1 and V2 responses.

### ping-pong.rs

**Simple request/response example**

Demonstrates:
- Basic HTTP endpoint setup
- Simple request handling
- Minimal service configuration

Run with:
```bash
cargo run --manifest-path=../../Cargo.toml --example ping-pong
```

Test:
```bash
curl http://localhost:8080/ping
```

## Learning Path

1. Start with `simple-api.rs` to understand the basic service structure
2. Move to `users-api.rs` to learn about API version management
3. Use `ping-pong.rs` as a template for minimal services

## Next Steps

After mastering these basics, explore:
- [Authorization examples](../authorization/) - Add Cedar policy-based access control
- [gRPC examples](../grpc/) - Combine HTTP and gRPC on a single port
- [Event-driven examples](../events/) - Build decoupled microservices
