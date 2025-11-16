---
title: CLI Overview
nextjs:
  metadata:
    title: CLI Overview
    description: Production-ready CLI tool for scaffolding and managing acton-service microservices
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


The Acton CLI is a powerful command-line tool that scaffolds production-ready microservices built with the acton-service framework.

## What is the Acton CLI?

The `acton` CLI is designed to help you quickly create, manage, and extend microservices with best practices built in. Whether you're building a simple HTTP API or a full-featured microservice with databases, caching, and event streaming, the CLI generates production-ready code that follows established patterns.

### Key Capabilities

- **Service Scaffolding**: Create new services with configurable features
- **Code Generation**: Generate boilerplate code with proper type safety
- **Endpoint Management**: Add HTTP endpoints to existing services
- **Worker Management**: Add background workers for event processing
- **Deployment Generation**: Create Kubernetes manifests and Docker configurations
- **Interactive & Non-Interactive Modes**: Choose your preferred workflow

## Installation

Install the CLI using Cargo:

```bash
cargo install acton-cli
```

Or build from source:

```bash
cargo build --release -p acton-cli
```

The binary will be available at `target/release/acton`.

## Quick Start

Create a minimal HTTP service with defaults:

```bash
acton service new my-api --yes
cd my-api
cargo run
```

Your service will start with:
- Health check endpoint at `/health`
- Configuration management
- Structured logging
- Graceful shutdown handling

## Design Philosophy

The Acton CLI follows these core principles:

### Progressive Disclosure
Start simple with `--yes` for defaults, add complexity only when needed. The CLI grows with your requirements.

### Safe by Default
Prevent mistakes through validation and type-safe code generation. All generated code compiles and follows Rust best practices.

### Educational
Generated code includes comments and documentation that teach framework patterns. Learn by example.

### Production-Ready
Every generated service meets operational standards with health checks, configuration management, and observability hooks.

### Discoverable
Self-documenting with excellent help text. Run any command with `--help` to see all available options.

## User Personas

The CLI supports different experience levels:

**Beginners**: Interactive wizard with prompts and educational output
```bash
acton service new my-service
# Follow the prompts
```

**Intermediate**: Fast scaffolding with command-line flags
```bash
acton service new my-service --http --database postgres --cache redis
```

**Senior Engineers**: Organization templates for standardized services (coming soon)

**DevOps/SRE**: Deployment validation and manifest generation (partially available)

## Command Structure

The CLI is organized into logical command groups:

```text
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

## Implementation Status

**Fully Implemented:**
- ✅ Interactive and non-interactive service creation
- ✅ Service scaffolding with multiple features
- ✅ Template-based code generation
- ✅ Git integration and automatic formatting
- ✅ `acton service add endpoint` - Add HTTP endpoints
- ✅ `acton service add worker` - Add background workers
- ✅ `acton service generate deployment` - Generate Kubernetes manifests

**Planned:**
- 🚧 `acton service add grpc` - Add gRPC services
- 🚧 `acton service add middleware` - Add custom middleware
- 🚧 `acton service add version` - Add API versions
- 🚧 `acton service validate` - Validate service quality
- 🚧 `acton service generate config` - Generate configuration
- 🚧 `acton service generate proto` - Generate proto files
- 🚧 `acton service dev` - Development tools

## Next Steps

- Learn about [Service Scaffolding](/docs/cli-scaffolding) to create new services
- Explore all [CLI Commands](/docs/cli-commands) available
- Review the [Getting Started](/docs/getting-started) guide for complete examples
