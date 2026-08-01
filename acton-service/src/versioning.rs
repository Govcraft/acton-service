//! API versioning utilities for managing API evolution
//!
//! This module provides utilities for URL path-based API versioning with deprecation support.
//!
//! ## URL Path Versioning
//!
//! The recommended approach is to version APIs through the URL path:
//! - `/v1/users` - Version 1 of the users API
//! - `/v2/users` - Version 2 of the users API
//!
//! ## Example
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//! use acton_service::versioning::{ApiVersion, versioned_router};
//!
//! async fn get_user_v1() -> Json<&'static str> {
//!     Json("User V1")
//! }
//!
//! async fn get_user_v2() -> Json<&'static str> {
//!     Json("User V2")
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let v1_routes = Router::new()
//!         .route("/users", get(get_user_v1));
//!
//!     let v2_routes = Router::new()
//!         .route("/users", get(get_user_v2));
//!
//!     let app = Router::new()
//!         .nest("/v1", versioned_router(ApiVersion::V1, v1_routes))
//!         .nest("/v2", versioned_router(ApiVersion::V2, v2_routes));
//!
//!     Ok(())
//! }
//! ```

use axum::{
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fmt;
use tracing::warn;

#[cfg(feature = "_metrics")]
use opentelemetry::KeyValue;

/// API version identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ApiVersion {
    /// API Version 1
    V1,
    /// API Version 2
    V2,
    /// API Version 3
    V3,
    /// API Version 4
    V4,
    /// API Version 5
    V5,
}

impl ApiVersion {
    /// Parse version from string (e.g., "v1", "V1", "1")
    pub fn parse(s: &str) -> Option<Self> {
        let lowercase = s.to_lowercase();
        let normalized = lowercase.trim_start_matches('v');
        match normalized {
            "1" => Some(Self::V1),
            "2" => Some(Self::V2),
            "3" => Some(Self::V3),
            "4" => Some(Self::V4),
            "5" => Some(Self::V5),
            _ => None,
        }
    }

    /// Get the version number as u8
    pub fn as_number(&self) -> u8 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
            Self::V3 => 3,
            Self::V4 => 4,
            Self::V5 => 5,
        }
    }

    /// Get the version as a path segment (e.g., "v1")
    pub fn as_path_segment(&self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
            Self::V3 => "v3",
            Self::V4 => "v4",
            Self::V5 => "v5",
        }
    }

    /// Check if this version is deprecated
    pub fn is_deprecated(&self, latest: ApiVersion) -> bool {
        *self < latest
    }
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_path_segment())
    }
}

impl From<ApiVersion> for u8 {
    fn from(version: ApiVersion) -> Self {
        version.as_number()
    }
}

/// Deprecation information for an API version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeprecationInfo {
    /// The deprecated API version
    pub version: ApiVersion,
    /// The recommended replacement version
    pub replacement: ApiVersion,
    /// Sunset date in RFC 3339 format (when the version will be removed)
    pub sunset_date: Option<String>,
    /// Optional deprecation message
    pub message: Option<String>,
}

impl DeprecationInfo {
    /// Create a new deprecation info
    pub fn new(version: ApiVersion, replacement: ApiVersion) -> Self {
        Self {
            version,
            replacement,
            sunset_date: None,
            message: None,
        }
    }

    /// Set the sunset date (RFC 3339 format)
    pub fn with_sunset_date(mut self, date: impl Into<String>) -> Self {
        self.sunset_date = Some(date.into());
        self
    }

    /// Set a custom deprecation message
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Generate deprecation header value
    fn deprecation_header(&self) -> String {
        format!("version=\"{}\"", self.version)
    }

    /// Generate sunset header value (if sunset date is set)
    fn sunset_header(&self) -> Option<String> {
        self.sunset_date.clone()
    }

    /// Generate Link header value pointing to replacement version
    fn link_header(&self) -> String {
        format!(
            "</{}/>; rel=\"successor-version\"",
            self.replacement.as_path_segment()
        )
    }
}

/// Create a versioned router with optional deprecation information
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::prelude::*;
/// use acton_service::versioning::{ApiVersion, versioned_router, DeprecationInfo};
///
/// async fn handler() -> &'static str {
///     "Hello"
/// }
///
/// # #[tokio::main]
/// # async fn main() {
/// let v1_routes = Router::new().route("/hello", get(handler));
///
/// // Non-deprecated version
/// let v2_routes = Router::new().route("/hello", get(handler));
/// let v2 = versioned_router(ApiVersion::V2, v2_routes);
///
/// // Deprecated version with sunset date
/// let deprecation = DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
///     .with_sunset_date("2026-12-31T23:59:59Z")
///     .with_message("This version will be removed on December 31, 2026.");
///
/// let v1 = versioned_router(ApiVersion::V1, v1_routes)
///     .deprecated(deprecation);
/// # }
/// ```
pub fn versioned_router(version: ApiVersion, router: Router) -> VersionedRouter {
    VersionedRouter {
        version,
        router,
        deprecation: None,
    }
}

/// A router wrapper that can have deprecation information attached
pub struct VersionedRouter {
    version: ApiVersion,
    router: Router,
    deprecation: Option<DeprecationInfo>,
}

impl VersionedRouter {
    /// Mark this version as deprecated
    pub fn deprecated(mut self, info: DeprecationInfo) -> Self {
        self.deprecation = Some(info);
        self
    }

    /// Convert to a regular Axum router with deprecation middleware applied
    pub fn into_router(self) -> Router {
        #[cfg(feature = "_metrics")]
        let version = self.version;
        let deprecation = self.deprecation.clone();

        // Always apply middleware for metrics tracking and optional deprecation headers
        self.router.layer(middleware::from_fn(move |req: Request, next: Next| {
            let deprecation = deprecation.clone();
            #[cfg(feature = "_metrics")]
            let version = version;
            async move {
                // If deprecated, log the usage
                if let Some(ref deprecation_info) = deprecation {
                    let path = req.uri().path();
                    if let Some(sunset) = &deprecation_info.sunset_date {
                        warn!(
                            path = %path,
                            deprecated_version = %deprecation_info.version,
                            replacement_version = %deprecation_info.replacement,
                            sunset_date = %sunset,
                            message = deprecation_info.message.as_deref().unwrap_or(""),
                            "Deprecated API version accessed"
                        );
                    } else {
                        warn!(
                            path = %path,
                            deprecated_version = %deprecation_info.version,
                            replacement_version = %deprecation_info.replacement,
                            message = deprecation_info.message.as_deref().unwrap_or(""),
                            "Deprecated API version accessed"
                        );
                    }
                }

                // Record metrics for all API version usage (deprecated or not)
                #[cfg(feature = "_metrics")]
                if let Some(meter) = crate::observability::get_meter() {
                    let counter = meter
                        .u64_counter("api.version.requests")
                        .with_description("Count of API requests by version")
                        .build();

                    let mut attributes = vec![
                        KeyValue::new("version", version.to_string()),
                        KeyValue::new("deprecated", deprecation.is_some().to_string()),
                    ];

                    if let Some(ref deprecation_info) = deprecation {
                        attributes.push(KeyValue::new(
                            "replacement_version",
                            deprecation_info.replacement.to_string(),
                        ));
                    }

                    counter.add(1, &attributes);
                }

                let mut response = next.run(req).await;

                // Add deprecation headers if this version is deprecated
                if let Some(ref deprecation_info) = deprecation {
                    let headers = response.headers_mut();

                    // Add Deprecation header (RFC 8594)
                    if let Ok(value) = HeaderValue::from_str(&deprecation_info.deprecation_header()) {
                        headers.insert("Deprecation", value);
                    }

                    // Add Sunset header if configured (RFC 8594)
                    if let Some(sunset) = deprecation_info.sunset_header() {
                        if let Ok(value) = HeaderValue::from_str(&sunset) {
                            headers.insert("Sunset", value);
                        }
                    }

                    // Add Link header pointing to replacement version
                    if let Ok(value) = HeaderValue::from_str(&deprecation_info.link_header()) {
                        headers.insert(header::LINK, value);
                    }

                    // Add custom warning header if message is provided
                    if let Some(ref message) = deprecation_info.message {
                        let warning = format!(
                            "299 - \"API version {} is deprecated. Please migrate to version {}. {}\"",
                            deprecation_info.version, deprecation_info.replacement, message
                        );
                        if let Ok(value) = HeaderValue::from_str(&warning) {
                            headers.insert(header::WARNING, value);
                        }
                    }
                }

                response
            }
        }))
    }

    /// Get the API version
    pub fn version(&self) -> ApiVersion {
        self.version
    }

    /// Check if this version is deprecated
    pub fn is_deprecated(&self) -> bool {
        self.deprecation.is_some()
    }
}

/// Helper to extract version from request path
///
/// This can be used in handlers that need to know which version was called
pub fn extract_version_from_path(path: &str) -> Option<ApiVersion> {
    // Extract version from paths like "/v1/users" or "/api/v2/users"
    path.split('/')
        .find(|segment| segment.starts_with('v') || segment.starts_with('V'))
        .and_then(ApiVersion::parse)
}

/// A deferred `Router::fallback` / `Router::fallback_service` call.
///
/// The handler or service is captured at the call site and applied once, in
/// `build_routes`, after every other route is in place. Boxing it this way is
/// what lets `with_fallback` and `with_fallback_service` share a single slot
/// despite having incompatible generic bounds.
type FallbackSlot<T> = Box<
    dyn FnOnce(Router<crate::state::AppState<T>>) -> Router<crate::state::AppState<T>>
        + Send
        + Sync,
>;

/// Builder for creating versioned API routers with enforcement
///
/// This builder ensures that all routes are versioned and provides a structured
/// way to manage multiple API versions with deprecation support.
///
/// The generic parameter `T` represents your custom configuration type that extends
/// the framework's base configuration. Use `()` (the default) if you don't need
/// custom configuration.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::prelude::*;
/// use acton_service::versioning::{ApiVersion, VersionedApiBuilder, DeprecationInfo};
///
/// async fn list_users_v1() -> &'static str { "Users V1" }
/// async fn list_users_v2() -> &'static str { "Users V2" }
///
/// // Without custom config (default)
/// let api = VersionedApiBuilder::new()
///     .add_version(ApiVersion::V1, |routes| {
///         routes.route("/users", get(list_users_v1))
///     })
///     .build_routes();
///
/// // With custom config
/// let api = VersionedApiBuilder::<MyCustomConfig>::new()
///     .add_version(ApiVersion::V1, |routes| {
///         routes.route("/users", get(list_users_v1))
///     })
///     .build_routes();  // Returns VersionedRoutes<MyCustomConfig>
/// ```
pub struct VersionedApiBuilder<T = ()>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    versions: Vec<(
        ApiVersion,
        Router<crate::state::AppState<T>>,
        Option<DeprecationInfo>,
    )>,
    base_path: Option<String>,
    #[cfg(feature = "htmx")]
    frontend_routes: Option<Router<crate::state::AppState<T>>>,
    fallback: Option<FallbackSlot<T>>,
}

impl Default for VersionedApiBuilder<()> {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionedApiBuilder<()> {
    /// Create a new versioned API builder
    ///
    /// Use this for services without custom configuration. Handlers can still
    /// access the framework's `AppState` for health checks and standard features.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let routes = VersionedApiBuilder::new()
    ///     .with_base_path("/api")
    ///     .add_version(ApiVersion::V1, |routes| {
    ///         routes.route("/users", get(list_users))
    ///     })
    ///     .build_routes();
    /// ```
    pub fn new() -> Self {
        Self {
            versions: Vec::new(),
            base_path: None,
            #[cfg(feature = "htmx")]
            frontend_routes: None,
            fallback: None,
        }
    }
}

impl<T> VersionedApiBuilder<T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    /// Create a new versioned API builder with custom configuration type
    ///
    /// Use this when your handlers need access to custom configuration via
    /// `State<AppState<YourCustomConfig>>`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// #[derive(Clone, Default, Serialize, Deserialize)]
    /// struct MyConfig {
    ///     api_key: String,
    /// }
    ///
    /// async fn handler(State(state): State<AppState<MyConfig>>) -> impl IntoResponse {
    ///     let api_key = &state.config().custom.api_key;
    ///     // ...
    /// }
    ///
    /// let routes = VersionedApiBuilder::<MyConfig>::with_config()
    ///     .with_base_path("/api")
    ///     .add_version(ApiVersion::V1, |routes| {
    ///         routes.route("/data", get(handler))
    ///     })
    ///     .build_routes();
    /// ```
    pub fn with_config() -> Self {
        Self {
            versions: Vec::new(),
            base_path: None,
            #[cfg(feature = "htmx")]
            frontend_routes: None,
            fallback: None,
        }
    }

    /// Set a base path for all versioned routes (e.g., "/api")
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let api = VersionedApiBuilder::new()
    ///     .with_base_path("/api")  // Routes will be /api/v1/users, /api/v2/users, etc.
    ///     .add_version(ApiVersion::V1, |routes| {
    ///         routes.route("/users", get(handler))
    ///     })
    ///     .build_routes();
    /// ```
    pub fn with_base_path(mut self, path: impl Into<String>) -> Self {
        let path = path.into();
        // Ensure path starts with / and doesn't end with /
        let normalized = if !path.starts_with('/') {
            format!("/{}", path.trim_end_matches('/'))
        } else {
            path.trim_end_matches('/').to_string()
        };
        self.base_path = Some(normalized);
        self
    }

    /// Add a non-deprecated API version
    ///
    /// The closure receives a `Router<AppState<T>>` so handlers can use
    /// `State<AppState<T>>` to access configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let api = VersionedApiBuilder::new()
    ///     .add_version(ApiVersion::V1, |routes| {
    ///         routes
    ///             .route("/users", get(list_users))
    ///             .route("/users/{id}", get(get_user))
    ///     })
    ///     .build_routes();
    /// ```
    pub fn add_version<F>(mut self, version: ApiVersion, routes: F) -> Self
    where
        F: FnOnce(Router<crate::state::AppState<T>>) -> Router<crate::state::AppState<T>>,
    {
        let router = routes(Router::new());
        self.versions.push((version, router, None));
        self
    }

    /// Add a deprecated API version with deprecation information
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let deprecation = DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
    ///     .with_sunset_date("2026-12-31T23:59:59Z")
    ///     .with_message("Please migrate to V2");
    ///
    /// let api = VersionedApiBuilder::new()
    ///     .add_version_deprecated(
    ///         ApiVersion::V1,
    ///         |routes| routes.route("/users", get(list_users_v1)),
    ///         deprecation
    ///     )
    ///     .build_routes();
    /// ```
    pub fn add_version_deprecated<F>(
        mut self,
        version: ApiVersion,
        routes: F,
        deprecation: DeprecationInfo,
    ) -> Self
    where
        F: FnOnce(Router<crate::state::AppState<T>>) -> Router<crate::state::AppState<T>>,
    {
        let router = routes(Router::new());
        self.versions.push((version, router, Some(deprecation)));
        self
    }

    /// Mark an existing version as deprecated
    ///
    /// This is useful when you want to add the version first and mark it
    /// deprecated later in the builder chain.
    ///
    /// # Panics
    ///
    /// Panics if the specified version hasn't been added yet.
    pub fn deprecate_version(mut self, version: ApiVersion, deprecation: DeprecationInfo) -> Self {
        let entry = self
            .versions
            .iter_mut()
            .find(|(v, _, _)| *v == version)
            .expect("Version must be added before deprecating");
        entry.2 = Some(deprecation);
        self
    }

    /// Add unversioned frontend routes (only available with htmx feature)
    ///
    /// Frontend routes are served at the application root and bypass API versioning.
    /// Use this for:
    /// - Server-rendered HTML pages (Askama templates)
    /// - HTMX partial fragments
    /// - Static content requiring server logic
    ///
    /// These routes coexist with versioned API routes. For example, you can have:
    /// - `/` - Frontend index page (unversioned)
    /// - `/login` - Frontend login page (unversioned)
    /// - `/api/v1/users` - Versioned API endpoint
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use acton_service::prelude::*;
    /// use acton_service::versioning::{ApiVersion, VersionedApiBuilder};
    ///
    /// async fn index() -> Html<&'static str> {
    ///     Html("<h1>Welcome</h1>")
    /// }
    ///
    /// async fn api_handler() -> Json<&'static str> {
    ///     Json("API V1")
    /// }
    ///
    /// let routes = VersionedApiBuilder::new()
    ///     .with_base_path("/api")
    ///     .add_version(ApiVersion::V1, |routes| {
    ///         routes.route("/data", get(api_handler))
    ///     })
    ///     .with_frontend_routes(|router| {
    ///         router
    ///             .route("/", get(index))
    ///             .route("/login", get(login_page))
    ///     })
    ///     .build_routes();
    /// ```
    #[cfg(feature = "htmx")]
    pub fn with_frontend_routes<F>(mut self, routes: F) -> Self
    where
        F: FnOnce(Router<crate::state::AppState<T>>) -> Router<crate::state::AppState<T>>,
    {
        let router = routes(Router::new());
        self.frontend_routes = Some(router);
        self
    }

    /// Handle every request that matches no other route
    ///
    /// This is the root-level catch-all. Use it for a transparent reverse
    /// proxy, a gateway that forwards what it does not itself serve, or simply
    /// a custom 404 body. Unlike `with_frontend_routes`, it needs no feature
    /// flags — a proxy has no frontend, and should not have to compile an HTML
    /// templating stack to reach this slot.
    ///
    /// The fallback is installed after health routes, frontend routes, and all
    /// versioned routes, so it never shadows them. It sees only paths that
    /// matched nothing else.
    ///
    /// Calling this more than once keeps the last handler. If you also set a
    /// fallback inside `with_frontend_routes` (`htmx` feature), this one wins.
    ///
    /// For a [`tower::Service`] rather than a handler — which is what an HTTP
    /// client forwarding upstream usually is — see
    /// [`with_fallback_service`](Self::with_fallback_service).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use acton_service::prelude::*;
    ///
    /// async fn not_found() -> impl IntoResponse {
    ///     (StatusCode::NOT_FOUND, Json(json!({ "error": "no such route" })))
    /// }
    ///
    /// let routes = VersionedApiBuilder::new()
    ///     .with_base_path("/api")
    ///     .add_version(ApiVersion::V1, |routes| {
    ///         routes.route("/data", get(api_handler))
    ///     })
    ///     .with_fallback(not_found)
    ///     .build_routes();
    /// ```
    pub fn with_fallback<H, Tp>(mut self, handler: H) -> Self
    where
        H: axum::handler::Handler<Tp, crate::state::AppState<T>>,
        Tp: 'static,
    {
        self.fallback = Some(Box::new(move |router| router.fallback(handler)));
        self
    }

    /// Handle every unmatched request with a [`tower::Service`]
    ///
    /// The service form of [`with_fallback`](Self::with_fallback), for when the
    /// catch-all is something that already speaks `Service<Request>` — an HTTP
    /// client forwarding to an upstream, a `ServeDir`, another `Router`.
    ///
    /// The same ordering rules apply: installed last, shadows nothing, and
    /// takes precedence over any fallback set inside `with_frontend_routes`.
    ///
    /// # Example
    ///
    /// A transparent reverse proxy that forwards every unhandled path upstream
    /// while keeping the framework's request-ID propagation, security headers,
    /// rate limiting, and panic recovery:
    ///
    /// ```rust,ignore
    /// use acton_service::prelude::*;
    /// use tower::service_fn;
    ///
    /// let upstream = service_fn(move |req: Request| async move {
    ///     // rewrite the authority, forward, return the upstream response
    ///     forward_to_upstream(req).await
    /// });
    ///
    /// let routes = VersionedApiBuilder::new()
    ///     .add_version(ApiVersion::V1, |routes| {
    ///         routes.route("/paywall", post(charge))
    ///     })
    ///     .with_fallback_service(upstream)
    ///     .build_routes();
    /// ```
    pub fn with_fallback_service<S>(mut self, service: S) -> Self
    where
        S: tower::Service<Request, Error = std::convert::Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        S::Response: IntoResponse,
        S::Future: Send + 'static,
    {
        self.fallback = Some(Box::new(move |router| router.fallback_service(service)));
        self
    }

    /// Build versioned routes (opaque VersionedRoutes type)
    ///
    /// This creates a `VersionedRoutes<T>` with all your versioned business routes
    /// plus automatic health and readiness endpoints at /health and /ready.
    /// This is the ONLY public way to create `VersionedRoutes`.
    ///
    /// The returned `VersionedRoutes<T>` is parameterized by your custom config type,
    /// ensuring type safety when used with `ServiceBuilder<T>`.
    pub fn build_routes(self) -> crate::service_builder::VersionedRoutes<T> {
        use axum::routing::get;

        // Start with health routes
        let mut router: Router<crate::state::AppState<T>> = Router::new()
            .route("/health", get(crate::health::health::<T>))
            .route("/ready", get(crate::health::readiness::<T>));

        // Add frontend routes (htmx feature only)
        // These are merged at root level before versioned API routes
        #[cfg(feature = "htmx")]
        if let Some(frontend_router) = self.frontend_routes {
            router = router.merge(frontend_router);
        }

        // Add all versioned routes
        for (version, version_router, deprecation) in self.versions {
            let version_path = format!("/{}", version.as_path_segment());
            let full_path = if let Some(ref base) = self.base_path {
                format!("{}{}", base, version_path)
            } else {
                version_path
            };

            // Apply deprecation middleware if needed
            let versioned = if let Some(deprecation) = deprecation {
                version_router.layer(middleware::from_fn(move |req: Request, next: Next| {
                    let deprecation = deprecation.clone();
                    async move {
                        // Log deprecated API usage
                        let path = req.uri().path().to_string();
                        if let Some(sunset) = &deprecation.sunset_date {
                            warn!(
                                path = %path,
                                deprecated_version = %deprecation.version,
                                replacement_version = %deprecation.replacement,
                                sunset_date = %sunset,
                                message = deprecation.message.as_deref().unwrap_or(""),
                                "Deprecated API version accessed"
                            );
                        } else {
                            warn!(
                                path = %path,
                                deprecated_version = %deprecation.version,
                                replacement_version = %deprecation.replacement,
                                message = deprecation.message.as_deref().unwrap_or(""),
                                "Deprecated API version accessed"
                            );
                        }

                        let mut response = next.run(req).await;

                        // Add deprecation headers
                        let headers = response.headers_mut();
                        if let Ok(value) = HeaderValue::from_str(&deprecation.deprecation_header()) {
                            headers.insert("Deprecation", value);
                        }
                        if let Some(sunset) = deprecation.sunset_header() {
                            if let Ok(value) = HeaderValue::from_str(&sunset) {
                                headers.insert("Sunset", value);
                            }
                        }
                        if let Ok(value) = HeaderValue::from_str(&deprecation.link_header()) {
                            headers.insert(header::LINK, value);
                        }
                        if let Some(ref message) = deprecation.message {
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
                }))
            } else {
                version_router
            };

            router = router.nest(&full_path, versioned);
        }

        // The root-level catch-all goes on last, once every real route is
        // registered. Applying it here rather than merging a fallback-carrying
        // router also sidesteps axum's "cannot merge two routers that both have
        // a fallback" panic when `with_frontend_routes` set one too — this call
        // simply replaces it.
        if let Some(fallback) = self.fallback {
            router = fallback(router);
        }

        crate::service_builder::VersionedRoutes::from_router_with_state(router)
    }

    /// Get the number of versions registered
    pub fn version_count(&self) -> usize {
        self.versions.len()
    }

    /// Check if a specific version has been added
    pub fn has_version(&self, version: ApiVersion) -> bool {
        self.versions.iter().any(|(v, _, _)| *v == version)
    }
}

/// Response wrapper that includes API version information
#[derive(Debug, Serialize, Deserialize)]
pub struct VersionedResponse<T> {
    /// API version used
    pub version: ApiVersion,
    /// Response data
    pub data: T,
}

impl<T> VersionedResponse<T> {
    /// Create a new versioned response
    pub fn new(version: ApiVersion, data: T) -> Self {
        Self { version, data }
    }
}

impl<T: Serialize> IntoResponse for VersionedResponse<T> {
    fn into_response(self) -> Response {
        match serde_json::to_vec(&self) {
            Ok(body) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                body,
            )
                .into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize response: {}", err),
            )
                .into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        assert_eq!(ApiVersion::parse("v1"), Some(ApiVersion::V1));
        assert_eq!(ApiVersion::parse("V1"), Some(ApiVersion::V1));
        assert_eq!(ApiVersion::parse("1"), Some(ApiVersion::V1));
        assert_eq!(ApiVersion::parse("v2"), Some(ApiVersion::V2));
        assert_eq!(ApiVersion::parse("3"), Some(ApiVersion::V3));
        assert_eq!(ApiVersion::parse("v99"), None);
    }

    #[test]
    fn test_version_comparison() {
        assert!(ApiVersion::V1 < ApiVersion::V2);
        assert!(ApiVersion::V2 > ApiVersion::V1);
        assert_eq!(ApiVersion::V1, ApiVersion::V1);
    }

    #[test]
    fn test_version_as_number() {
        assert_eq!(ApiVersion::V1.as_number(), 1);
        assert_eq!(ApiVersion::V2.as_number(), 2);
        assert_eq!(ApiVersion::V5.as_number(), 5);
    }

    #[test]
    fn test_version_deprecation() {
        assert!(ApiVersion::V1.is_deprecated(ApiVersion::V2));
        assert!(!ApiVersion::V2.is_deprecated(ApiVersion::V2));
        assert!(!ApiVersion::V3.is_deprecated(ApiVersion::V2));
    }

    #[test]
    fn test_extract_version_from_path() {
        assert_eq!(extract_version_from_path("/v1/users"), Some(ApiVersion::V1));
        assert_eq!(
            extract_version_from_path("/api/v2/users/123"),
            Some(ApiVersion::V2)
        );
        assert_eq!(extract_version_from_path("/users"), None);
    }

    #[test]
    fn test_deprecation_info() {
        let info = DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
            .with_sunset_date("2026-12-31T23:59:59Z")
            .with_message("Please migrate soon");

        assert_eq!(info.version, ApiVersion::V1);
        assert_eq!(info.replacement, ApiVersion::V2);
        assert_eq!(info.sunset_date, Some("2026-12-31T23:59:59Z".to_string()));
        assert_eq!(info.message, Some("Please migrate soon".to_string()));
    }

    #[test]
    fn test_deprecation_headers() {
        let info = DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
            .with_sunset_date("2026-12-31T23:59:59Z");

        assert_eq!(info.deprecation_header(), "version=\"v1\"");
        assert_eq!(
            info.sunset_header(),
            Some("2026-12-31T23:59:59Z".to_string())
        );
        assert_eq!(info.link_header(), "</v2/>; rel=\"successor-version\"");
    }

    #[test]
    fn test_versioned_api_builder_basic() {
        let builder = VersionedApiBuilder::new()
            .add_version(ApiVersion::V1, |routes| {
                routes.route("/users", axum::routing::get(|| async { "V1" }))
            })
            .add_version(ApiVersion::V2, |routes| {
                routes.route("/users", axum::routing::get(|| async { "V2" }))
            });

        assert_eq!(builder.version_count(), 2);
        assert!(builder.has_version(ApiVersion::V1));
        assert!(builder.has_version(ApiVersion::V2));
        assert!(!builder.has_version(ApiVersion::V3));
    }

    #[test]
    fn test_versioned_api_builder_with_base_path() {
        let builder = VersionedApiBuilder::new()
            .with_base_path("/api")
            .add_version(ApiVersion::V1, |routes| {
                routes.route("/users", axum::routing::get(|| async { "V1" }))
            });

        assert_eq!(builder.version_count(), 1);
        assert!(builder.has_version(ApiVersion::V1));
    }

    #[test]
    fn test_versioned_api_builder_with_deprecation() {
        let deprecation = DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
            .with_sunset_date("2026-12-31T23:59:59Z");

        let builder = VersionedApiBuilder::new()
            .add_version_deprecated(
                ApiVersion::V1,
                |routes| routes.route("/users", axum::routing::get(|| async { "V1" })),
                deprecation,
            )
            .add_version(ApiVersion::V2, |routes| {
                routes.route("/users", axum::routing::get(|| async { "V2" }))
            });

        assert_eq!(builder.version_count(), 2);
    }

    #[test]
    fn test_versioned_api_builder_deprecate_existing() {
        let builder = VersionedApiBuilder::new()
            .add_version(ApiVersion::V1, |routes| {
                routes.route("/users", axum::routing::get(|| async { "V1" }))
            })
            .deprecate_version(
                ApiVersion::V1,
                DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2),
            );

        assert_eq!(builder.version_count(), 1);
    }

    #[test]
    #[should_panic(expected = "Version must be added before deprecating")]
    fn test_versioned_api_builder_deprecate_nonexistent() {
        let _ = VersionedApiBuilder::new().deprecate_version(
            ApiVersion::V1,
            DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2),
        );
    }

    #[test]
    #[cfg(feature = "htmx")]
    fn test_versioned_api_builder_with_frontend_routes() {
        // Test that frontend routes can be added alongside versioned routes
        let _routes = VersionedApiBuilder::new()
            .with_base_path("/api")
            .add_version(ApiVersion::V1, |routes| {
                routes.route("/data", axum::routing::get(|| async { "API V1" }))
            })
            .with_frontend_routes(|router| {
                router
                    .route("/", axum::routing::get(|| async { "Home" }))
                    .route("/login", axum::routing::get(|| async { "Login" }))
            })
            .build_routes();

        // If we get here without panicking, the routes were built successfully
        // The actual routing behavior is tested via integration tests
    }

    #[test]
    #[cfg(feature = "htmx")]
    fn test_versioned_api_builder_frontend_routes_only() {
        // Test that frontend routes can be used without any versioned API routes
        let _routes = VersionedApiBuilder::new()
            .with_frontend_routes(|router| {
                router
                    .route("/", axum::routing::get(|| async { "Home" }))
                    .route("/about", axum::routing::get(|| async { "About" }))
            })
            .build_routes();

        // If we get here without panicking, the routes were built successfully
    }

    mod fallback {
        use super::*;
        use axum::body::Body;
        use tower::ServiceExt as _;

        /// Drive one request through a built router and report what came back.
        ///
        /// Construction-only assertions cannot tell a fallback that catches
        /// everything from one that catches nothing, so every test here routes
        /// a real request.
        async fn get(
            routes: crate::service_builder::VersionedRoutes<()>,
            path: &str,
        ) -> (StatusCode, String) {
            let crate::service_builder::VersionedRoutes::WithState(router) = routes else {
                panic!("build_routes always yields a stateful router");
            };
            let app = router.with_state(crate::state::AppState::<()>::default());

            let response = app
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .body(Body::empty())
                        .expect("request builds"),
                )
                .await
                .expect("router is infallible");

            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body collects");
            (status, String::from_utf8_lossy(&bytes).into_owned())
        }

        fn builder_with_api() -> VersionedApiBuilder<()> {
            VersionedApiBuilder::new()
                .with_base_path("/api")
                .add_version(ApiVersion::V1, |routes| {
                    routes.route("/data", axum::routing::get(|| async { "API V1" }))
                })
        }

        #[tokio::test]
        async fn unmatched_paths_reach_the_fallback() {
            let routes = builder_with_api()
                .with_fallback(|| async { "caught" })
                .build_routes();

            let (status, body) = get(routes, "/anything/at/all").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, "caught");
        }

        #[tokio::test]
        async fn without_a_fallback_unmatched_paths_still_404() {
            let routes = builder_with_api().build_routes();

            let (status, _) = get(routes, "/anything/at/all").await;
            assert_eq!(status, StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn the_fallback_does_not_shadow_versioned_routes() {
            let routes = builder_with_api()
                .with_fallback(|| async { "caught" })
                .build_routes();

            let (status, body) = get(routes, "/api/v1/data").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, "API V1");
        }

        #[tokio::test]
        async fn the_fallback_does_not_shadow_health_probes() {
            let routes = builder_with_api()
                .with_fallback(|| async { "caught" })
                .build_routes();

            let (status, body) = get(routes, "/health").await;
            assert_eq!(status, StatusCode::OK);
            assert_ne!(body, "caught");
        }

        #[tokio::test]
        async fn a_fallback_service_catches_unmatched_paths() {
            // The proxy shape: a tower::Service, not a handler.
            let upstream = tower::service_fn(|_req: Request| async move {
                Ok::<_, std::convert::Infallible>(Response::new(Body::from("upstream")))
            });

            let routes = builder_with_api()
                .with_fallback_service(upstream)
                .build_routes();

            let (status, body) = get(routes, "/merchant/checkout").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, "upstream");
        }

        #[tokio::test]
        async fn a_fallback_service_does_not_shadow_versioned_routes() {
            let upstream = tower::service_fn(|_req: Request| async move {
                Ok::<_, std::convert::Infallible>(Response::new(Body::from("upstream")))
            });

            let routes = builder_with_api()
                .with_fallback_service(upstream)
                .build_routes();

            let (status, body) = get(routes, "/api/v1/data").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, "API V1");
        }

        #[tokio::test]
        async fn the_last_fallback_wins() {
            let routes = builder_with_api()
                .with_fallback(|| async { "first" })
                .with_fallback(|| async { "second" })
                .build_routes();

            let (_, body) = get(routes, "/unmatched").await;
            assert_eq!(body, "second");
        }

        #[tokio::test]
        async fn a_fallback_works_with_no_versions_registered() {
            // A pure proxy registers no versioned routes at all.
            let routes = VersionedApiBuilder::new()
                .with_fallback(|| async { "proxied" })
                .build_routes();

            let (status, body) = get(routes, "/upstream/path").await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, "proxied");
        }

        #[tokio::test]
        #[cfg(feature = "htmx")]
        async fn a_fallback_coexists_with_frontend_routes() {
            let routes = builder_with_api()
                .with_frontend_routes(|router| {
                    router.route("/", axum::routing::get(|| async { "Home" }))
                })
                .with_fallback(|| async { "caught" })
                .build_routes();

            let (_, home) = get(routes, "/").await;
            assert_eq!(home, "Home");

            let routes = builder_with_api()
                .with_frontend_routes(|router| {
                    router.route("/", axum::routing::get(|| async { "Home" }))
                })
                .with_fallback(|| async { "caught" })
                .build_routes();

            let (_, other) = get(routes, "/somewhere-else").await;
            assert_eq!(other, "caught");
        }

        #[tokio::test]
        #[cfg(feature = "htmx")]
        async fn with_fallback_overrides_a_frontend_fallback_without_panicking() {
            // Merging two routers that both carry a fallback panics inside
            // axum. Applying ours after the merge replaces theirs instead.
            let routes = builder_with_api()
                .with_frontend_routes(|router| router.fallback(|| async { "frontend" }))
                .with_fallback(|| async { "explicit" })
                .build_routes();

            let (_, body) = get(routes, "/unmatched").await;
            assert_eq!(body, "explicit");
        }
    }
}
