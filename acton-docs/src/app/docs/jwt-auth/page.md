---
title: JWT Authentication
nextjs:
  metadata:
    title: JWT Authentication
    description: Secure your services with industry-standard JWT authentication supporting multiple signing algorithms and optional token revocation.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---


acton-service provides production-ready JWT authentication middleware with comprehensive algorithm support and optional Redis-backed token revocation.

## Quick Start

JWT authentication is **automatically enabled** when configured in config.toml. No manual middleware setup is needed.

```rust
// JWT authentication is automatically applied by ServiceBuilder
// when configured in config.toml (see Configuration Options below)

ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await?;
```

Configure JWT in `config.toml`:

```toml
[jwt]
secret = "your-secret-key"
algorithm = "HS256"
```

The JWT middleware will automatically validate tokens on all protected routes and extract claims into the request context.

## Token Generation

{% callout type="warning" title="Critical: Token Generation Not Included" %}
acton-service provides **token validation** but does NOT include a token generation/signing service. You must implement token generation separately in your authentication service or login endpoint.
{% /callout %}

### Why Separate Generation?

**Security best practice:** Token generation requires access to private keys and should be isolated in a dedicated authentication service. Validation only needs public keys, which can be safely distributed.

**Typical architecture:**
```
Auth Service (generates tokens)  →  API Services (validate tokens)
   - Has private key                 - Have public key only
   - /login endpoint                 - Protected endpoints
   - Signs JWTs                      - Verify signatures
```

### Generating Tokens (Example)

Use a JWT library like `jsonwebtoken` to create tokens in your login handler:

```rust
use jsonwebtoken::{encode, EncodingKey, Header, Algorithm};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,           // User ID
    username: String,
    email: String,
    roles: Vec<String>,
    exp: usize,           // Expiration timestamp
    iat: usize,           // Issued at timestamp
    jti: String,          // Unique token ID (for revocation)
}

async fn login(
    credentials: Json<LoginRequest>
) -> Result<Json<LoginResponse>, AuthError> {
    // 1. Validate credentials (check password, etc.)
    let user = authenticate_user(&credentials.username, &credentials.password).await?;

    // 2. Create claims
    let now = chrono::Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: format!("user:{}", user.id),
        username: user.username,
        email: user.email,
        roles: user.roles,
        exp: now + 3600,  // Expires in 1 hour
        iat: now,
        jti: uuid::Uuid::new_v4().to_string(),  // Unique ID for revocation
    };

    // 3. Sign token with private key
    let token = encode(
        &Header::new(Algorithm::RS256),
        &claims,
        &EncodingKey::from_rsa_pem(include_bytes!("private-key.pem"))?
    )?;

    Ok(Json(LoginResponse { token }))
}
```

### Token Lifetime Recommendations

```rust
// Short-lived access tokens (recommended)
exp: now + 900,      // 15 minutes

// Medium-lived access tokens
exp: now + 3600,     // 1 hour

// Long-lived access tokens (avoid in production)
exp: now + 86400,    // 24 hours

// Use refresh tokens for longer sessions
// Access token: 15 minutes
// Refresh token: 7 days (stored securely, can be revoked)
```

### Refresh Token Pattern

For production, use short-lived access tokens with long-lived refresh tokens:

```rust
// Login returns both tokens
{
  "access_token": "eyJ...",   // 15 min, used for API calls
  "refresh_token": "xyz...",  // 7 days, stored securely
  "expires_in": 900
}

// Client refreshes access token when expired
POST /auth/refresh
Authorization: Bearer <refresh_token>

Response:
{
  "access_token": "eyJ...",   // New 15 min token
  "expires_in": 900
}
```

## Protected Routes vs Public Routes

### How Route Protection Works

**By default, ALL routes require authentication** when JWT middleware is configured. To make routes public, you must explicitly exclude them.

### Configuration-Based Protection

**Option 1: Exclude Specific Paths** (recommended for most services)

```toml
[jwt]
secret = "your-secret-key"
algorithm = "RS256"

# Routes that DON'T require authentication
exclude_paths = [
    "/health",
    "/ready",
    "/login",
    "/register",
    "/public/*",          # Wildcard patterns supported
    "/api/v1/docs/*"
]
```

With this config:
- ✅ `/health` → Public (no token required)
- ✅ `/login` → Public (obviously!)
- ✅ `/public/terms` → Public (matches wildcard)
- ❌ `/api/v1/users` → Protected (requires valid JWT)
- ❌ `/admin/settings` → Protected (requires valid JWT)

**Option 2: Include Specific Paths** (recommended for high-security services)

```toml
[jwt]
secret = "your-secret-key"
algorithm = "RS256"

# ONLY these routes require authentication (all others are public)
include_paths = [
    "/api/v1/*",
    "/admin/*"
]
```

With this config:
- ✅ `/health` → Public (not in include list)
- ✅ `/docs` → Public (not in include list)
- ❌ `/api/v1/users` → Protected (matches include pattern)
- ❌ `/admin/settings` → Protected (matches include pattern)

### Code-Based Protection (Advanced)

{% callout type="warning" title="Configuration is Recommended" %}
For most use cases, use **configuration-based protection** (`exclude_paths` or `include_paths`). Code-based middleware is an advanced pattern that still requires using `VersionedApiBuilder`.
{% /callout %}

If you need fine-grained per-version control, apply middleware within the versioned builder:

```rust
use acton_service::prelude::*;
use acton_service::middleware::JwtAuthLayer;

// Create JWT middleware layer
let jwt_layer = JwtAuthLayer::new(jwt_config);

let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        // V1: Some endpoints public, some protected
        router
            .route("/login", post(login))           // Public
            .route("/register", post(register))     // Public
            .route("/users", get(list_users)        // Protected
                .layer(jwt_layer.clone()))
            .route("/profile", get(get_profile)     // Protected
                .layer(jwt_layer.clone()))
    })
    .add_version(ApiVersion::V2, |router| {
        // V2: All endpoints require auth
        router
            .route("/users", get(list_users_v2))
            .route("/profile", get(get_profile_v2))
            .layer(jwt_layer)  // Apply to all V2 routes
    })
    .build_routes();

ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await?;
```

**Note:** `/health` and `/ready` are automatically added by `ServiceBuilder` and are always public (not part of versioned routes).

### Health Checks and JWT

{% callout type="note" title="Health Endpoints Always Public" %}
The `/health` and `/ready` endpoints are **automatically excluded** from JWT authentication, even if not in your `exclude_paths`. They must remain public for Kubernetes liveness/readiness probes to work.
{% /callout %}

### Common Public Endpoints

Typically exclude from authentication:
- `/health`, `/ready` - Health checks (automatic)
- `/login`, `/register`, `/reset-password` - Authentication endpoints
- `/docs`, `/openapi.json` - API documentation
- `/public/*` - Public assets, terms of service, privacy policy
- `/webhooks/*` - Third-party webhook endpoints (use different auth)

## Supported Algorithms

The JWT authentication middleware supports industry-standard signing algorithms:

**RSA Algorithms**
- **RS256** - RSA signature with SHA-256 (recommended for production)
- **RS384** - RSA signature with SHA-384
- **RS512** - RSA signature with SHA-512

**ECDSA Algorithms**
- **ES256** - ECDSA signature with SHA-256 (recommended for production)
- **ES384** - ECDSA signature with SHA-384

**HMAC Algorithms**
- **HS256** - HMAC with SHA-256 (shared secret)
- **HS384** - HMAC with SHA-384 (shared secret)
- **HS512** - HMAC with SHA-512 (shared secret)

### Algorithm Selection Guide

**Use RS256 or ES256 for production:**
- Public/private key pairs allow distributed validation
- Private keys remain secure on signing server only
- Public keys can be safely distributed to all services
- ES256 offers smaller signatures and faster verification

**Avoid HMAC in distributed systems:**
- Single shared secret must be distributed to all services
- Secret compromise affects all services simultaneously
- Cannot distinguish between signing and validation permissions

## Claims Structure

JWT tokens must include standard and custom claims:

**Standard Claims**
- `sub` (subject) - User or client identifier (e.g., "user:123")
- `exp` (expiration) - Token expiration timestamp
- `iat` (issued at) - Token creation timestamp
- `jti` (JWT ID) - Unique token identifier (required for revocation)

**Custom Claims**
- `roles` - Array of role identifiers (e.g., ["user", "admin"])
- `perms` - Array of permission strings (e.g., ["read:documents", "write:documents"])
- `username` - User's display name
- `email` - User's email address
- `client_id` - Client application identifier (for service-to-service auth)

**Example Token Payload:**
```json
{
  "sub": "user:123",
  "username": "alice",
  "email": "alice@example.com",
  "roles": ["user", "premium"],
  "perms": ["read:documents", "write:documents", "delete:own"],
  "exp": 1735689600,
  "iat": 1735603200,
  "jti": "unique-token-id-abc123"
}
```

## Token Revocation

acton-service supports immediate token revocation using Redis as a revocation list store.

### Enabling Token Revocation

```toml
[jwt]
secret = "your-secret-key"
algorithm = "RS256"
revocation_enabled = true

[redis]
url = "redis://localhost:6379"
```

### How Revocation Works

1. **Token Validation**: JWT middleware extracts `jti` claim from token
2. **Revocation Check**: Checks Redis for revoked token entry
3. **Decision**: Rejects request if token is revoked, allows if valid

**Revocation Entry Format:**
```text
Key: jwt:revoked:{jti}
Value: {reason}
TTL: Token expiration time - current time
```

### Revoking Tokens

Revoke a token programmatically:

```rust
use redis::AsyncCommands;

async fn revoke_token(
    redis: &mut redis::aio::Connection,
    jti: &str,
    reason: &str,
    expires_at: i64,
) -> Result<(), redis::RedisError> {
    let key = format!("jwt:revoked:{}", jti);
    let ttl = (expires_at - chrono::Utc::now().timestamp()) as usize;

    redis.set_ex(&key, reason, ttl).await?;
    Ok(())
}
```

**Via HTTP Endpoint (if implemented):**
```bash
curl -X POST http://localhost:8080/admin/revoke-token \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"jti": "token-id-to-revoke", "reason": "User logout"}'
```

### Revocation Use Cases

**User Logout**
- Immediately invalidate tokens on explicit logout
- Prevents reuse of tokens after session termination

**Security Incidents**
- Revoke compromised tokens immediately
- No need to wait for natural expiration
- Minimizes breach impact window

**Permission Changes**
- Revoke tokens when user roles/permissions change
- Forces re-authentication to get updated claims
- Maintains authorization consistency

**Account Suspension**
- Revoke all user tokens on account suspension
- Immediate access termination across all sessions

## Configuration Options

```toml
[jwt]
# Secret key or path to public key file
secret = "your-secret-key"
# Or for RS256/ES256:
# public_key_path = "/path/to/public-key.pem"

# Algorithm: RS256, RS384, RS512, ES256, ES384, HS256, HS384, HS512
algorithm = "RS256"

# Enable token revocation checking
revocation_enabled = true

# Clock skew tolerance in seconds (handles time sync issues)
leeway_seconds = 60

# Required audience claim (optional)
required_audience = "api.example.com"

# Required issuer claim (optional)
required_issuer = "auth.example.com"
```

## Security Best Practices

**Use Strong Algorithms**
- Prefer RS256 or ES256 in production
- Avoid HS256 for multi-service architectures
- Use minimum 2048-bit keys for RSA

**Set Appropriate Expiration**
- Short-lived tokens (15-60 minutes) for user sessions
- Longer tokens (hours/days) for service-to-service auth
- Implement refresh token rotation

**Protect Secret Keys**
- Never commit secrets to version control
- Use environment variables or secret managers
- Rotate keys periodically

**Enable Revocation for Critical Systems**
- Implement revocation for user-facing applications
- Monitor Redis performance under load
- Set appropriate TTLs to prevent unbounded growth

**Validate All Claims**
- Always check `exp` to prevent expired token usage
- Validate `aud` and `iss` when using multiple auth servers
- Verify required custom claims exist

**Use HTTPS Only**
- Never send JWT tokens over unencrypted connections
- Tokens in transit can be intercepted and replayed
- Configure strict transport security headers

## Integration with Authorization

JWT authentication works seamlessly with Cedar authorization. Both are automatically applied by ServiceBuilder when configured:

```rust
ServiceBuilder::new()
    .with_routes(routes)
    .build()  // Both JWT and Cedar are auto-applied based on config
    .serve()
    .await?;
```

Configure both JWT and Cedar in `config.toml`:

```toml
[jwt]
secret = "your-secret-key"
algorithm = "RS256"

[cedar]
enabled = true
policies_path = "/path/to/policies"
```

**How it works:**
1. ServiceBuilder automatically applies JWT middleware first (validates tokens, extracts claims)
2. Cedar authorization middleware is applied second (uses claims for fine-grained access control)
3. JWT claims automatically populate the Cedar principal entity:
   - `sub` → Principal identifier
   - `roles` → Principal role attributes
   - `perms` → Principal permission attributes

This automatic composition means you write less code while getting both authentication and authorization out of the box.

## Troubleshooting

**401 Unauthorized - Invalid Signature**
- Verify secret key matches signing key
- Check algorithm configuration matches token
- Ensure public key file is accessible and valid

**401 Unauthorized - Token Expired**
- Token `exp` claim is in the past
- Generate new token with future expiration
- Check for clock skew between services

**401 Unauthorized - Token Revoked**
- Token JTI exists in Redis revocation list
- User logged out or token was explicitly revoked
- Issue new token after re-authentication

**403 Forbidden After Successful Authentication**
- JWT auth passed but authorization denied
- Check Cedar policies or permission requirements
- Verify token claims include required roles/permissions

## Next Steps

- [Implement Cedar Authorization](/docs/cedar-auth) - Add fine-grained access control
- [Configure Token Revocation](/docs/cache) - Set up Redis for revocation lists
- [Add Rate Limiting](/docs/rate-limiting) - Use JWT claims for per-user limits
