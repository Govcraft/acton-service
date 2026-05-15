//! GraphQL transport for acton-service (requires the `graphql` feature).
//!
//! This module integrates [`async-graphql`](https://docs.rs/async-graphql) as a
//! third sibling transport next to HTTP (Axum) and gRPC (Tonic). Schemas are
//! mounted by [`ServiceBuilder::with_versioned_graphql`] underneath the same
//! versioned Axum router used for REST endpoints, so they inherit the full
//! middleware stack — authentication, tracing, rate limiting, CORS, and
//! everything else.
//!
//! # Versioning
//!
//! GraphQL endpoints are versioned per-path. A schema registered for
//! [`ApiVersion::V1`] is mounted at `/{base}/v1/graphql`, V2 at
//! `/{base}/v2/graphql`, and so on. This matches the framework's existing
//! path-based versioning ([`VersionedApiBuilder`](crate::versioning::VersionedApiBuilder)).
//!
//! # Authentication
//!
//! When PASETO or JWT middleware is enabled, [`Claims`](crate::middleware::Claims)
//! placed into request extensions are automatically injected into the GraphQL
//! request's data. Resolvers retrieve them through the
//! [`GraphQLContextExt::claims`] extension trait.
//!
//! # Cedar authorization
//!
//! Under the `graphql-cedar` feature, resolvers can call
//! [`CedarResolverCheck::authorize`] to evaluate Cedar policies using the same
//! [`CedarAuthz`](crate::middleware::cedar::CedarAuthz) instance that protects
//! HTTP and gRPC.
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//! use acton_service::graphql::{VersionedGraphQLBuilder, GraphQLContextExt};
//! use async_graphql::{Object, Schema, EmptyMutation, EmptySubscription, Context};
//!
//! struct Query;
//!
//! #[Object]
//! impl Query {
//!     async fn me(&self, ctx: &Context<'_>) -> String {
//!         ctx.claims()
//!             .map(|c| c.sub.clone())
//!             .unwrap_or_else(|| "anonymous".into())
//!     }
//! }
//!
//! let schema = Schema::build(Query, EmptyMutation, EmptySubscription).finish();
//!
//! let graphql = VersionedGraphQLBuilder::new()
//!     .with_base_path("/api")
//!     .add_version(ApiVersion::V1, schema)
//!     .build();
//!
//! ServiceBuilder::new()
//!     .with_routes(VersionedApiBuilder::new().build_routes())
//!     .with_versioned_graphql(graphql)
//!     .build()
//!     .serve()
//!     .await?;
//! ```

mod builder;
mod context;
#[doc(hidden)]
pub mod mount;
mod service;

#[cfg(feature = "graphql-cedar")]
mod cedar;

pub use builder::{
    apply_config_to_builder, GraphQLBuilder, VersionedGraphQL, VersionedGraphQLBuilder,
};
pub use context::GraphQLContextExt;

#[cfg(feature = "graphql-cedar")]
pub use cedar::{CedarResolverCheck, CedarResolverError};

/// Re-export the underlying `async_graphql` crate so consumers don't need to
/// add a direct dependency. Build schemas as
/// `acton_service::graphql::async_graphql::Schema::build(...)`.
pub use async_graphql;
