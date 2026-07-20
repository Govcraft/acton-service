//! Cedar authorization middleware for HTTP and gRPC
//!
//! This middleware integrates AWS Cedar policy-based authorization into acton-service.
//! It validates authorization requests against Cedar policies after JWT authentication.

use axum::{
    body::Body,
    extract::{MatchedPath, Request, State},
    http::{HeaderMap, Method},
    middleware::Next,
    response::Response,
};
use cedar_policy::{
    Authorizer, Context, Decision, Entities, EntityUid, PolicySet, Request as CedarRequest,
};
use chrono::{Datelike, Timelike};
use figment;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    config::{CedarConfig, Config},
    error::Error,
    middleware::token::Claims,
};

/// Builder for Cedar authorization middleware
///
/// Use this to construct a `CedarAuthz` instance with custom configuration.
///
/// # Examples
///
/// Simple case (defaults):
/// ```rust,ignore
/// let cedar = CedarAuthz::builder(cedar_config)
///     .build()
///     .await?;
/// ```
///
/// With custom path normalizer:
/// ```rust,ignore
/// let cedar = CedarAuthz::builder(cedar_config)
///     .with_path_normalizer(normalize_fn)
///     .build()
///     .await?;
/// ```
///
/// Full customization:
/// ```rust,ignore
/// let cedar = CedarAuthz::builder(cedar_config)
///     .with_path_normalizer(normalize_fn)
///     .with_cache(redis_cache)
///     .build()
///     .await?;
/// ```
pub struct CedarAuthzBuilder {
    config: CedarConfig,
    path_normalizer: Option<fn(&str) -> String>,
    #[cfg(feature = "cache")]
    cache: Option<Arc<dyn PolicyCache>>,
}

impl CedarAuthzBuilder {
    /// Create a new builder with the given configuration
    pub fn new(config: CedarConfig) -> Self {
        Self {
            config,
            path_normalizer: None,
            #[cfg(feature = "cache")]
            cache: None,
        }
    }

    /// Set a custom path normalizer
    ///
    /// By default, Cedar uses a generic path normalizer that replaces UUIDs and numeric IDs
    /// with `{id}` placeholders. Use this method to provide custom normalization logic for
    /// your application's specific path patterns.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn custom_normalizer(path: &str) -> String {
    ///     // Example: /articles/my-article-slug-123 -> /articles/{slug}
    ///     path.replace("/articles/", "/articles/{slug}/")
    /// }
    ///
    /// let cedar = CedarAuthz::builder(cedar_config)
    ///     .with_path_normalizer(custom_normalizer)
    ///     .build()
    ///     .await?;
    /// ```
    pub fn with_path_normalizer(mut self, normalizer: fn(&str) -> String) -> Self {
        self.path_normalizer = Some(normalizer);
        self
    }

    /// Set policy cache (optional, for performance)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let redis_cache = RedisPolicyCache::new(redis_pool);
    ///
    /// let cedar = CedarAuthz::builder(cedar_config)
    ///     .with_cache(redis_cache)
    ///     .build()
    ///     .await?;
    /// ```
    #[cfg(feature = "cache")]
    pub fn with_cache<C: PolicyCache + 'static>(mut self, cache: C) -> Self {
        self.cache = Some(Arc::new(cache));
        self
    }

    /// Build the CedarAuthz instance (async)
    ///
    /// This loads the Cedar policies from the configured file path.
    pub async fn build(self) -> Result<CedarAuthz, Error> {
        // Load policies from file (using spawn_blocking for file I/O)
        let path = self.config.policy_path.clone();
        let policies = tokio::task::spawn_blocking(move || std::fs::read_to_string(&path))
            .await
            .map_err(|e| Error::Internal(format!("Task join error: {}", e)))?
            .map_err(|e| {
                Error::Config(Box::new(figment::Error::from(format!(
                    "Failed to read Cedar policy file from '{}': {}",
                    self.config.policy_path.display(),
                    e
                ))))
            })?;

        let policy_set: PolicySet = policies.parse().map_err(|e| {
            Error::Config(Box::new(figment::Error::from(format!(
                "Failed to parse Cedar policies: {}",
                e
            ))))
        })?;

        Ok(CedarAuthz {
            authorizer: Arc::new(Authorizer::new()),
            policy_set: Arc::new(RwLock::new(policy_set)),
            config: Arc::new(self.config),
            #[cfg(feature = "cache")]
            cache: self.cache,
            path_normalizer: self.path_normalizer,
        })
    }
}

/// Cedar authorization middleware state
#[derive(Clone)]
pub struct CedarAuthz {
    /// Cedar authorizer (stateless evaluator)
    authorizer: Arc<Authorizer>,

    /// Cedar policy set (policies loaded from file)
    policy_set: Arc<RwLock<PolicySet>>,

    /// Configuration
    config: Arc<CedarConfig>,

    /// Policy cache (optional, requires cache feature)
    #[cfg(feature = "cache")]
    cache: Option<Arc<dyn PolicyCache>>,

    /// Custom path normalizer (optional, defaults to normalize_path_generic)
    path_normalizer: Option<fn(&str) -> String>,
}

impl CedarAuthz {
    /// Create a builder for CedarAuthz
    ///
    /// This is the recommended way to construct CedarAuthz instances.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let cedar = CedarAuthz::builder(cedar_config)
    ///     .with_path_normalizer(normalize_fn)
    ///     .with_cache(redis_cache)
    ///     .build()
    ///     .await?;
    /// ```
    pub fn builder(config: CedarConfig) -> CedarAuthzBuilder {
        CedarAuthzBuilder::new(config)
    }

    /// Create CedarAuthz from config with defaults (convenience method)
    ///
    /// This is a shortcut for `CedarAuthz::builder(config).build().await`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let cedar = CedarAuthz::from_config(cedar_config).await?;
    /// ```
    pub async fn from_config(config: CedarConfig) -> Result<Self, Error> {
        Self::builder(config).build().await
    }

    /// Create CedarAuthz from full app config (convenience method)
    ///
    /// Automatically extracts Cedar config and creates the middleware if Cedar is enabled.
    /// Returns `None` if Cedar is disabled or not configured.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(cedar) = CedarAuthz::from_app_config(&config).await? {
    ///     // Use cedar middleware
    /// }
    /// ```
    pub async fn from_app_config(config: &Config) -> Result<Option<Self>, Error> {
        match &config.cedar {
            Some(cedar_config) if cedar_config.enabled => {
                Ok(Some(Self::from_config(cedar_config.clone()).await?))
            }
            _ => Ok(None),
        }
    }

    /// Evaluate a Cedar authorization request.
    ///
    /// This is the shared policy-evaluation entry point used by the HTTP
    /// middleware, the gRPC Tower layer, and resolver-level checks (e.g.
    /// `acton_service::graphql::CedarResolverCheck`). It builds the entity
    /// hierarchy from the supplied claims, consults the cache (if enabled),
    /// runs the policy set against the request, and writes the result back
    /// to the cache. The result honors the `fail_open` config flag: when
    /// `fail_open == true` a denial is reported as `Decision::Allow` and a
    /// warning is logged.
    ///
    /// Returns `Error::Internal` on Cedar construction failures and surfaces
    /// cache errors verbatim.
    pub async fn authorize(
        &self,
        principal: &EntityUid,
        action: &EntityUid,
        resource: &EntityUid,
        context: Context,
        claims: &Claims,
    ) -> Result<Decision, Error> {
        let cedar_request = CedarRequest::new(
            principal.clone(),
            action.clone(),
            resource.clone(),
            context,
            None,
        )
        .map_err(|e| Error::Internal(format!("Failed to build Cedar request: {}", e)))?;

        // Check cache (if enabled).
        #[cfg(feature = "cache")]
        if let Some(cache) = &self.cache {
            if let Some(decision) = cache.get(&cedar_request).await? {
                return Ok(self.apply_fail_open(decision, principal, action));
            }
        }

        let policy_set = self.policy_set.read().await;
        let entities = build_entities(claims)?;

        let raw_decision = self
            .authorizer
            .is_authorized(&cedar_request, &policy_set, &entities)
            .decision();

        // Cache decision (if enabled).
        #[cfg(feature = "cache")]
        if let Some(cache) = &self.cache {
            let _ = cache
                .set(&cedar_request, raw_decision, self.config.cache_ttl_secs)
                .await;
        }

        Ok(self.apply_fail_open(raw_decision, principal, action))
    }

    /// Apply the `fail_open` policy and log denials.
    fn apply_fail_open(
        &self,
        decision: Decision,
        principal: &EntityUid,
        action: &EntityUid,
    ) -> Decision {
        match decision {
            Decision::Allow => Decision::Allow,
            Decision::Deny => {
                tracing::warn!(
                    principal = ?principal,
                    action = ?action,
                    "Cedar policy denied request"
                );
                if self.config.fail_open {
                    tracing::warn!("Cedar policy denied but fail_open=true, allowing request");
                    Decision::Allow
                } else {
                    Decision::Deny
                }
            }
        }
    }

    /// Middleware function to evaluate Cedar policies (HTTP)
    pub async fn middleware(
        State(authz): State<Self>,
        request: Request<Body>,
        next: Next,
    ) -> Result<Response, Error> {
        // Skip if Cedar is disabled
        if !authz.config.enabled {
            return Ok(next.run(request).await);
        }

        // Skip authorization for health and readiness endpoints
        let path = request.uri().path();
        if path == "/health" || path == "/ready" {
            return Ok(next.run(request).await);
        }

        // Extract JWT claims (inserted by JWT middleware)
        let claims = request
            .extensions()
            .get::<Claims>()
            .ok_or_else(|| {
                Error::Unauthorized(
                    "Missing JWT claims. Ensure JWT middleware runs before Cedar middleware."
                        .to_string(),
                )
            })?
            .clone();

        #[cfg(feature = "audit")]
        let audit_logger = request
            .extensions()
            .get::<crate::audit::AuditLogger>()
            .cloned();
        #[cfg(feature = "audit")]
        let audit_source = {
            use crate::audit::event::AuditSource;
            AuditSource {
                ip: request
                    .headers()
                    .get("x-forwarded-for")
                    .or_else(|| request.headers().get("x-real-ip"))
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.split(',').next().unwrap_or(s).trim().to_string()),
                user_agent: request
                    .headers()
                    .get("user-agent")
                    .and_then(|v| v.to_str().ok())
                    .map(String::from),
                subject: Some(claims.sub.clone()),
                request_id: request
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(String::from),
            }
        };

        // Extract request information
        let method = request.method().clone();

        // Build Cedar authorization request
        let principal = build_principal(&claims)?;
        let action = build_action_http(&method, &request, authz.path_normalizer)?;
        let context = build_context_http(request.headers(), &claims)?;
        let resource = build_resource()?;

        match authz
            .authorize(&principal, &action, &resource, context, &claims)
            .await?
        {
            Decision::Allow => Ok(next.run(request).await),
            Decision::Deny => {
                #[cfg(feature = "audit")]
                if let Some(ref logger) = audit_logger {
                    if logger.config().audit_auth_events {
                        logger
                            .log_auth(
                                crate::audit::event::AuditEventKind::AuthPermissionDenied,
                                crate::audit::event::AuditSeverity::Warning,
                                audit_source,
                            )
                            .await;
                    }
                }
                Err(Error::Forbidden("Access denied by policy".to_string()))
            }
        }
    }

    /// Reload policies from file (for hot-reload support)
    pub async fn reload_policies(&self) -> Result<(), Error> {
        let path = self.config.policy_path.clone();
        let policies = tokio::task::spawn_blocking(move || std::fs::read_to_string(&path))
            .await
            .map_err(|e| Error::Internal(format!("Task join error: {}", e)))?
            .map_err(|e| Error::Internal(format!("Failed to read policy file: {}", e)))?;

        let new_policy_set: PolicySet = policies
            .parse()
            .map_err(|e| Error::Internal(format!("Failed to parse policies: {}", e)))?;

        let mut policy_set = self.policy_set.write().await;
        *policy_set = new_policy_set;

        tracing::info!(
            "Cedar policies reloaded from {}",
            self.config.policy_path.display()
        );
        Ok(())
    }
}

/// Build Cedar resource entity
///
/// Returns a generic default resource for authorization checks.
/// Most authorization policies can be implemented using just the principal (user/roles)
/// and action (HTTP method + path), without needing typed resources.
///
/// For applications that need typed resources with attributes (e.g., Document::"doc_id"
/// with owner_id for ownership checks), this can be extended via a trait in the future.
fn build_resource() -> Result<EntityUid, Error> {
    r#"Resource::"default""#
        .parse()
        .map_err(|e| Error::Internal(format!("Failed to parse resource: {}", e)))
}

/// Build Cedar principal from JWT claims
fn build_principal(claims: &Claims) -> Result<EntityUid, Error> {
    // Principal format: User::"user:123" or Client::"client:abc"
    let principal_str = if claims.is_user() {
        format!(r#"User::"{}""#, claims.sub)
    } else if claims.is_client() {
        format!(r#"Client::"{}""#, claims.sub)
    } else {
        format!(r#"Principal::"{}""#, claims.sub)
    };

    let principal: EntityUid = principal_str
        .parse()
        .map_err(|e| Error::Internal(format!("Invalid principal: {}", e)))?;

    Ok(principal)
}

/// Build Cedar action from HTTP method and request
///
/// Uses Axum's MatchedPath to get the route pattern (most accurate).
/// Falls back to path normalization (custom or default) if MatchedPath is not available.
fn build_action_http(
    method: &Method,
    request: &Request<Body>,
    path_normalizer: Option<fn(&str) -> String>,
) -> Result<EntityUid, Error> {
    // Try to get Axum's matched path first (e.g., "/users/:id")
    let normalized_path = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_string())
        .unwrap_or_else(|| {
            // Use custom normalizer if provided, otherwise use default
            match path_normalizer {
                Some(normalizer) => normalizer(request.uri().path()),
                None => normalize_path_generic(request.uri().path()),
            }
        });

    let action_str = format!(r#"Action::"{} {}""#, method, normalized_path);

    let action: EntityUid = action_str
        .parse()
        .map_err(|e| Error::Internal(format!("Invalid action: {}", e)))?;

    // Debug logging to see what action was generated
    tracing::debug!(
        method = %method,
        path = %request.uri().path(),
        normalized = %normalized_path,
        action = %action,
        "Built Cedar action"
    );

    Ok(action)
}

/// Normalize path by replacing common ID patterns with placeholders
///
/// This is a generic fallback used when Axum's MatchedPath is not available.
/// It handles the most common ID patterns:
/// - UUIDs: replaced with {id}
/// - Numeric IDs: replaced with {id}
fn normalize_path_generic(path: &str) -> String {
    // Replace UUIDs with {id}
    let uuid_pattern =
        regex::Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}").unwrap();
    let path = uuid_pattern.replace_all(path, "{id}");

    // Replace numeric IDs at end of path segments
    let numeric_pattern = regex::Regex::new(r"/(\d+)(?:/|$)").unwrap();
    let path = numeric_pattern.replace_all(&path, "/{id}");

    path.to_string()
}

/// Build Cedar context from HTTP headers and claims
fn build_context_http(headers: &HeaderMap, claims: &Claims) -> Result<Context, Error> {
    let mut context_map = serde_json::Map::new();

    // Add user roles
    context_map.insert("roles".to_string(), json!(claims.roles));

    // Add permissions
    context_map.insert("permissions".to_string(), json!(claims.perms));

    // Add email if present
    if let Some(email) = &claims.email {
        context_map.insert("email".to_string(), json!(email));
    }

    // Add username if present
    if let Some(username) = &claims.username {
        context_map.insert("username".to_string(), json!(username));
    }

    // Add timestamp
    let now = chrono::Utc::now();
    context_map.insert(
        "timestamp".to_string(),
        json!({
            "unix": now.timestamp(),
            "hour": now.hour(),
            "dayOfWeek": now.weekday().to_string(),
        }),
    );

    // Add IP address (from X-Forwarded-For or X-Real-IP)
    if let Some(ip) = extract_client_ip(headers) {
        context_map.insert("ip".to_string(), json!(ip));
    }

    // Add request ID if present
    if let Some(request_id) = headers.get("x-request-id").and_then(|v| v.to_str().ok()) {
        context_map.insert("requestId".to_string(), json!(request_id));
    }

    // Add user-agent if present
    if let Some(user_agent) = headers.get("user-agent").and_then(|v| v.to_str().ok()) {
        context_map.insert("userAgent".to_string(), json!(user_agent));
    }

    Context::from_json_value(serde_json::Value::Object(context_map), None)
        .map_err(|e| Error::Internal(format!("Failed to build context: {}", e)))
}

/// Extract client IP from headers
fn extract_client_ip(headers: &HeaderMap) -> Option<String> {
    // Try X-Forwarded-For header first (for proxied requests)
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Ok(xff_str) = xff.to_str() {
            // Take first IP in comma-separated list
            return xff_str.split(',').next().map(|s| s.trim().to_string());
        }
    }

    // Try X-Real-IP header
    if let Some(xri) = headers.get("x-real-ip") {
        if let Ok(xri_str) = xri.to_str() {
            return Some(xri_str.to_string());
        }
    }

    None
}

/// Build entity hierarchy from claims
///
/// Creates the principal entity (User or Client) with roles and permissions.
/// This is sufficient for most authorization policies that check:
/// - Who is making the request (principal)
/// - What they want to do (action)
/// - What roles/permissions they have (in context)
fn build_entities(claims: &Claims) -> Result<Entities, Error> {
    use serde_json::Value;

    // Create principal entity (User or Client) with attributes
    let entity = json!({
        "uid": {
            "type": if claims.is_user() { "User" } else { "Client" },
            "id": claims.sub.clone()
        },
        "attrs": {
            "email": claims.email.clone().unwrap_or_default(),
            "roles": claims.roles.clone(),
            "permissions": claims.perms.clone(),
            "sub": claims.sub.clone(),
        },
        "parents": []
    });

    Entities::from_json_value(Value::Array(vec![entity]), None)
        .map_err(|e| Error::Internal(format!("Failed to build entities: {}", e)))
}

/// Trait for policy decision caching
#[cfg(feature = "cache")]
#[async_trait::async_trait]
pub trait PolicyCache: Send + Sync {
    async fn get(&self, request: &CedarRequest) -> Result<Option<Decision>, Error>;
    async fn set(
        &self,
        request: &CedarRequest,
        decision: Decision,
        ttl_secs: u64,
    ) -> Result<(), Error>;
}

/// Redis-based policy cache implementation
#[cfg(feature = "cache")]
pub struct RedisPolicyCache {
    pool: deadpool_redis::Pool,
}

#[cfg(feature = "cache")]
impl RedisPolicyCache {
    pub fn new(pool: deadpool_redis::Pool) -> Self {
        Self { pool }
    }

    fn cache_key(request: &CedarRequest) -> String {
        // Generate cache key from request
        // Format: cedar:authz:{principal}:{action}:{resource}
        format!(
            "cedar:authz:{}:{}:{}",
            request
                .principal()
                .map(|p| p.to_string())
                .unwrap_or_else(|| "None".to_string()),
            request
                .action()
                .map(|a| a.to_string())
                .unwrap_or_else(|| "None".to_string()),
            request
                .resource()
                .map(|r| r.to_string())
                .unwrap_or_else(|| "None".to_string()),
        )
    }
}

#[cfg(feature = "cache")]
#[async_trait::async_trait]
impl PolicyCache for RedisPolicyCache {
    async fn get(&self, request: &CedarRequest) -> Result<Option<Decision>, Error> {
        use deadpool_redis::redis::AsyncCommands;

        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| Error::Internal(format!("Redis connection failed: {}", e)))?;

        let key = Self::cache_key(request);
        let value: Option<String> = conn
            .get(&key)
            .await
            .map_err(|e| Error::Internal(format!("Redis GET failed: {}", e)))?;

        Ok(value.and_then(|v| match v.as_str() {
            "allow" => Some(Decision::Allow),
            "deny" => Some(Decision::Deny),
            _ => None,
        }))
    }

    async fn set(
        &self,
        request: &CedarRequest,
        decision: Decision,
        ttl_secs: u64,
    ) -> Result<(), Error> {
        use deadpool_redis::redis::AsyncCommands;

        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| Error::Internal(format!("Redis connection failed: {}", e)))?;

        let key = Self::cache_key(request);
        let value = match decision {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
        };

        conn.set_ex::<_, _, ()>(&key, value, ttl_secs)
            .await
            .map_err(|e| Error::Internal(format!("Redis SETEX failed: {}", e)))?;

        Ok(())
    }
}

// ============================================================================
// gRPC Tower Layer Implementation
// ============================================================================

#[cfg(feature = "grpc")]
use std::future::Future;
#[cfg(feature = "grpc")]
use std::pin::Pin;
#[cfg(feature = "grpc")]
use std::task::{Context as TaskContext, Poll};
#[cfg(feature = "grpc")]
use tonic::{server::NamedService, Status};
#[cfg(feature = "grpc")]
use tower::{Layer, Service};

/// Tower Layer for Cedar authorization in gRPC services
///
/// Operates at the HTTP level (`http::Request<B>` → `http::Response<B>`), the
/// shape of tonic's generated servers, so a wrapped service can be registered
/// with [`GrpcServicesBuilder::add_service`](crate::grpc::server::GrpcServicesBuilder::add_service)
/// (the `NamedService` impl forwards the inner service's name). Authorization
/// requires [`Claims`] in the request extensions, so an HTTP-level
/// authentication layer such as
/// [`GrpcTokenAuthLayer`](crate::grpc::middleware::GrpcTokenAuthLayer) must
/// wrap this layer (run before it). Denials are returned as gRPC status
/// responses (`PERMISSION_DENIED`), not transport errors.
///
/// # Example
/// ```ignore
/// let cedar_layer = CedarAuthzLayer::new(cedar);
/// let auth_layer = GrpcTokenAuthLayer::new(paseto_auth);
///
/// let services = GrpcServicesBuilder::new()
///     .add_service(auth_layer.layer(cedar_layer.layer(MyServiceServer::new(svc))))
///     .build(None);
/// ```
///
/// When Cedar and token auth are configured through [`Config`], the framework
/// applies both to all gRPC routes automatically; this layer is for manual
/// composition.
#[cfg(feature = "grpc")]
#[derive(Clone)]
pub struct CedarAuthzLayer {
    authz: CedarAuthz,
}

#[cfg(feature = "grpc")]
impl CedarAuthzLayer {
    /// Create a new Cedar authorization layer for gRPC
    pub fn new(authz: CedarAuthz) -> Self {
        Self { authz }
    }
}

#[cfg(feature = "grpc")]
impl<S> Layer<S> for CedarAuthzLayer {
    type Service = CedarAuthzService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CedarAuthzService {
            inner,
            authz: self.authz.clone(),
        }
    }
}

/// Tower Service for Cedar authorization in gRPC
///
/// See [`CedarAuthzLayer`] for usage and composition requirements.
#[cfg(feature = "grpc")]
#[derive(Clone)]
pub struct CedarAuthzService<S> {
    inner: S,
    authz: CedarAuthz,
}

#[cfg(feature = "grpc")]
impl<S: NamedService> NamedService for CedarAuthzService<S> {
    const NAME: &'static str = S::NAME;
}

#[cfg(feature = "grpc")]
impl<S, ReqBody, ResBody> Service<http::Request<ReqBody>> for CedarAuthzService<S>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
    ResBody: Default + Send + 'static,
{
    type Response = http::Response<ResBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut TaskContext<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        // Take the ready inner service and leave a fresh clone in its place,
        // so the readiness obtained via poll_ready is the one consumed here.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        let authz = self.authz.clone();

        // Owned copies, so the authorization future does not borrow the
        // request (whose body type need not be Sync) across an await point.
        let headers = req.headers().clone();
        let extensions = req.extensions().clone();
        let method_path = req.uri().path().to_string();

        Box::pin(async move {
            match authorize_grpc_request(&authz, &headers, &extensions, &method_path).await {
                Ok(()) => inner.call(req).await,
                Err(status) => Ok(status.into_http()),
            }
        })
    }
}

/// Evaluate Cedar policies for a gRPC request at the HTTP level.
///
/// The gRPC method (`/package.Service/Method`) is read from the request URI
/// path and the metadata-derived fields from the HTTP headers, which carry
/// the same data tonic exposes as `MetadataMap`. Health and reflection
/// service methods are exempt, mirroring the `/health` and `/ready`
/// exemptions on the HTTP side.
#[cfg(feature = "grpc")]
async fn authorize_grpc_request(
    authz: &CedarAuthz,
    headers: &HeaderMap,
    extensions: &http::Extensions,
    method_path: &str,
) -> Result<(), Status> {
    // Skip if Cedar is disabled
    if !authz.config.enabled {
        return Ok(());
    }

    if crate::grpc::middleware::is_grpc_infra_path(method_path) {
        return Ok(());
    }

    // Extract claims from request extensions (set by a token auth layer)
    let claims = extensions
        .get::<Claims>()
        .ok_or_else(|| {
            Status::unauthenticated(
                "Missing authentication claims. Ensure a token authentication \
                 layer runs before the Cedar layer.",
            )
        })?
        .clone();

    // Build Cedar authorization request
    let principal =
        build_principal(&claims).map_err(|_| Status::internal("Failed to build principal"))?;

    let action =
        build_action_grpc(method_path).map_err(|_| Status::internal("Failed to build action"))?;

    let context = build_context_grpc(headers, &claims)
        .map_err(|_| Status::internal("Failed to build context"))?;

    let resource = build_resource().map_err(|_| Status::internal("Failed to parse resource"))?;

    let decision = authz
        .authorize(&principal, &action, &resource, context, &claims)
        .await
        .map_err(|e| Status::internal(format!("Cedar authorization error: {}", e)))?;

    match decision {
        Decision::Allow => Ok(()),
        Decision::Deny => {
            tracing::warn!(
                method = %method_path,
                "Cedar policy denied gRPC request"
            );
            #[cfg(feature = "audit")]
            if let Some(logger) = extensions.get::<crate::audit::AuditLogger>() {
                if logger.config().audit_auth_events {
                    use crate::audit::event::AuditSource;
                    let source = AuditSource {
                        ip: extract_grpc_client_ip(headers),
                        user_agent: headers
                            .get("user-agent")
                            .and_then(|v| v.to_str().ok())
                            .map(String::from),
                        subject: Some(claims.sub.clone()),
                        request_id: headers
                            .get("x-request-id")
                            .and_then(|v| v.to_str().ok())
                            .map(String::from),
                    };
                    logger
                        .log_auth(
                            crate::audit::event::AuditEventKind::AuthPermissionDenied,
                            crate::audit::event::AuditSeverity::Warning,
                            source,
                        )
                        .await;
                }
            }
            Err(Status::permission_denied("Access denied by policy"))
        }
    }
}

/// Build Cedar action from gRPC method path
///
/// gRPC method paths are in format: /package.Service/Method
/// We convert to Cedar action: Action::"/package.Service/Method"
#[cfg(feature = "grpc")]
fn build_action_grpc(method_path: &str) -> Result<EntityUid, Error> {
    let action_str = format!(r#"Action::"{}""#, method_path);
    let action: EntityUid = action_str
        .parse()
        .map_err(|e| Error::Internal(format!("Invalid gRPC action: {}", e)))?;
    Ok(action)
}

/// Build Cedar context from gRPC request headers and claims
///
/// gRPC metadata is carried in HTTP headers, so reading headers here is
/// equivalent to reading tonic's `MetadataMap`.
#[cfg(feature = "grpc")]
fn build_context_grpc(headers: &HeaderMap, claims: &Claims) -> Result<Context, Error> {
    let mut context_map = serde_json::Map::new();

    // Add user roles
    context_map.insert("roles".to_string(), json!(claims.roles));

    // Add permissions
    context_map.insert("permissions".to_string(), json!(claims.perms));

    // Add email if present
    if let Some(email) = &claims.email {
        context_map.insert("email".to_string(), json!(email));
    }

    // Add username if present
    if let Some(username) = &claims.username {
        context_map.insert("username".to_string(), json!(username));
    }

    // Add timestamp
    let now = chrono::Utc::now();
    context_map.insert(
        "timestamp".to_string(),
        json!({
            "unix": now.timestamp(),
            "hour": now.hour(),
            "dayOfWeek": now.weekday().to_string(),
        }),
    );

    // Add IP address from gRPC request headers
    if let Some(ip) = extract_grpc_client_ip(headers) {
        context_map.insert("ip".to_string(), json!(ip));
    }

    // Add request ID if present
    if let Some(request_id) = headers.get("x-request-id").and_then(|v| v.to_str().ok()) {
        context_map.insert("requestId".to_string(), json!(request_id));
    }

    // Add user-agent if present
    if let Some(user_agent) = headers.get("user-agent").and_then(|v| v.to_str().ok()) {
        context_map.insert("userAgent".to_string(), json!(user_agent));
    }

    Context::from_json_value(serde_json::Value::Object(context_map), None)
        .map_err(|e| Error::Internal(format!("Failed to build gRPC context: {}", e)))
}

/// Extract client IP from gRPC request headers
#[cfg(feature = "grpc")]
fn extract_grpc_client_ip(headers: &HeaderMap) -> Option<String> {
    // Try X-Forwarded-For header first
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Ok(xff_str) = xff.to_str() {
            return xff_str.split(',').next().map(|s| s.trim().to_string());
        }
    }

    // Try X-Real-IP header
    if let Some(xri) = headers.get("x-real-ip") {
        if let Ok(xri_str) = xri.to_str() {
            return Some(xri_str.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_generic() {
        assert_eq!(
            normalize_path_generic("/api/v1/users/123"),
            "/api/v1/users/{id}"
        );
        assert_eq!(
            normalize_path_generic("/api/v1/users/550e8400-e29b-41d4-a716-446655440000"),
            "/api/v1/users/{id}"
        );
        assert_eq!(normalize_path_generic("/api/v1/users"), "/api/v1/users");
    }

    #[test]
    fn test_build_principal() {
        let claims = Claims {
            sub: "user:123".to_string(),
            email: Some("test@example.com".to_string()),
            username: Some("testuser".to_string()),
            roles: vec!["user".to_string()],
            perms: vec![],
            exp: 0,
            iat: None,
            jti: None,
            iss: None,
            aud: None,
            custom: Default::default(),
        };

        let principal = build_principal(&claims).unwrap();
        assert_eq!(principal.to_string(), r#"User::"user:123""#);
    }

    // Note: test_build_action_http removed as it requires constructing a full Request<Body>
    // which is complex. The path normalization logic is tested via test_normalize_path_generic.
    // Integration tests should cover the full middleware flow.

    #[cfg(feature = "grpc")]
    mod grpc_layer {
        use super::super::*;
        use std::convert::Infallible;

        fn test_claims() -> Claims {
            Claims {
                sub: "user:123".to_string(),
                email: None,
                username: None,
                roles: vec!["user".to_string()],
                perms: vec![],
                exp: 0,
                iat: None,
                jti: None,
                iss: None,
                aud: None,
                custom: Default::default(),
            }
        }

        async fn test_authz(policy: &str, enabled: bool) -> CedarAuthz {
            let policy_file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(policy_file.path(), policy).unwrap();
            let config = CedarConfig {
                enabled,
                policy_path: policy_file.path().to_path_buf(),
                hot_reload: false,
                hot_reload_interval_secs: 60,
                cache_enabled: false,
                cache_ttl_secs: 60,
                fail_open: false,
            };
            CedarAuthz::builder(config).build().await.unwrap()
        }

        /// Minimal HTTP-level service in the shape of a tonic generated server
        #[derive(Clone)]
        struct TestSvc;

        impl NamedService for TestSvc {
            const NAME: &'static str = "test.v1.TestService";
        }

        impl Service<http::Request<String>> for TestSvc {
            type Response = http::Response<String>;
            type Error = Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(&mut self, _cx: &mut TaskContext<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }

            fn call(&mut self, _req: http::Request<String>) -> Self::Future {
                std::future::ready(Ok(http::Response::new("ok".to_string())))
            }
        }

        fn grpc_request(path: &str, claims: Option<Claims>) -> http::Request<String> {
            let mut req = http::Request::builder()
                .uri(path)
                .body(String::new())
                .unwrap();
            if let Some(claims) = claims {
                req.extensions_mut().insert(claims);
            }
            req
        }

        fn grpc_status(resp: &http::Response<String>) -> Option<&str> {
            resp.headers()
                .get("grpc-status")
                .and_then(|v| v.to_str().ok())
        }

        #[test]
        fn named_service_impl_forwards_the_inner_name() {
            assert_eq!(
                <CedarAuthzService<TestSvc> as NamedService>::NAME,
                "test.v1.TestService"
            );
        }

        #[tokio::test]
        async fn permitted_request_reaches_the_inner_service() {
            let authz = test_authz("permit(principal, action, resource);", true).await;
            let mut svc = CedarAuthzLayer::new(authz).layer(TestSvc);
            let resp = svc
                .call(grpc_request("/test.v1.TestService/Do", Some(test_claims())))
                .await
                .unwrap();
            assert_eq!(resp.body(), "ok");
        }

        #[tokio::test]
        async fn denied_request_gets_permission_denied_not_a_transport_error() {
            // An empty policy set denies by default (fail_open = false)
            let authz = test_authz("", true).await;
            let mut svc = CedarAuthzLayer::new(authz).layer(TestSvc);
            let resp = svc
                .call(grpc_request("/test.v1.TestService/Do", Some(test_claims())))
                .await
                .unwrap();
            // tonic Code::PermissionDenied == 7
            assert_eq!(grpc_status(&resp), Some("7"));
        }

        #[tokio::test]
        async fn missing_claims_is_unauthenticated() {
            let authz = test_authz("permit(principal, action, resource);", true).await;
            let mut svc = CedarAuthzLayer::new(authz).layer(TestSvc);
            let resp = svc
                .call(grpc_request("/test.v1.TestService/Do", None))
                .await
                .unwrap();
            // tonic Code::Unauthenticated == 16
            assert_eq!(grpc_status(&resp), Some("16"));
        }

        #[tokio::test]
        async fn disabled_cedar_passes_through() {
            let authz = test_authz("", false).await;
            let mut svc = CedarAuthzLayer::new(authz).layer(TestSvc);
            let resp = svc
                .call(grpc_request("/test.v1.TestService/Do", None))
                .await
                .unwrap();
            assert_eq!(resp.body(), "ok");
        }

        #[tokio::test]
        async fn health_service_is_exempt() {
            // Deny-everything policy set, no claims: the health service must
            // still be reachable for infrastructure probes.
            let authz = test_authz("", true).await;
            let mut svc = CedarAuthzLayer::new(authz).layer(TestSvc);
            let resp = svc
                .call(grpc_request("/grpc.health.v1.Health/Check", None))
                .await
                .unwrap();
            assert_eq!(resp.body(), "ok");
        }

        #[tokio::test]
        async fn cedar_wrapped_service_registers_with_the_grpc_builder() {
            use tonic::body::Body as TonicBody;

            /// Tonic-body twin of TestSvc to satisfy `add_service` bounds
            #[derive(Clone)]
            struct TonicSvc;

            impl NamedService for TonicSvc {
                const NAME: &'static str = "test.v1.TonicService";
            }

            impl Service<http::Request<TonicBody>> for TonicSvc {
                type Response = http::Response<TonicBody>;
                type Error = Infallible;
                type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

                fn poll_ready(
                    &mut self,
                    _cx: &mut TaskContext<'_>,
                ) -> Poll<Result<(), Self::Error>> {
                    Poll::Ready(Ok(()))
                }

                fn call(&mut self, _req: http::Request<TonicBody>) -> Self::Future {
                    std::future::ready(Ok(http::Response::new(TonicBody::empty())))
                }
            }

            // The point of this test is that the issue's repro now compiles:
            // a Cedar-wrapped (and auth-wrapped) tonic service is accepted by
            // GrpcServicesBuilder::add_service.
            let authz = test_authz("permit(principal, action, resource);", true).await;
            let _routes = crate::grpc::server::GrpcServicesBuilder::new()
                .add_service(CedarAuthzLayer::new(authz).layer(TonicSvc))
                .build(None);
        }
    }
}
