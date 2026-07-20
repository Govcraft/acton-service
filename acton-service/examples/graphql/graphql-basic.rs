//! Versioned GraphQL example.
//!
//! Demonstrates:
//! * Registering two GraphQL schemas (V1, V2) under `/api/v1/graphql` and
//!   `/api/v2/graphql` via [`VersionedGraphQLBuilder`].
//! * Accessing authenticated claims from inside a resolver via the
//!   [`GraphQLContextExt`] trait.
//! * Cedar policy authorization at the resolver level via
//!   [`CedarResolverCheck`] (only compiled when the `graphql-cedar` feature is
//!   active).
//!
//! Run with:
//!   cargo run --example graphql-basic --features graphql,auth
//!
//! Then:
//!   # Browser
//!   open http://localhost:8080/api/v1/graphql        # GraphiQL UI
//!
//!   # Query
//!   curl -X POST http://localhost:8080/api/v1/graphql \
//!        -H 'content-type: application/json' \
//!        -d '{"query":"{ hello }"}'

use acton_service::graphql::{GraphQLContextExt, VersionedGraphQLBuilder};
use acton_service::prelude::*;
use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema};

#[cfg(feature = "graphql-cedar")]
use acton_service::graphql::CedarResolverCheck;

struct QueryV1;

#[Object]
impl QueryV1 {
    /// Unauthenticated hello world.
    async fn hello(&self) -> &'static str {
        "Hello from GraphQL V1!"
    }

    /// Returns the subject of the authenticated principal, or "anonymous".
    async fn whoami(&self, ctx: &Context<'_>) -> String {
        ctx.claims()
            .map(|c| c.sub.clone())
            .unwrap_or_else(|| "anonymous".to_string())
    }
}

struct QueryV2;

#[Object]
impl QueryV2 {
    /// V2 enhancement: returns roles in addition to the subject.
    async fn me(&self, ctx: &Context<'_>) -> async_graphql::Result<Me> {
        let claims = ctx.require_claims()?;
        Ok(Me {
            sub: claims.sub.clone(),
            roles: claims.roles.clone(),
        })
    }

    /// Reads a "document". Under the `graphql-cedar` feature this is guarded
    /// by a Cedar policy: the subject must be allowed by an
    /// `Action::"readDocument"` policy on `Document::"<id>"`.
    async fn document(&self, _ctx: &Context<'_>, id: String) -> async_graphql::Result<String> {
        #[cfg(feature = "graphql-cedar")]
        CedarResolverCheck::for_context(_ctx)?
            .with_action("readDocument")
            .with_resource_type("Document")
            .with_resource_id(&id)
            .authorize()
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        Ok(format!("Document {} contents", id))
    }
}

#[derive(async_graphql::SimpleObject)]
struct Me {
    sub: String,
    roles: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |r| r)
        .add_version(ApiVersion::V2, |r| r)
        .build_routes();

    let schema_v1 = Schema::build(QueryV1, EmptyMutation, EmptySubscription).finish();
    let schema_v2 = Schema::build(QueryV2, EmptyMutation, EmptySubscription).finish();

    let graphql = VersionedGraphQLBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, schema_v1)
        .add_version(ApiVersion::V2, schema_v2)
        .build();

    ServiceBuilder::new()
        .with_routes(routes)
        .with_versioned_graphql(graphql)
        .build()
        .serve()
        .await?;

    Ok(())
}
