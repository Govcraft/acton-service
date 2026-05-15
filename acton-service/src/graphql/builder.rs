//! Versioned schema builder for the GraphQL transport.
//!
//! Mirrors [`VersionedApiBuilder`](crate::versioning::VersionedApiBuilder) in
//! spirit: schemas are bound to an [`ApiVersion`] and mounted at
//! `/{base_path}/{version}/graphql`. The opaque [`VersionedGraphQL`] return
//! type can only be constructed by this builder, so it is structurally
//! impossible to register an unversioned GraphQL schema with
//! [`ServiceBuilder::with_versioned_graphql`](crate::service_builder::ServiceBuilder::with_versioned_graphql).

use std::sync::Arc;

use async_graphql::{Executor, ObjectType, SchemaBuilder, SubscriptionType};
use futures::stream::BoxStream;

use crate::config::GraphQLConfig;
use crate::versioning::{ApiVersion, DeprecationInfo};

#[cfg(feature = "graphql-cedar")]
use crate::middleware::cedar::CedarAuthz;

use super::service::ActonGraphQL;

/// Type-erased GraphQL executor. The `Schema<Q, M, S>` generics are erased
/// here so the registry can hold heterogeneous schemas keyed by version.
pub(crate) type ErasedExecutor = Arc<
    dyn Fn(async_graphql::BatchRequest) -> futures::future::BoxFuture<'static, async_graphql::BatchResponse>
        + Send
        + Sync,
>;

pub(crate) type StreamFactory = Arc<
    dyn Fn(async_graphql::Request) -> BoxStream<'static, async_graphql::Response> + Send + Sync,
>;

/// A `Clone`able wrapper that satisfies `async_graphql::Executor` by
/// delegating to the type-erased closures captured in `ErasedExecutor` and
/// `StreamFactory`. This lets the mount layer build a concrete
/// `ActonGraphQL<E>` without knowing the user's `Query`/`Mutation`/`Subscription`
/// types.
#[derive(Clone)]
pub(crate) struct ErasedSchema {
    batch: ErasedExecutor,
    stream: StreamFactory,
}

impl Executor for ErasedSchema {
    fn execute(
        &self,
        request: async_graphql::Request,
    ) -> impl std::future::Future<Output = async_graphql::Response> + Send {
        let fut = (self.batch)(async_graphql::BatchRequest::Single(request));
        async move {
            match fut.await {
                async_graphql::BatchResponse::Single(s) => s,
                async_graphql::BatchResponse::Batch(mut b) => b.pop().unwrap_or_default(),
            }
        }
    }

    fn execute_batch(
        &self,
        batch_request: async_graphql::BatchRequest,
    ) -> impl std::future::Future<Output = async_graphql::BatchResponse> + Send {
        (self.batch)(batch_request)
    }

    fn execute_stream(
        &self,
        request: async_graphql::Request,
        _session_data: Option<Arc<async_graphql::Data>>,
    ) -> BoxStream<'static, async_graphql::Response> {
        (self.stream)(request)
    }
}

impl ErasedSchema {
    fn from_executor<E>(executor: E) -> Self
    where
        E: Executor + Clone + Send + Sync + 'static,
    {
        let batch_exec = executor.clone();
        let stream_exec = executor;
        Self {
            batch: Arc::new(move |req| {
                let exec = batch_exec.clone();
                Box::pin(async move { exec.execute_batch(req).await })
            }),
            stream: Arc::new(move |req| {
                let stream = stream_exec.execute_stream(req, None);
                Box::pin(stream)
            }),
        }
    }
}

/// A single registered GraphQL endpoint.
pub(crate) struct VersionedSchemaEntry {
    pub(crate) version: ApiVersion,
    pub(crate) deprecation: Option<DeprecationInfo>,
    pub(crate) executor: ErasedSchema,
}

/// Opaque collection of versioned GraphQL schemas.
///
/// Construct via [`VersionedGraphQLBuilder`]. Pass to
/// [`ServiceBuilder::with_versioned_graphql`](crate::service_builder::ServiceBuilder::with_versioned_graphql).
pub struct VersionedGraphQL {
    pub(crate) base_path: Option<String>,
    pub(crate) entries: Vec<VersionedSchemaEntry>,
}

impl VersionedGraphQL {
    /// Number of registered versions.
    pub fn version_count(&self) -> usize {
        self.entries.len()
    }

    /// Whether the builder registered the given version.
    pub fn has_version(&self, version: ApiVersion) -> bool {
        self.entries.iter().any(|e| e.version == version)
    }

    /// Effective base path (e.g. `/api`), if one was configured.
    pub fn base_path(&self) -> Option<&str> {
        self.base_path.as_deref()
    }

    /// Iterate over the registered API versions in registration order. Useful
    /// for OpenAPI augmentation and other introspection use cases.
    pub fn versions(&self) -> impl Iterator<Item = ApiVersion> + '_ {
        self.entries.iter().map(|e| e.version)
    }

    /// Build the per-version `ActonGraphQL` service used by the mount layer.
    #[allow(dead_code)]
    pub(crate) fn build_service_for(
        executor: ErasedSchema,
        #[cfg(feature = "graphql-cedar")] cedar: Option<CedarAuthz>,
    ) -> ActonGraphQL<ErasedSchema> {
        ActonGraphQL::new(
            executor,
            #[cfg(feature = "graphql-cedar")]
            cedar,
        )
    }
}

/// Builder for [`VersionedGraphQL`].
pub struct VersionedGraphQLBuilder {
    base_path: Option<String>,
    entries: Vec<VersionedSchemaEntry>,
}

impl Default for VersionedGraphQLBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionedGraphQLBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            base_path: None,
            entries: Vec::new(),
        }
    }

    /// Set the base path used to prefix every versioned endpoint. Defaults to
    /// no prefix (so endpoints land at `/v1/graphql`, `/v2/graphql`, ...).
    ///
    /// Best practice is to match the `with_base_path` you supplied to
    /// [`VersionedApiBuilder`](crate::versioning::VersionedApiBuilder).
    pub fn with_base_path(mut self, path: impl Into<String>) -> Self {
        let path = path.into();
        let normalized = if !path.starts_with('/') {
            format!("/{}", path.trim_end_matches('/'))
        } else {
            path.trim_end_matches('/').to_string()
        };
        self.base_path = Some(normalized);
        self
    }

    /// Register a schema for a specific API version.
    ///
    /// Accepts any [`Executor`] — most commonly an `async_graphql::Schema`,
    /// but also includes federation/relay wrappers.
    pub fn add_version<E>(mut self, version: ApiVersion, executor: E) -> Self
    where
        E: Executor + Clone + Send + Sync + 'static,
    {
        let entry = VersionedSchemaEntry {
            version,
            deprecation: None,
            executor: ErasedSchema::from_executor(executor),
        };
        self.entries.push(entry);
        self
    }

    /// Register a schema and mark the version as deprecated. Deprecation
    /// headers (`Deprecation`, `Sunset`, `Link`, `Warning`) will be added to
    /// every response for this endpoint.
    pub fn add_version_deprecated<E>(
        mut self,
        version: ApiVersion,
        executor: E,
        deprecation: DeprecationInfo,
    ) -> Self
    where
        E: Executor + Clone + Send + Sync + 'static,
    {
        let entry = VersionedSchemaEntry {
            version,
            deprecation: Some(deprecation),
            executor: ErasedSchema::from_executor(executor),
        };
        self.entries.push(entry);
        self
    }

    /// Finalize and produce the opaque [`VersionedGraphQL`] collection.
    pub fn build(self) -> VersionedGraphQL {
        VersionedGraphQL {
            base_path: self.base_path,
            entries: self.entries,
        }
    }
}

/// Compatibility alias — older drafts of the docs referred to a
/// `GraphQLBuilder` type. Both names build the same value.
pub type GraphQLBuilder = VersionedGraphQLBuilder;

/// Apply the depth/complexity limits and introspection flag from
/// [`GraphQLConfig`] to a `SchemaBuilder`. The introspection toggle uses
/// `SchemaBuilder::disable_introspection` when the config disables it.
///
/// # Example
///
/// ```rust,ignore
/// use async_graphql::{EmptyMutation, EmptySubscription, Schema};
/// use acton_service::graphql::apply_config_to_builder;
///
/// let cfg = config.graphql.clone().unwrap_or_default();
/// let builder = Schema::build(Query, EmptyMutation, EmptySubscription);
/// let schema = apply_config_to_builder(builder, &cfg).finish();
/// ```
pub fn apply_config_to_builder<Q, M, S>(
    mut builder: SchemaBuilder<Q, M, S>,
    config: &GraphQLConfig,
) -> SchemaBuilder<Q, M, S>
where
    Q: ObjectType + 'static,
    M: ObjectType + 'static,
    S: SubscriptionType + 'static,
{
    if let Some(depth) = config.max_query_depth {
        builder = builder.limit_depth(depth);
    }
    if let Some(complexity) = config.max_query_complexity {
        builder = builder.limit_complexity(complexity);
    }
    if !config.introspection_enabled {
        builder = builder.disable_introspection();
    }
    builder
}
