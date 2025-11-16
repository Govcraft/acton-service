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

{% callout type="warning" title="The Problem with Optional Versioning" %}
Optional versioning modules are frequently ignored by developers, leading to:

- Unversioned APIs that break consumers
- Inconsistent versioning patterns across services
- No deprecation headers
- Breaking changes without warning
{% /callout %}

**The Solution**: The framework uses **Rust's type system** to encourage versioning best practices:

- `VersionedRoutes` can ONLY be created by `VersionedApiBuilder` (opaque type with private fields)
- `ServiceBuilder` only accepts `VersionedRoutes` and provides automatic health/readiness endpoints
- Alternative: Use `Router::new().merge(routes)` for manual setup
- Compile-time safety prevents accidental route manipulation

---

## Key Features

The framework offers:

- **VersionedApiBuilder**: Type-safe builder that creates opaque `VersionedRoutes` containing only your versioned business routes
- **ServiceBuilder** or **Manual Router**: Both require you to add health/readiness endpoints (framework does not auto-add them)
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

    // ServiceBuilder provides automatic health/readiness endpoints
    // and proper middleware stack
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
- `/health` → Health check
- `/ready` → Readiness check
- `/api/v1/users` → V1 handler
- `/api/v2/users` → V2 handler

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

## Best Practices

### 1. Use VersionedApiBuilder for All Business Routes

The framework's type system prevents manual construction of `VersionedRoutes`:

❌ **WON'T COMPILE** - Cannot create VersionedRoutes manually:
```rust
// ❌ This will NOT compile - VersionedRoutes has private fields!
let routes = VersionedRoutes { router: Router::new() };  // ERROR: private field
```

✅ **RECOMMENDED** - Use VersionedApiBuilder and add health/readiness:
```rust
let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router.route("/users", get(handler))
    })
    .build_routes();  // Returns VersionedRoutes

let app = Router::new()
    .merge(routes)  // VersionedRoutes can be merged into Router
    .route("/health", get(health))
    .route("/ready", get(readiness))
    .with_state(state);

Server::new(config).serve(app).await?;
```

### 2. Always Add Health and Readiness Endpoints

```rust
let app = Router::new()
    .merge(versioned_routes)
    .route("/health", get(health))
    .route("/ready", get(readiness))
    .with_state(state);
```

### 3. Set Sunset Dates

Always include sunset dates when deprecating:

```rust
DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
    .with_sunset_date("2026-06-30T23:59:59Z")  // RFC 3339 format
    .with_message("Reason for deprecation")
```

### 4. Provide Clear Migration Messages

```rust
// ❌ BAD
.with_message("This version is deprecated")

// ✅ GOOD
.with_message("V1 is deprecated. Migrate to V2 for improved pagination. See migration guide: https://docs.example.com/v1-to-v2")
```

### 5. Maintain At Least Two Versions

Keep N and N-1 versions active during migration:

```rust
VersionedApiBuilder::new()
    .add_version_deprecated(ApiVersion::V2, |router| {...}, deprecation)  // Old version
    .add_version(ApiVersion::V3, |router| {...})  // Current version
    .build_routes()
```

---

## Monitoring Deprecation

### Logging Deprecated API Usage

```rust
// Add middleware to log deprecated version usage
async fn log_api_version(req: Request, next: Next) -> Response {
    let version = extract_version_from_path(req.uri().path());
    if let Some(version) = version {
        if version.is_deprecated(ApiVersion::V3) {
            warn!(
                path = %req.uri().path(),
                version = %version,
                "Deprecated API version accessed"
            );
        }
    }
    next.run(req).await
}
```

### Metrics

Track version usage with OpenTelemetry:

```rust
use opentelemetry::metrics::Counter;

let version_counter: Counter<u64> = meter
    .u64_counter("api.version.requests")
    .build();

// In middleware
version_counter.add(1, &[
    KeyValue::new("version", version.to_string()),
    KeyValue::new("deprecated", version.is_deprecated(latest).to_string()),
]);
```

---

## Migration Checklist

When deprecating an API version:

- [ ] Create new version with changes
- [ ] Add deprecation info with sunset date (6+ months out)
- [ ] Update documentation with migration guide
- [ ] Add monitoring for deprecated version usage
- [ ] Notify API consumers via:
  - [ ] Email/Slack announcements
  - [ ] API changelog
  - [ ] Deprecation headers (automatic via framework)
- [ ] Monitor usage over time
- [ ] Remove version after sunset date with zero usage

---

## FAQ

### Q: Can I use plain Router without VersionedApiBuilder?

**Yes, but not recommended.** While you can use plain `Router::new()` and skip versioning entirely, this is strongly discouraged. The framework encourages versioned routes through:

1. **VersionedRoutes opaque type**: Can ONLY be created via `VersionedApiBuilder::build_routes()`
2. **Compile-time safety**: You cannot accidentally construct `VersionedRoutes` manually

```rust
// ❌ This won't compile - VersionedRoutes has private fields
let routes = VersionedRoutes { router: Router::new() };  // ERROR

// ✅ Correct - use VersionedApiBuilder
let routes = VersionedApiBuilder::new()
    .add_version(ApiVersion::V1, |router| router.route("/users", get(handler)))
    .build_routes();

let app = Router::new()
    .merge(routes)
    .route("/health", get(health))
    .with_state(state);
```

### Q: How many versions should I maintain?

**Minimum**: 2 versions (N and N-1)
**Recommended**: 2-3 versions with clear sunset dates

### Q: What if I need to support 5+ versions?

{% callout type="warning" title="Version Overload" %}
Consider using a version proxy or deprecating aggressively. Maintaining many versions indicates API instability.
{% /callout %}

### Q: Can I mix versioned and unversioned routes?

**Yes, but not recommended**:
```rust
let versioned_routes = VersionedApiBuilder::new()
    .add_version(ApiVersion::V1, |router| router.route("/users", get(handler)))
    .build_routes();

// You CAN mix, but it defeats the purpose of versioning
let app = Router::new()
    .merge(versioned_routes)  // Versioned routes
    .route("/legacy", get(legacy_handler))  // Unversioned route (discouraged)
    .route("/health", get(health))  // OK - health/readiness are unversioned
    .route("/ready", get(readiness));
```

{% callout type="note" title="Recommendation" %}
Keep all business logic in versioned routes. Only health/readiness should be unversioned.
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
✅ **Explicit health/readiness** - Always add health and readiness endpoints to your router
✅ **Set sunset dates when deprecating** - Clear migration timelines
✅ **Provide migration guidance** - Help developers upgrade
✅ **Monitor deprecated version usage** - Track adoption metrics
✅ **Maintain N and N-1 versions** - Gradual migration path

By leveraging **Rust's type system**, acton-service strongly encourages versioned APIs through opaque types and compile-time checks. Use `VersionedApiBuilder` for all business routes and add health/readiness explicitly.
