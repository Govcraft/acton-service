# Acton CLI

Production-ready CLI for scaffolding and managing microservices built with the acton-service framework.

## Features

✅ **Fully Implemented:**
- Interactive and non-interactive service creation
- Comprehensive command structure
- Service scaffolding with multiple features
- Template-based code generation
- Git integration
- Automatic code formatting
- `acton service add endpoint` - Add HTTP endpoints ✨ NEW
- `acton service add worker` - Add background workers ✨ NEW
- `acton service generate deployment` - Generate K8s manifests ✨ NEW

🚧 **Planned:**
- `acton service add grpc` - Add gRPC services
- `acton service add middleware` - Add custom middleware
- `acton service add version` - Add API versions
- `acton service validate` - Validate service quality
- `acton service generate config` - Generate configuration
- `acton service generate proto` - Generate proto files
- `acton service dev` - Development tools

## Installation

Build from source:

```bash
cargo build --release -p acton-cli
```

The binary will be available at `target/release/acton`.

## Usage

### Command Structure

```
acton
├── service
│   ├── new <service-name>          # Create new service
│   ├── add
│   │   ├── endpoint                # Add HTTP endpoint
│   │   ├── grpc                    # Add gRPC service
│   │   ├── worker                  # Add background worker
│   │   ├── middleware              # Add middleware
│   │   └── version                 # Add API version
│   ├── generate
│   │   ├── deployment              # Generate deployment configs
│   │   ├── config                  # Generate config file
│   │   └── proto                   # Generate proto file
│   ├── validate                    # Validate service
│   └── dev
│       ├── run                     # Run development server
│       ├── health                  # Check service health
│       └── logs                    # View logs
└── [future top-level commands]
```

### Creating a New Service

#### Interactive Mode (Beginner-Friendly)

```bash
acton service new my-service
```

This will prompt you for:
- Service type (HTTP/gRPC/Both)
- Database support
- Caching support
- Event streaming
- Observability features

#### Non-Interactive Mode (Fast)

```bash
acton service new my-service \
    --http \
    --database postgres \
    --cache redis \
    --observability
```

#### Quick Start (Minimal)

```bash
acton service new my-service --yes
```

Creates a minimal HTTP service with defaults.

### Available Options

```
--http                 Enable HTTP REST API (default)
--grpc                 Enable gRPC service
--full                 Enable both HTTP and gRPC
--database <TYPE>      Add database (postgres)
--cache <TYPE>         Add caching (redis)
--events <TYPE>        Add event streaming (nats)
--auth <TYPE>          Add authentication (jwt)
--observability        Enable OpenTelemetry tracing
--resilience           Enable circuit breaker, retry, etc.
--rate-limit           Enable rate limiting
--openapi              Generate OpenAPI/Swagger
--template <NAME>      Use organization template
--path <DIR>           Create in specific directory
--no-git               Skip git initialization
-i, --interactive      Interactive mode
-y, --yes              Accept all defaults
--dry-run              Show what would be generated
```

## Examples

### Simple HTTP API

```bash
acton service new todo-api --yes
cd todo-api
cargo run
```

### Full-Stack Service

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

### HTTP + gRPC Dual Protocol

```bash
acton service new gateway \
    --full \
    --database postgres
```

## Generated Project Structure

```
my-service/
├── Cargo.toml                 # Dependencies with correct features
├── config.toml                # Complete configuration
├── Dockerfile                 # Multi-stage build
├── .dockerignore
├── .gitignore
├── README.md                  # Generated documentation
├── build.rs                   # Proto compilation (if gRPC)
├── proto/                     # Proto files (if gRPC)
└── src/
    ├── main.rs               # Service entry point
    └── handlers.rs           # HTTP handlers (if HTTP)
```

## Design Philosophy

The Acton CLI follows these principles:

1. **Progressive Disclosure** - Simple by default, powerful when needed
2. **Safe by Default** - Prevent mistakes through validation
3. **Educational** - Generated code teaches framework patterns
4. **Production-Ready** - Every service meets operational standards
5. **Discoverable** - Self-documenting with excellent help

## User Personas Supported

- **Beginners** - Interactive wizard, educational output
- **Intermediate** - Fast scaffolding with flags
- **Senior Engineers** - Organization templates (coming soon)
- **DevOps/SRE** - Deployment validation (coming soon)

## Architecture

The CLI is built with:

- **Command parsing**: `clap` with derive macros
- **Interactivity**: `dialoguer` for prompts
- **Templating**: `handlebars` for code generation
- **Progress**: `indicatif` for progress bars
- **Colors**: `colored` and `console` for output

## Development

### Project Structure

```
acton-cli/
├── src/
│   ├── main.rs                    # Entry point
│   ├── commands/
│   │   └── service/               # Service commands
│   │       ├── new.rs            # ✅ Implemented
│   │       ├── add/              # 🚧 Stubs
│   │       ├── generate/         # 🚧 Stubs
│   │       ├── validate.rs       # 🚧 Stub
│   │       └── dev/              # 🚧 Stubs
│   ├── templates/                 # Code generation templates
│   │   ├── service.rs            # ✅ main.rs templates
│   │   ├── cargo_toml.rs         # ✅ Cargo.toml generation
│   │   ├── config.rs             # ✅ config.toml generation
│   │   ├── handlers.rs           # ✅ Handler templates
│   │   └── deployment.rs         # ✅ Dockerfile templates
│   ├── utils/                     # Utilities
│   │   ├── format.rs             # Name conversions
│   │   ├── git.rs                # Git operations
│   │   └── cargo.rs              # Cargo operations
│   └── validator/                 # 🚧 Validation logic
└── Cargo.toml
```

### Building

```bash
# Development build
cargo build -p acton-cli

# Release build
cargo build --release -p acton-cli

# Run directly
cargo run -p acton-cli -- service new test-api --yes
```

### Testing

```bash
# Create a test service
./target/debug/acton service new test-service --yes --path /tmp

# Verify it was created
ls -la /tmp/test-service

# Clean up
rm -rf /tmp/test-service
```

## Command Examples

### Adding HTTP Endpoints

```bash
# Add a GET endpoint
acton service add endpoint GET /users --version v1

# Add a POST endpoint with full options
acton service add endpoint POST /users \
    --handler create_user \
    --model User \
    --validate \
    --openapi

# Preview what would be generated
acton service add endpoint GET /users/:id --dry-run
```

### Adding Background Workers

```bash
# Add a NATS worker
acton service add worker email-worker \
    --source nats \
    --stream emails \
    --subject "emails.>"

# Add a Redis Stream worker
acton service add worker notification-worker \
    --source redis-stream \
    --stream notifications

# Preview worker generation
acton service add worker my-worker --source nats --stream events --dry-run
```

### Generating Deployments

```bash
# Generate basic Kubernetes manifests
acton service generate deployment

# Generate with autoscaling and monitoring
acton service generate deployment \
    --hpa \
    --monitoring \
    --replicas 3

# Generate complete production setup
acton service generate deployment \
    --namespace production \
    --hpa \
    --monitoring \
    --ingress \
    --tls \
    --registry gcr.io/myproject \
    --image-tag v1.0.0

# Preview deployment manifests
acton service generate deployment --dry-run
```

## Next Steps

To continue implementation:

1. **Add gRPC Command** - gRPC service generation
2. **Add Middleware Command** - Custom middleware handlers
3. **Add Version Command** - API versioning support
4. **Validate Command** - Quality checks and scoring
5. **Generate Config Command** - Configuration file generation
6. **Dev Commands** - Development server, health checks, logs

## Contributing

The CLI is designed to be extended. Key extension points:

- **Templates** - Add new code templates in `src/templates/`
- **Commands** - Add new commands in `src/commands/service/`
- **Validators** - Add validation rules in `src/validator/`
- **Utilities** - Add helper functions in `src/utils/`

## License

MIT
