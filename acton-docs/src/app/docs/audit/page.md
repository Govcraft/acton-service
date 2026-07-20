---
title: Audit Logging
nextjs:
  metadata:
    title: Audit Logging
    description: Immutable audit trails with BLAKE3 hash chaining, Syslog RFC 5424 export, and OTLP integration for security compliance
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

Tamper-evident audit logging with BLAKE3 hash chaining, automatic auth event capture, and SIEM export via Syslog RFC 5424 and OpenTelemetry Logs.

## Overview

acton-service provides immutable audit trails for security compliance. Every audit event is sealed into a BLAKE3 hash chain, guaranteeing tamper detection. Events are processed sequentially by an internal actor, persisted to your configured database backend with append-only enforcement, and exported to SIEM systems via Syslog or OTLP.

{% callout type="note" title="Actor-Based Processing" %}
Audit events are processed by an **AuditAgent** (acton-reactive actor) that owns the hash chain state and guarantees correct ordering. Events are sent via fire-and-forget message passing, so audit logging never blocks request handling. Events emitted during startup — including the `ConfigLoaded` compliance event — are buffered until the hash chain initializes, then sealed in emission order, so nothing is lost while storage connects. See [Reactive Architecture](/docs/reactive-architecture) for details.
{% /callout %}

### How It Works

```text
HTTP Request --> AuditMiddleware --> ActorHandle::send() --> AuditAgent --+--> DB (append-only)
Auth Events --> AuditLogger.log() -------------------------+             +--> Syslog RFC 5424
                                                                         +--> OTLP Logs (optional)
```

### Feature Interactions

| Features Enabled | Behavior |
|---|---|
| `audit` alone | In-memory hash chain + syslog export |
| `audit` + `database`/`turso`/`surrealdb` | Persistent append-only storage |
| `audit` + `clickhouse` | Analytical audit storage (MergeTree, time-partitioned) |
| `audit` + `observability` | OTLP log export via tracing |
| `audit` + token auth (PASETO/JWT) | Automatic auth event emission |

## Installation

Enable the audit feature:

```toml
[dependencies]
{% $dep.auditOnly %}
```

With a database backend for persistent storage:

```toml
[dependencies]
{% $dep.auditDatabase %}
```

Or with ClickHouse for analytical audit storage:

```toml
[dependencies]
{% $dep.clickhouseAudit %}
```

## Configuration

Add an `[audit]` section to your `config.toml`:

```toml
[audit]
enabled = true
audit_all_requests = false        # Audit every HTTP request
audit_auth_events = true          # Auto-audit auth events (login, logout, etc.)
otlp_logs_enabled = false         # Export via OTLP (requires observability feature)
audited_routes = ["/api/v1/admin/*"]   # Glob patterns for per-route auditing
excluded_routes = ["/health", "/ready", "/metrics"]

[audit.syslog]
transport = "udp"                 # "udp", "tcp", or "none"
address = "127.0.0.1:514"
facility = 13                     # 13 = audit (RFC 5424)
# app_name = "my-service"         # Defaults to service.name
```

### Environment Variable Override

```bash
ACTON_AUDIT_ENABLED=true
ACTON_AUDIT_AUDIT_ALL_REQUESTS=true
ACTON_AUDIT_SYSLOG_TRANSPORT=tcp
ACTON_AUDIT_SYSLOG_ADDRESS=syslog.example.com:514
```

### Configuration Options

- **enabled**: Enable or disable audit logging globally (default: `true`)
- **audit_all_requests**: Log every HTTP request as an audit event (default: `false`)
- **audit_auth_events**: Automatically emit events for auth actions (default: `true`)
- **otlp_logs_enabled**: Export audit events via OpenTelemetry Logs (default: `false`, requires `observability` feature)
- **audited_routes**: Glob patterns for routes to audit (e.g., `"/api/v1/admin/*"`)
- **excluded_routes**: Routes to never audit, even when `audit_all_requests` is true (default: `["/health", "/ready"]`)

## Basic Usage

The audit logger is available via `AppState` when the `audit` feature is enabled:

```rust
use acton_service::prelude::*;

async fn delete_user(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<()>> {
    // Perform the deletion
    // ...

    // Log a custom audit event
    if let Some(logger) = state.audit_logger() {
        logger.log_custom(
            "user.delete",
            AuditSeverity::Warning,
            Some(serde_json::json!({ "user_id": id })),
        ).await;
    }

    Ok(Json(()))
}
```

### Auth Events (Automatic)

When `audit_auth_events` is enabled (default), the PASETO and JWT middleware automatically emit audit events:

| Event Kind | When Emitted | Severity |
|---|---|---|
| `AuthLoginSuccess` | Token validated successfully | Notice |
| `AuthTokenMissing` | Bearer token missing or malformed on a protected route | Informational |
| `AuthTokenInvalid` | Bearer token failed validation (bad signature, expired, malformed claims) | Warning |
| `AuthTokenRevoked` | Revoked token presented. Event metadata carries `jti` for SIEM correlation. | Warning |
| `AuthPermissionDenied` | Cedar policy returned `Deny` (HTTP middleware and gRPC tower service) | Warning |
| `HttpRequestDenied` | Rate-limit rejection (`Error::RateLimitExceeded`), from both the Redis-backed and governor limiters | Warning |
| `AuthLogout` | `TypedSession<AuthSession>::logout()` called; subject is the previously authenticated user | Notice |

> `AuthLoginFailed` is no longer emitted by the auth middleware. It is reserved for application-level login handlers (e.g. `POST /auth/login`) where credentials are submitted. The middleware emits `AuthTokenMissing` or `AuthTokenInvalid` instead, so unauthenticated probes against protected routes (health checks, scanners) no longer drown out real failed-login signal. See the 0.27 release notes for the migration.

No additional code is required. These events include the client IP, user agent, request ID, and authenticated subject. The IP and request ID come from the request-context middleware, which resolves them once per request — from `X-Forwarded-For` / `X-Real-IP` when an upstream proxy supplies them, falling back to the direct TCP peer address otherwise — and runs after the request ID is generated. Token-failure events therefore carry a usable source even when a proxy strips forwarding headers (see [Execution Order](/docs/middleware#execution-order)).

### Auth Storage Decorators

Token refresh, API-key lifecycle, and OAuth callbacks run through storage and provider traits that your application constructs, so the framework cannot emit those events automatically. Wrap what you build with the decorators in `acton_service::audit` and every backend emits the same events:

```rust
use acton_service::audit::{AuditedApiKeyStorage, AuditedOAuthProvider, AuditedRefreshStorage};

let logger = state.audit_logger().expect("audit enabled").clone();
let refresh = AuditedRefreshStorage::new(RedisRefreshStorage::new(pool.clone()), logger.clone());
let api_keys = AuditedApiKeyStorage::new(RedisApiKeyStorage::new(pool, "sk_live"), logger.clone());
let google = AuditedOAuthProvider::new(google_provider, logger);
```

| Decorator | Emits | When |
|---|---|---|
| `AuditedRefreshStorage` | `AuthTokenRefresh` | Successful token rotation; source carries the user ID plus the IP/user agent captured at issuance |
| `AuditedApiKeyStorage` | `AuthApiKeyCreated` / `AuthApiKeyRevoked` | Successful create/revoke; metadata carries key ID, name, prefix, and scopes — never the key or its hash |
| `AuditedOAuthProvider` | `AuthOAuthCallback` | Authorization-code exchange, success (Notice) or failure (Warning) |

Failed operations propagate their errors without emitting lifecycle events, and all decorator emissions honor `audit_auth_events`.

## Per-Route Auditing

Mark specific routes for auditing with custom event names:

```rust
use acton_service::prelude::*;

let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router
            // These routes are audited with custom event names
            .route("/admin/users/:id", delete(delete_user)
                .layer(Extension(AuditRoute::new("user.delete"))))
            .route("/admin/settings", put(update_settings)
                .layer(Extension(AuditRoute::new("settings.update"))))
            // This route is NOT audited (unless audit_all_requests is true)
            .route("/users", get(list_users))
    })
    .build_routes();
```

Routes annotated with `AuditRoute` are always audited, regardless of the `audited_routes` config patterns.

### Route Pattern Matching

The `audited_routes` config supports simple glob patterns:

```toml
[audit]
audited_routes = [
    "/api/v1/admin/*",        # Any single segment under /admin/
    "/api/v1/admin/**",       # Any path starting with /admin/
    "/api/v1/users/*/delete", # DELETE-like paths with any user ID
]
```

## Hash Chain Integrity

Every audit event is sealed into a BLAKE3 hash chain. Each event's hash covers:

- Sequence number (monotonically increasing)
- Previous event's hash (chain linkage)
- Event ID, timestamp, kind, severity
- Service name, HTTP method, path, status code
- Authenticated subject (if present)

### Verifying the Chain

```rust
use acton_service::audit::{verify_chain, AuditEvent};

// Fetch events from storage
let events: Vec<AuditEvent> = storage.query_range(from, to, 1000).await?;

// Verify the hash chain is intact
match verify_chain(&events) {
    Ok(()) => println!("Chain integrity verified"),
    Err(e) => eprintln!("Tamper detected: {}", e),
}
```

The chain detects:
- **Modified events**: Hash won't match recalculated value
- **Deleted events**: Sequence gaps or broken chain links
- **Reordered events**: Previous hash won't match prior event
- **Inserted events**: Chain linkage will be broken

## Database Storage

When a database feature is enabled alongside `audit`, events are persisted with append-only enforcement:

### PostgreSQL

```toml
[dependencies]
{% $dep.auditDatabase %}
```

Uses `CREATE RULE` to prevent updates and deletes on the `audit_events` table.

### Turso (libsql)

```toml
[dependencies]
{% dep(["audit", "turso"]) %}
```

Uses `CREATE TRIGGER ... RAISE(ABORT)` to enforce immutability.

### SurrealDB

```toml
[dependencies]
{% dep(["audit", "surrealdb"]) %}
```

Uses `PERMISSIONS FOR update, delete NONE` on the audit table.

{% callout type="warning" title="Append-Only Enforcement" %}
The storage implementations create database rules/triggers that prevent any modification or deletion of audit records. This is enforced at the database level, not just in application code. A database administrator with direct schema access could still remove these protections, so protect your database credentials accordingly.
{% /callout %}

## Syslog Export (RFC 5424)

Audit events are automatically formatted as RFC 5424 syslog messages and sent via UDP or TCP:

```text
<109>1 2026-01-15T10:30:00.000Z my-service acton-audit - - [audit@0 event_id="..." kind="AuthLoginSuccess" severity="Notice" sequence="42" hash="abc123..."] User login successful
```

### Syslog Configuration

```toml
[audit.syslog]
transport = "udp"              # "udp", "tcp", or "none" to disable
address = "127.0.0.1:514"     # Syslog server address
facility = 13                  # RFC 5424 facility (13 = audit)
app_name = "my-service"       # Override app name (defaults to service.name)
```

### Integration with SIEM Systems

Configure your SIEM to receive RFC 5424 messages:

- **Splunk**: Configure a UDP/TCP input on port 514, parse structured data from the `[audit@0]` SD-ELEMENT
- **Elastic/ELK**: Use Filebeat's syslog input or Logstash's syslog filter
- **Datadog**: Configure a syslog source via the Datadog Agent
- **Graylog**: Add a Syslog UDP/TCP input

## OTLP Export

When the `observability` feature is also enabled, audit events are exported as structured OpenTelemetry log records:

```toml
[dependencies]
{% $dep.audit %}
```

```toml
[audit]
otlp_logs_enabled = true

[otlp]
endpoint = "http://otel-collector:4317"
```

Events are emitted via `tracing::info!` with structured fields:

```text
audit.event_id = "550e8400-..."
audit.kind = "AuthLoginSuccess"
audit.severity = "Notice"
audit.sequence = 42
audit.hash = "abc123..."
audit.service = "my-service"
```

These fields are automatically picked up by any OpenTelemetry-compatible collector.

## Custom Audit Events

Emit custom events from anywhere in your application:

```rust
use acton_service::prelude::*;

async fn process_payment(
    State(state): State<AppState>,
    Json(payment): Json<PaymentRequest>,
) -> Result<Json<PaymentResponse>> {
    // Process payment...
    let result = charge_card(&payment).await?;

    // Log audit event with metadata
    if let Some(logger) = state.audit_logger() {
        logger.log_custom(
            "payment.processed",
            AuditSeverity::Informational,
            Some(serde_json::json!({
                "amount": payment.amount,
                "currency": payment.currency,
                "payment_id": result.id,
            })),
        ).await;
    }

    Ok(Json(result))
}
```

### Event Kinds

Built-in event kinds for common operations:

| Kind | Description |
|---|---|
| `AuthLoginSuccess` | Successful authentication |
| `AuthLoginFailed` | Failed credential-submission attempt (emit from app login handlers; the middleware no longer emits this) |
| `AuthTokenMissing` | Bearer token missing on protected route (middleware-emitted) |
| `AuthTokenInvalid` | Bearer token failed validation (middleware-emitted) |
| `AuthLogout` | User logout |
| `AuthTokenRefresh` | Token refresh |
| `AuthTokenRevoked` | Token revocation |
| `AuthPasswordChanged` | Password change |
| `AuthApiKeyCreated` | API key created |
| `AuthApiKeyRevoked` | API key revoked |
| `AuthOAuthCallback` | OAuth callback processed |
| `AuthPermissionDenied` | Authorization denied |
| `AuthAccountLocked` | Account locked after repeated login failures (requires `login-lockout`) |
| `AuthAccountUnlocked` | Account unlocked (requires `login-lockout`) |
| `AccountCreated` | Account created (requires `accounts`) |
| `AccountDisabled` | Account disabled by an administrator (requires `accounts`) |
| `AccountEnabled` | Account enabled or re-activated (requires `accounts`) |
| `AccountLocked` | Account locked due to a security event (requires `accounts`) |
| `AccountUnlocked` | Account unlocked (requires `accounts`) |
| `AccountExpired` | Account expired (requires `accounts`) |
| `AccountDeleted` | Account deleted (requires `accounts`) |
| `AccountUpdated` | Account updated — profile, email verification, password, or roles (requires `accounts`) |
| `AuthKeyRotated` | Signing key rotated (new key active, old key draining) |
| `AuthKeyRetired` | Signing key retired after its drain period expired |
| `AuthKeyRotationFailed` | Key rotation failed |
| `ConfigLoaded` | Configuration loaded at startup (NIST CM-3) |
| `ConfigDriftDetected` | Active configuration differs from on-disk sources (NIST CM-3) |
| `HttpRequest` | HTTP request (from middleware) |
| `HttpRequestDenied` | Denied HTTP request (rate limit, auth failure, etc.) |
| `Custom(String)` | Custom application event |

### Severity Levels

Severity levels map to RFC 5424 syslog severity values:

| Severity | Syslog Value | Usage |
|---|---|---|
| `Emergency` | 0 | System is unusable |
| `Alert` | 1 | Immediate action required |
| `Critical` | 2 | Critical conditions |
| `Error` | 3 | Error conditions |
| `Warning` | 4 | Warning conditions |
| `Notice` | 5 | Normal but significant |
| `Informational` | 6 | Informational messages |
| `Debug` | 7 | Debug-level messages |

## Best Practices

**DO:**
- ✅ Enable `audit_auth_events` in production (it's on by default)
- ✅ Use per-route `AuditRoute` annotations for sensitive operations
- ✅ Configure syslog export to a SIEM for real-time monitoring
- ✅ Periodically verify hash chain integrity
- ✅ Use a persistent database backend for compliance requirements
- ✅ Exclude health check routes from auditing

**DON'T:**
- ❌ Enable `audit_all_requests` without considering volume (can be noisy)
- ❌ Store audit events only in-memory for production (no persistence)
- ❌ Disable database-level append-only protections
- ❌ Log sensitive data (passwords, tokens) in audit event metadata

## Production Deployment

### Recommended Configuration

```toml
[audit]
enabled = true
audit_all_requests = false
audit_auth_events = true
otlp_logs_enabled = true
audited_routes = ["/api/v1/admin/*", "/api/v1/billing/*"]
excluded_routes = ["/health", "/ready", "/metrics"]

[audit.syslog]
transport = "tcp"                 # TCP for reliable delivery
address = "syslog.internal:514"
facility = 13
```

### Kubernetes Integration

```yaml
env:
  - name: ACTON_AUDIT_ENABLED
    value: "true"
  - name: ACTON_AUDIT_SYSLOG_TRANSPORT
    value: "tcp"
  - name: ACTON_AUDIT_SYSLOG_ADDRESS
    value: "syslog-service.logging.svc.cluster.local:514"
```

### Compliance Considerations

The audit system supports common compliance frameworks:

- **SOC 2**: Immutable audit trails with hash chain verification
- **PCI DSS**: Logging of authentication events and access to sensitive data
- **HIPAA**: Audit controls for access to protected health information
- **GDPR**: Record of processing activities with tamper detection

Ensure your syslog/SIEM retention policies match your compliance requirements.

## Related Features

- **[Auth Overview](/docs/auth)** - Authentication features that emit audit events
- **[Token Authentication](/docs/token-auth)** - PASETO/JWT middleware with automatic audit integration
- **[Observability](/docs/observability)** - OpenTelemetry tracing and OTLP export
- **[Configuration](/docs/configuration)** - Environment and file-based configuration
- **[Health Checks](/docs/health-checks)** - Service health monitoring
