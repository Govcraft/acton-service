//! Extension trait for accessing acton-service request data from inside
//! `async_graphql` resolvers.

use async_graphql::Context;

use crate::middleware::token::Claims;

/// Convenience accessors for data the framework injects into every GraphQL
/// request.
///
/// The framework's `GraphQL` service copies the following from the Axum
/// request `Extensions` map into the GraphQL `Request::data` map:
///
/// * [`Claims`] — present whenever the PASETO or JWT middleware (or any other
///   token validator) has authenticated the request.
/// * [`CedarAuthz`](crate::middleware::cedar::CedarAuthz) — present when the
///   `graphql-cedar` feature is enabled and Cedar is configured on the
///   service builder.
///
/// Other framework or user data (database pools, key managers, etc.) should
/// be attached to the schema at build time with
/// `SchemaBuilder::data(...)`.
///
/// # Example
///
/// ```rust,ignore
/// use async_graphql::{Context, Object};
/// use acton_service::graphql::GraphQLContextExt;
///
/// struct Query;
///
/// #[Object]
/// impl Query {
///     async fn whoami(&self, ctx: &Context<'_>) -> String {
///         ctx.claims()
///             .map(|c| c.sub.clone())
///             .unwrap_or_else(|| "anonymous".into())
///     }
/// }
/// ```
pub trait GraphQLContextExt<'ctx> {
    /// Fetch the authenticated user's claims, if the request was authenticated.
    fn claims(&self) -> Option<&'ctx Claims>;

    /// Require authenticated claims. Returns a GraphQL error formatted as
    /// `Unauthorized` if no claims are present.
    fn require_claims(&self) -> async_graphql::Result<&'ctx Claims>;
}

impl<'ctx> GraphQLContextExt<'ctx> for Context<'ctx> {
    fn claims(&self) -> Option<&'ctx Claims> {
        self.data_opt::<Claims>()
    }

    fn require_claims(&self) -> async_graphql::Result<&'ctx Claims> {
        self.claims()
            .ok_or_else(|| async_graphql::Error::new("Unauthorized: missing authentication"))
    }
}
