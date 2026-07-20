---
title: Feature Flags
nextjs:
  metadata:
    title: Feature Flags
    description: Choose the right feature flags to keep compile times fast and binaries small
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


acton-service uses feature flags to keep compile times fast and binary sizes small. Enable only what you need.

## Quick Decision Tree

```text
┌─────────────────────────────────────────┐
│ What are you building?                  │
└─────────────────────────────────────────┘
                    │
        ┌───────────┴───────────┐
        │                       │
   REST API                gRPC Service
        │                       │
        ▼                       ▼
   ["http",              ["grpc",
    "observability"]      "observability"]
        │                       │
        ├───────────────────────┤
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need real-time communication?    │
└─────────────────────────────────────────┘
        │
        ├─── Yes ──▶ Add "websocket"
        └─── No  ──▶ Skip
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need a database?                 │
└─────────────────────────────────────────┘
        │
        ├─── Yes ──▶ Add "database"
        └─── No  ──▶ Skip
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need caching?                    │
└─────────────────────────────────────────┘
        │
        ├─── Yes ──▶ Add "cache"
        └─── No  ──▶ Skip
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need events/messaging?           │
└─────────────────────────────────────────┘
        │
        ├─── Yes ──▶ Add "events"
        └─── No  ──▶ Skip
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need authentication?             │
└─────────────────────────────────────────┘
        │
        ├─── Password + tokens ──▶ Add "auth"
        ├─── Social login ───────▶ Add "auth", "oauth", "cache"
        └─── API keys only ──────▶ Add "auth", "cache"
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need token authentication?       │
└─────────────────────────────────────────┘
        │
        ├─── PASETO (default) ──▶ No flag needed
        └─── JWT (legacy) ──────▶ Add "jwt"
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need HTTP sessions (HTMX/SSR)?   │
└─────────────────────────────────────────┘
        │
        ├─── Dev ──▶ Add "session-memory"
        └─── Prod ─▶ Add "session-redis"
        │
        ▼
┌─────────────────────────────────────────┐
│ Do you need advanced features?          │
└─────────────────────────────────────────┘
        │
        ├─── Fine-grained authorization ──▶ Add "cedar-authz"
        ├─── Brute force protection ─────▶ Add "login-lockout"
        ├─── Rate limiting ───────────────▶ Add "governor"
        ├─── Resilience ──────────────────▶ Add "resilience"
        ├─── Metrics (OTLP push) ─────────▶ Add "otel-metrics"
        ├─── Metrics (Prometheus pull) ───▶ Add "prometheus-metrics"
        └─── OpenAPI ─────────────────────▶ Add "openapi"
```

---

## Core Features

### `http`
**Included in default features**

Enables HTTP REST API support via Axum.

**When to use**: Building REST APIs (most common use case)

**Dependencies**: Axum, Tower

```toml
{% $dep.httpOnly %}
```

### `observability`
**Included in default features**

Enables structured logging and OpenTelemetry tracing.

**When to use**: Always (highly recommended for production)

**Dependencies**: tracing, tracing-subscriber, OpenTelemetry

```toml
{% $dep.observability %}
```

---

## Protocol Features

### `grpc`

Enables gRPC support via Tonic. Can run on the same port as HTTP with automatic protocol detection.

**When to use**: Building gRPC services or dual HTTP+gRPC services

**Dependencies**: tonic, prost

```toml
{% $dep.grpcOnly %}
```

### `websocket`

Enables WebSocket support for real-time bidirectional communication. Uses Axum's built-in WebSocket support.

**When to use**: Building real-time applications (chat, live updates, gaming)

**Dependencies**: None (uses axum's ws feature)

**Provides**:
- WebSocket upgrade handlers
- Connection management with unique IDs
- Broadcaster for message distribution
- Actor-based room management

```toml
{% $dep.websocketOnly %}
```

See the [WebSocket Guide](/docs/websocket) for detailed usage.

### `graphql`

Enables a GraphQL transport (async-graphql + Axum). Mounts as a third sibling transport alongside `http` and `grpc`, under the same versioned router and middleware stack.

**When to use**: Exposing a versioned GraphQL endpoint (`/api/v1/graphql`)

**Dependencies**: async-graphql, async-graphql-axum, bytes

**Provides**:
- `VersionedGraphQLBuilder` for registering schemas per API version
- `GraphQLContextExt` for reading authenticated claims inside resolvers

```toml
acton-service = { version = "{% version() %}", features = ["graphql"] }
```

### `graphql-cedar`

GraphQL with Cedar policy authorization callable from resolvers.

**When to use**: Enforcing fine-grained Cedar policies at the resolver level

**Enables**: `graphql`, `cedar-authz`

**Provides**:
- `CedarResolverCheck` for policy checks inside GraphQL resolvers

```toml
acton-service = { version = "{% version() %}", features = ["graphql-cedar"] }
```

---

## Data Layer Features

{% callout type="warning" title="Pick exactly one primary backend" %}
`database`, `turso`, and `surrealdb` are pairwise mutually exclusive and fail the build if combined. See [Database Backend Exclusivity](#database-backend-exclusivity).
{% /callout %}

### `database`

PostgreSQL connection pooling via SQLx with automatic health checks and retry logic.

**When to use**: Your service needs a SQL database

**Dependencies**: sqlx with postgres feature

**Provides**:
- Automatic connection pool management
- Health checks for database connections
- Retry logic on connection failures

```toml
{% $dep.databaseOnly %}
```

### `turso`

Turso/libsql database support for edge-friendly SQLite with cloud sync capabilities.

**When to use**: Building edge applications, mobile backends, or need SQLite with cloud durability

**Dependencies**: libsql

**Provides**:
- Local, Remote, and EmbeddedReplica connection modes
- Automatic retry with exponential backoff
- Optional encryption (AES-256-CBC)
- Background sync for embedded replicas

```toml
{% $dep.tursoOnly %}
```

See the [Turso Guide](/docs/turso) for detailed usage.

### `surrealdb`

SurrealDB multi-model database support (document, graph, and relational in one store).

**When to use**: Your data model needs documents or graph relations rather than plain SQL

**Dependencies**: surrealdb

**Provides**:
- SurrealDB connection management
- `SurrealDbHealth` pool health reporting
- Shared `DatabaseError` / `DatabaseOperation` error taxonomy

```toml
{% $dep.surrealdbOnly %}
```

**Note**: Mutually exclusive with `database` and `turso`.

### `cache`

Redis connection pooling with support for token revocation and distributed rate limiting.

**When to use**: Need caching, session storage, or rate limiting

**Dependencies**: redis, deadpool-redis

**Provides**:
- Redis connection pool
- Token revocation support (PASETO and JWT)
- Distributed rate limiting

```toml
{% $dep.cacheOnly %}
```

### `events`

NATS JetStream client for event-driven architecture and pub/sub messaging.

**When to use**: Building event-driven microservices

**Dependencies**: async-nats

**Provides**:
- NATS connection management
- JetStream support
- Pub/sub messaging

```toml
{% $dep.eventsOnly %}
```

### `clickhouse`

ClickHouse analytical database client. Composable with any primary backend (or none) — it is not subject to the database exclusivity rule.

**When to use**: Writing high-volume analytical or audit data alongside your primary store

**Dependencies**: clickhouse

**Provides**:
- `AnalyticsWriter` for batched analytical writes
- `ClickHouseHealth` pool health reporting

```toml
{% $dep.clickhouseOnly %}
```

### `repository`

Generic repository traits for database CRUD abstractions. No extra dependencies — trait definitions only.

**When to use**: You want a backend-agnostic CRUD abstraction over your storage layer

**Dependencies**: None

**Provides**:
- `Repository` and `SoftDeleteRepository` traits
- `RelationLoader`, `OrderDirection`, `RepositoryError` taxonomy

```toml
acton-service = { version = "{% version() %}", features = ["repository"] }
```

### `handlers`

Pre-built REST CRUD handler traits built on top of `repository`.

**When to use**: You want conventional list/get/create/update/delete REST handlers without writing them by hand

**Enables**: `repository`

**Provides**:
- `CollectionHandler` and `SoftDeleteHandler` traits
- `ListQuery`, `ListResponse`, `ItemResponse`, `PaginationMeta`
- `ApiError` taxonomy and `DEFAULT_PER_PAGE` / `MAX_PER_PAGE` limits

```toml
acton-service = { version = "{% version() %}", features = ["handlers"] }
```

### `pagination`

Core pagination primitives (offset, cursor, filtering, sorting, search).

**When to use**: Any list endpoint that needs paging

**Dependencies**: paginator-rs

**Provides**:
- `Paginator`, `PaginationParams`, `PaginatorResponse`
- Cursor pagination (`Cursor`, `CursorDirection`)
- Filter, sort, and search builders

### `pagination-axum`

Axum extractors and responses for pagination. Enables `pagination`.

**Dependencies**: paginator-axum

**Provides**: `PaginationQuery` extractor, `PaginatedJson` response, `create_link_header()`

### `pagination-sqlx`

SQLx query integration for pagination. Enables `pagination`.

**Dependencies**: paginator-sqlx

**Provides**: `PaginateQuery`, `PaginatedQuery`, `QueryBuilderExt`, `validate_field_name()`

### `pagination-full`

Meta-feature enabling `pagination`, `pagination-axum`, and `pagination-sqlx` together.

```toml
acton-service = { version = "{% version() %}", features = ["pagination-full"] }
```

---

## Session Features

### `session`

Base session support. This feature is automatically included when using `session-memory` or `session-redis`.

**When to use**: Building HTMX or server-rendered applications with session state

**Dependencies**: tower-sessions, time

### `session-memory`

In-memory session storage for development and single-instance deployments.

**When to use**: Local development, testing, or single-server applications

**Dependencies**: tower-sessions-memory-store

**Provides**:
- In-memory session store
- Cookie-based session IDs
- Flash messages
- CSRF protection
- TypedSession for type-safe session data

```toml
acton-service = { version = "{% version() %}", features = ["session-memory"] }
```

### `session-redis`

Redis-backed session storage for production multi-instance deployments.

**When to use**: Production deployments with multiple application instances

**Dependencies**: tower-sessions-redis-store (fred)

**Provides**:
- Distributed session storage
- Session persistence across restarts
- All features from `session-memory`

```toml
acton-service = { version = "{% version() %}", features = ["session-redis"] }
```

**Note**: Uses `fred` Redis client internally (separate from `cache` feature's `deadpool-redis`).

See the [Session Management Guide](/docs/session) for detailed usage.

---

## HTMX Features

### `htmx`

HTMX request extractors and response helpers for hypermedia-driven applications.

**When to use**: Building interactive web applications with HTMX

**Dependencies**: axum-htmx

**Provides**:
- Request extractors: `HxRequest`, `HxTarget`, `HxTrigger`, `HxPrompt`, `HxCurrentUrl`
- Response headers: `HxRedirect`, `HxRefresh`, `HxReswap`, `HxRetarget`, `HxLocation`
- Custom responders: `HtmlFragment`, `HxTriggerEvents`, `OutOfBandSwap`
- Auto-Vary middleware for correct caching
- Helper functions: `is_htmx_request()`, `fragment_or_full()`

```toml
acton-service = { version = "{% version() %}", features = ["htmx"] }
```

See the [HTMX Integration Guide](/docs/htmx) for detailed usage.

### `askama`

Compile-time checked HTML templates using Askama with Jinja2-like syntax.

**When to use**: Server-side rendering with type-safe templates

**Dependencies**: askama, askama_web

**Provides**:
- `TemplateContext` for common page data (auth, flash, CSRF, path)
- `HtmlTemplate` responder with HTMX header support
- Template helper functions: `truncate()`, `pluralize()`, `classes()`
- Compile-time template validation (errors at build time, not runtime)

```toml
acton-service = { version = "{% version() %}", features = ["askama"] }
```

See the [Askama Templates Guide](/docs/askama) for detailed usage.

### `sse`

Server-Sent Events for real-time server-to-client updates.

**When to use**: Real-time notifications, live data updates, progress indicators

**Dependencies**: None (uses axum's built-in SSE)

**Provides**:
- `SseBroadcaster` for managing multiple client connections
- `BroadcastMessage` for event construction
- HTMX helpers: `htmx_event()`, `htmx_trigger()`, `htmx_oob_event()`
- Connection management with `ConnectionId` and `SseConnection`
- Channel-based broadcasting for user-specific events

```toml
acton-service = { version = "{% version() %}", features = ["sse"] }
```

See the [Server-Sent Events Guide](/docs/sse) for detailed usage.

### `htmx-full`

Meta-feature enabling all HTMX-related features together.

**When to use**: Building complete HTMX applications with templates, SSE, and sessions

**Enables**: `htmx`, `askama`, `sse`, `session-memory`

```toml
acton-service = { version = "{% version() %}", features = ["htmx-full"] }
```

**Note**: For production, replace `htmx-full` with explicit features and use `session-redis` instead of `session-memory`:

```toml
acton-service = { version = "{% version() %}", features = [
    "htmx", "askama", "sse", "session-redis"
] }
```

---

## Authentication Features

### `auth`

Core authentication module with password hashing (Argon2id) and token generation (PASETO).

**When to use**: Building user authentication with password login and/or stateless tokens

**Dependencies**: argon2, rand, blake3, base64

**Note**: PASETO validation is always available — `rusty_paseto` is a core (non-optional) dependency, so token verification works without this flag. Enable `auth` for password hashing, token *generation*, API keys, and key rotation.

**Provides**:
- Password hashing with OWASP-recommended Argon2id
- PASETO V4 token generation (local and public modes)
- Refresh token storage (Redis, PostgreSQL, Turso)
- API key generation and validation
- ClaimsBuilder for ergonomic token creation

```toml
acton-service = { version = "{% version() %}", features = ["auth"] }
```

See the [Auth Overview](/docs/auth) for choosing the right auth features.

### `oauth`

OAuth/OIDC provider integration for social login and enterprise SSO. Requires `auth` feature.

**When to use**: Adding "Sign in with Google/GitHub" or enterprise OIDC SSO

**Dependencies**: oauth2, openidconnect, base64

**Provides**:
- GoogleProvider for Google OAuth
- GitHubProvider for GitHub OAuth
- CustomOidcProvider for any OIDC-compliant provider
- State management with CSRF protection
- Normalized user info across providers

```toml
acton-service = { version = "{% version() %}", features = ["auth", "oauth", "cache"] }
```

**Note**: Requires `cache` for state management in production.

See the [OAuth/OIDC Guide](/docs/oauth) for detailed usage.

### `auth-full`

Meta-feature that enables all authentication features.

**When to use**: Need complete auth support including password hashing, tokens, OAuth, JWT, and storage

**Enables**: `auth`, `oauth`, `jwt`, `cache`, `database`, `login-lockout`, `accounts`

```toml
acton-service = { version = "{% version() %}", features = ["auth-full"] }
```

**⚠️ Warning**: `auth-full` includes many dependencies. For production, only enable what you need. Because it pulls in `database` (PostgreSQL), `auth-full` is mutually exclusive with `turso` and `surrealdb` — see [Database Backend Exclusivity](#database-backend-exclusivity).

---

## Middleware & Resilience Features

### `cedar-authz`

AWS Cedar policy-based authorization for fine-grained access control.

**When to use**: Need fine-grained access control with declarative policies

**Dependencies**: cedar-policy

**Provides**:
- Declarative Cedar policy files for resource-based permissions
- Role-based and attribute-based access control (RBAC/ABAC)
- Manual policy reload endpoint (automatic hot-reload in progress)
- Optional Redis caching for sub-5ms policy decisions
- HTTP and gRPC support with customizable path normalization
- Layered security with JWT authentication

```toml
{% $dep.cedarAuthz %}
```

**Note**: Works best with `cache` feature for policy decision caching.

### `resilience`

Circuit breaker, retry, and bulkhead patterns for production services.

**When to use**: Production services calling external dependencies

**Dependencies**: tower-resilience

**Provides**:
- Circuit breaker (prevent cascading failures)
- Exponential backoff retry
- Bulkhead (concurrency limiting)

```toml
{% $dep.resilience %}
```

### `governor`

Advanced rate limiting with per-user limits via token claims.

**When to use**: Need sophisticated rate limiting beyond basic throttling

**Dependencies**: governor

**Provides**:
- Per-second/minute/hour rate limits
- Per-user rate limiting via token claims (PASETO or JWT)
- In-memory rate limiting

```toml
{% $dep.governor %}
```

### `otel-metrics`

HTTP metrics collection via OpenTelemetry, pushed to a collector over OTLP.

**When to use**: Need detailed request metrics and you run an OpenTelemetry collector

**Dependencies**: opentelemetry-instrumentation-tower, opentelemetry-otlp

**Provides**:
- Request count, duration, size metrics
- Active request tracking
- HTTP status code distribution
- OTLP push export (15s interval) to the configured `[otlp]` endpoint

```toml
{% $dep.otelMetrics %}
```

### `prometheus-metrics`

The same OpenTelemetry HTTP metrics, exposed for a direct Prometheus scrape — no collector required.

**When to use**: Operators point Prometheus straight at the service, or running a collector is not worth it

**Dependencies**: opentelemetry-instrumentation-tower, opentelemetry-prometheus

**Provides**:
- `GET /metrics` in Prometheus text-exposition format, mounted alongside `/health` and `/ready`
- The same request metrics as `otel-metrics`, plus any application metrics registered through `get_meter()`
- Independent of `otel-metrics`: enable either or both; with both, one meter provider feeds both exporters

The endpoint is unauthenticated like `/health` — it exposes route names and traffic statistics, so restrict access at the network layer if that matters in your deployment. `/metrics` is excluded from audit-logged routes by default.

```toml
{% $dep.prometheusMetrics %}
```

### `tls`

Rustls-based HTTPS listener for terminating TLS directly in the service, plus a
`client_tls` module for presenting a client certificate when this service
*calls* another mutual-TLS service.

**When to use**: Serving HTTPS without a TLS-terminating proxy in front, and/or
calling peers that require mutual TLS

**Dependencies**: tokio-rustls, rustls-pki-types, zeroize, webpki-roots

**Provides**:
- TLS-enabled server listener
- Certificate and private key loading
- `ClientIdentityConfig` plus a `client_tls` module (`load_rustls_client_config`,
  `load_reqwest_identity`, `reqwest_client_builder`, `tonic_client_tls_config`
  under `grpc`) for outbound mutual TLS, including a `ClientIdentitySource` for
  clients that need to rotate their certificate at runtime

```toml
acton-service = { version = "{% version() %}", features = ["tls"] }
```

**Note**: TLS still requires exactly one crypto provider — see [Cryptographic Provider](#cryptographic-provider).

### `journald`

Native systemd journal integration with structured fields. Writes tracing events directly to journald with native journal fields instead of embedding JSON strings.

**When to use**: Deploying on Linux with systemd, want native `journalctl` field filtering

**Dependencies**: tracing-journald

**Provides**:
- Native structured journal fields (MESSAGE, PRIORITY, CODE_FILE, CODE_LINE)
- Custom span/event fields as journal fields
- Configurable syslog identifier for `journalctl -t` filtering
- Optional suppression of JSON stdout to prevent double logging

```toml
{% $dep.journaldOnly %}
```

### `jwt`

JWT token authentication support. PASETO is the default token format and requires no feature flag.

**When to use**: Need JWT tokens for compatibility with existing systems, third-party services, or legacy requirements

**Dependencies**: jsonwebtoken

**Provides**:
- JWT token validation (RS256, RS384, RS512, ES256, ES384, HS256, HS384, HS512)
- Integration with existing JWT infrastructure
- Same Claims API as PASETO

```toml
{% $dep.jwtOnly %}
```

{% callout type="note" title="PASETO is Default" %}
Token authentication via PASETO requires no feature flag and is the recommended default. Only enable `jwt` if you specifically need JWT compatibility. See [Token Authentication](/docs/token-auth) for details.
{% /callout %}

### `login-lockout`

Progressive delay and account lockout for brute force protection on login endpoints. Depends on `auth` + `cache`.

**When to use**: Protecting login endpoints from credential stuffing and brute force attacks

**Dependencies**: None (uses existing auth + cache infrastructure)

**Provides**:
- Per-identity failed attempt tracking in Redis
- Configurable progressive delays (exponential backoff)
- Automatic account lockout after threshold
- Notification hooks for lockout lifecycle events
- Optional auto-enforcement middleware
- Audit integration (when `audit` feature is active)

```toml
{% $dep.loginLockout %}
```

See the [Login Lockout Guide](/docs/login-lockout) for detailed usage.

### `accounts`

Account lifecycle management (NIST AC-2): create, update, suspend, and delete user accounts. Enables `auth`.

**When to use**: You need managed user accounts with a status lifecycle rather than raw credentials

**Dependencies**: None (builds on `auth`)

**Provides**:
- `AccountService`, `Account`, `AccountId`, `AccountStatus`
- `AccountStorage` trait, `CreateAccount` / `UpdateAccount` inputs
- `AccountEvent` / `AccountNotification` lifecycle hooks
- `AuditAccountNotification` when `audit` is also enabled

```toml
acton-service = { version = "{% version() %}", features = ["accounts"] }
```

### `account-handlers`

Pre-built REST handlers for account management. Enables `accounts`.

**When to use**: You want ready-made account endpoints instead of writing them

**Provides**: `account_routes()` — a mountable Axum router for account CRUD

```toml
acton-service = { version = "{% version() %}", features = ["account-handlers"] }
```

### `audit`

Tamper-evident audit logging with BLAKE3 hash chaining.

**When to use**: Compliance regimes that require a verifiable audit trail

**Dependencies**: blake3

**Provides**:
- `AuditLogger`, `AuditEvent`, `AuditEventKind`, `AuditSeverity`, `AuditSource`
- `AuditStorage` trait and `AuditRoute` for per-route auditing
- Alerting via `AuditAlertHook` / `AlertConfig`
- Automatic integration with `login-lockout` and `accounts` when those are enabled

`audit` is part of the `full` feature set. When the feature is compiled in, audit logging is **enabled by default** (set `[audit] enabled = false` to opt out), and the audit agent requires a multi-threaded tokio runtime — on a current-thread runtime (such as a default `#[tokio::test]`), `build()` records a startup error and `serve()` refuses to start rather than running without the configured audit trail. The same requirement applies to every actor-backed subsystem — background workers, actor extensions, Cedar authorization, Redis sessions, and key rotation — each of which records its own subsystem-named startup error on a current-thread runtime instead of panicking inside tokio.

```toml
{% $dep.auditOnly %}
```

---

## Documentation Features

### `openapi`

OpenAPI/Swagger documentation generation with multiple UI options.

**When to use**: Need API documentation UI

**Dependencies**: utoipa, utoipa-swagger-ui

**Provides**:
- Swagger UI
- ReDoc UI
- RapiDoc UI
- Auto-generated OpenAPI specs

```toml
{% $dep.openapiOnly %}
```

---

## Common Configurations

### Minimal REST API
**Use case**: Simple REST API, no database

```toml
[dependencies]
{% $dep.http %}
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~10MB (stripped)
**Compile time**: ~30s (clean build)

### REST API with Database
**Use case**: Standard CRUD API with PostgreSQL

```toml
[dependencies]
{% $dep.database %}
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~12MB (stripped)
**Compile time**: ~45s (clean build)

### Full-Featured REST API
**Use case**: Production API with all bells and whistles

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = [
    "http",
    "observability",
    "database",
    "cache",
    "resilience",
    "governor",
    "otel-metrics",
    "openapi"
] }
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~18MB (stripped)
**Compile time**: ~90s (clean build)

### REST API with Cedar Authorization
**Use case**: Secure API with fine-grained policy-based access control

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = [
    "http",
    "observability",
    "database",
    "cache",           # Required for Cedar policy caching
    "cedar-authz",     # Policy-based authorization
    "resilience"
] }
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~16MB (stripped)
**Compile time**: ~75s (clean build)

### Dual HTTP + gRPC Service
**Use case**: Service exposing both REST and gRPC APIs

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = [
    "http",
    "grpc",
    "observability",
    "database"
] }
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~15MB (stripped)
**Compile time**: ~60s (clean build)

### Event-Driven Microservice
**Use case**: Background worker processing NATS events

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = [
    "http",           # For health endpoints
    "observability",
    "events",         # NATS support
    "database",
    "cache"
] }
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~14MB (stripped)
**Compile time**: ~55s (clean build)

### HTMX / Server-Rendered App
**Use case**: Traditional web app with server-rendered HTML and HTMX

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = [
    "http",
    "observability",
    "database",
    "session-memory"   # Use "session-redis" in production
] }
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~13MB (stripped)
**Compile time**: ~50s (clean build)

**Provides**: Cookie-based sessions, flash messages, CSRF protection, TypedSession.

### Everything (Development/Prototyping)
**Use case**: Exploring all features, quick prototyping

```toml
[dependencies]
{% $dep.full %}
tokio = { version = "1", features = ["full"] }
```

**Binary size**: ~20MB (stripped)
**Compile time**: ~120s (clean build)

**⚠️ Warning**: `full` includes everything. For production, only enable what you need.

---

## Database Backend Exclusivity

acton-service supports three primary database backends, and they are **pairwise
mutually exclusive**. Enable exactly one:

| Feature | Backend | Use when |
|---------|---------|----------|
| `database` | PostgreSQL (SQLx) | Standard server-side SQL workloads |
| `turso` | Turso / libsql (SQLite) | Edge deployments, embedded replicas |
| `surrealdb` | SurrealDB | Multi-model (document/graph) workloads |

Enabling any two fails the build with a `compile_error!` from `src/lib.rs`:

```text
Features `database` (PostgreSQL) and `turso` (libsql) are mutually exclusive.
Enable only one database backend.
```

The same guard exists for `database` + `surrealdb` and `turso` + `surrealdb`.

{% callout type="warning" title="Watch for transitive database features" %}
The meta-features `auth-full` and `full` both enable `database`. Combining
either with `turso` or `surrealdb` trips the same compile error. If you need
Turso or SurrealDB with authentication, list the auth features explicitly
(`auth`, `oauth`, `jwt`, `cache`, `login-lockout`, `accounts`) instead of using
`auth-full`.
{% /callout %}

`clickhouse` is **not** a primary backend and is exempt from this rule — it is an
analytical store that composes with any of the three (or with none).

---

## Cryptographic Provider

acton-service uses `rustls` for all TLS, and you must select one — and only
one — crypto provider:

| Feature | Default? | Use when |
|---------|----------|----------|
| `crypto-aws-lc-rs` | Yes | Default; required for FIPS 140-3 paths |
| `crypto-ring` | No | Build environment cannot tolerate `aws-lc-rs`'s C toolchain requirements |

Enabling both fails at compile time. Enabling neither fails at compile time.
See [Crypto Provider](/docs/crypto-provider) for the full story, including
FIPS guidance and the `ensure_default_crypto_provider()` bootstrap for
binaries that drive TLS clients without the framework's listener.

## Feature Dependencies

Some features work better together:

| Feature | Recommended Companions | Why |
|---------|----------------------|-----|
| `auth` | `cache` or `database` | Refresh token and API key storage backends |
| `oauth` | `auth`, `cache` | OAuth state management needs Redis for CSRF protection |
| `cedar-authz` | `cache` | Policy decision caching dramatically improves performance (10-50ms → 1-5ms) |
| `cache` | `governor` | Distributed rate limiting needs Redis |
| `otel-metrics` | `observability` | Metrics require tracing foundation |
| `prometheus-metrics` | `http` | The `/metrics` scrape endpoint is served by the HTTP router |
| `journald` | `observability` | Journald layer works alongside OTLP tracing |
| `resilience` | `http` or `grpc` | Resilience patterns apply to HTTP/gRPC calls |
| `openapi` | `http` | OpenAPI docs are for HTTP endpoints |
| `session-redis` | Production deployments | Sessions persist across restarts and work across multiple instances |
| `session-memory` | `session-redis` | Use memory for dev, Redis for production |
| `login-lockout` | `audit` | Emit audit events when accounts are locked/unlocked |

---

## Troubleshooting

### "cannot find type `AppState`"
**Solution**: You're probably missing required features. Add `http` and `observability`.

### "no method named `db` found for struct `AppState`"
**Solution**: Add the `database` feature flag. The pool accessor is `state.db().await`, which returns `Option<PgPool>`.

### "Features `database` (PostgreSQL) and `turso` (libsql) are mutually exclusive"
**Solution**: You enabled more than one database backend. Pick exactly one of `database`, `turso`, or `surrealdb`. See [Database Backend Exclusivity](#database-backend-exclusivity). Watch for meta-features that pull one in transitively — `auth-full` and `full` both enable `database`.

### "could not find `tonic` in the list"
**Solution**: Add `grpc` feature flag.

### Very slow compile times
**Solution**: You might have `full` enabled. Only enable features you actually use.

### Large binary size
**Solution**:
1. Remove unused features
2. Build with `--release`
3. Strip symbols: `strip target/release/my-service`

---

## Best Practices

### Start Small

Begin with minimal features and add as needed:

```toml
# Start here
features = ["http", "observability"]

# Add as you grow
features = ["http", "observability", "database"]

# Production-ready
features = ["http", "observability", "database", "cache", "resilience"]
```

### Production Recommendations

**Minimum for production**:
```toml
features = ["http", "observability", "resilience"]
```

**Recommended for production**:
```toml
features = [
    "http",
    "observability",
    "database",        # If you need it
    "cache",          # For sessions/rate limiting/Cedar caching
    "cedar-authz",    # Fine-grained authorization (optional)
    "resilience",     # Circuit breaker, retry
    "otel-metrics"    # Monitoring
]
```

### CI/CD Optimization

Use different feature sets for different build stages:

```yaml
# Fast CI check
cargo check --features "http,observability"

# Full integration tests
cargo test --features "http,observability,database,cache"

# Production build
cargo build --release --features "http,observability,database,cache,resilience,otel-metrics"
```

---

## Need More Help?

- [Quickstart](/docs/quickstart) - Get started in 5 minutes
- [Tutorial](/docs/tutorial) - Step-by-step service guide
- [Examples](/docs/examples) - Working examples for each feature
- [Cargo.toml](https://github.com/Govcraft/acton-service/blob/main/acton-service/Cargo.toml) - Feature definitions
