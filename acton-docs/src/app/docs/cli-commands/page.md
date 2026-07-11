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
- `--graphql` - Scaffold a versioned GraphQL transport (Axum + async-graphql)

Project Options:
- `--template <NAME>` - Organization template name. Accepted, but service generation currently resolves templates from your XDG config directory — see `acton setup templates`.
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
- `<PATH>` - Route path (e.g., `/users`, `/users/{id}`)

**Options:**
- `--version <VERSION>` - API version (default: `v1`)
- `--handler <NAME>` - Handler function name
- `--auth <TYPE>` - Require authentication
- `--rate-limit <LIMIT>` - Rate limit (requests per minute)
- `--model <NAME>` - Generate associated model struct
- `--validate` - Add request validation
- `--response <TYPE>` - Response type (default: `json`)
- `--cache` - Add caching layer
- `--event <NAME>` - Publish event after success
- `--openapi` - Add OpenAPI annotations
- `--dry-run` - Preview without creating

{% callout type="note" title="axum 0.8 path syntax" %}
Path parameters use braces: `/users/{id}`. The older colon form (`/users/:id`) is not valid in axum 0.8.
{% /callout %}

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
acton service add endpoint GET /users/{id} \
    --handler get_user_by_id \
    --version v1
```

Add an authenticated, rate-limited endpoint:
```bash
acton service add endpoint DELETE /users/{id} \
    --handler delete_user \
    --auth jwt \
    --rate-limit 60
```

Preview endpoint generation:
```bash
acton service add endpoint GET /users/{id} --dry-run
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
- `--source <SOURCE>` - Event source (**required**)
- `--stream <NAME>` - Stream name (**required**)
- `--subject <PATTERN>` - NATS subject pattern
- `--dry-run` - Preview without creating

**Examples:**

Add a NATS worker:
```bash
acton service add worker email-worker \
    --source nats \
    --stream emails \
    --subject "emails.>"
```

Add a worker with a wildcard subject:
```bash
acton service add worker order-processor \
    --source nats \
    --stream orders \
    --subject "orders.*"
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
- `--namespace <NAME>` - Kubernetes namespace
- `--replicas <N>` - Number of replicas (default: 3)
- `--hpa` - Enable HorizontalPodAutoscaler
- `--monitoring` - Generate ServiceMonitor for Prometheus
- `--ingress` - Generate Ingress resource
- `--tls` - Enable TLS/HTTPS

Resource Limits:
- `--memory <SIZE>` - Memory limit (default: `512Mi`)
- `--cpu <MILLICORES>` - CPU limit (default: `500m`)

Container Registry:
- `--registry <URL>` - Container registry (e.g., gcr.io/myproject)
- `--image-tag <TAG>` - Image tag (default: `latest`)

Options:
- `--env <STAGE>` - Environment stage
- `--output <DIR>` - Output directory (default: `./deployment`)
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

Files are written to the `--output` directory (default `./deployment`):

- `deployment.yaml` - Deployment resource
- `service.yaml` - Service resource
- `hpa.yaml` - HorizontalPodAutoscaler (if `--hpa`)
- `ingress.yaml` - Ingress resource (if `--ingress`)
- `servicemonitor.yaml` - ServiceMonitor (if `--monitoring`)

The `Dockerfile` and `.dockerignore` are generated by `acton service new`, not by this command.

### acton service add grpc

Add a gRPC service definition and implementation.

**Syntax:**
```bash
acton service add grpc <SERVICE_NAME> [OPTIONS]
```

**Arguments:**
- `<SERVICE_NAME>` - Service name (PascalCase)

**Options:**
- `--package <NAME>` - Proto package
- `--method <NAME>` - Add RPC method
- `--request <TYPE>` - Request message type
- `--response <TYPE>` - Response message type
- `--health` - Enable health checks (default: true)
- `--reflection` - Enable server reflection (default: true)
- `--streaming` - Add streaming support
- `--handler` - Generate handler implementation
- `--client` - Generate client code
- `--interceptor <TYPE>` - Add interceptor
- `--dry-run` - Preview without creating

**Examples:**

```bash
acton service add grpc UserService \
    --package users.v1 \
    --method GetUser \
    --request GetUserRequest \
    --response GetUserResponse \
    --handler
```

### acton service add graphql

Add a versioned GraphQL transport (Axum + async-graphql) to an existing service.

**Syntax:**
```bash
acton service add graphql [OPTIONS]
```

**Options:**
- `--version <VERSION>` - API version to scaffold the schema under (default: `v1`)
- `--cedar` - Enable Cedar resolver authorization (requires the `cedar-authz` feature)
- `--dry-run` - Preview without creating

**Examples:**

```bash
acton service add graphql --version v1
acton service add graphql --version v2 --cedar
```

### acton service add middleware

Show how to wire a given middleware type into the service stack.

**Syntax:**
```bash
acton service add middleware <TYPE> [OPTIONS]
```

**Arguments:**
- `<TYPE>` - Middleware type. One of:
  - `jwt`, `auth`, `authentication`
  - `resilience`, `circuit-breaker`, `retry`
  - `metrics`, `otel`, `opentelemetry`
  - `governor`, `rate-limit`, `ratelimit`
  - `cors`
  - `compression`
  - `panic`, `catch-panic`
  - `request-tracking`, `request-id`
  - `timeout`
  - `all`, `list` - show every available middleware

**Options:**
- `--dry-run` - Preview without creating

**Examples:**

```bash
acton service add middleware list      # See every supported type
acton service add middleware cors
acton service add middleware rate-limit
```

### acton service add version

Add a new API version to the service.

**Syntax:**
```bash
acton service add version <VERSION> [OPTIONS]
```

**Arguments:**
- `<VERSION>` - Version name (e.g., `v2`)

**Options:**
- `--from <FROM>` - Copy routes from an existing version
- `--dry-run` - Preview without creating

**Examples:**

```bash
acton service add version v2
acton service add version v2 --from v1
```

### acton service validate

Validate a service against best practices and score it.

**Syntax:**
```bash
acton service validate [PATH] [OPTIONS]
```

**Arguments:**
- `[PATH]` - Path to the service directory (default: `.`)

**Options:**
- `--check <TYPE>` - Run a specific check
- `--all` - Run all checks
- `--deployment` - Focus on deployment readiness
- `--security` - Focus on security checks
- `--format <FORMAT>` - Output format (default: `text`)
- `-v, --verbose` - Show detailed output
- `-q, --quiet` - Only show errors and score
- `--ci` - CI-friendly output
- `--min-score <SCORE>` - Minimum passing score (default: `8.0`)
- `--strict` - Treat warnings as errors
- `--fix` - Auto-fix issues where possible
- `--report <FILE>` - Write report to file

**Examples:**

```bash
acton service validate
acton service validate --all --verbose
acton service validate --ci --min-score 9.0 --strict
acton service validate --security --report security-report.txt
```

### acton service generate config

Generate a configuration file for the service.

**Syntax:**
```bash
acton service generate config [OPTIONS]
```

**Options:**
- `--output <PATH>` - Output path
- `--examples` - Include examples
- `--dry-run` - Preview without creating

**Examples:**

```bash
acton service generate config --examples
```

### acton service generate proto

Generate a proto file for a gRPC service.

**Syntax:**
```bash
acton service generate proto <SERVICE> [OPTIONS]
```

**Arguments:**
- `<SERVICE>` - Service name

**Options:**
- `--output <PATH>` - Output path
- `--dry-run` - Preview without creating

**Examples:**

```bash
acton service generate proto UserService
```

### acton service dev

Development tools for running and inspecting services.

**Syntax:**
```bash
acton service dev run [--watch] [--port <PORT>]
acton service dev health [--verbose] [--url <URL>]
acton service dev logs [-f, --follow] [--level <LEVEL>] [--filter <PATTERN>]
```

**`dev run` options:**
- `--watch` - Watch for changes and reload
- `--port <PORT>` - Port to listen on

**`dev health` options:**
- `--verbose` - Show detailed output
- `--url <URL>` - Service URL (default: `http://localhost:8080`)

**`dev logs` options:**
- `-f, --follow` - Follow log output
- `--level <LEVEL>` - Filter by log level
- `--filter <PATTERN>` - Filter by pattern

**Examples:**

```bash
acton service dev run --watch --port 3000
acton service dev health --verbose
acton service dev logs --follow --level info
```

## Setup Commands

### acton setup completions

Generate and install shell completions.

**Syntax:**
```bash
acton setup completions [OPTIONS]
```

**Options:**
- `-s, --shell <SHELL>` - Shell to generate completions for (auto-detected from `$SHELL` if omitted). Supported: `bash`, `zsh`, `fish`, `powershell`, `elvish`.
- `--stdout` - Write to stdout instead of installing
- `--show-instructions` - Show installation instructions only

**Examples:**

```bash
acton setup completions                        # Auto-detect and install
acton setup completions --shell zsh
acton setup completions --shell bash --stdout > ~/.local/share/bash-completion/completions/acton
```

### acton setup templates

Initialize and manage user-customizable code-generation templates in your XDG config directory. Templates you don't modify fall back to the embedded defaults.

**Syntax:**
```bash
acton setup templates [OPTIONS]
```

**Options:**
- `--list` - List all available templates
- `--show-path` - Show the templates directory path

**Examples:**

```bash
acton setup templates              # Initialize user templates
acton setup templates --list
acton setup templates --show-path
```

## Global Options

These options work with all commands:

- `-h, --help` - Show help information
- `-V, --version` - Show version information

{% callout type="note" title="No global verbose/quiet flags" %}
`-v/--verbose` and `-q/--quiet` are **not** global. They are specific to `acton service validate` (and `dev health` / `dev logs` have their own flags). Passing them to other commands is an error.
{% /callout %}

**Examples:**

Show help for a command:
```bash
acton service new --help
```

Show CLI version:
```bash
acton --version
```

Verbose validation output:
```bash
acton service validate --verbose
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
acton service add endpoint GET /users/{id} --handler get_user

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
acton service add endpoint GET /payments/{id} --handler get_payment

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

- `SHELL` - Used by `acton setup completions` to auto-detect your shell when `--shell` is omitted
- `NO_COLOR` - Disables colored output

Custom templates are not configured via an environment variable. They live in your XDG config directory:

```bash
acton setup templates --show-path   # Print the templates directory
```

## Getting Help

For detailed help on any command:

```bash
acton --help                      # Top-level help
acton service --help              # Service commands help
acton service new --help          # Specific command help
acton setup --help                # Setup commands help
```

## Next Steps

- See [CLI Overview](/docs/cli-overview) for design philosophy
- Learn [Service Scaffolding](/docs/cli-scaffolding) patterns
- Review the [Quickstart](/docs/quickstart) for complete examples
