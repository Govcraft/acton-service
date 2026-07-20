//! OpenAPI documentation utilities
//!
//! This module provides utilities for generating OpenAPI/Swagger documentation
//! using the utoipa crate. It integrates with the versioning and responses modules
//! to provide complete API documentation.
//!
//! ## Features
//!
//! - Automatic OpenAPI 3.0 spec generation
//! - Swagger UI integration
//! - ReDoc UI integration
//! - Support for API versioning
//! - Type-safe schemas from Rust types
//!
//! ## Example
//!
//! ```rust,ignore
//! use acton_service::prelude::*;
//! use acton_service::openapi::{OpenApiBuilder, SwaggerUI};
//! use utoipa::{OpenApi, ToSchema};
//!
//! #[derive(Serialize, Deserialize, ToSchema)]
//! struct User {
//!     id: u64,
//!     name: String,
//! }
//!
//! #[utoipa::path(
//!     get,
//!     path = "/users",
//!     responses(
//!         (status = 200, description = "List users", body = Vec<User>)
//!     )
//! )]
//! async fn list_users() -> Json<Vec<User>> {
//!     Json(vec![])
//! }
//!
//! #[derive(OpenApi)]
//! #[openapi(paths(list_users), components(schemas(User)))]
//! struct ApiDoc;
//!
//! let app = Router::new()
//!     .merge(SwaggerUI::with_spec("/swagger-ui", ApiDoc::openapi()));
//! ```

use axum::Router;
use utoipa_swagger_ui::SwaggerUi;

/// Builder for creating OpenAPI documentation with Swagger UI
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::openapi::OpenApiBuilder;
/// use utoipa::OpenApi;
///
/// #[derive(OpenApi)]
/// #[openapi(paths(get_users, create_user))]
/// struct ApiDoc;
///
/// let api_docs = OpenApiBuilder::new(ApiDoc::openapi())
///     .title("My API")
///     .version("1.0.0")
///     .description("API for managing users")
///     .build();
/// ```
pub struct OpenApiBuilder {
    openapi: utoipa::openapi::OpenApi,
}

impl OpenApiBuilder {
    /// Create a new OpenAPI builder from an existing OpenApi instance
    pub fn new(openapi: utoipa::openapi::OpenApi) -> Self {
        Self { openapi }
    }

    /// Set the API title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.openapi.info.title = title.into();
        self
    }

    /// Set the API version
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.openapi.info.version = version.into();
        self
    }

    /// Set the API description
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.openapi.info.description = Some(description.into());
        self
    }

    /// Set the terms of service URL
    pub fn terms_of_service(mut self, terms: impl Into<String>) -> Self {
        self.openapi.info.terms_of_service = Some(terms.into());
        self
    }

    /// Set contact information
    pub fn contact(mut self, name: impl Into<String>, email: impl Into<String>) -> Self {
        use utoipa::openapi::ContactBuilder;
        self.openapi.info.contact = Some(
            ContactBuilder::new()
                .name(Some(name.into()))
                .email(Some(email.into()))
                .build(),
        );
        self
    }

    /// Set license information
    pub fn license(mut self, name: impl Into<String>, url: Option<String>) -> Self {
        use utoipa::openapi::LicenseBuilder;
        let mut builder = LicenseBuilder::new().name(name.into());
        if let Some(url) = url {
            builder = builder.url(Some(url));
        }
        self.openapi.info.license = Some(builder.build());
        self
    }

    /// Add a server URL
    pub fn server(mut self, url: impl Into<String>, description: Option<String>) -> Self {
        use utoipa::openapi::ServerBuilder;
        let mut builder = ServerBuilder::new().url(url.into());
        if let Some(desc) = description {
            builder = builder.description(Some(desc));
        }
        self.openapi
            .servers
            .get_or_insert_with(Vec::new)
            .push(builder.build());
        self
    }

    /// Build the final OpenAPI specification
    pub fn build(self) -> utoipa::openapi::OpenApi {
        self.openapi
    }
}

/// Swagger UI integration for OpenAPI documentation
///
/// Provides a router with Swagger UI at the specified path.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::openapi::SwaggerUI;
/// use utoipa::OpenApi;
///
/// #[derive(OpenApi)]
/// #[openapi(paths(get_users))]
/// struct ApiDoc;
///
/// let app = Router::new()
///     .merge(SwaggerUI::with_spec("/swagger-ui", ApiDoc::openapi()));
/// ```
pub struct SwaggerUI;

impl SwaggerUI {
    /// Create a Swagger UI router with OpenAPI specification
    ///
    /// # Arguments
    ///
    /// * `path` - The base path for Swagger UI (e.g., "/swagger-ui")
    /// * `openapi` - The OpenAPI specification
    pub fn with_spec(path: &'static str, openapi: utoipa::openapi::OpenApi) -> Router {
        SwaggerUi::new(path)
            .url("/api-docs/openapi.json", openapi)
            .into()
    }

    /// Create Swagger UI with multiple API versions
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let app = Router::new()
    ///     .merge(SwaggerUI::with_versions(
    ///         "/swagger-ui".to_string(),
    ///         vec![
    ///             ("/api-docs/v1/openapi.json".to_string(), v1_openapi),
    ///             ("/api-docs/v2/openapi.json".to_string(), v2_openapi),
    ///         ]
    ///     ));
    /// ```
    pub fn with_versions(
        path: String,
        versions: Vec<(String, utoipa::openapi::OpenApi)>,
    ) -> Router {
        let mut swagger_ui = SwaggerUi::new(path);

        for (url, openapi) in versions {
            swagger_ui = swagger_ui.url(url, openapi);
        }

        swagger_ui.into()
    }
}

/// RapiDoc UI integration for OpenAPI documentation
///
/// Provides an alternative documentation UI to Swagger
pub struct RapiDoc;

impl RapiDoc {
    /// Create a RapiDoc UI endpoint
    ///
    /// Returns HTML that loads RapiDoc with the OpenAPI spec
    pub fn html(spec_url: &str) -> String {
        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>API Documentation</title>
    <script type="module" src="https://unpkg.com/rapidoc/dist/rapidoc-min.js"></script>
</head>
<body>
    <rapi-doc
        spec-url="{}"
        theme="dark"
        render-style="read"
        show-header="true"
        allow-try="true"
        allow-server-selection="true"
    ></rapi-doc>
</body>
</html>"#,
            spec_url
        )
    }
}

/// ReDoc UI integration for OpenAPI documentation
///
/// Provides a clean, three-panel documentation layout
pub struct ReDoc;

impl ReDoc {
    /// Create a ReDoc UI endpoint
    ///
    /// Returns HTML that loads ReDoc with the OpenAPI spec
    pub fn html(spec_url: &str) -> String {
        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>API Documentation</title>
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <link href="https://fonts.googleapis.com/css?family=Montserrat:300,400,700|Roboto:300,400,700" rel="stylesheet">
    <style>
        body {{
            margin: 0;
            padding: 0;
        }}
    </style>
</head>
<body>
    <redoc spec-url='{}'></redoc>
    <script src="https://cdn.redoc.ly/redoc/latest/bundles/redoc.standalone.js"></script>
</body>
</html>"#,
            spec_url
        )
    }
}

/// Helper macro to derive OpenAPI components from response types
///
/// This macro simplifies deriving ToSchema for response types
#[cfg(feature = "openapi")]
#[macro_export]
macro_rules! openapi_response {
    ($name:ident) => {
        impl utoipa::ToSchema for $name {
            fn schema() -> (
                &'static str,
                utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
            ) {
                (
                    stringify!($name),
                    utoipa::openapi::ObjectBuilder::new()
                        .schema_type(utoipa::openapi::SchemaType::Object)
                        .into(),
                )
            }
        }
    };
}

/// GraphQL endpoint documentation (cosmetic listing in the OpenAPI spec).
///
/// Adds one `POST /{base}/v{n}/graphql` entry per registered API version so
/// the Swagger/ReDoc UI surfaces the GraphQL endpoint alongside REST routes.
/// The request body is documented as a generic `application/json` payload —
/// for a typed schema, point the consumer at the `/graphql` endpoint itself,
/// which serves GraphiQL with full introspection.
#[cfg(feature = "graphql")]
pub mod graphql {
    use utoipa::openapi::{
        path::{HttpMethod, OperationBuilder, PathItem},
        request_body::RequestBodyBuilder,
        ContentBuilder, OpenApi, ResponseBuilder, ResponsesBuilder,
    };

    use crate::versioning::ApiVersion;

    /// Augment an OpenAPI spec with a POST entry for each registered GraphQL
    /// version.
    pub fn add_graphql_paths(
        mut openapi: OpenApi,
        base_path: Option<&str>,
        versions: &[ApiVersion],
    ) -> OpenApi {
        for version in versions {
            let path = match base_path {
                Some(b) => format!("{}/{}/graphql", b, version.as_path_segment()),
                None => format!("/{}/graphql", version.as_path_segment()),
            };

            let request_body = RequestBodyBuilder::new()
                .description(Some("GraphQL request payload"))
                .content(
                    "application/json",
                    ContentBuilder::new()
                        .schema(Some(json_object_schema()))
                        .build(),
                )
                .required(Some(utoipa::openapi::Required::True))
                .build();

            let responses = ResponsesBuilder::new()
                .response(
                    "200",
                    ResponseBuilder::new()
                        .description("GraphQL response (data and/or errors)")
                        .content(
                            "application/json",
                            ContentBuilder::new()
                                .schema(Some(json_object_schema()))
                                .build(),
                        ),
                )
                .build();

            let operation = OperationBuilder::new()
                .summary(Some(format!(
                    "GraphQL endpoint ({})",
                    version.as_path_segment()
                )))
                .description(Some(
                    "Execute a GraphQL query or mutation. GET on the same path serves GraphiQL.",
                ))
                .tag("graphql")
                .request_body(Some(request_body))
                .responses(responses)
                .build();

            let path_item = PathItem::new(HttpMethod::Post, operation);
            openapi.paths.paths.insert(path, path_item);
        }
        openapi
    }

    fn json_object_schema() -> utoipa::openapi::RefOr<utoipa::openapi::Schema> {
        use utoipa::openapi::{ObjectBuilder, Type};
        utoipa::openapi::RefOr::T(utoipa::openapi::Schema::Object(
            ObjectBuilder::new().schema_type(Type::Object).build(),
        ))
    }

    /// Convenience wrapper that infers the version list from a
    /// [`VersionedGraphQL`](crate::graphql::VersionedGraphQL).
    pub fn add_paths_from_versioned(
        openapi: OpenApi,
        graphql: &crate::graphql::VersionedGraphQL,
    ) -> OpenApi {
        let versions: Vec<ApiVersion> = graphql.versions().collect();
        add_graphql_paths(openapi, graphql.base_path(), &versions)
    }
}

/// OpenAPI security scheme helpers
pub mod security {
    use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};

    /// Create a Bearer token security scheme (for JWT)
    pub fn bearer_auth() -> SecurityScheme {
        SecurityScheme::Http(
            HttpBuilder::new()
                .scheme(HttpAuthScheme::Bearer)
                .bearer_format("JWT")
                .build(),
        )
    }

    /// Create an API key security scheme (header-based)
    pub fn api_key_header(name: &str) -> SecurityScheme {
        use utoipa::openapi::security::{ApiKey, ApiKeyValue};
        SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new(name)))
    }

    /// Create an API key security scheme (query parameter)
    pub fn api_key_query(name: &str) -> SecurityScheme {
        use utoipa::openapi::security::{ApiKey, ApiKeyValue};
        SecurityScheme::ApiKey(ApiKey::Query(ApiKeyValue::new(name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openapi_builder() {
        let openapi = utoipa::openapi::OpenApiBuilder::new()
            .info(
                utoipa::openapi::InfoBuilder::new()
                    .title("Test")
                    .version("1.0.0")
                    .build(),
            )
            .build();

        let builder = OpenApiBuilder::new(openapi)
            .title("My API")
            .version("2.0.0")
            .description("Test API")
            .contact("Test User", "test@example.com")
            .license(
                "MIT",
                Some("https://opensource.org/licenses/MIT".to_string()),
            )
            .server("https://api.example.com", Some("Production".to_string()));

        let result = builder.build();
        assert_eq!(result.info.title, "My API");
        assert_eq!(result.info.version, "2.0.0");
        assert_eq!(result.info.description, Some("Test API".to_string()));
        assert!(result.servers.is_some());
    }

    #[test]
    fn test_rapidoc_html() {
        let html = RapiDoc::html("/api-docs/openapi.json");
        assert!(html.contains("rapidoc"));
        assert!(html.contains("/api-docs/openapi.json"));
    }

    #[test]
    fn test_redoc_html() {
        let html = ReDoc::html("/api-docs/openapi.json");
        assert!(html.contains("redoc"));
        assert!(html.contains("/api-docs/openapi.json"));
    }
}
