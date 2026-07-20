---
title: Middleware Overview
nextjs:
  metadata:
    title: Middleware Overview
    description: Production-ready middleware stack for HTTP and gRPC services with comprehensive authentication, authorization, resilience, and observability features.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


acton-service provides a middleware library for backend services. All middleware works with both HTTP and gRPC protocols.

## Quick Example

> **Note**: Middleware in acton-service is automatically applied by `ServiceBuilder` based on your configuration file. Individual middleware layers are not manually composed via `.with_middleware()` - the framework handles this for you based on enabled features and config settings.

## Available Middleware

### Authentication & Authorization

**Token Authentication (PASETO/JWT)**
- PASETO V4 tokens (default) - secure by design with no algorithm confusion
- JWT support via feature flag for legacy systems (RS256, ES256, HS256/384/512)
- Claims structure with roles, permissions, user/client identification
- Redis-backed token revocation for immediate invalidation
- Learn more: [Token Authentication](/docs/token-auth)

**Cedar Policy-Based Authorization**
- AWS Cedar integration for fine-grained access control
- Declarative policy files for resource-based permissions
- Role-based and attribute-based access control (RBAC/ABAC)
- Manual policy reload endpoint (automatic hot-reload in progress)
- Optional Redis caching for sub-5ms policy decisions
- HTTP and gRPC support with customizable path normalization
- Learn more: [Cedar Authorization](/docs/cedar-auth)

### Resilience & Reliability

**Circuit Breaker**
- Configurable failure rate monitoring with automatic recovery
- Prevents cascading failures in distributed systems
- Configurable open/half-open/closed state transitions

**Retry Logic**
- Exponential backoff with configurable maximum attempts
- Intelligent retry for transient failures
- Respects idempotency requirements

**Bulkhead**
- Concurrency limiting with wait timeouts
- Prevents resource exhaustion and overload
- Isolates request pools for different operations

Learn more: [Resilience Patterns](/docs/resilience)

### Rate Limiting

**Redis-backed Rate Limiting**
- Distributed rate limiting for multi-instance deployments
- Consistent limits across service replicas
- Production-ready for horizontal scaling

**Governor Rate Limiting**
- Local in-memory limiting with per-second/minute/hour presets
- Lower latency for single-instance deployments
- No external dependencies required

**Advanced Features**
- Per-user and per-client limits via token claims
- Customizable limit buckets and windows
- Graceful handling of rate limit exceeded scenarios

Learn more: [Rate Limiting](/docs/rate-limiting)

### Observability

**Request Tracking**
- TypeID-based request ID generation (`req_` prefix with UUIDv7) for human-readable, time-sortable identifiers
- Automatic correlation across service boundaries
- Essential for debugging distributed transactions

**Distributed Tracing Headers**
- Standards-compliant header propagation
- Supports x-request-id, x-trace-id, x-span-id, x-correlation-id
- Integrates with OpenTelemetry and Jaeger

**OpenTelemetry Metrics**
- HTTP request count and duration histograms
- Active request gauges for load monitoring
- Request and response size tracking
- Custom metric instrumentation support

**Sensitive Header Masking**
- Automatic masking in logs for security
- Protects authorization tokens, cookies, API keys
- Configurable sensitive header patterns

Learn more: [Observability](/docs/observability)

### Standard HTTP Middleware

**Compression**
- Multiple encoding support: gzip, br (Brotli), deflate, zstd
- Automatic content negotiation based on Accept-Encoding
- Reduces bandwidth and improves response times

**CORS**
- Configurable cross-origin resource sharing policies
- Supports preflight requests and credentials
- Fine-grained control over allowed origins, methods, headers

**Timeouts**
- Configurable per-request timeouts
- Prevents resource leaks from hanging requests
- Graceful timeout error responses

**Body Size Limits**
- Prevents oversized payload attacks
- Configurable limits per endpoint
- Returns clear error messages for violations

**Panic Recovery**
- Graceful handling of unexpected panics
- Detailed error logging for debugging
- Prevents service crashes from request handlers

## Middleware Types

The middleware that `ServiceBuilder` wires up lives in `acton_service::middleware`:

| Type | Purpose | Feature |
| --- | --- | --- |
| `PasetoAuth` | PASETO V4 token validation (default) | always available |
| `JwtAuth` | JWT token validation | `jwt` |
| `RedisTokenRevocation` | Redis-backed token revocation | `cache` |
| `CedarAuthz` | Cedar policy authorization | `cedar-authz` |
| `GovernorRateLimit` | Local in-memory rate limiting | `governor` |
| `RateLimit` | Redis-backed distributed rate limiting | `cache` |
| `ResilienceConfig` | Circuit breaker and bulkhead layers | `resilience` |
| `RequestContext` | Per-request client IP, request ID, and user agent, resolved once | always available |

## Execution Order

`ServiceBuilder::build()` applies the middleware it manages in a fixed order. You do not choose this order — it is determined by the framework so that each stage sees the state the previous one produced:

```text
Request → framework middleware (request ID, tracing, CORS, compression, timeouts)
        → Request context                    — resolves client IP, request ID, user agent
        → Token auth (PasetoAuth / JwtAuth)  — populates Claims
        → Audit logging                      — sees Claims
        → Cedar authorization                — sees Claims
        → Governor rate limiting             — sees Claims for per-user limits
        → Handler
```

The request-context stage runs after the request ID has been generated and before anything that consumes it. It resolves the client IP once — from `X-Forwarded-For` or `X-Real-IP` when present, falling back to the TCP peer address — and stores it, together with the request ID and user agent, in a `RequestContext` request extension. Every downstream consumer (auth failure audit events, the audit logger, custom handlers) reads that extension instead of re-parsing headers, so audit events carry a real IP and request ID even when an upstream proxy strips forwarding headers.

Token authentication runs next so that `Claims` are available downstream: audit records the authenticated principal, Cedar evaluates policies against it, and the rate limiter applies `per_user_rpm` / `per_client_rpm` buckets keyed by the caller. Anonymous requests fall back to per-IP limits.

If you assemble a `Router` by hand instead of using `ServiceBuilder`, preserve this ordering yourself: apply `request_context_middleware` outside (before) the auth and audit layers, and inside (after) `request_id_layer()`. Consumers fall back to raw header extraction when the extension is missing, but that fallback cannot see the peer address or a generated request ID.

Each stage is skipped when its configuration or feature flag is absent — for example, the governor layer is only attached when the `governor` feature is enabled and `rate_limit.auto_apply` is `true`.

## HTTP and gRPC Compatibility

All middleware works identically for HTTP and gRPC services. The same configuration applies to both protocols automatically. Middleware is automatically applied by `ServiceBuilder` during initialization based on your configuration file - no manual composition is required.

## Next Steps

- [Configure Token Authentication](/docs/token-auth) - Secure your endpoints with PASETO (default) or JWT tokens
- [Implement Cedar Authorization](/docs/cedar-auth) - Add fine-grained access control
- [Add Rate Limiting](/docs/rate-limiting) - Protect against abuse and overload
- [Enable Resilience Patterns](/docs/resilience) - Build fault-tolerant services
