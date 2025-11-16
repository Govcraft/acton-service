# gRPC Examples

Examples demonstrating gRPC service integration with acton-service.

## Examples

### single-port.rs

**HTTP REST + gRPC on a single port**

Demonstrates:
- Running HTTP REST API and gRPC services on port 8080
- Automatic protocol detection based on content-type header
- gRPC requests (application/grpc) route to tonic services
- All other requests route to axum HTTP handlers

Run with:
```bash
cargo run --manifest-path=../../Cargo.toml --example single-port --features grpc
```

## Prerequisites

The gRPC examples require:
- The `grpc` feature flag
- Protocol buffer definitions in `proto/` directory
- `tonic` and `prost` dependencies

## Protocol Buffer Setup

For your own projects, use the acton-service build utilities for proto compilation. Add to your `build.rs`:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "grpc")]
    {
        acton_service::build_utils::compile_service_protos()?;
    }
    Ok(())
}
```

This automatically compiles all `.proto` files in your `proto/` directory.

## Testing

### HTTP Endpoints

```bash
curl http://localhost:8080/health
curl http://localhost:8080/ready
curl http://localhost:8080/api/v1/hello
```

### gRPC Endpoints

Use `grpcurl` or any gRPC client:

```bash
# List services
grpcurl -plaintext localhost:8080 list

# Call a method
grpcurl -plaintext -d '{"name": "world"}' \
  localhost:8080 hello.HelloService/SayHello
```

## Next Steps

- Explore [event-driven examples](../events/) for gRPC + event bus patterns
- See the acton-service documentation for more gRPC configuration options
