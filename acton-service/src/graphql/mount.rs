//! Wires a [`VersionedGraphQL`] into an Axum `Router<()>` so it can be merged
//! into the main application router. Mounting happens after route assembly
//! but before middleware application, so GraphQL endpoints inherit the full
//! framework middleware stack (auth, tracing, CORS, rate limiting, etc).

use axum::{
    http::{header, HeaderValue},
    middleware::{self, Next},
    response::{Html, Response},
    routing::MethodRouter,
    Router,
};

use crate::config::GraphQLConfig;
use crate::versioning::DeprecationInfo;

#[cfg(feature = "graphql-cedar")]
use crate::middleware::cedar::CedarAuthz;

use super::builder::VersionedGraphQL;
use super::service::ActonGraphQL;

/// Build a `Router<()>` that exposes every registered GraphQL version at the
/// configured path. Returns `None` if there are no registered versions.
///
/// This is the entry point used by [`ServiceBuilder::build`](crate::service_builder::ServiceBuilder::build);
/// it is also exposed under `#[doc(hidden)]` so integration tests can mount
/// schemas without spinning up the full service.
pub fn build_router(
    graphql: VersionedGraphQL,
    config: Option<&GraphQLConfig>,
    #[cfg(feature = "graphql-cedar")] cedar: Option<CedarAuthz>,
) -> Option<Router<()>> {
    if graphql.entries.is_empty() {
        return None;
    }

    let graphiql_enabled = config.map(|c| c.graphiql_enabled).unwrap_or(true);

    let mut router = Router::<()>::new();
    for entry in graphql.entries {
        let path = endpoint_path(graphql.base_path.as_deref(), entry.version);
        let method_router = build_method_router(
            entry.executor,
            &path,
            graphiql_enabled,
            #[cfg(feature = "graphql-cedar")]
            cedar.clone(),
        );

        let with_deprecation = if let Some(deprecation) = entry.deprecation {
            // Apply per-version deprecation headers + warning log.
            let layered =
                Router::<()>::new()
                    .route(&path, method_router)
                    .layer(middleware::from_fn(move |req, next: Next| {
                        let deprecation = deprecation.clone();
                        async move { apply_deprecation_headers(req, next, deprecation).await }
                    }));
            layered
        } else {
            Router::<()>::new().route(&path, method_router)
        };

        router = router.merge(with_deprecation);
    }
    Some(router)
}

fn endpoint_path(base: Option<&str>, version: crate::versioning::ApiVersion) -> String {
    let version_segment = version.as_path_segment();
    match base {
        Some(b) => format!("{}/{}/graphql", b, version_segment),
        None => format!("/{}/graphql", version_segment),
    }
}

fn build_method_router(
    executor: super::builder::ErasedSchema,
    endpoint: &str,
    graphiql_enabled: bool,
    #[cfg(feature = "graphql-cedar")] cedar: Option<CedarAuthz>,
) -> MethodRouter<()> {
    let service = ActonGraphQL::new(
        executor,
        #[cfg(feature = "graphql-cedar")]
        cedar,
    );

    if graphiql_enabled {
        let endpoint = endpoint.to_string();
        axum::routing::post_service(service).get(move || {
            let endpoint = endpoint.clone();
            async move {
                Html(
                    async_graphql::http::GraphiQLSource::build()
                        .endpoint(&endpoint)
                        .finish(),
                )
            }
        })
    } else {
        axum::routing::post_service(service)
    }
}

async fn apply_deprecation_headers(
    req: axum::extract::Request,
    next: Next,
    deprecation: DeprecationInfo,
) -> Response {
    let path = req.uri().path().to_string();
    if let Some(sunset) = &deprecation.sunset_date {
        tracing::warn!(
            path = %path,
            deprecated_version = %deprecation.version,
            replacement_version = %deprecation.replacement,
            sunset_date = %sunset,
            message = deprecation.message.as_deref().unwrap_or(""),
            "Deprecated GraphQL API version accessed"
        );
    } else {
        tracing::warn!(
            path = %path,
            deprecated_version = %deprecation.version,
            replacement_version = %deprecation.replacement,
            message = deprecation.message.as_deref().unwrap_or(""),
            "Deprecated GraphQL API version accessed"
        );
    }

    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&format!("version=\"{}\"", deprecation.version)) {
        headers.insert("Deprecation", value);
    }
    if let Some(sunset) = &deprecation.sunset_date {
        if let Ok(value) = HeaderValue::from_str(sunset) {
            headers.insert("Sunset", value);
        }
    }
    if let Ok(value) = HeaderValue::from_str(&format!(
        "</{}/>; rel=\"successor-version\"",
        deprecation.replacement.as_path_segment()
    )) {
        headers.insert(header::LINK, value);
    }
    if let Some(message) = &deprecation.message {
        let warning = format!(
            "299 - \"API version {} is deprecated. Please migrate to version {}. {}\"",
            deprecation.version, deprecation.replacement, message
        );
        if let Ok(value) = HeaderValue::from_str(&warning) {
            headers.insert(header::WARNING, value);
        }
    }
    response
}
