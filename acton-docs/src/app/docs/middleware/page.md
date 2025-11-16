---
title: Middleware Overview
nextjs:
  metadata:
    title: Middleware Overview
    description: Production-ready middleware stack for HTTP and gRPC services with comprehensive authentication, authorization, resilience, and observability features.
---

acton-service provides a batteries-included middleware stack designed for production microservices. All middleware works seamlessly with both HTTP and gRPC protocols.

## Quick Example

> **Note**: Middleware in acton-service is automatically applied by `ServiceBuilder` based on your configuration file. Individual middleware layers are not manually composed via `.with_middleware()` - the framework handles this for you based on enabled features and config settings.

## Available Middleware

### Authentication & Authorization

**JWT Authentication**
- Full token validation with RS256, ES256, HS256/384/512 algorithms
- Claims structure with roles, permissions, user/client identification
- Redis-backed token revocation for immediate invalidation
- Learn more: [JWT Authentication](/docs/jwt-auth)

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
- Per-user and per-client limits via JWT claims
- Customizable limit buckets and windows
- Graceful handling of rate limit exceeded scenarios

Learn more: [Rate Limiting](/docs/rate-limiting)

### Observability

**Request Tracking**
- UUID-based request ID generation and propagation
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

## Middleware Composition

Middleware layers execute in reverse order of application. The last layer applied executes first:

```rust
router
    .layer(AuthLayer)       // Executes third
    .layer(RateLimitLayer)  // Executes second
    .layer(TracingLayer)    // Executes first
```

**Best Practice Order:**
1. Request tracking (first - generates correlation IDs)
2. Panic recovery (catches all downstream panics)
3. Authentication (validates identity early)
4. Authorization (checks permissions after authentication)
5. Rate limiting (throttle after auth to use user-specific limits)
6. Resilience patterns (protect downstream services)
7. Compression (last - compresses final response)

## HTTP and gRPC Compatibility

All middleware works identically for HTTP and gRPC services. The same configuration applies to both protocols automatically. Middleware is automatically applied by `ServiceBuilder` during initialization based on your configuration file - no manual composition is required.

## Next Steps

- [Configure JWT Authentication](/docs/jwt-auth) - Secure your endpoints with token-based auth
- [Implement Cedar Authorization](/docs/cedar-auth) - Add fine-grained access control
- [Add Rate Limiting](/docs/rate-limiting) - Protect against abuse and overload
- [Enable Resilience Patterns](/docs/resilience) - Build fault-tolerant services
