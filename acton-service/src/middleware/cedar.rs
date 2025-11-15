//! Cedar authorization middleware for HTTP and gRPC
//!
//! This middleware integrates AWS Cedar policy-based authorization into acton-service.
//! It validates authorization requests against Cedar policies after JWT authentication.

use axum::{
    body::Body,
    extract::{Request, State},
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
        })
    }

    /// Set policy cache (optional, for performance)
    #[cfg(feature = "cache")]
    pub fn with_cache<C: PolicyCache + 'static>(mut self, cache: C) -> Self {
        self.cache = Some(Arc::new(cache));
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
        let path = request.uri().path().to_string();

        // Build Cedar authorization request
        let principal = build_principal(&claims)?;
        let action = build_action_http(&method, &path)?;
        let context = build_context_http(request.headers(), &claims)?;

        // Build a dummy resource (we don't use resource-based authorization yet)
        let resource: EntityUid = r#"Resource::"default""#
            .parse()
            .map_err(|e| Error::Internal(format!("Failed to parse resource: {}", e)))?;

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

/// Build Cedar action from HTTP method and path
fn build_action_http(method: &Method, path: &str) -> Result<EntityUid, Error> {
    // Action format: Action::"GET /api/v1/users"
    // Normalize path by removing IDs (e.g., /users/123 -> /users/:id)
    let normalized_path = normalize_path(path);
    let action_str = format!(r#"Action::"{} {}""#, method, normalized_path);

    let action: EntityUid = action_str
        .parse()
        .map_err(|e| Error::Internal(format!("Invalid action: {}", e)))?;

    Ok(action)
}

/// Normalize path by replacing IDs with placeholders
fn normalize_path(path: &str) -> String {
    // Replace UUIDs with :id
    let uuid_pattern =
        regex::Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}").unwrap();
    let path = uuid_pattern.replace_all(path, ":id");

    // Replace numeric IDs with :id
    let numeric_pattern = regex::Regex::new(r"/\d+(?:/|$)").unwrap();
    let path = numeric_pattern.replace_all(&path, "/:id$1");

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
fn build_entities(claims: &Claims) -> Result<Entities, Error> {
    // For now, we create minimal entities
    // In a real implementation, you would fetch entity data from your database
    let entity_json = json!([
        {
            "uid": {
                "type": if claims.is_user() { "User" } else { "Client" },
                "id": claims.sub.clone()
            },
            "attrs": {
                "email": claims.email.clone().unwrap_or_default(),
                "roles": claims.roles.clone(),
                "permissions": claims.perms.clone(),
            },
            "parents": []
        }
    ]);

    Entities::from_json_value(entity_json, None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("/api/v1/users/123"), "/api/v1/users/:id");
        assert_eq!(
            normalize_path("/api/v1/users/550e8400-e29b-41d4-a716-446655440000"),
            "/api/v1/users/:id"
        );
        assert_eq!(normalize_path("/api/v1/users"), "/api/v1/users");
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

    #[test]
    fn test_build_action_http() {
        let method = Method::GET;
        let path = "/api/v1/users/123";

        let action = build_action_http(&method, path).unwrap();
        assert_eq!(action.to_string(), r#"Action::"GET /api/v1/users/:id""#);
    }
}
