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
    config::CedarConfig,
    error::Error,
    middleware::jwt::Claims,
};

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
    /// Create a new Cedar authorization middleware
    pub async fn new(config: CedarConfig) -> Result<Self, Error> {
        // Load policies from file (using spawn_blocking for file I/O)
        let path = config.policy_path.clone();
        let policies = tokio::task::spawn_blocking(move || std::fs::read_to_string(&path))
            .await
            .map_err(|e| Error::Internal(format!("Task join error: {}", e)))?
            .map_err(|e| {
                Error::Config(Box::new(figment::Error::from(format!(
                    "Failed to read Cedar policy file from '{}': {}",
                    config.policy_path.display(),
                    e
                ))))
            })?;

        let policy_set: PolicySet = policies.parse().map_err(|e| {
            Error::Config(Box::new(figment::Error::from(format!(
                "Failed to parse Cedar policies: {}",
                e
            ))))
        })?;

        Ok(Self {
            authorizer: Arc::new(Authorizer::new()),
            policy_set: Arc::new(RwLock::new(policy_set)),
            config: Arc::new(config),
            #[cfg(feature = "cache")]
            cache: None,
            path_normalizer: None,
        })
    }

    /// Set policy cache (optional, for performance)
    #[cfg(feature = "cache")]
    pub fn with_cache<C: PolicyCache + 'static>(mut self, cache: C) -> Self {
        self.cache = Some(Arc::new(cache));
        self
    }

    /// Set a custom path normalizer
    ///
    /// By default, Cedar uses a generic path normalizer that replaces UUIDs and numeric IDs
    /// with `{id}` placeholders. Use this method to provide custom normalization logic for
    /// your application's specific path patterns.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use acton_service::middleware::cedar::CedarAuthz;
    /// use acton_service::config::CedarConfig;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let cedar_config = CedarConfig::default();
    /// // Define a custom normalizer for slug-based routes
    /// fn custom_normalizer(path: &str) -> String {
    ///     // Example: /articles/my-article-slug-123 -> /articles/{slug}
    ///     path.replace("/articles/", "/articles/{slug}/")
    /// }
    ///
    /// let authz = CedarAuthz::new(cedar_config)
    ///     .await?
    ///     .with_path_normalizer(custom_normalizer);
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_path_normalizer(mut self, normalizer: fn(&str) -> String) -> Self {
        self.path_normalizer = Some(normalizer);
        self
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

        // Extract request information
        let method = request.method().clone();

        // Build Cedar authorization request
        let principal = build_principal(&claims)?;
        let action = build_action_http(&method, &request, authz.path_normalizer)?;
        let context = build_context_http(request.headers(), &claims)?;

        // Build resource (generic default)
        let resource = build_resource()?;

        let cedar_request = CedarRequest::new(
            principal.clone(),
            action.clone(),
            resource.clone(),
            context,
            None, // Schema: None (optional)
        )
        .map_err(|e| Error::Internal(format!("Failed to build Cedar request: {}", e)))?;

        // Check cache (if enabled)
        #[cfg(feature = "cache")]
        if let Some(cache) = &authz.cache {
            if let Some(decision) = cache.get(&cedar_request).await? {
                match decision {
                    Decision::Allow => return Ok(next.run(request).await),
                    Decision::Deny => {
                        return Err(Error::Forbidden("Access denied by policy".to_string()))
                    }
                }
            }
        }

        // Evaluate policies
        let policy_set = authz.policy_set.read().await;
        let entities = build_entities(&claims)?;

        let response = authz.authorizer.is_authorized(
            &cedar_request,
            &policy_set,
            &entities,
        );

        // Handle decision
        match response.decision() {
            Decision::Allow => {
                // Cache decision (if enabled)
                #[cfg(feature = "cache")]
                if let Some(cache) = &authz.cache {
                    let _ = cache
                        .set(&cedar_request, Decision::Allow, authz.config.cache_ttl_secs)
                        .await;
                }

                // Allow request to proceed
                Ok(next.run(request).await)
            }
            Decision::Deny => {
                tracing::warn!(
                    principal = ?principal,
                    action = ?action,
                    "Cedar policy denied request"
                );

                // Cache denial (if enabled)
                #[cfg(feature = "cache")]
                if let Some(cache) = &authz.cache {
                    let _ = cache
                        .set(&cedar_request, Decision::Deny, authz.config.cache_ttl_secs)
                        .await;
                }

                if authz.config.fail_open {
                    tracing::warn!("Cedar policy denied but fail_open=true, allowing request");
                    Ok(next.run(request).await)
                } else {
                    Err(Error::Forbidden("Access denied by policy".to_string()))
                }
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
    if let Some(request_id) = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
    {
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
            return xff_str
                .split(',')
                .next()
                .map(|s| s.trim().to_string());
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
use tonic::{body::Body as TonicBody, Request as TonicRequest, Response as TonicResponse, Status};
#[cfg(feature = "grpc")]
use tower::{Layer, Service};
#[cfg(feature = "grpc")]
use std::task::{Context as TaskContext, Poll};
#[cfg(feature = "grpc")]
use std::pin::Pin;
#[cfg(feature = "grpc")]
use std::future::Future;

/// Tower Layer for Cedar authorization in gRPC services
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
#[cfg(feature = "grpc")]
#[derive(Clone)]
pub struct CedarAuthzService<S> {
    inner: S,
    authz: CedarAuthz,
}

#[cfg(feature = "grpc")]
impl<S, ReqBody> Service<TonicRequest<ReqBody>> for CedarAuthzService<S>
where
    S: Service<TonicRequest<ReqBody>, Response = TonicResponse<TonicBody>, Error = Status>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut TaskContext<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: TonicRequest<ReqBody>) -> Self::Future {
        let mut inner = self.inner.clone();
        let authz = self.authz.clone();

        Box::pin(async move {
            // Skip if Cedar is disabled
            if !authz.config.enabled {
                return inner.call(req).await;
            }

            // Extract JWT claims from request extensions (set by JWT interceptor)
            let claims = req
                .extensions()
                .get::<Claims>()
                .ok_or_else(|| {
                    Status::unauthenticated(
                        "Missing JWT claims. Ensure JWT interceptor runs before Cedar layer.",
                    )
                })?
                .clone();

            // Extract gRPC method path from metadata
            let method_path = req
                .metadata()
                .get(":path")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown")
                .to_string();

            // Build Cedar authorization request
            let principal = build_principal(&claims).map_err(|_| {
                Status::internal("Failed to build principal")
            })?;

            let action = build_action_grpc(&method_path).map_err(|_| {
                Status::internal("Failed to build action")
            })?;

            let context = build_context_grpc(req.metadata(), &claims).map_err(|_| {
                Status::internal("Failed to build context")
            })?;

            // For gRPC, we use default resource
            let resource: EntityUid = r#"Resource::"default""#
                .parse()
                .map_err(|_| Status::internal("Failed to parse resource"))?;

            let cedar_request = CedarRequest::new(
                principal.clone(),
                action.clone(),
                resource.clone(),
                context,
                None,
            )
            .map_err(|_| Status::internal("Failed to build Cedar request"))?;

            // Check cache (if enabled)
            #[cfg(feature = "cache")]
            if let Some(cache) = &authz.cache {
                if let Ok(Some(decision)) = cache.get(&cedar_request).await {
                    match decision {
                        Decision::Allow => return inner.call(req).await,
                        Decision::Deny => {
                            return Err(Status::permission_denied("Access denied by policy"))
                        }
                    }
                }
            }

            // Evaluate policies
            let policy_set = authz.policy_set.read().await;
            let entities = build_entities(&claims).map_err(|_| {
                Status::internal("Failed to build entities")
            })?;

            let response = authz.authorizer.is_authorized(
                &cedar_request,
                &policy_set,
                &entities,
            );

            // Handle decision
            match response.decision() {
                Decision::Allow => {
                    // Cache decision (if enabled)
                    #[cfg(feature = "cache")]
                    if let Some(cache) = &authz.cache {
                        let _ = cache
                            .set(&cedar_request, Decision::Allow, authz.config.cache_ttl_secs)
                            .await;
                    }

                    inner.call(req).await
                }
                Decision::Deny => {
                    tracing::warn!(
                        principal = ?principal,
                        action = ?action,
                        method = %method_path,
                        "Cedar policy denied gRPC request"
                    );

                    // Cache denial (if enabled)
                    #[cfg(feature = "cache")]
                    if let Some(cache) = &authz.cache {
                        let _ = cache
                            .set(&cedar_request, Decision::Deny, authz.config.cache_ttl_secs)
                            .await;
                    }

                    if authz.config.fail_open {
                        tracing::warn!("Cedar policy denied but fail_open=true, allowing gRPC request");
                        inner.call(req).await
                    } else {
                        Err(Status::permission_denied("Access denied by policy"))
                    }
                }
            }
        })
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

/// Build Cedar context from gRPC metadata and claims
#[cfg(feature = "grpc")]
fn build_context_grpc(
    metadata: &tonic::metadata::MetadataMap,
    claims: &Claims,
) -> Result<Context, Error> {
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

    // Add IP address from gRPC metadata
    if let Some(ip) = extract_grpc_client_ip(metadata) {
        context_map.insert("ip".to_string(), json!(ip));
    }

    // Add request ID if present
    if let Some(request_id) = metadata.get("x-request-id").and_then(|v| v.to_str().ok()) {
        context_map.insert("requestId".to_string(), json!(request_id));
    }

    // Add user-agent if present
    if let Some(user_agent) = metadata.get("user-agent").and_then(|v| v.to_str().ok()) {
        context_map.insert("userAgent".to_string(), json!(user_agent));
    }

    Context::from_json_value(serde_json::Value::Object(context_map), None)
        .map_err(|e| Error::Internal(format!("Failed to build gRPC context: {}", e)))
}

/// Extract client IP from gRPC metadata
#[cfg(feature = "grpc")]
fn extract_grpc_client_ip(metadata: &tonic::metadata::MetadataMap) -> Option<String> {
    // Try X-Forwarded-For header first
    if let Some(xff) = metadata.get("x-forwarded-for") {
        if let Ok(xff_str) = xff.to_str() {
            return xff_str.split(',').next().map(|s| s.trim().to_string());
        }
    }

    // Try X-Real-IP header
    if let Some(xri) = metadata.get("x-real-ip") {
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
        };

        let principal = build_principal(&claims).unwrap();
        assert_eq!(principal.to_string(), r#"User::"user:123""#);
    }

    // Note: test_build_action_http removed as it requires constructing a full Request<Body>
    // which is complex. The path normalization logic is tested via test_normalize_path_generic.
    // Integration tests should cover the full middleware flow.
}
