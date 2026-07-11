---
title: GraphQL Guide
nextjs:
  metadata:
    title: GraphQL Guide
    description: Mount versioned GraphQL schemas under the same router as REST and gRPC, with authentication, Cedar authorization, and GraphiQL.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) and [API Versioning](/docs/api-versioning) before reading this guide. See also the [gRPC Guide](/docs/grpc-guide) for the sibling transport story.
{% /callout %}

---

acton-service ships a GraphQL transport built on
[`async-graphql`](https://docs.rs/async-graphql) and `async-graphql-axum`.
Schemas are mounted underneath the framework's versioned router so they share
auth, tracing, rate limiting, and Cedar middleware with REST endpoints.

---

## Overview

- **Versioned by path** — register one schema per `ApiVersion`; mounted at
  `/{base}/v{n}/graphql`.
- **GraphiQL** — `GET` on the endpoint serves the GraphiQL UI by default.
- **Auth propagation** — `Claims` placed in the Axum request `Extensions` by
  PASETO/JWT middleware are forwarded into the resolver `Context`.
- **Cedar authorization** — resolver-level checks via
  `CedarResolverCheck` reuse the same `CedarAuthz` instance used by HTTP.
- **Deprecation** — register deprecated versions with `DeprecationInfo` to
  emit the standard `Deprecation`, `Sunset`, `Link`, and `Warning` headers.

> Subscriptions over WebSocket are not yet wired in v1.

---

## Feature flags

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["graphql"] }
# Resolver-level Cedar checks:
# acton-service = { version = "{% version() %}", features = ["graphql-cedar"] }

# async-graphql derive macros emit paths to the crate by name, so add it as a
# direct dependency too:
async-graphql = "7.2"
```

---

## Quick start

```rust
use acton_service::prelude::*;
use acton_service::graphql::{GraphQLContextExt, VersionedGraphQLBuilder};
use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema};

struct Query;

#[Object]
impl Query {
    async fn hello(&self) -> &'static str { "world" }

    async fn whoami(&self, ctx: &Context<'_>) -> String {
        ctx.claims().map(|c| c.sub.clone()).unwrap_or("anon".into())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |r| r)
        .build_routes();

    let schema = Schema::build(Query, EmptyMutation, EmptySubscription).finish();
    let graphql = VersionedGraphQLBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, schema)
        .build();

    ServiceBuilder::new()
        .with_routes(routes)
        .with_versioned_graphql(graphql)
        .build()
        .serve()
        .await?;
    Ok(())
}
```

The schema lands at `POST /api/v1/graphql`; GraphiQL is served on `GET`.

---

## Versioning

`VersionedGraphQLBuilder::add_version` binds a schema to an `ApiVersion`.
Register multiple versions to evolve your API:

```rust
let graphql = VersionedGraphQLBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, schema_v1)
    .add_version(ApiVersion::V2, schema_v2)
    .build();
```

Mark a version deprecated to inject RFC 8594 headers:

```rust
use acton_service::versioning::DeprecationInfo;

let deprecation = DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
    .with_sunset_date("2027-01-01T00:00:00Z")
    .with_message("upgrade to V2");

let graphql = VersionedGraphQLBuilder::new()
    .with_base_path("/api")
    .add_version_deprecated(ApiVersion::V1, schema_v1, deprecation)
    .add_version(ApiVersion::V2, schema_v2)
    .build();
```

---

## Authentication

Any middleware that inserts a `Claims` into the request extensions will be
visible to resolvers. The framework's PASETO and JWT middleware do this
automatically; tests and custom middleware can do the same with
`request.extensions_mut().insert(claims)`.

Inside a resolver:

```rust
use acton_service::graphql::GraphQLContextExt;

async fn me(ctx: &Context<'_>) -> async_graphql::Result<String> {
    let claims = ctx.require_claims()?;       // -> async_graphql::Error if anonymous
    Ok(claims.sub.clone())
}
```

`Context::claims()` returns `Option<&Claims>`; `require_claims()` returns a
GraphQL error formatted as `Unauthorized` when claims are missing.

---

## Cedar resolver authorization

Enable the `graphql-cedar` feature, configure Cedar on the `ServiceBuilder`
(or via `[cedar]` in `config.toml`), then call into the policy engine from
inside a resolver:

```rust
use acton_service::graphql::CedarResolverCheck;

async fn document(ctx: &Context<'_>, id: String) -> async_graphql::Result<String> {
    CedarResolverCheck::for_context(ctx)?
        .with_action("readDocument")
        .with_resource_type("Document")
        .with_resource_id(&id)
        .authorize()
        .await
        .map_err(|e| async_graphql::Error::new(e.to_string()))?;
    Ok(format!("Document {} contents", id))
}
```

The check uses the same `CedarAuthz` instance the HTTP and gRPC middleware
use, so policies stay centralized.

---

## Configuration

```toml
[graphql]
enabled = true
graphiql_enabled = true
introspection_enabled = true
# max_query_depth = 12
# max_query_complexity = 200
```

Apply the depth/complexity limits to a `SchemaBuilder` via
`apply_config_to_builder`:

```rust
use acton_service::graphql::apply_config_to_builder;

let builder = Schema::build(Query, EmptyMutation, EmptySubscription);
let cfg = config.graphql.clone().unwrap_or_default();
let schema = apply_config_to_builder(builder, &cfg).finish();
```

---

## OpenAPI integration

Surface GraphQL endpoints in your Swagger/ReDoc UI:

```rust
use acton_service::openapi::graphql::add_paths_from_versioned;

let spec = MyApiDoc::openapi();
let spec = add_paths_from_versioned(spec, &graphql);
```

Each registered version is added as a `POST /{base}/v{n}/graphql` entry with
generic JSON request and response bodies.

---

## CLI scaffolding

```bash
acton service new my-svc --graphql           # new project with GraphQL wired in
acton service add graphql                    # retrofit an existing project
acton service add graphql --cedar            # include a Cedar-protected example
```

The scaffold generates `src/graphql.rs` with a sample `Query`, exposes a
`build()` function that returns a `VersionedGraphQL`, and updates `main.rs`
to call `ServiceBuilder::with_versioned_graphql`.

---

## See also

- [API Versioning](/docs/api-versioning) — how `ApiVersion` and
  `DeprecationInfo` work across REST, gRPC, and GraphQL.
- [Cedar Authorization](/docs/cedar-auth) — policy authoring and HTTP
  middleware setup.
- [OpenAPI/Swagger](/docs/openapi) — how schemas are exposed via Swagger UI.
