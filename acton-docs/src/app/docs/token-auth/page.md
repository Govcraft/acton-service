---
title: Token Authentication
nextjs:
  metadata:
    title: Token Authentication
    description: Secure your services with PASETO (default) or JWT authentication, featuring automatic middleware setup and optional token revocation.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

acton-service provides production-ready token authentication middleware with **PASETO as the secure default** and JWT available as a feature-gated option. Both support optional Redis-backed token revocation.

## Quick Start

Token authentication is **automatically enabled** when configured in config.toml. No manual middleware setup is needed.

```rust
// Token authentication is automatically applied by ServiceBuilder
// when configured in config.toml (see Configuration Options below)

ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await?;
```

Configure PASETO (default) in `config.toml`:

```toml
[token]
format = "paseto"
version = "v4"
purpose = "local"
key_path = "./keys/paseto.key"
```

The token middleware will automatically validate tokens on all protected routes and extract claims into the request context.

## PASETO vs JWT

**PASETO** (Platform-Agnostic Security Tokens) is the default and recommended token format:

| Feature | PASETO | JWT |
|---------|--------|-----|
| **Security** | Secure by default, no algorithm confusion | Requires careful algorithm selection |
| **Algorithm agility** | Fixed algorithms per version | Many algorithms, some insecure |
| **Default in acton-service** | Yes | Requires `jwt` feature flag |
| **Key types** | V4: Ed25519 (public) or symmetric (local) | RSA, ECDSA, HMAC |

**When to use JWT:**
- Integrating with existing JWT-based systems
- Third-party services that only accept JWT
- Legacy compatibility requirements

## Token Generation

{% callout type="warning" title="Critical: Token Generation Not Included" %}
acton-service provides **token validation** but does NOT include a token generation/signing service. You must implement token generation separately in your authentication service or login endpoint.
{% /callout %}

### Why Separate Generation?

**Security best practice:** Token generation requires access to private/secret keys and should be isolated in a dedicated authentication service. Validation only needs public keys (for PASETO public or JWT RSA/ECDSA) or can use symmetric keys.

**Typical architecture:**
```
Auth Service (generates tokens)  →  API Services (validate tokens)
   - Has secret key                  - Have validation key
   - /login endpoint                 - Protected endpoints
   - Signs tokens                    - Verify signatures
```

### Generating PASETO Tokens (Example)

Use the `rusty_paseto` crate to create tokens in your login handler:

```rust
use rusty_paseto::prelude::*;
use serde_json::json;

async fn login(
    credentials: Json<LoginRequest>
) -> Result<Json<LoginResponse>, AuthError> {
    // 1. Validate credentials
    let user = authenticate_user(&credentials.username, &credentials.password).await?;

    // 2. Load your symmetric key (32 bytes for v4.local)
    let key_bytes: [u8; 32] = load_key_from_secure_storage()?;
    let key = PasetoSymmetricKey::<V4, Local>::from(Key::from(&key_bytes));

    // 3. Create token with claims
    let now = chrono::Utc::now();
    let exp = now + chrono::Duration::hours(1);

    let token = PasetoBuilder::<V4, Local>::default()
        .set_claim(SubjectClaim::from(format!("user:{}", user.id)))
        .set_claim(ExpirationClaim::try_from(exp.to_rfc3339())?)
        .set_claim(IssuedAtClaim::try_from(now.to_rfc3339())?)
        .set_claim(TokenIdentifierClaim::from(uuid::Uuid::new_v4().to_string()))
        .set_claim(CustomClaim::try_from(("email", user.email.as_str()))?)
        .set_claim(CustomClaim::try_from(("roles", json!(user.roles)))?)
        .build(&key)?;

    Ok(Json(LoginResponse { token }))
}
```

### Token Lifetime Recommendations

```rust
// Short-lived access tokens (recommended)
exp: now + Duration::minutes(15),

// Medium-lived access tokens
exp: now + Duration::hours(1),

// Long-lived access tokens (avoid in production)
exp: now + Duration::hours(24),

// Use refresh tokens for longer sessions
// Access token: 15 minutes
// Refresh token: 7 days (stored securely, can be revoked)
```

## Protected Routes vs Public Routes

### How Route Protection Works

**By default, ALL routes require authentication** when token middleware is configured. To make routes public, you must explicitly exclude them.

### Configuration-Based Protection

**Option 1: Exclude Specific Paths** (recommended for most services)

```toml
[token]
format = "paseto"
version = "v4"
purpose = "local"
key_path = "./keys/paseto.key"

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
- `/health` → Public (no token required)
- `/login` → Public (obviously!)
- `/public/terms` → Public (matches wildcard)
- `/api/v1/users` → Protected (requires valid token)
- `/admin/settings` → Protected (requires valid token)

### Health Checks and Token Auth

{% callout type="note" title="Health Endpoints Always Public" %}
The `/health` and `/ready` endpoints are **automatically excluded** from token authentication, even if not in your `exclude_paths`. They must remain public for Kubernetes liveness/readiness probes to work.
{% /callout %}

## PASETO Configuration

### V4 Local (Symmetric - Recommended for Single Service)

Uses a 32-byte symmetric key for both encryption and decryption.

```toml
[token]
format = "paseto"
version = "v4"
purpose = "local"
key_path = "./keys/paseto.key"   # 32-byte raw key file
issuer = "my-service"            # Optional: validate issuer claim
audience = "api.example.com"     # Optional: validate audience claim
```

**Generate a key:**
```bash
# Generate 32 random bytes
head -c 32 /dev/urandom > keys/paseto.key
chmod 600 keys/paseto.key
```

### V4 Public (Asymmetric - Recommended for Distributed Systems)

Uses Ed25519 public key for signature verification (auth service signs with private key).

```toml
[token]
format = "paseto"
version = "v4"
purpose = "public"
key_path = "./keys/ed25519-public.key"  # 32-byte Ed25519 public key
issuer = "auth.example.com"
audience = "api.example.com"
```

**Generate Ed25519 key pair:**
```bash
# Generate key pair using openssl
openssl genpkey -algorithm ED25519 -out ed25519-private.pem
openssl pkey -in ed25519-private.pem -pubout -out ed25519-public.pem

# Extract raw 32-byte public key
openssl pkey -in ed25519-public.pem -pubin -outform DER | tail -c 32 > ed25519-public.key
```

## JWT Configuration (Requires `jwt` Feature)

{% callout type="warning" title="Feature Flag Required" %}
JWT support requires enabling the `jwt` feature flag in your `Cargo.toml`:

```toml
acton-service = { version = "{% version() %}", features = ["jwt"] }
```
{% /callout %}

### RS256 (RSA Signature)

```toml
[token]
format = "jwt"
public_key_path = "./keys/jwt-public.pem"
algorithm = "RS256"
issuer = "auth.example.com"
audience = "api.example.com"
```

### ES256 (ECDSA Signature)

```toml
[token]
format = "jwt"
public_key_path = "./keys/ec-public.pem"
algorithm = "ES256"
```

### HS256 (HMAC - Shared Secret)

```toml
[token]
format = "jwt"
public_key_path = "./keys/jwt-secret.key"  # Raw secret file
algorithm = "HS256"
```

{% callout type="warning" title="Avoid HMAC in Distributed Systems" %}
HMAC algorithms (HS256/384/512) require sharing the same secret across all services. Prefer RS256 or ES256 for multi-service architectures where only the auth service needs the private key.
{% /callout %}

### Supported JWT Algorithms

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

## Claims Structure

Token claims are extracted into a `Claims` struct available in request handlers:

**Required Claims:**
- `sub` (subject) - User or client identifier (e.g., "user:123")
- `exp` (expiration) - Token expiration (ISO8601 for PASETO, Unix timestamp for JWT)

**Optional Claims:**
- `iat` (issued at) - Token creation timestamp
- `jti` (token ID) - Unique token identifier (required for revocation)
- `iss` (issuer) - Token issuer
- `aud` (audience) - Intended audience
- `roles` - Array of role identifiers (e.g., ["user", "admin"])
- `perms` - Array of permission strings (e.g., ["read:documents"])
- `username` - User's display name
- `email` - User's email address

**Example PASETO Payload:**
```json
{
  "sub": "user:123",
  "username": "alice",
  "email": "alice@example.com",
  "roles": ["user", "premium"],
  "perms": ["read:documents", "write:documents"],
  "exp": "2024-12-31T23:59:59+00:00",
  "iat": "2024-01-01T00:00:00+00:00",
  "jti": "unique-token-id-abc123"
}
```

### Accessing Claims in Handlers

```rust
use acton_service::prelude::*;
use axum::Extension;

async fn protected_handler(
    Extension(claims): Extension<Claims>,
) -> impl IntoResponse {
    // Access user information
    let user_id = &claims.sub;
    let username = claims.username.as_deref().unwrap_or("unknown");

    // Check roles
    if claims.has_role("admin") {
        // Admin-specific logic
    }

    // Check permissions
    if claims.perms.contains(&"write:documents".to_string()) {
        // Permission-specific logic
    }

    format!("Hello, {}!", username)
}
```

## Token Revocation

acton-service supports immediate token revocation using Redis as a revocation list store. This works with both PASETO and JWT tokens.

### Enabling Token Revocation

```toml
[token]
format = "paseto"
version = "v4"
purpose = "local"
key_path = "./keys/paseto.key"

[redis]
url = "redis://localhost:6379"
```

When Redis is configured, token revocation is automatically enabled. The middleware checks each token's `jti` claim against the revocation list.

### How Revocation Works

1. **Token Validation**: Middleware extracts `jti` claim from token
2. **Revocation Check**: Checks Redis for revoked token entry
3. **Decision**: Rejects request if token is revoked, allows if valid

**Revocation Entry Format:**
```text
Key: token:revoked:{jti}
Value: {reason}
TTL: Token expiration time - current time
```

### Revoking Tokens Programmatically

```rust
use acton_service::middleware::RedisTokenRevocation;
use acton_service::middleware::TokenRevocation;

async fn logout(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, Error> {
    if let (Some(redis), Some(jti)) = (state.redis().await, &claims.jti) {
        let revocation = RedisTokenRevocation::new(redis);

        // Calculate TTL from token expiration
        let ttl = (claims.exp - chrono::Utc::now().timestamp()) as u64;

        // Revoke the token
        revocation.revoke(jti, ttl).await?;
    }

    Ok(StatusCode::NO_CONTENT)
}
```

### Revocation Use Cases

- **User Logout**: Immediately invalidate tokens on explicit logout
- **Security Incidents**: Revoke compromised tokens without waiting for expiration
- **Permission Changes**: Force re-authentication when roles/permissions change
- **Account Suspension**: Revoke all user tokens immediately

## gRPC Support

Token authentication is available for gRPC services via interceptors:

```rust
use acton_service::grpc::{paseto_auth_interceptor, request_id_interceptor};
use acton_service::middleware::PasetoAuth;
use std::sync::Arc;

// Create PASETO auth from config
let paseto_config = match &config.token {
    Some(TokenConfig::Paseto(cfg)) => cfg,
    _ => panic!("Expected PASETO config"),
};
let paseto_auth = Arc::new(PasetoAuth::new(paseto_config)?);

// Build gRPC service with interceptors
let service = MyServiceServer::with_interceptor(
    service_impl,
    move |req| {
        let req = request_id_interceptor(req)?;
        paseto_auth_interceptor(paseto_auth.clone())(req)
    }
);
```

For JWT (with `jwt` feature):
```rust
use acton_service::grpc::jwt_auth_interceptor;
use acton_service::middleware::JwtAuth;

let jwt_auth = Arc::new(JwtAuth::new(&jwt_config)?);
let interceptor = jwt_auth_interceptor(jwt_auth);
```

## Integration with Cedar Authorization

Token authentication works seamlessly with Cedar authorization. Both are automatically applied by ServiceBuilder when configured:

```toml
[token]
format = "paseto"
version = "v4"
purpose = "local"
key_path = "./keys/paseto.key"

[cedar]
enabled = true
policies_path = "/path/to/policies"
```

**How it works:**
1. Token middleware validates token and extracts claims
2. Cedar middleware uses claims for fine-grained access control
3. Claims automatically populate the Cedar principal entity:
   - `sub` → Principal identifier
   - `roles` → Principal role attributes
   - `perms` → Principal permission attributes

## Security Best Practices

**Use PASETO by default**
- PASETO is secure by design with no algorithm confusion attacks
- Prefer V4 (latest version) for best security

**Set appropriate expiration**
- Short-lived tokens (15-60 minutes) for user sessions
- Implement refresh token rotation for longer sessions

**Protect secret keys**
- Never commit keys to version control
- Use environment variables or secret managers
- Set restrictive file permissions (chmod 600)

**Enable revocation for critical systems**
- Implement revocation for user-facing applications
- Required for immediate logout and incident response

**Use HTTPS only**
- Never send tokens over unencrypted connections
- Configure strict transport security headers

## Troubleshooting

**401 Unauthorized - Invalid Token**
- Verify key file is accessible and correct format
- Check that key matches the one used for signing
- Ensure token hasn't expired

**401 Unauthorized - Token Revoked**
- Token's JTI exists in Redis revocation list
- Issue new token after re-authentication

**403 Forbidden After Successful Authentication**
- Token auth passed but authorization denied
- Check Cedar policies or permission requirements
- Verify token claims include required roles/permissions

**Feature `jwt` not found**
- Add `jwt` feature to your Cargo.toml:
  ```toml
  acton-service = { version = "{% version() %}", features = ["jwt"] }
  ```

**PASETO key size error**
- V4 local requires exactly 32 bytes
- V4 public requires exactly 32 bytes (Ed25519 public key)

## Next Steps

- [Implement Cedar Authorization](/docs/cedar-auth) - Add fine-grained access control
- [Configure Redis](/docs/cache) - Set up Redis for token revocation
- [Add Rate Limiting](/docs/rate-limiting) - Use token claims for per-user limits
