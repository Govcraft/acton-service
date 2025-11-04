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
use serde::{Deserialize, Serialize};
use std::fmt;

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
        if let Some(deprecation) = self.deprecation {
            self.router.layer(middleware::from_fn(move |req: Request, next: Next| {
                let deprecation = deprecation.clone();
                async move {
                    let mut response = next.run(req).await;
                    let headers = response.headers_mut();

                    // Add Deprecation header (RFC 8594)
                    if let Ok(value) = HeaderValue::from_str(&deprecation.deprecation_header()) {
                        headers.insert("Deprecation", value);
                    }

                    // Add Sunset header if configured (RFC 8594)
                    if let Some(sunset) = deprecation.sunset_header() {
                        if let Ok(value) = HeaderValue::from_str(&sunset) {
                            headers.insert("Sunset", value);
                        }
                    }

                    // Add Link header pointing to replacement version
                    if let Ok(value) = HeaderValue::from_str(&deprecation.link_header()) {
                        headers.insert(header::LINK, value);
                    }

                    // Add custom warning header if message is provided
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
            self.router
        }
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

/// Builder for creating versioned API routers with enforcement
///
/// This builder ensures that all routes are versioned and provides a structured
/// way to manage multiple API versions with deprecation support.
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
/// let api = VersionedApiBuilder::new()
///     .add_version(ApiVersion::V1, |routes| {
///         routes.route("/users", get(list_users_v1))
///     })
///     .add_version_deprecated(
///         ApiVersion::V2,
///         |routes| routes.route("/users", get(list_users_v2)),
///         DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
///             .with_sunset_date("2026-12-31T23:59:59Z")
///     )
///     .build();
/// ```
pub struct VersionedApiBuilder {
    versions: Vec<(ApiVersion, Router, Option<DeprecationInfo>)>,
    base_path: Option<String>,
}

impl Default for VersionedApiBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionedApiBuilder {
    /// Create a new versioned API builder
    pub fn new() -> Self {
        Self {
            versions: Vec::new(),
            base_path: None,
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
    ///     .build();
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
    /// # Example
    ///
    /// ```rust,ignore
    /// let api = VersionedApiBuilder::new()
    ///     .add_version(ApiVersion::V2, |routes| {
    ///         routes
    ///             .route("/users", get(list_users))
    ///             .route("/users/{id}", get(get_user))
    ///     })
    ///     .build();
    /// ```
    pub fn add_version<F>(mut self, version: ApiVersion, routes: F) -> Self
    where
        F: FnOnce(Router) -> Router,
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
    ///     .build();
    /// ```
    pub fn add_version_deprecated<F>(
        mut self,
        version: ApiVersion,
        routes: F,
        deprecation: DeprecationInfo,
    ) -> Self
    where
        F: FnOnce(Router) -> Router,
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

    /// Build the versioned API router
    ///
    /// Returns a Router with all versions properly nested under their
    /// version paths with deprecation middleware applied where configured.
    pub fn build(self) -> Router {
        let mut router = Router::new();

        for (version, version_router, deprecation) in self.versions {
            let versioned = if let Some(deprecation) = deprecation {
                versioned_router(version, version_router)
                    .deprecated(deprecation)
                    .into_router()
            } else {
                versioned_router(version, version_router).into_router()
            };

            let version_path = format!("/{}", version.as_path_segment());

            router = if let Some(ref base) = self.base_path {
                // Nest under base path + version: /api/v1, /api/v2, etc.
                let full_path = format!("{}{}", base, version_path);
                router.nest(&full_path, versioned)
            } else {
                // Nest directly under version: /v1, /v2, etc.
                router.nest(&version_path, versioned)
            };
        }

        router
    }

    /// Build versioned routes (opaque VersionedRoutes type)
    ///
    /// This creates a `VersionedRoutes` with all your versioned business routes.
    /// This is the ONLY public way to create `VersionedRoutes`.
    ///
    /// Use with ServiceBuilder for automatic health/readiness endpoints,
    /// or with manual Router construction where you add your own.
    pub fn build_routes(self) -> crate::service_builder::VersionedRoutes {
        crate::service_builder::VersionedRoutes::from_router(self.build())
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
        assert_eq!(
            extract_version_from_path("/v1/users"),
            Some(ApiVersion::V1)
        );
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
        assert_eq!(
            info.sunset_date,
            Some("2026-12-31T23:59:59Z".to_string())
        );
        assert_eq!(info.message, Some("Please migrate soon".to_string()));
    }

    #[test]
    fn test_deprecation_headers() {
        let info = DeprecationInfo::new(ApiVersion::V1, ApiVersion::V2)
            .with_sunset_date("2026-12-31T23:59:59Z");

        assert_eq!(info.deprecation_header(), "version=\"v1\"");
        assert_eq!(info.sunset_header(), Some("2026-12-31T23:59:59Z".to_string()));
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
}
