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
