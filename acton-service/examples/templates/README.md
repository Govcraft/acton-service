# Configuration Templates

Template files for starting new acton-service projects.

## Templates

### config.toml.example

**Service Configuration Template**

Complete configuration template showing all available options for acton-service.

Sections include:
- Service metadata (name, version)
- Network configuration (HTTP/gRPC ports)
- Database connection settings
- Redis cache configuration
- JWT authentication settings
- Cedar authorization policies
- OpenTelemetry/observability setup
- Feature flags

Copy and customize:
```bash
cp config.toml.example config.toml
# Edit config.toml for your service
```

Configuration precedence (highest to lowest):
1. Environment variables (`ACTON_SERVICE_PORT=8080`)
2. `config.toml` file
3. Default values

### build.rs.example

**Build Script for Protocol Buffer Compilation**

Template `build.rs` for projects using gRPC/Protocol Buffers.

Features:
- Automatic proto file compilation
- Integration with acton-service build utilities
- Conditional compilation based on features

Copy and customize:
```bash
cp build.rs.example build.rs
# Add to your Cargo.toml:
# [build-dependencies]
# acton-service = { version = "x.y.z", features = ["build-utils"] }
```

## Usage

### Starting a New Project

1. Create your project structure:
```bash
cargo new my-service
cd my-service
```

2. Add acton-service dependency:
```bash
cargo add acton-service --features <features-you-need>
```

3. Copy relevant templates:
```bash
cp examples/templates/config.toml.example config.toml
```

4. If using gRPC:
```bash
mkdir proto
cp examples/templates/build.rs.example build.rs
# Add your .proto files to proto/
```

5. Customize configuration for your service

## Configuration Tips

### Environment Variables

All config values can be overridden via environment variables:

```bash
# Service settings
export ACTON_SERVICE_NAME=my-service
export ACTON_SERVICE_PORT=8080

# Database
export ACTON_DATABASE_URL=postgres://localhost/mydb

# Redis cache
export ACTON_CACHE_ENABLED=true
export ACTON_CACHE_URL=redis://localhost:6379

# JWT
export ACTON_JWT_SECRET=your-secret-key

# Cedar authorization
export ACTON_CEDAR_ENABLED=true
export ACTON_CEDAR_POLICY_PATH=policies.cedar
```

### Feature Flags

Enable features in `Cargo.toml`:

```toml
[dependencies]
acton-service = { version = "x.y.z", features = [
    "grpc",           # gRPC support
    "cedar-authz",    # Cedar policy authorization
    "cache",          # Redis caching
    "observability",  # OpenTelemetry metrics/tracing
] }
```

### Multi-Environment Configuration

Use multiple config files:

```
config/
├── base.toml          # Shared configuration
├── development.toml   # Dev overrides
├── staging.toml       # Staging overrides
└── production.toml    # Production overrides
```

Load based on environment:
```bash
export ACTON_CONFIG=config/production.toml
```

## Next Steps

- Review the [basic examples](../basic/) to see configuration in action
- Check the main acton-service documentation for all config options
- Explore example-specific configurations in other categories
