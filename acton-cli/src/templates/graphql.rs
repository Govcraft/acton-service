//! GraphQL module scaffolding for `acton service new --graphql` and
//! `acton service add graphql`.
//!
//! Produces a minimal `src/graphql.rs` that:
//!  * Defines a sample `Query` root resolver
//!  * Builds a `Schema` with `EmptyMutation`/`EmptySubscription`
//!  * Returns a `VersionedGraphQL` that the generated `main.rs` plugs into
//!    `ServiceBuilder::with_versioned_graphql`.
//!
//! Cedar-aware variants additionally show how to call
//! [`acton_service::graphql::CedarResolverCheck`] from inside a resolver.

/// Returns the full source of a `src/graphql.rs` to drop into a new project.
pub fn generate_module() -> String {
    generate_module_with_cedar(false)
}

/// Variant that emits a Cedar-protected example field. Used by
/// `acton service add graphql --cedar`.
pub fn generate_module_with_cedar(cedar: bool) -> String {
    let cedar_field = if cedar {
        r#"

    /// Cedar-protected example. Requires policy permitting
    /// `Action::"readDocument"` on `Document::"<id>"` for the principal.
    async fn document(
        &self,
        ctx: &async_graphql::Context<'_>,
        id: String,
    ) -> async_graphql::Result<String> {
        use acton_service::graphql::CedarResolverCheck;
        CedarResolverCheck::for_context(ctx)?
            .with_action("readDocument")
            .with_resource_type("Document")
            .with_resource_id(&id)
            .authorize()
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(format!("Document {} contents", id))
    }
"#
    } else {
        ""
    };

    format!(
        r#"//! GraphQL transport (scaffolded by `acton service new --graphql`).
//!
//! Mount via:
//!     ServiceBuilder::new()
//!         .with_routes(rest_routes)
//!         .with_versioned_graphql(crate::graphql::build())
//!         .build()
//!         .serve()
//!         .await?;

use acton_service::graphql::{{GraphQLContextExt, VersionedGraphQL, VersionedGraphQLBuilder}};
use acton_service::versioning::ApiVersion;
use async_graphql::{{Context, EmptyMutation, EmptySubscription, Object, Schema}};

pub struct Query;

#[Object]
impl Query {{
    /// Unauthenticated hello.
    async fn hello(&self) -> &'static str {{
        "Hello from GraphQL!"
    }}

    /// Returns the authenticated principal's subject, or "anonymous".
    async fn whoami(&self, ctx: &Context<'_>) -> String {{
        ctx.claims()
            .map(|c| c.sub.clone())
            .unwrap_or_else(|| "anonymous".to_string())
    }}{cedar_field}
}}

/// Build the versioned GraphQL collection. Wire this into `ServiceBuilder`.
pub fn build() -> VersionedGraphQL {{
    let schema = Schema::build(Query, EmptyMutation, EmptySubscription).finish();
    VersionedGraphQLBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, schema)
        .build()
}}
"#,
        cedar_field = cedar_field
    )
}
