---
title: API Versioning
nextjs:
  metadata:
    title: Type-Safe API Versioning
    description: Compile-time enforced API versioning with automatic deprecation headers
---

The **acton-service** framework provides **type-safe API versioning** that strongly encourages proper versioning practices through Rust's type system. The framework uses opaque types and compile-time checks to ensure APIs are versioned correctly.

---

## Why Use Type-Safe Versioning?

{% callout type="warning" title="Challenges with Optional Versioning" %}
When versioning is optional, teams may encounter:

- Unversioned APIs that break consumers
- Inconsistent versioning patterns across services
- Missing deprecation headers
- Breaking changes without warning
{% /callout %}

**The Approach**: The framework uses **Rust's type system** to enforce versioning:

- `VersionedRoutes` can ONLY be created by `VersionedApiBuilder` (opaque type with private fields)
- `ServiceBuilder.with_routes()` is the ONLY way to use VersionedRoutes (enforced by type system)
- `ServiceBuilder` automatically provides `/health` and `/ready` endpoints
- Compile-time enforcement prevents bypassing versioned routes

---

## Key Features

The framework offers:

- **VersionedApiBuilder**: Type-safe builder that creates opaque `VersionedRoutes` containing only your versioned business routes
- **Automatic Health Endpoints**: `ServiceBuilder` automatically provides `/health` and `/ready` endpoints
- **Automatic Deprecation Headers**: RFC 8594 compliant deprecation headers sent automatically
- **Sunset Date Management**: Clear migration timelines with sunset date enforcement

---

## Quick Start

### Recommended Approach: VersionedApiBuilder

Create versioned routes using the type-safe builder:

```rust
use acton_service::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Create versioned routes (returns opaque VersionedRoutes type)
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router
                .route("/users", get(list_users).post(create_user))
                .route("/users/:id", get(get_user).put(update_user).delete(delete_user))
        })
        .build_routes();  // Returns VersionedRoutes (opaque type)

    // ServiceBuilder automatically adds /health and /ready endpoints
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

This creates routes at:
- `GET  /health` (automatic from ServiceBuilder)
- `GET  /ready` (automatic from ServiceBuilder)
- `GET  /api/v1/users`
- `POST /api/v1/users`
- `GET  /api/v1/users/:id`
- `PUT  /api/v1/users/:id`
- `DELETE /api/v1/users/:id`

### Multiple Versions

```rust
let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router.route("/users", get(list_users_v1))
    })
    .add_version(ApiVersion::V2, |router| {
        router.route("/users", get(list_users_v2))
    })
    .build_routes();

ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await
```

Routes:
- `/health` → Health check (automatic)
- `/ready` → Readiness check (automatic)
- `/api/v1/users` → V1 handler
- `/api/v2/users` → V2 handler

---

## When to Create a New API Version

{% callout type="note" title="Decision Framework" %}
Version when making **breaking changes** that would cause existing clients to fail or behave incorrectly. Don't version for additive, backward-compatible changes.
{% /callout %}

### Breaking Changes (Require New Version)

Create a new API version when you:

**1. Remove Fields from Responses**
```rust
// V1
{
  "id": 123,
  "name": "Alice",
  "email": "alice@example.com"  // ← Removing this field = breaking
}

// V2 (new version required)
{
  "id": 123,
  "name": "Alice"
}
```

**2. Change Field Types**
```rust
// V1: ID as string
{"id": "123", "name": "Alice"}

// V2: ID as integer (breaks parsing)
{"id": 123, "name": "Alice"}
```

**3. Rename Fields**
```rust
// V1
{"user_id": 123}

// V2 (renamed field = breaking)
{"id": 123}
```

**4. Change Field Semantics**
```rust
// V1: timestamp in seconds
{"created_at": 1704063600}

// V2: timestamp in milliseconds (breaks date parsing)
{"created_at": 1704063600000}
```

**5. Make Optional Fields Required**
```rust
// V1
POST /users
{"name": "Alice"}  // email optional

// V2 (now required = breaking)
POST /users
{"name": "Alice", "email": "alice@example.com"}  // email required
```

**6. Change Response Structure**
```rust
// V1: flat structure
{"id": 123, "name": "Alice", "email": "alice@example.com"}

// V2: nested structure (breaks field access)
{"id": 123, "profile": {"name": "Alice", "email": "alice@example.com"}}
```

**7. Change URL Patterns**
```rust
// V1
GET /users/{id}

// V2 (different pattern = breaking)
GET /users/{username}
```

**8. Change HTTP Status Codes**
```rust
// V1: Returns 404 for not found
GET /users/999 → 404 Not Found

// V2: Returns 200 with null (breaks error handling)
GET /users/999 → 200 OK {"user": null}
```

### Non-Breaking Changes (DON'T Version)

These changes are backward-compatible and should be made to existing versions:

**1. Add New Optional Fields to Responses**
```rust
// V1: Original
{"id": 123, "name": "Alice"}

// V1: Enhanced (clients ignore new fields)
{"id": 123, "name": "Alice", "created_at": "2024-01-01"}  // ✅ Safe
```

**2. Add New Optional Fields to Requests**
```rust
// V1: Original
POST /users
{"name": "Alice"}

// V1: Enhanced (server ignores if missing)
POST /users
{"name": "Alice", "preferences": {...}}  // ✅ Safe
```

**3. Add New Endpoints**
```rust
// V1: Add new endpoint
GET /api/v1/users/{id}/settings  // ✅ Safe
```

**4. Make Required Fields Optional**
```rust
// V1: Original (email required)
POST /users
{"name": "Alice", "email": "..."}

// V1: Relaxed (email now optional)
POST /users
{"name": "Alice"}  // ✅ Safe - more permissive
```

**5. Add New Error Codes**
```rust
// V1: Returns 400 or 500
POST /users

// V1: Enhanced (new 429 rate limit error)
POST /users → 429 Too Many Requests  // ✅ Safe - clients already handle errors
```

**6. Change Internal Implementation**
```rust
// V1: Change database, algorithms, caching - anything that doesn't affect API contract
// ✅ Safe - clients don't see implementation
```

### Decision Tree

```
Is this change backward-compatible?
│
├─ YES → Update existing version (V1, V2, etc.)
│         Examples: Add optional field, new endpoint, relax validation
│
└─ NO → Breaking change detected
    │
    ├─ Are there active clients using current version?
    │  │
    │  ├─ YES → Create new version (V2, V3, etc.)
    │  │         Mark old version as deprecated
    │  │         Set sunset date (6-12 months)
    │  │         Communicate migration path
    │  │
    │  └─ NO → Can update current version
    │           (if no one is using it yet)
```

### Real-World Examples

**Example 1: Adding Search Feature**
```rust
// V1: List all users
GET /api/v1/users
Response: [...]

// V1: Add optional query parameter (backward-compatible)
GET /api/v1/users?search=alice
Response: [...]

// ✅ No new version needed - optional parameter
```

**Example 2: Changing Date Format**
```rust
// V1: Unix timestamp
{"created_at": 1704063600}

// V2: ISO 8601 string (BREAKING - parsing changes)
{"created_at": "2024-01-01T00:00:00Z"}

// ✅ New version required
```

**Example 3: Pagination Addition**
```rust
// V1: Returns array
GET /api/v1/users
Response: [...]

// V2: Returns paginated object (BREAKING - response structure changed)
GET /api/v2/users
Response: {"users": [...], "page": 1, "total": 100}

// ✅ New version required
```

### Migration Timeline Recommendations

When creating new versions:

**Conservative (Large User Base)**
- Announce deprecation immediately
- Support old version for 12 months
- Send sunset date 6 months in advance
- Force migration after 12 months

**Moderate (Standard)**
- Announce deprecation immediately
- Support old version for 6 months
- Send sunset date 3 months in advance
- Force migration after 6 months

**Aggressive (Small/Internal)**
- Announce deprecation immediately
- Support old version for 3 months
- Send sunset date 1 month in advance
- Force migration after 3 months

### Version Number Strategy

**Semantic Versioning for APIs:**
- V1, V2, V3 - Major versions only (recommended)
- Don't use V1.1, V1.2 - confusing for consumers
- Save minor/patch versions for implementation details

**When to increment:**
- V1 → V2: Any breaking change
- V2 → V3: Another breaking change
- Skip numbers if needed (V1 → V3 is fine if V2 was never released)

---

## Deprecation Management

### Marking a Version as Deprecated

```rust
let deprecation = DeprecationInfo::new(ApiVersion::V1, ApiVersion::V3)
    .with_sunset_date("2025-12-31T23:59:59Z")
    .with_message("Please migrate to V3 by end of 2025");

let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version_deprecated(
        ApiVersion::V1,
        |router| router.route("/users", get(list_users_v1)),
        deprecation
    )
    .add_version(ApiVersion::V3, |router| {
        router.route("/users", get(list_users_v3))
    })
    .build_routes();

ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await
```

When clients call `/api/v1/users`, they automatically receive RFC 8594 compliant headers:

```http
Deprecation: version="v1"
Sunset: 2025-12-31T23:59:59Z
Link: </v3/>; rel="successor-version"
Warning: 299 - "API version v1 is deprecated. Please migrate to version v3. Please migrate to V3 by end of 2025"
```

### Alternative: Deprecate After Adding

```rust
let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router.route("/users", get(list_users_v1))
    })
    .deprecate_version(
        ApiVersion::V1,
        DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
    )
    .build_routes();
```

---

## API Evolution Patterns

### Scenario 1: Adding a Field (Non-Breaking)

```rust
// V1: Original response
#[derive(Serialize)]
struct UserV1 {
    id: u64,
    username: String,
}

// V2: Add email field (backward compatible if clients ignore unknown fields)
#[derive(Serialize)]
struct UserV2 {
    id: u64,
    username: String,
    email: String,  // New field
}

let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router.route("/users", get(list_users_v1))
    })
    .add_version(ApiVersion::V2, |router| {
        router.route("/users", get(list_users_v2))
    })
    .build_routes();
```

{% callout type="note" title="Recommendation" %}
Keep V1 active but announce V2 as preferred version.
{% /callout %}

### Scenario 2: Breaking Change (Different ID Type)

```rust
// V2: Integer IDs
#[derive(Serialize)]
struct UserV2 {
    id: u64,
    username: String,
}

// V3: String UUIDs (BREAKING CHANGE)
#[derive(Serialize)]
struct UserV3 {
    id: String,  // Changed from u64 to String
    username: String,
}

let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version_deprecated(
        ApiVersion::V2,
        |router| router.route("/users/:id", get(get_user_v2)),
        DeprecationInfo::new(ApiVersion::V2, ApiVersion::V3)
            .with_sunset_date("2026-06-30T23:59:59Z")
            .with_message("V3 uses UUID strings for better scalability")
    )
    .add_version(ApiVersion::V3, |router| {
        router.route("/users/:id", get(get_user_v3))
    })
    .build_routes();
```

**Deprecation Timeline**:
1. Release V3
2. Mark V2 as deprecated with 6-month sunset period
3. Monitor V2 usage
4. Remove V2 after sunset date

### Scenario 3: Renaming an Endpoint

```rust
// V1: /articles
// V2: /posts (rename)

let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version_deprecated(
        ApiVersion::V1,
        |router| router.route("/articles", get(list_articles)),
        DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
            .with_message("Endpoint renamed from /articles to /posts")
    )
    .add_version(ApiVersion::V2, |router| {
        router.route("/posts", get(list_posts))
    })
    .build_routes();
```

---

## Recommended Patterns

### 1. Use VersionedApiBuilder + ServiceBuilder

The framework enforces versioned routes through the type system:

```rust
// ✅ ONLY WAY - Type-enforced versioning with automatic health endpoints
let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router.route("/users", get(handler))
    })
    .build_routes();  // Returns VersionedRoutes (opaque type)

// ServiceBuilder.with_routes() is the ONLY way to use VersionedRoutes
// Automatically includes /health and /ready endpoints
ServiceBuilder::new()
    .with_routes(routes)  // Type system enforces this!
    .build()
    .serve()
    .await?;
```

❌ **WON'T COMPILE** - ServiceBuilder.with_routes() only accepts VersionedRoutes:
```rust
let app = Router::new().route("/users", get(handler));
ServiceBuilder::new().with_routes(app).build();
//                                  ^^^ ERROR: expected VersionedRoutes, found Router
```

### 2. Set Sunset Dates (Recommended)

Sunset dates are optional in both the implementation and the RFC standards:

```rust
DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
    .with_sunset_date("2026-06-30T23:59:59Z")  // RFC 3339 format - RECOMMENDED
    .with_message("Reason for deprecation")
```

{% callout type="note" title="Why Not Enforced?" %}
Unlike API versioning (which the type system enforces), sunset dates are `Option<String>` because:
- **RFC 9745** makes the Sunset header optional: "can be used in addition"
- Sometimes deprecation is announced before a removal date is known
- Allows gradual rollout: deprecate first, announce sunset date later

**Recommendation**: Include sunset dates to give consumers clear migration timelines. The framework provides the field but doesn't enforce it to match RFC flexibility.
{% /callout %}

### 3. Provide Clear Migration Messages

```rust
// ❌ BAD
.with_message("This version is deprecated")

// ✅ GOOD
.with_message("V1 is deprecated. Migrate to V2 for improved pagination. See migration guide: https://docs.example.com/v1-to-v2")
```

### 4. Maintain At Least Two Versions

Keep N and N-1 versions active during migration:

```rust
VersionedApiBuilder::new()
    .add_version_deprecated(ApiVersion::V2, |router| {...}, deprecation)  // Old version
    .add_version(ApiVersion::V3, |router| {...})  // Current version
    .build_routes()
```

---

## Monitoring Deprecation

{% callout type="note" title="Automatic Logging" %}
The framework **automatically logs** all deprecated API usage with structured logging. Every request to a deprecated endpoint is logged at the `warn` level with details including the path, deprecated version, replacement version, sunset date (if set), and any custom message.
{% /callout %}

### Automatic Logging of Deprecated API Usage

When you mark a version as deprecated, the framework automatically logs every access to that version:

```rust
// When you add a deprecated version like this:
let deprecation = DeprecationInfo::new(ApiVersion::V1, ApiVersion::V3)
    .with_sunset_date("2025-12-31T23:59:59Z")
    .with_message("Please migrate to V3 by end of 2025");

let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version_deprecated(
        ApiVersion::V1,
        |router| router.route("/users", get(list_users_v1)),
        deprecation
    )
    .build_routes();
```

Every request to `/api/v1/users` will automatically generate a structured log entry:

```text
WARN deprecated_api_version_accessed path=/api/v1/users deprecated_version=v1 replacement_version=v3 sunset_date=2025-12-31T23:59:59Z message="Please migrate to V3 by end of 2025"
```

This logging happens automatically - no additional code required!

### Automatic OpenTelemetry Metrics

{% callout type="note" title="Automatic Metrics (otel-metrics feature)" %}
When the `otel-metrics` feature is enabled, the framework **automatically tracks OpenTelemetry metrics** for all API version usage. Every request is recorded with detailed attributes for monitoring and alerting.
{% /callout %}

The framework automatically records the `api.version.requests` counter metric with the following attributes:

- `version`: The API version accessed (e.g., "v1", "v2", "v3")
- `deprecated`: Whether the version is deprecated ("true" or "false")
- `replacement_version`: The recommended replacement version (only for deprecated versions)

**Example metric data:**

```
# Non-deprecated version
api.version.requests{version="v3", deprecated="false"} = 1250

# Deprecated version
api.version.requests{version="v1", deprecated="true", replacement_version="v3"} = 45
api.version.requests{version="v2", deprecated="true", replacement_version="v3"} = 120
```

This allows you to:
- Monitor which API versions are being used
- Track deprecated API usage for migration planning
- Set up alerts when deprecated versions exceed thresholds
- Visualize version adoption over time

**Enabling metrics:**

```toml
# Cargo.toml
[dependencies]
{% $dep.metrics %}
```

The metrics are automatically exported via OTLP when observability is configured - no additional code needed!

---

## Migration Checklist

When deprecating an API version:

- [ ] Create new version with changes
- [ ] Add deprecation info with sunset date (6+ months out)
- [ ] Update documentation with migration guide
- [ ] Set up alerts for deprecated version usage (logs and metrics are automatic)
- [ ] Notify API consumers via:
  - [ ] Email/Slack announcements
  - [ ] API changelog
  - [ ] Deprecation headers (automatic via framework)
- [ ] Monitor usage over time (via automatic logs and metrics)
- [ ] Remove version after sunset date with zero usage

---

## FAQ

### Q: Can I use plain Router without VersionedApiBuilder?

**No.** The framework **enforces** versioned routes through Rust's type system:

1. **VersionedRoutes opaque type**: Can ONLY be created via `VersionedApiBuilder::build_routes()`
2. **ServiceBuilder.with_routes()**: Only accepts VersionedRoutes (type-enforced at compile time)
3. **No escape hatch**: You cannot bypass versioning if you want to use ServiceBuilder

```rust
// ✅ ONLY WAY - Type system enforces versioning
let routes = VersionedApiBuilder::new()
    .add_version(ApiVersion::V1, |router| router.route("/users", get(handler)))
    .build_routes();  // Returns VersionedRoutes

ServiceBuilder::new()
    .with_routes(routes)  // Type-enforced: must be VersionedRoutes
    .build()
    .serve()
    .await?;

// ❌ WON'T COMPILE - Unversioned routes rejected
let app = Router::new().route("/users", get(handler));
ServiceBuilder::new().with_routes(app).build();
//                                  ^^^ ERROR: expected VersionedRoutes, found Router
```

### Q: How many versions should I maintain?

**Minimum**: 2 versions (N and N-1)
**Recommended**: 2-3 versions with clear sunset dates

### Q: What if I need to support 5+ versions?

{% callout type="warning" title="Version Overload" %}
Consider using a version proxy or deprecating aggressively. Maintaining many versions indicates API instability.
{% /callout %}

### Q: Can I mix versioned and unversioned routes?

**No.** ServiceBuilder only accepts VersionedRoutes and provides automatic health/readiness:

```rust
let routes = VersionedApiBuilder::new()
    .add_version(ApiVersion::V1, |router| router.route("/users", get(handler)))
    .build_routes();

// ServiceBuilder automatically provides /health and /ready
// All business routes MUST be versioned
ServiceBuilder::new()
    .with_routes(routes)  // Only VersionedRoutes accepted
    .build()
    .serve()
    .await?;
```

{% callout type="note" title="Type Enforcement" %}
The framework **enforces** that all business logic goes through VersionedApiBuilder. Health and readiness endpoints are automatically provided by ServiceBuilder.
{% /callout %}

### Q: How do I version gRPC endpoints?

The framework currently only enforces HTTP versioning. For gRPC, use package versioning:

```protobuf
package myservice.v1;
package myservice.v2;
```

---

## Examples

See the following examples in the acton-service repository:
- `examples/users-api.rs` - Basic versioned API with multiple endpoint handlers
- `examples/simple-api.rs` - Simple API with basic routing patterns
- `examples/cedar-authz.rs` - Versioned API with authorization

---

## Summary

✅ **Type-safe versioning** - `VersionedRoutes` can only be created by `VersionedApiBuilder`

✅ **Compile-time safety** - Cannot accidentally construct `VersionedRoutes` (private fields)

✅ **Opaque type** - Prevents manual manipulation of versioned routes

✅ **Automatic health endpoints** - `ServiceBuilder` automatically adds `/health` and `/ready`

✅ **Set sunset dates when deprecating** - Clear migration timelines

✅ **Provide migration guidance** - Help developers upgrade

✅ **Monitor deprecated version usage** - Track adoption metrics

✅ **Maintain N and N-1 versions** - Gradual migration path

By leveraging **Rust's type system**, acton-service strongly encourages versioned APIs through opaque types and compile-time checks. Use `VersionedApiBuilder` for all business routes and `ServiceBuilder` for automatic health/readiness endpoints.
