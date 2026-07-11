---
title: CLI Overview
nextjs:
  metadata:
    title: CLI Overview
    description: Production-ready CLI tool for scaffolding and managing acton-service backend services
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

{% callout type="warning" title="Experimental - Active Development" %}
The Acton CLI is currently experimental and under active development. Breaking changes should be expected between releases. APIs, command structure, and generated code patterns may change without prior notice.
{% /callout %}

---


The Acton CLI is a powerful command-line tool that scaffolds production-ready backend services built with the acton-service framework.

## What is the Acton CLI?

The `acton` CLI is designed to help you quickly create, manage, and extend backend services with best practices built in. Whether you're building a simple HTTP API or a full-featured service with databases, caching, and event streaming, the CLI generates production-ready code that follows established patterns.

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

**Senior Engineers**: Customizable templates for standardized services
```bash
acton setup templates   # Initialize and edit templates in your config directory
```

**DevOps/SRE**: Deployment validation and manifest generation
```bash
acton service validate --deployment
acton service generate deployment --platform kubernetes
```

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
│   │   ├── middleware              # Show middleware wiring
│   │   ├── graphql                 # Add GraphQL transport
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
└── setup
    ├── completions                 # Generate/install shell completions
    └── templates                   # Manage user-customizable templates
```

## Implementation Status

All commands listed above are implemented and available:

**Service creation and scaffolding:**
- ✅ Interactive and non-interactive service creation
- ✅ Service scaffolding with multiple features
- ✅ Template-based code generation
- ✅ Git integration and automatic formatting

**Adding components:**
- ✅ `acton service add endpoint` - Add HTTP endpoints
- ✅ `acton service add grpc` - Add gRPC services
- ✅ `acton service add worker` - Add background workers
- ✅ `acton service add middleware` - Show middleware wiring for a given type
- ✅ `acton service add graphql` - Add a versioned GraphQL transport
- ✅ `acton service add version` - Add API versions

**Generation, validation, and dev tools:**
- ✅ `acton service generate deployment` - Generate Kubernetes manifests
- ✅ `acton service generate config` - Generate configuration
- ✅ `acton service generate proto` - Generate proto files
- ✅ `acton service validate` - Validate service quality
- ✅ `acton service dev` - Run, health-check, and tail logs

**Setup:**
- ✅ `acton setup completions` - Generate/install shell completions (bash, zsh, fish, powershell, elvish)
- ✅ `acton setup templates` - Initialize, list, and locate user-customizable templates

## GraphQL Transport

Add a versioned GraphQL endpoint to an existing service:

```bash
acton service add graphql --version v1
```

Pass `--cedar` to wire Cedar resolver authorization (requires the `cedar-authz` feature):

```bash
acton service add graphql --version v1 --cedar
```

You can also scaffold GraphQL at creation time with `acton service new my-api --graphql`.

## Setup Commands

Install shell completions for your current shell (auto-detected):

```bash
acton setup completions
```

Initialize user-customizable templates in your XDG config directory, then edit any template to change what the CLI generates:

```bash
acton setup templates            # Initialize templates
acton setup templates --list     # List available templates
acton setup templates --show-path # Print the templates directory
```

Templates you don't modify fall back to the embedded defaults.

## Next Steps

- Learn about [Service Scaffolding](/docs/cli-scaffolding) to create new services
- Explore all [CLI Commands](/docs/cli-commands) available
- Review the [Quickstart](/docs/quickstart) guide for complete examples
