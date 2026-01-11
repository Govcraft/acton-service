# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Documentation

- Reposition as backend framework that scales to microservices
- Update docs site metadata
- **tier-4**: Update example documentation
- **tier-3**: Update feature documentation
- **tier-2**: Update entry point documentation
- **tier-1**: Update Hero component messaging
- **tier-1**: Update docs homepage positioning
- **tier-1**: Update lib.rs crate documentation
- **tier-1**: Update acton-service Cargo.toml description
- **tier-1**: Reposition README as backend framework
- Sync all version files to 0.9.0
- Update version to 0.9.0

### Features

- **session**: Add HTTP session management for HTMX/SSR applications
## [acton-service-v0.9.0] - 2026-01-11

### Documentation

- **turso**: Add Turso/libsql database documentation
- **websocket**: Add WebSocket feature documentation

### Features

- **websocket**: Add feature-gated WebSocket support with room management
- Add git-cliff for automated changelog generation
## [acton-service-v0.8.0] - 2026-01-11

### Bug Fixes

- **observability**: Coordinate tracing init via shared Once guard
- **examples**: Correct ping-pong required-features
- **examples**: Correct ping-pong required-features and doctest

### Documentation

- **reactive-architecture**: Add Event Broker section
- Add migration guide for v0.7 to v0.8
- Add agent architecture notes to pool documentation
- Update quickstart and configuration with agent spawning
- Add TypeID Request IDs documentation
- Add BackgroundWorker guide
- Add Reactive Architecture guide
- Update documentation for TypeID-based request IDs
- Add web app integration guide for HTMX and session-based auth
- Update sponsor section
- **readme**: Add GitHub Sponsors link

### Features

- **turso**: Add Turso/libsql database support as feature-gated capability
- **database**: Unify env var and fix graceful shutdown
- **examples**: Add database example with Docker and migrations
- **versioning**: Make VersionedApiBuilder generic over custom config type
- **prelude**: Re-export common framework dependencies
- **examples**: Add BackgroundWorker example
- **agents**: Make agent-based pool management the default architecture
- **agents**: Add BackgroundWorker for managed task execution
- **agents**: Add JwtRevocationService with write-behind Redis persistence
- **state**: Add broker support for event-driven architecture
- **agents**: Add HealthMonitorAgent and reactive health updates
- **builder**: Integrate acton-reactive runtime with ServiceBuilder
- **agents**: Add acton-reactive pool agents for database, Redis, and NATS
- **ids**: Integrate mti crate for type-safe request identifiers

### Miscellaneous

- **deps**: Use published acton-reactive 7.0.0
- **deps**: Remove unused dependencies and fix example compilation
- Remove repo-specific FUNDING.yml (inherited from org)
- Add GitHub Sponsors funding configuration

### Refactoring

- Update to acton-reactive 0.7.0 with Actor naming
- **agents**: Simplify architecture to hide internal implementation
- **agents**: Make acton-reactive core dependency and internalize agents

### Tests

- **turso**: Add integration tests for local database and TursoDbAgent
## [acton-service-v0.7.0] - 2025-11-18

### Documentation

- **readme**: Add custom config extension documentation
- Add custom config extension documentation

### Features

- Add generic config extension support

### Miscellaneous

- **docs**: Bump version to 0.7.0
- Add GitHub release notes configuration
- Remove docs folder
- **docs**: Update version to 0.6.0 and add CLI experimental warning
## [acton-service-v0.6.0] - 2025-11-17

### Bug Fixes

- **docs**: Correct broken example links in README
- **docs**: Correct example file paths in tutorial
- **docs**: Use valid callout type in tutorial page
- **docs**: Correct Markdoc function call syntax in link tags
- **docs**: Simplify link tag and use githubUrl function
- **docs**: Use proper component for link tag instead of inline function
- **docs**: Use custom link tag for variable-based URLs
- **docs**: Remove backticks from link text to fix markdown rendering
- **docs**: Improve Markdoc variable interpolation in links
- **docs**: Use hardcoded GitHub URLs instead of Markdoc variables for example links
- **docs**: Add basePath to internal links in Markdoc transformer
- Correct file paths and documentation after examples reorganization
- **docs**: Resolve TypeScript module import error in markdoc config
- **docs**: Remove unsupported claims and irrelevant comparisons from comparison page
- **docs**: Enforce VersionedApiBuilder in all code examples
- **docs**: Remove non-existent middleware API, document automatic JWT configuration
- **docs**: Correct example file paths in api-versioning to match actual repository structure
- **docs**: Correct resilience API parameter types and method names
- **docs**: Replace non-existent API with config-based rate limiting approach
- **docs**: Remove non-existent .with_middleware() API, document automatic middleware
- **docs**: Correct version numbers and database access method in troubleshooting
- **docs**: Correct acton-service version in feature-flags from 0.3 to 0.2 (18 instances)
- **docs**: Add missing .await calls and fix method names in events page
- **docs**: Correct default HTTP port from 3000 to 8080 in cli-scaffolding
- **docs**: Correct Kubernetes health probe paths in faq
- **docs**: Correct database access methods from database() to db() with proper async/Option handling
- **docs**: Correct health check JSON response structures (5 instances)
- **docs**: Add missing return type annotations to main functions in comparison
- **docs**: Correct health endpoint paths in examples
- **docs**: Correct acton-service version in quickstart from 0.3 to 0.2
- **docs**: Make Fence language parameter optional with rust default
- **middleware**: Skip JWT and Cedar auth for health/ready endpoints
- **observability**: Respect RUST_LOG environment variable
- **service**: Apply middleware stack in ServiceBuilder
- **cedar**: Correct middleware execution order
- **cedar**: Resolve nested runtime error in auto-middleware
- **cedar**: Update path parameters to Axum 0.8 syntax
- **cedar**: Remove unnecessary cast in Redis cache

### Documentation

- Update README to reference online documentation
- **tutorial**: Add comprehensive production API tutorial
- **examples**: Update documentation to reflect reorganized examples structure
- Organize examples directory by feature category
- Replace hardcoded versions with Markdoc variables
- Add service discovery documentation page
- Centralize GitHub repository URL using DRY principle
- Add documentation site link to README (#2)
- Remove subjective language and unsupported claims from documentation
- Add documentation site link to README and repo description
- **high-priority**: Add Redis vs Governor decision guide and lazy_init explanation
- **critical**: Add missing content for JWT, database, and API versioning
- Add glossary, concepts page, and navigation headers to address curse of knowledge issues
- **jwt**: Clarify JWT token revocation is fully implemented
- **cedar**: Add Cedar authorization feature to documentation and clarify hot-reload status
- **cedar**: Improve Cedar example documentation and simplify test script
- **cedar**: Improve README with auto-setup and verified test commands
- **cedar**: Add comprehensive Cedar authorization example

### Features

- **docs**: Add tutorial to navigation menu
- **docs**: Implement proper Markdoc variable interpolation in link nodes
- **docs**: Add GitHub Pages deployment workflow (#1)
- **docs**: Add GitHub Pages deployment workflow
- **docs**: Add version display to logo and improve formatting
- **versioning**: Add automatic logging and metrics for deprecated API usage
- **docs**: Add Next.js documentation website with acton-service branding
- **cedar**: Add builder pattern and fix permission-based authorization
- **cedar**: Add customizable path normalizer with builder pattern
- **cedar**: Merge Cedar authorization implementation
- **framework**: Auto-apply JWT and Cedar middleware in ServiceBuilder
- **cedar**: Make example self-contained with auto-setup
- **cedar**: Export Cedar types in prelude module
- **cedar**: Add gRPC Tower Layer for Cedar authorization
- **cedar**: Implement HTTP authorization middleware
- **cedar**: Add CedarConfig to configuration system
- **cedar**: Add Cedar authorization feature flag and dependency

### Miscellaneous

- **docs**: Remove duplicate markdown files already in acton-docs site

### Refactoring

- **cedar**: Make middleware generic and framework-grade
## [acton-service-v0.5.2] - 2025-11-11

### Features

- **prelude**: Re-export Response type
## [acton-service-v0.5.1] - 2025-11-11

### Documentation

- **tutorial**: Add custom state and headers sections

### Features

- **prelude**: Re-export HeaderMap and HeaderValue
## [acton-service-v0.5.0] - 2025-11-11

### Bug Fixes

- **security**: Change CORS default from permissive to restrictive
## [acton-service-v0.4.0] - 2025-11-11

### Bug Fixes

- **cli**: Remove unexpected cfg from build.rs template
- **cli**: Ensure generated services compile and run
- **cli**: Update service templates to reflect actual implementation
- **config**: Prevent XDG directory creation and fix config template
- **cli**: Correct import generation for generated services
- Correct GitHub organization capitalization to Govcraft
- **build**: Remove unreachable code in compile_protos_with_descriptor
- **service**: Use loaded config in ServiceBuilder's AppState

### Documentation

- Add comprehensive onboarding documentation for improved developer experience
- **readme**: Reposition value propositions beyond API versioning
- **cli**: Enhance gRPC port configuration documentation and messaging
- Update documentation to reflect implemented features
- Add comprehensive README and MIT LICENSE
- **service**: Update ServiceBuilder docs to reflect automatic initialization

### Features

- **framework**: Add production-ready error messages and pool monitoring
- **middleware**: Implement JWT revocation with Redis backend
- **taskfile**: Add release-service task and rename release to release-cli
- **taskfile**: Add release task for versioning CLI
- **cli**: Implement user-customizable template system
- **build**: Add Taskfile for CLI build and installation
- **grpc**: Implement single-port HTTP + gRPC multiplexing
- **middleware**: Implement production-ready metrics middleware with OpenTelemetry
- **middleware**: Implement production-ready resilience patterns
- **observability**: Implement full OpenTelemetry OTLP integration
- **cli**: Implement shell completions command
- **cli**: Implement validate command with comprehensive service validation
- **cli**: Implement grpc command with comprehensive gRPC setup guide
- **cli**: Implement middleware command with comprehensive middleware guides
- **cli**: Implement add version command with comprehensive guidance
- **cli**: Implement dev run command
- **cli**: Implement dev logs command with helpful guidance
- **cli**: Implement dev health command
- **cli**: Implement generate proto command
- **cli**: Implement generate config command
- **cli**: Implement add endpoint, add worker, and generate deployment commands
- **cli**: Implement acton CLI with service scaffolding
- **grpc**: Add build utilities for proto compilation
- **examples**: Add event-driven microservice example
- **examples**: Add ping-pong HTTP to gRPC example
- **grpc**: Implement health check and reflection services (Phase 3)
- **grpc**: Implement Phase 2 middleware parity with HTTP
- **grpc**: Add basic gRPC infrastructure to acton-service framework
- **health**: Integrate proper health.rs handlers with dependency checking
- **service**: Add automatic config loading and tracing initialization
- **services**: Add production API gateway
- **services**: Add production backend service
- **examples**: Add API gateway with gRPC client
- **examples**: Add backend service with dual-protocol support
- **examples**: Add simple API examples
- **acton-service**: Add observability and API features
- **acton-service**: Add server runtime and state management
- **acton-service**: Add infrastructure integrations
- **acton-service**: Add middleware layer
- **acton-service**: Add error handling and response types
- **acton-service**: Add configuration module
- **acton-service**: Add library core and manifest
- Add gRPC protocol buffer definitions

### Miscellaneous

- **deps**: Restore acton-service version to 0.3.0
- **deps**: Use last published acton-service version (0.2.0)
- **deps**: Add version specification for acton-service dependency
- Add crates.io publication metadata
- **services**: Remove production service scaffolds
- Add workspace configuration and examples overview
- Add workspace dependency lock file
- Add project configuration files

### Performance

- **deps**: Optimize dependency features to reduce compile time and binary size

### Refactoring

- **cli**: Remove unused template generation functions

### Tests

- **grpc**: Add single-port example to verify HTTP + gRPC multiplexing
- **observability**: Add comprehensive tests and working example
<!-- generated by git-cliff -->
