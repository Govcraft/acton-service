---
title: OpenAPI/Swagger
nextjs:
  metadata:
    title: OpenAPI/Swagger
    description: OpenAPI specification generation with multiple UI options (Swagger UI, RapiDoc, ReDoc) and multi-version API documentation support
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


Generate interactive API documentation with OpenAPI specifications and multiple UI options for comprehensive API exploration.

---

## Overview

acton-service provides built-in OpenAPI/Swagger documentation support with automatic schema generation, multiple UI options, and first-class support for versioned APIs. Documentation is automatically generated from your route definitions and type annotations.

## Installation

Enable the OpenAPI feature:

```toml
[dependencies]
{% $dep.openapi %}
```

## Configuration

{% callout type="note" title="OpenAPI is configured in code, not TOML" %}
There is no `[openapi]` section in `config.toml` and no `ACTON_OPENAPI_*` environment variables. OpenAPI documentation is assembled entirely in Rust: you describe the spec with `utoipa`, refine its metadata with `OpenApiBuilder`, and mount whichever UI you want onto your `Router`. Nothing is auto-served — if you don't mount a UI route, no documentation endpoint exists.
{% /callout %}

Use `OpenApiBuilder` to set the API metadata (title, version, description, contact, license, servers) on a spec produced by the `#[derive(OpenApi)]` macro:

```rust
use acton_service::openapi::OpenApiBuilder;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(paths(list_users, create_user), components(schemas(User)))]
struct ApiDoc;

let spec = OpenApiBuilder::new(ApiDoc::openapi())
    .title("My Service API")
    .version("1.0.0")
    .description("Production API service")
    .contact("API Team", "api@example.com")
    .license("MIT", Some("https://opensource.org/licenses/MIT".to_string()))
    .server("https://api.example.com", Some("Production".to_string()))
    .build();
```

`build()` returns a `utoipa::openapi::OpenApi` that you hand to one of the UI helpers below.

## Basic Usage

Enable OpenAPI documentation in your service:

```rust
use acton_service::prelude::*;
use utoipa::{OpenApi, ToSchema};

#[derive(Serialize, Deserialize, ToSchema)]
struct User {
    id: i64,
    name: String,
    email: String,
}

#[derive(Serialize, Deserialize, ToSchema)]
struct CreateUserRequest {
    name: String,
    email: String,
}

/// List all users
#[utoipa::path(
    get,
    path = "/users",
    responses(
        (status = 200, description = "List of users", body = Vec<User>)
    ),
    tag = "users"
)]
async fn list_users(
    State(state): State<AppState>
) -> Result<Json<Vec<User>>> {
    let db = state
        .db()
        .await
        .ok_or_else(|| Error::Internal("database unavailable".into()))?;
    let users = sqlx::query_as!(User, "SELECT id, name, email FROM users")
        .fetch_all(&db)
        .await?;
    Ok(Json(users))
}

/// Create a new user
#[utoipa::path(
    post,
    path = "/users",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created", body = User),
        (status = 400, description = "Invalid request")
    ),
    tag = "users"
)]
async fn create_user(
    State(state): State<AppState>,
    Json(request): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<User>)> {
    let db = state
        .db()
        .await
        .ok_or_else(|| Error::Internal("database unavailable".into()))?;
    let user = sqlx::query_as!(
        User,
        "INSERT INTO users (name, email) VALUES ($1, $2) RETURNING *",
        request.name,
        request.email
    )
    .fetch_one(&db)
    .await?;

    Ok((StatusCode::CREATED, Json(user)))
}

#[derive(OpenApi)]
#[openapi(
    paths(list_users, create_user),
    components(schemas(User, CreateUserRequest)),
    tags(
        (name = "users", description = "User management endpoints")
    )
)]
struct ApiDoc;

#[tokio::main]
async fn main() -> Result<()> {
    use acton_service::openapi::SwaggerUI;

    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router
                .route("/users", get(list_users))
                .route("/users", post(create_user))
                .merge(SwaggerUI::with_spec("/swagger-ui", ApiDoc::openapi()))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

`SwaggerUI::with_spec(path, spec)` registers two things: the UI itself at `path`, and the raw specification at `/api-docs/openapi.json`. Both are relative to wherever you merge the returned `Router` — in the example above they are merged inside the `V1` version router, so they land under the `/api/v1` prefix:

```bash
# Swagger UI
http://localhost:8080/api/v1/swagger-ui

# OpenAPI specification (the URL SwaggerUI registers for the spec)
http://localhost:8080/api/v1/api-docs/openapi.json
```

Merge it into a top-level `Router` instead and the same routes appear at `/swagger-ui` and `/api-docs/openapi.json`.

> **Note on OpenAPI Integration**: OpenAPI/Swagger UI integration is handled via the `Router::merge()` method combined with `SwaggerUI::with_spec()`, not through `ServiceBuilder` methods. This allows you to flexibly place OpenAPI documentation routes within your versioned API structure. The `SwaggerUI` router can be merged into any `Router` instance during the version building phase.

## Multiple UI Options

The `openapi` module exports three UI helpers, but they are **not** interchangeable, and none of them is mounted for you:

| Helper | Signature | Returns | You must |
| --- | --- | --- | --- |
| `SwaggerUI` | `with_spec(path, spec)` / `with_versions(path, versions)` | `Router` | `merge()` it into your router |
| `RapiDoc` | `html(spec_url)` | `String` (HTML) | Add your own route that returns it |
| `ReDoc` | `html(spec_url)` | `String` (HTML) | Add your own route that returns it |

There is no configuration switch that turns these on. If you want a UI, you wire its route yourself.

### Swagger UI

The only helper that produces a mountable `Router`. `with_spec()` serves the UI **and** the spec:

```rust
use acton_service::openapi::SwaggerUI;

let router = Router::new()
    .route("/users", get(list_users))
    .merge(SwaggerUI::with_spec("/swagger-ui", ApiDoc::openapi()));
```

**Features:**
- Interactive API testing
- Request/response examples
- Authentication support
- Model schema visualization

### RapiDoc

`RapiDoc::html(spec_url)` returns an HTML **string** pointing at a spec URL you already serve. Wire it to a route yourself:

```rust
use acton_service::openapi::{RapiDoc, SwaggerUI};
use axum::response::Html;

let router = Router::new()
    .route("/users", get(list_users))
    // SwaggerUI::with_spec also publishes the spec at /api-docs/openapi.json,
    // which is what RapiDoc renders below.
    .merge(SwaggerUI::with_spec("/swagger-ui", ApiDoc::openapi()))
    .route(
        "/rapidoc",
        get(|| async { Html(RapiDoc::html("/api-docs/openapi.json")) }),
    );
```

**Features:**
- Responsive design
- Three layout modes (column, row, focused)
- Syntax highlighting
- Code samples in multiple languages

### ReDoc

`ReDoc::html(spec_url)` works exactly the same way — an HTML string you serve from a route of your choosing:

```rust
use acton_service::openapi::{ReDoc, SwaggerUI};
use axum::response::Html;

let router = Router::new()
    .route("/users", get(list_users))
    .merge(SwaggerUI::with_spec("/swagger-ui", ApiDoc::openapi()))
    .route(
        "/redoc",
        get(|| async { Html(ReDoc::html("/api-docs/openapi.json")) }),
    );
```

**Features:**
- Responsive three-panel design
- Search functionality
- Downloadable OpenAPI spec
- Menu/navigation sidebar

### Serving Several UIs

Because each UI is just a router merge or a route, serving all three is a matter of wiring all three against the same spec URL:

```rust
use acton_service::openapi::{RapiDoc, ReDoc, SwaggerUI};
use axum::response::Html;

const SPEC_URL: &str = "/api-docs/openapi.json";

let router = Router::new()
    .route("/users", get(list_users))
    .merge(SwaggerUI::with_spec("/swagger-ui", ApiDoc::openapi()))
    .route("/rapidoc", get(|| async { Html(RapiDoc::html(SPEC_URL)) }))
    .route("/redoc", get(|| async { Html(ReDoc::html(SPEC_URL)) }));
```

## Multi-Version Documentation

Document multiple API versions in a single specification:

```rust
#[derive(OpenApi)]
#[openapi(
    info(
        title = "My Service API",
        version = "1.0.0",
    ),
    paths(
        v1::list_users,
        v1::create_user,
    ),
    components(schemas(v1::User, v1::CreateUserRequest)),
    tags(
        (name = "v1", description = "API Version 1 (deprecated)")
    )
)]
struct ApiDocV1;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "My Service API",
        version = "2.0.0",
    ),
    paths(
        v2::list_users,
        v2::create_user,
        v2::update_user,
    ),
    components(schemas(v2::User, v2::CreateUserRequest, v2::UpdateUserRequest)),
    tags(
        (name = "v2", description = "API Version 2 (current)")
    )
)]
struct ApiDocV2;

#[tokio::main]
async fn main() -> Result<()> {
    use acton_service::openapi::SwaggerUI;

    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router
                .route("/users", get(v1::list_users))
                .route("/users", post(v1::create_user))
                .merge(SwaggerUI::with_spec("/swagger-ui/v1", ApiDocV1::openapi()))
        })
        .add_version(ApiVersion::V2, |router| {
            router
                .route("/users", get(v2::list_users))
                .route("/users", post(v2::create_user))
                .route("/users/{id}", put(v2::update_user))
                .merge(SwaggerUI::with_spec("/swagger-ui/v2", ApiDocV2::openapi()))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await
}
```

Each `with_spec()` call publishes its UI at the given path and its spec at `/api-docs/openapi.json`, both under the version prefix they were merged into:

```bash
# V1 documentation
http://localhost:8080/api/v1/swagger-ui/v1

# V2 documentation
http://localhost:8080/api/v2/swagger-ui/v2

# Version-specific specs
http://localhost:8080/api/v1/api-docs/openapi.json
http://localhost:8080/api/v2/api-docs/openapi.json
```

Alternatively, mount a single Swagger UI at the top level that offers a version dropdown, using `SwaggerUI::with_versions()` — it takes an owned `String` path and a list of `(spec_url, spec)` pairs, and returns a `Router`:

```rust
let docs = SwaggerUI::with_versions(
    "/swagger-ui".to_string(),
    vec![
        ("/api-docs/v1/openapi.json".to_string(), ApiDocV1::openapi()),
        ("/api-docs/v2/openapi.json".to_string(), ApiDocV2::openapi()),
    ],
);

let app = Router::new().merge(routes).merge(docs);
```

## Authentication Documentation

Document authentication schemes:

```rust
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};

#[derive(OpenApi)]
#[openapi(
    paths(list_users, create_user),
    components(schemas(User)),
    modifiers(&SecurityAddon)
)]
struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .build()
                ),
            )
        }
    }
}

/// List users (requires authentication)
#[utoipa::path(
    get,
    path = "/users",
    responses(
        (status = 200, description = "List of users", body = Vec<User>),
        (status = 401, description = "Unauthorized")
    ),
    security(
        ("bearer_auth" = [])
    ),
    tag = "users"
)]
async fn list_users(
    claims: JwtClaims,
    State(state): State<AppState>
) -> Result<Json<Vec<User>>> {
    // ...
}
```

## Advanced Schema Annotations

### Response Examples

```rust
/// Get user by ID
#[utoipa::path(
    get,
    path = "/users/{id}",
    params(
        ("id" = i64, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User found", body = User,
            example = json!({
                "id": 1,
                "name": "John Doe",
                "email": "john@example.com"
            })
        ),
        (status = 404, description = "User not found")
    )
)]
async fn get_user(
    Path(id): Path<i64>,
    State(state): State<AppState>
) -> Result<Json<User>> {
    // ...
}
```

### Complex Schema Types

```rust
#[derive(Serialize, Deserialize, ToSchema)]
struct PaginatedResponse<T> {
    #[schema(example = json!([]))]
    data: Vec<T>,

    #[schema(example = 1)]
    page: u32,

    #[schema(example = 10)]
    per_page: u32,

    #[schema(example = 100)]
    total: u64,
}

#[utoipa::path(
    get,
    path = "/users",
    params(
        ("page" = Option<u32>, Query, description = "Page number"),
        ("per_page" = Option<u32>, Query, description = "Items per page")
    ),
    responses(
        (status = 200, description = "Paginated users", body = PaginatedResponse<User>)
    )
)]
async fn list_users_paginated(
    Query(params): Query<PaginationParams>,
    State(state): State<AppState>
) -> Result<Json<PaginatedResponse<User>>> {
    // ...
}
```

### Validation Constraints

```rust
use validator::Validate;

#[derive(Serialize, Deserialize, ToSchema, Validate)]
struct CreateUserRequest {
    #[validate(length(min = 1, max = 100))]
    #[schema(example = "John Doe", min_length = 1, max_length = 100)]
    name: String,

    #[validate(email)]
    #[schema(example = "john@example.com", format = "email")]
    email: String,

    #[validate(range(min = 18, max = 120))]
    #[schema(example = 25, minimum = 18, maximum = 120)]
    age: u8,
}
```

## Production Configuration

### Disable in Production

Documentation routes exist only if you mount them, so "disabling docs" means not merging the UI router. Gate the merge on whatever signal you already have — for example `[service].environment` from your config, or a Cargo feature:

```rust
use acton_service::openapi::SwaggerUI;

let config = Config::<()>::load()?;

let mut router = Router::new().route("/users", get(list_users));

// Only expose docs outside production.
if config.service.environment != "production" {
    router = router.merge(SwaggerUI::with_spec("/swagger-ui", ApiDoc::openapi()));
}
```

Compile-time gating keeps the UI and spec out of the production binary entirely:

```rust
#[cfg(debug_assertions)]
let router = router.merge(SwaggerUI::with_spec("/swagger-ui", ApiDoc::openapi()));
```

### Custom Documentation URL

Serve documentation at a custom path:

```rust
use acton_service::openapi::SwaggerUI;

let routes = VersionedApiBuilder::new()
    .add_version(ApiVersion::V1, |router| {
        router
            .route("/users", get(list_users))
            .merge(SwaggerUI::with_spec("/docs", ApiDoc::openapi()))
    })
    .build_routes();

ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await
```

### External Specification

Load OpenAPI spec from external file:

```rust
use acton_service::openapi::SwaggerUI;

let spec = std::fs::read_to_string("openapi.yaml")?;
let openapi: utoipa::openapi::OpenApi = serde_yaml::from_str(&spec)?;

let routes = VersionedApiBuilder::new()
    .add_version(ApiVersion::V1, |router| {
        router
            .route("/users", get(list_users))
            .merge(SwaggerUI::with_spec("/swagger-ui", openapi))
    })
    .build_routes();

ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await
```

## Best Practices

### Document All Endpoints

```rust
// ✅ Good - comprehensive documentation
#[utoipa::path(
    post,
    path = "/users",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created", body = User),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Unauthorized"),
        (status = 409, description = "User already exists")
    ),
    tag = "users"
)]
```

### Use Descriptive Examples

```rust
#[derive(ToSchema)]
struct User {
    #[schema(example = 42)]
    id: i64,

    #[schema(example = "John Doe")]
    name: String,

    #[schema(example = "john.doe@example.com", format = "email")]
    email: String,
}
```

### Group Related Endpoints

```rust
#[openapi(
    paths(
        users::list_users,
        users::create_user,
        users::get_user,
        users::update_user,
        users::delete_user,
    ),
    tags(
        (name = "users", description = "User management operations"),
        (name = "auth", description = "Authentication endpoints")
    )
)]
```

### Version Your Schemas

```rust
// ✅ Good - versioned schemas
mod v1 {
    #[derive(ToSchema)]
    struct User {
        id: i64,
        name: String,
    }
}

mod v2 {
    #[derive(ToSchema)]
    struct User {
        id: i64,
        name: String,
        email: String,  // New field in V2
    }
}
```

## Related Features

- **[API Versioning](/docs/api-versioning)** - Type-safe API versioning
- **[Authentication](/docs/authentication)** - JWT authentication documentation
- **[Configuration](/docs/configuration)** - Service configuration reference (note: OpenAPI has no config section — it is wired in code)
