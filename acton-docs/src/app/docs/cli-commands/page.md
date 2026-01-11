---
title: CLI Commands
nextjs:
  metadata:
    title: CLI Commands
    description: Complete reference for all Acton CLI commands with examples and options
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


Complete reference for all commands available in the Acton CLI tool.

## Service Commands

All service management commands are under the `acton service` namespace.

### acton service new

Create a new backend service with configurable features.

**Syntax:**
```bash
acton service new <service-name> [OPTIONS]
```

**Arguments:**
- `<service-name>` - Name of the service to create (kebab-case recommended)

**Options:**

Service Type:
- `--http` - Enable HTTP REST API (default)
- `--grpc` - Enable gRPC service
- `--full` - Enable both HTTP and gRPC

Data Layer:
- `--database <TYPE>` - Add database (postgres)
- `--cache <TYPE>` - Add caching (redis)

Event Streaming:
- `--events <TYPE>` - Add event streaming (nats)

Authentication:
- `--auth <TYPE>` - Add authentication (jwt)

Features:
- `--observability` - Enable OpenTelemetry tracing
- `--resilience` - Enable circuit breaker, retry patterns
- `--rate-limit` - Enable rate limiting
- `--openapi` - Generate OpenAPI/Swagger

Project Options:
- `--template <NAME>` - Use organization template
- `--path <DIR>` - Create in specific directory
- `--no-git` - Skip git initialization

Mode:
- `-i, --interactive` - Interactive mode with prompts
- `-y, --yes` - Accept all defaults
- `--dry-run` - Show what would be generated

**Examples:**

Minimal service with defaults:
```bash
acton service new my-api --yes
```

Full-featured service:
```bash
acton service new user-service \
    --http \
    --database postgres \
    --cache redis \
    --events nats \
    --auth jwt \
    --observability \
    --resilience
```

Interactive mode:
```bash
acton service new my-service --interactive
```

Preview generation:
```bash
acton service new test-service --yes --dry-run
```

### acton service add endpoint

Add a new HTTP endpoint to an existing service.

**Syntax:**
```bash
acton service add endpoint <METHOD> <PATH> [OPTIONS]
```

**Arguments:**
- `<METHOD>` - HTTP method (GET, POST, PUT, DELETE, PATCH)
- `<PATH>` - Route path (e.g., `/users`, `/users/:id`)

**Options:**
- `--handler <NAME>` - Handler function name
- `--version <VERSION>` - API version (e.g., v1, v2)
- `--model <TYPE>` - Request/response model type
- `--validate` - Add request validation
- `--openapi` - Add OpenAPI documentation
- `--dry-run` - Preview without creating

**Examples:**

Add a GET endpoint:
```bash
acton service add endpoint GET /users --version v1
```

Add a POST endpoint with full options:
```bash
acton service add endpoint POST /users \
    --handler create_user \
    --model User \
    --validate \
    --openapi
```

Add endpoint with path parameters:
```bash
acton service add endpoint GET /users/:id \
    --handler get_user_by_id \
    --version v1
```

Preview endpoint generation:
```bash
acton service add endpoint GET /users/:id --dry-run
```

**What It Generates:**

- Handler function in `src/handlers.rs` or versioned module
- Route registration in router
- Request/response types (if `--model` specified)
- Validation logic (if `--validate` specified)
- OpenAPI annotations (if `--openapi` specified)

### acton service add worker

Add a background worker for event processing.

**Syntax:**
```bash
acton service add worker <worker-name> [OPTIONS]
```

**Arguments:**
- `<worker-name>` - Name of the worker

**Options:**
- `--source <TYPE>` - Event source (nats, redis-stream)
- `--stream <NAME>` - Stream name
- `--subject <PATTERN>` - NATS subject pattern (for NATS)
- `--group <NAME>` - Consumer group name
- `--dry-run` - Preview without creating

**Examples:**

Add a NATS worker:
```bash
acton service add worker email-worker \
    --source nats \
    --stream emails \
    --subject "emails.>"
```

Add a Redis Stream worker:
```bash
acton service add worker notification-worker \
    --source redis-stream \
    --stream notifications
```

Add worker with consumer group:
```bash
acton service add worker order-processor \
    --source nats \
    --stream orders \
    --subject "orders.*" \
    --group order-processors
```

Preview worker generation:
```bash
acton service add worker my-worker \
    --source nats \
    --stream events \
    --dry-run
```

**What It Generates:**

- Worker module with event handler
- Stream/subject subscription setup
- Message processing logic template
- Error handling and retry logic
- Integration with service lifecycle

### acton service generate deployment

Generate Kubernetes manifests and deployment configurations.

**Syntax:**
```bash
acton service generate deployment [OPTIONS]
```

**Options:**

Kubernetes:
- `--namespace <NAME>` - Kubernetes namespace (default: default)
- `--replicas <N>` - Number of replicas (default: 1)
- `--hpa` - Enable Horizontal Pod Autoscaler
- `--monitoring` - Enable monitoring (ServiceMonitor)
- `--ingress` - Generate Ingress resource
- `--tls` - Enable TLS for Ingress

Resource Limits:
- `--memory <AMOUNT>` - Memory limit (e.g., 512Mi, 1Gi)
- `--cpu <AMOUNT>` - CPU limit (e.g., 500m, 1)

Container Registry:
- `--registry <URL>` - Container registry (e.g., gcr.io/myproject)
- `--image-tag <TAG>` - Image tag (default: latest)

Options:
- `--dry-run` - Preview without creating files

**Examples:**

Basic Kubernetes manifests:
```bash
acton service generate deployment
```

Production setup with autoscaling:
```bash
acton service generate deployment \
    --hpa \
    --monitoring \
    --replicas 3
```

Complete production deployment:
```bash
acton service generate deployment \
    --namespace production \
    --hpa \
    --monitoring \
    --ingress \
    --tls \
    --registry gcr.io/myproject \
    --image-tag v1.0.0 \
    --memory 1Gi \
    --cpu 1
```

Preview deployment manifests:
```bash
acton service generate deployment --dry-run
```

**What It Generates:**

- `k8s/deployment.yaml` - Deployment resource
- `k8s/service.yaml` - Service resource
- `k8s/hpa.yaml` - HorizontalPodAutoscaler (if `--hpa`)
- `k8s/ingress.yaml` - Ingress resource (if `--ingress`)
- `k8s/servicemonitor.yaml` - ServiceMonitor (if `--monitoring`)
- `Dockerfile` - Multi-stage build (if not exists)
- `.dockerignore` - Docker ignore rules

## Planned Commands

These commands are planned for future releases:

### acton service add grpc

Add a gRPC service definition and implementation.

**Planned Syntax:**
```bash
acton service add grpc <service-name> \
    --method <MethodName> \
    --request <RequestType> \
    --response <ResponseType>
```

### acton service add middleware

Add custom middleware to the HTTP stack.

**Planned Syntax:**
```bash
acton service add middleware <name> \
    --type <auth|logging|metrics|custom>
```

### acton service add version

Add a new API version to the service.

**Planned Syntax:**
```bash
acton service add version <version> \
    --from <existing-version>
```

### acton service validate

Validate service quality and best practices.

**Planned Syntax:**
```bash
acton service validate [OPTIONS]
```

Will check:
- Code quality and linting
- Test coverage
- Configuration completeness
- Security best practices
- Documentation

### acton service generate config

Generate configuration files for different environments.

**Planned Syntax:**
```bash
acton service generate config \
    --env <dev|staging|production>
```

### acton service generate proto

Generate gRPC proto files from service definition.

**Planned Syntax:**
```bash
acton service generate proto \
    --service <ServiceName>
```

### acton service dev

Development tools for running and testing services.

**Planned Syntax:**
```bash
# Run development server with hot reload
acton service dev run

# Check service health
acton service dev health

# View service logs
acton service dev logs
```

## Global Options

These options work with all commands:

- `-h, --help` - Show help information
- `-V, --version` - Show version information
- `-v, --verbose` - Enable verbose output
- `-q, --quiet` - Suppress output

**Examples:**

Show help for a command:
```bash
acton service new --help
```

Show CLI version:
```bash
acton --version
```

Verbose output:
```bash
acton service new my-api --yes --verbose
```

## Command Chaining

Common workflows combining multiple commands:

### Create and Extend Service

```bash
# Create base service
acton service new user-service \
    --http \
    --database postgres \
    --yes

# Navigate to service
cd user-service

# Add endpoints
acton service add endpoint GET /users --version v1
acton service add endpoint POST /users --handler create_user
acton service add endpoint GET /users/:id --handler get_user

# Add worker
acton service add worker user-events \
    --source nats \
    --stream users \
    --subject "users.*"

# Generate deployment
acton service generate deployment --hpa --monitoring
```

### Full Production Setup

```bash
# Create production-ready service
acton service new payment-service \
    --http \
    --grpc \
    --database postgres \
    --cache redis \
    --events nats \
    --auth jwt \
    --observability \
    --resilience \
    --rate-limit

cd payment-service

# Add payment endpoints
acton service add endpoint POST /payments --handler create_payment
acton service add endpoint GET /payments/:id --handler get_payment

# Add payment processor worker
acton service add worker payment-processor \
    --source nats \
    --stream payments \
    --subject "payments.process"

# Generate Kubernetes manifests
acton service generate deployment \
    --namespace production \
    --hpa \
    --monitoring \
    --ingress \
    --tls \
    --registry gcr.io/mycompany \
    --image-tag v1.0.0
```

## Exit Codes

The CLI uses standard exit codes:

- `0` - Success
- `1` - General error
- `2` - Invalid arguments
- `130` - Interrupted by user (Ctrl+C)

## Environment Variables

Some commands respect environment variables:

- `ACTON_TEMPLATE_PATH` - Custom template directory
- `ACTON_CONFIG_PATH` - Default config location
- `NO_COLOR` - Disable colored output

**Example:**
```bash
export ACTON_TEMPLATE_PATH=/path/to/custom/templates
acton service new my-service --yes
```

## Getting Help

For detailed help on any command:

```bash
acton --help                      # Top-level help
acton service --help              # Service commands help
acton service new --help          # Specific command help
```

## Next Steps

- See [CLI Overview](/docs/cli-overview) for design philosophy
- Learn [Service Scaffolding](/docs/cli-scaffolding) patterns
- Review [Getting Started](/docs/getting-started) for complete examples
