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
acton-service = { version = "0.2", features = ["openapi", "http", "observability"] }
```

## Configuration

Configure OpenAPI documentation in your service configuration:

```toml
# ~/.config/acton-service/my-service/config.toml
[openapi]
enabled = true
title = "My Service API"
version = "1.0.0"
description = "Production API service"
contact_name = "API Team"
contact_email = "api@example.com"
license_name = "MIT"

# UI preferences
ui = "swagger"  # Options: swagger, rapidoc, redoc, all
serve_spec = true  # Serve OpenAPI JSON at /openapi.json
```

### Environment Variables

```bash
ACTON_OPENAPI_ENABLED=true
ACTON_OPENAPI_UI=swagger
```

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
    let db = state.database()?;
    let users = sqlx::query_as!(User, "SELECT id, name, email FROM users")
        .fetch_all(db)
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
    let db = state.database()?;
    let user = sqlx::query_as!(
        User,
        "INSERT INTO users (name, email) VALUES ($1, $2) RETURNING *",
        request.name,
        request.email
    )
    .fetch_one(db)
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

Access the documentation:

```bash
# Swagger UI
http://localhost:8080/swagger-ui

# OpenAPI specification
http://localhost:8080/openapi.json
```

> **Note on OpenAPI Integration**: OpenAPI/Swagger UI integration is handled via the `Router::merge()` method combined with `SwaggerUI::with_spec()`, not through `ServiceBuilder` methods. This allows you to flexibly place OpenAPI documentation routes within your versioned API structure. The `SwaggerUI` router can be merged into any `Router` instance during the version building phase.

## Multiple UI Options

acton-service supports three popular OpenAPI UI frameworks:

### Swagger UI

The traditional, feature-rich OpenAPI UI:

```toml
[openapi]
ui = "swagger"
```

```bash
http://localhost:8080/swagger-ui
```

**Features:**
- Interactive API testing
- Request/response examples
- Authentication support
- Model schema visualization

### RapiDoc

Modern, customizable API documentation:

```toml
[openapi]
ui = "rapidoc"
```

```bash
http://localhost:8080/rapidoc
```

**Features:**
- Responsive design
- Three layout modes (column, row, focused)
- Syntax highlighting
- Code samples in multiple languages

### ReDoc

Clean, three-panel documentation:

```toml
[openapi]
ui = "redoc"
```

```bash
http://localhost:8080/redoc
```

**Features:**
- Responsive three-panel design
- Search functionality
- Downloadable OpenAPI spec
- Menu/navigation sidebar

### All UI Options

Serve all three UIs simultaneously:

```toml
[openapi]
ui = "all"
```

```bash
http://localhost:8080/swagger-ui
http://localhost:8080/rapidoc
http://localhost:8080/redoc
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
                .route("/users/:id", put(v2::update_user))
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

Access version-specific documentation:

```bash
# V1 documentation
http://localhost:8080/swagger-ui/v1

# V2 documentation
http://localhost:8080/swagger-ui/v2

# Version-specific specs
http://localhost:8080/openapi/v1.json
http://localhost:8080/openapi/v2.json
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

Disable OpenAPI documentation in production environments:

```toml
[openapi]
enabled = false  # Disable docs in production
```

Or use environment-based configuration:

```bash
# Development
ACTON_OPENAPI_ENABLED=true cargo run

# Production
ACTON_OPENAPI_ENABLED=false cargo run
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
- **[Configuration](/docs/configuration)** - OpenAPI configuration options
