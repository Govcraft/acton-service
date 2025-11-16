---
title: Feature Comparison
nextjs:
  metadata:
    title: Feature Comparison
    description: How acton-service compares to Axum, Actix-Web, Rocket, and other frameworks
---

How acton-service compares to popular Rust web frameworks and other ecosystems.

## Quick Comparison Table

| Feature | acton-service | Axum | Actix-Web | Rocket |
|---------|--------------|------|-----------|---------|
| **Type-enforced versioning** | ✅ Built-in | ❌ Manual | ❌ Manual | ❌ Manual |
| **Dual HTTP+gRPC** | ✅ Single port | ⚠️ Separate | ❌ No | ❌ No |
| **Auto health checks** | ✅ Built-in | ❌ Manual | ❌ Manual | ❌ Manual |
| **Observability** | ✅ Built-in | ⚠️ Manual | ⚠️ Manual | ❌ No |
| **Circuit breaker** | ✅ Built-in | ⚠️ Via Tower | ⚠️ Manual | ❌ No |
| **Connection pools** | ✅ Managed | ❌ Manual | ⚠️ Limited | ⚠️ Limited |
| **API deprecation** | ✅ RFC 8594 | ❌ Manual | ❌ Manual | ❌ Manual |
| **Configuration** | ✅ XDG + env | ❌ Manual | ⚠️ Basic | ✅ Good |
| **Learning curve** | Medium | Low | Medium | Low |
| **Flexibility** | Opinionated | Very flexible | Flexible | Opinionated |
| **Production focus** | ✅✅✅ | ⚠️ DIY | ✅✅ | ⚠️ Limited |

---

## Detailed Comparisons

### vs Axum

**Axum** is the foundation for acton-service. If you know Axum, you'll feel at home.

#### When to use Axum

- **Maximum flexibility**: You want total control over your architecture
- **Custom patterns**: You're building something unconventional
- **Learning**: You want to understand web frameworks deeply
- **Minimalism**: You prefer bare-bones libraries

```rust
// Axum: You wire everything yourself
use axum::{Router, routing::get};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/users", get(list_users));  // No versioning enforcement

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .unwrap();

    axum::serve(listener, app).await.unwrap();
}
```

#### When to use acton-service

- **Team enforcement**: You need the type system to prevent mistakes
- **Production defaults**: You want batteries-included best practices
- **Rapid development**: You need to ship features, not infrastructure
- **Consistency**: You're building multiple microservices

```rust
// acton-service: Best practices enforced
use acton_service::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .add_version(ApiVersion::V1, |router| {
            router.route("/users", get(list_users))  // Versioning enforced!
        })
        .build_routes();  // Health checks included automatically

    ServiceBuilder::new()
        .with_routes(routes)  // Config, tracing, all automatic
        .build()
        .serve()
        .await
}
```

{% callout type="note" %}
**Migration from Axum**: Easy - handlers are 100% compatible.
{% /callout %}

---

### vs Actix-Web

**Actix-Web** is a mature, high-performance framework with its own runtime.

#### When to use Actix-Web

- **Maximum performance**: Actix has historically had excellent benchmarks
- **Mature ecosystem**: Lots of middleware and extensions
- **Actor model**: You want actor-based concurrency
- **Flexibility**: Need framework flexibility with good defaults

```rust
// Actix-Web
use actix_web::{web, App, HttpServer};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .route("/users", web::get().to(list_users))  // No versioning
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
```

#### When to use acton-service

- **Tokio ecosystem**: You're using tokio-based libraries
- **Type safety**: You want compile-time guarantees
- **gRPC support**: You need HTTP+gRPC on same service
- **Enforced patterns**: You prefer opinionated frameworks

```rust
// acton-service: Type-safe, versioned, observable
#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .add_version(ApiVersion::V1, |router| {
            router.route("/users", get(list_users))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await?;

    Ok(())
}
```

{% callout type="note" %}
**Migration from Actix**: Moderate - handler patterns are different.
{% /callout %}

**Key differences**:
- acton-service uses tokio runtime, Actix uses its own
- acton-service enforces versioning, Actix doesn't
- acton-service has built-in gRPC support
- Actix has more third-party middleware available

---

### vs Rocket

**Rocket** is an opinionated, developer-friendly framework focused on ergonomics.

#### When to use Rocket

- **Developer experience**: Amazing compile-time checks and error messages
- **Code generation**: You love macros and derive attributes
- **Type-safe routing**: Built-in type-safe routing and validation
- **Beginner-friendly**: Great documentation and learning resources

```rust
// Rocket: Macro-based, beginner-friendly
#[macro_use] extern crate rocket;

#[get("/users")]
fn list_users() -> String {
    "Users".to_string()
}

#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount("/", routes![list_users])  // No versioning
}
```

#### When to use acton-service

- **Production features**: You need circuit breakers, observability, health checks
- **gRPC**: You need gRPC support
- **API evolution**: You need enforced versioning with deprecation
- **Microservices**: Building multiple services with consistent patterns

```rust
// acton-service: Production-ready with enforced patterns
#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .add_version(ApiVersion::V1, |router| {
            router.route("/users", get(list_users))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await?;

    Ok(())
}
```

{% callout type="note" %}
**Migration from Rocket**: Moderate - different programming model.
{% /callout %}

**Key differences**:
- Rocket uses macros extensively, acton-service uses builder pattern
- acton-service enforces versioning, Rocket doesn't
- acton-service has built-in gRPC, Rocket is HTTP-only
- Rocket has simpler learning curve for beginners

---

### vs Spring Boot (Java)

For those coming from Spring Boot or similar JVM frameworks.

#### Spring Boot equivalent features

| Spring Boot | acton-service |
|-------------|--------------|
| `@RestController` | `VersionedApiBuilder` |
| `@GetMapping("/users")` | `router.route("/users", get(handler))` |
| `application.yml` | `config.toml` + env vars |
| Spring Actuator | Built-in `/health` and `/ready` |
| Spring Cloud Circuit Breaker | Built-in resilience middleware |
| Spring Data JPA | SQLx (compile-time checked) |
| Sleuth/Zipkin | OpenTelemetry (built-in) |
| `@Valid` annotation | Manual validation (more control) |
| Auto-configuration | `ServiceBuilder` with sensible defaults |

```java
// Spring Boot
@RestController
@RequestMapping("/api/v1")
public class UserController {
    @GetMapping("/users")
    public List<User> listUsers() {
        return userService.findAll();
    }
}
```

```rust
// acton-service equivalent
let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router.route("/users", get(list_users))
    })
    .build_routes();
```

**Why switch from Spring Boot?**:
- **Performance**: 10-50x lower latency, 10x less memory
- **Binary size**: 15MB vs 150MB+ JAR files
- **Startup time**: Milliseconds vs seconds
- **Type safety**: Compile-time guarantees prevent entire classes of bugs
- **No GC pauses**: Predictable performance

**Tradeoffs**:
- Smaller ecosystem than Spring
- Steeper learning curve (Rust + async)
- Less magic, more explicit code

---

## Feature Breakdown

### API Versioning

| Framework | Approach | Enforcement |
|-----------|----------|-------------|
| **acton-service** | Type-enforced path versioning | ✅ Compile-time |
| Axum | Manual implementation | ❌ None |
| Actix-Web | Manual implementation | ❌ None |
| Rocket | Manual implementation | ❌ None |

{% callout type="warning" %}
**acton-service is the only Rust framework with compile-time enforced versioning.**
{% /callout %}

### Observability

| Framework | Tracing | Metrics | Logs | Effort |
|-----------|---------|---------|------|--------|
| **acton-service** | ✅ OpenTelemetry | ✅ OTLP | ✅ JSON | Zero |
| Axum | ⚠️ Manual setup | ⚠️ Manual | ⚠️ Manual | High |
| Actix-Web | ⚠️ Manual setup | ⚠️ Manual | ⚠️ Manual | High |
| Rocket | ⚠️ Limited | ❌ No | ✅ Basic | Medium |

### Health Checks

| Framework | Liveness | Readiness | Dependency Checks |
|-----------|----------|-----------|-------------------|
| **acton-service** | ✅ Built-in | ✅ Built-in | ✅ Automatic |
| Axum | ❌ Manual | ❌ Manual | ❌ Manual |
| Actix-Web | ❌ Manual | ❌ Manual | ❌ Manual |
| Rocket | ❌ Manual | ❌ Manual | ❌ Manual |

### Resilience Patterns

| Framework | Circuit Breaker | Retry | Bulkhead | Rate Limit |
|-----------|----------------|-------|----------|------------|
| **acton-service** | ✅ Built-in | ✅ Built-in | ✅ Built-in | ✅ Built-in |
| Axum | ⚠️ Tower | ⚠️ Tower | ⚠️ Tower | ⚠️ Tower |
| Actix-Web | ⚠️ Manual | ⚠️ Manual | ❌ No | ⚠️ Manual |
| Rocket | ❌ No | ❌ No | ❌ No | ❌ No |

## Performance Comparison

Based on typical CRUD operations (not official benchmarks):

| Framework | Req/sec | Latency (p50) | Latency (p99) | Memory |
|-----------|---------|---------------|---------------|---------|
| acton-service | ~100k | ~0.5ms | ~2ms | ~10MB |
| Axum | ~110k | ~0.4ms | ~1.8ms | ~8MB |
| Actix-Web | ~120k | ~0.3ms | ~1.5ms | ~12MB |
| Rocket | ~80k | ~0.8ms | ~3ms | ~15MB |

{% callout type="note" %}
Performance depends heavily on your application logic. These are rough estimates for simple CRUD operations.

**acton-service overhead**: Negligible (~5%) compared to raw Axum.
{% /callout %}

---

## When to Choose Each

### Choose acton-service if:
- ✅ Building production microservices at scale
- ✅ Working in a team that needs enforced patterns
- ✅ Need comprehensive out-of-the-box features
- ✅ Want to avoid "decision fatigue" on architecture
- ✅ Need both HTTP and gRPC support
- ✅ API versioning and evolution are critical
- ✅ You value fast time-to-production

### Choose Axum if:
- ✅ You want maximum flexibility
- ✅ Building something unconventional
- ✅ You enjoy making architectural decisions
- ✅ Need absolute minimal overhead
- ✅ Learning Rust web development

### Choose Actix-Web if:
- ✅ You need maximum performance
- ✅ You prefer actor-based concurrency
- ✅ You want a mature, stable framework
- ✅ Large ecosystem of middleware matters

### Choose Rocket if:
- ✅ Developer ergonomics are priority #1
- ✅ You're new to Rust web development
- ✅ You prefer macro-based APIs
- ✅ Building simpler web applications

---

## Migration Paths

### From Axum to acton-service

**Effort**: Low (1-2 hours)

Your handlers work unchanged. Just wrap them in versioning:

```rust
// Before (Axum)
let app = Router::new()
    .route("/users", get(list_users))
    .route("/users/{id}", get(get_user));

// After (acton-service)
let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router
            .route("/users", get(list_users))
            .route("/users/{id}", get(get_user))
    })
    .build_routes();

ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await?;
```

### From Actix-Web to acton-service

**Effort**: Medium (4-8 hours)

Handler signatures change, but logic remains the same:

```rust
// Before (Actix)
async fn get_user(path: web::Path<u64>) -> impl Responder {
    HttpResponse::Ok().json(user)
}

// After (acton-service)
async fn get_user(Path(id): Path<u64>) -> Json<User> {
    Json(user)
}
```

### From Rocket to acton-service

**Effort**: Medium (4-8 hours)

Different programming models, but concepts translate:

```rust
// Before (Rocket)
#[get("/users/<id>")]
fn get_user(id: u64) -> Json<User> {
    Json(user)
}

// After (acton-service)
async fn get_user(Path(id): Path<u64>) -> Json<User> {
    Json(user)
}

// In main:
router.route("/users/{id}", get(get_user))
```

---

## Decision Matrix

Answer these questions:

1. **Do you need enforced API versioning?**
   - Yes → acton-service
   - No → Axum or Actix-Web

2. **Are you building microservices for production?**
   - Yes → acton-service or Actix-Web
   - Prototyping → Axum or Rocket

3. **Do you need gRPC + HTTP on same service?**
   - Yes → acton-service
   - No → Any framework

4. **Is your team new to Rust?**
   - Yes → Rocket or acton-service (opinionated)
   - No → Axum (flexible)

5. **Do you want maximum performance?**
   - Yes → Actix-Web or Axum
   - Good enough → acton-service or Rocket

6. **Do you need observability out of the box?**
   - Yes → acton-service
   - Can implement → Any framework

---

## Summary

**acton-service is best for**:
- Production microservices with teams
- Services requiring both HTTP and gRPC
- APIs that will evolve over time
- Organizations wanting enforced best practices
- Fast time-to-production with comprehensive features

**Choose alternatives when**:
- You need maximum flexibility (Axum)
- You need maximum performance (Actix-Web)
- You're learning or prototyping (Rocket)
- You're building something unconventional

---

## Learn More

- [Quickstart Guide](/docs/quickstart) - Get started with acton-service
- [Tutorial](/docs/tutorial) - Build your first service
- [Feature Flags](/docs/features) - Understand features
- [Examples](https://github.com/govcraft/acton-service/tree/main/acton-service/examples) - Working code examples
