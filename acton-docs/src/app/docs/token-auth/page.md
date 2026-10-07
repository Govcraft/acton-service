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

acton-service provides production-ready token authentication middleware with **PASETO as the secure default** and JWT available as a feature-gated option. Both support token-ID and subject revocation through Redis or the supported authentication databases.

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
        // Custom claims — any serializable value
        .set_claim(CustomClaim::try_from(("tenant_id", user.tenant_id.as_str()))?)
        .set_claim(CustomClaim::try_from(("subscription_tier", "enterprise"))?)
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

**By default, ALL routes require authentication** when token middleware is configured. Infrastructure paths (`/health`, `/ready`, `/swagger-ui`, `/api-docs`) are always exempt. For application-level routes that should bypass token auth (e.g. session-based frontend routes), use `public_paths`.

### Configuration-Based Protection

```toml
[token]
format = "paseto"
version = "v4"
purpose = "local"
key_path = "./keys/paseto.key"

# Path prefixes that bypass token authentication.
# Use for session-based frontend routes that coexist with token-protected API routes.
public_paths = ["/admin/", "/forge/", "/login"]
```

With this config:
- `/health` → Public (infrastructure, always exempt)
- `/admin/login` → Public (matches `/admin/` prefix)
- `/forge/Contact/entities` → Public (matches `/forge/` prefix)
- `/login` → Public (matches `/login` prefix)
- `/api/v1/users` → Protected (requires valid token)

Routes listed in `public_paths` use prefix matching — `/admin/` matches `/admin/login`, `/admin/users`, etc.

### Health Checks and Token Auth

{% callout type="note" title="Health Endpoints Always Public" %}
The `/health` and `/ready` endpoints are **automatically excluded** from token authentication, even if not in your `public_paths`. They must remain public for Kubernetes liveness/readiness probes to work.
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
- `jti` (token ID) - Unique identifier required for individual-token revocation; subject revocation also covers tokens without it
- `iss` (issuer) - Token issuer
- `aud` (audience) - Intended audience
- `roles` - Array of role identifiers (e.g., ["user", "admin"])
- `perms` - Array of permission strings (e.g., ["read:documents"])
- `username` - User's display name
- `email` - User's email address

**Custom Claims:**

Any additional fields in the token payload are captured as custom claims in a `HashMap<String, serde_json::Value>`. This allows application-specific data like tenant IDs, feature flags, or subscription tiers to be included in tokens without modifying the `Claims` struct. See [Token Generation - Custom Claims](/docs/token-generation#custom-claims) for how to create tokens with custom claims.

**Example PASETO Payload:**
```json
{
  "sub": "user:123",
  "username": "alice",
  "email": "alice@example.com",
  "roles": ["user", "premium"],
  "perms": ["read:documents", "write:documents"],
  "tenant_id": "org-42",
  "subscription_tier": "enterprise",
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

    // Access custom claims
    let tenant: Option<String> = claims.custom_claim_as("tenant_id");
    let tier: Option<String> = claims.custom_claim_as("subscription_tier");

    format!("Hello, {}!", username)
}
```

## Token Revocation

Revocation works with PASETO and JWT on HTTP and gRPC. Select Redis, PostgreSQL, SQL Server, Turso/libsql, or SurrealDB explicitly. Each database backend uses its usual feature and connection configuration; Redis requires `cache`. Token validation alone does not require `auth` or Redis.

```toml
[token]
format = "paseto"
version = "v4"
purpose = "local"
key_path = "./keys/paseto.key"

[revocation]
backend = "postgres"   # redis | postgres | mssql | turso | surrealdb
namespace = "my-service"

[database]
url = "postgres://localhost/my_service"
```

`ServiceBuilder` installs the configured checker on both transports. Unsupported features, missing connection configuration, or revocation without token authentication fail startup. Disconnected pools, missing schemas, and lookup errors deny protected requests. Infrastructure and configured public paths retain their authentication bypass behavior.

### Initialize storage before serving

Database implementations expose idempotent `initialize()` migrations and `cleanup_expired()` maintenance. They create `token_revocations`, keyed by namespace, record kind, and identifier, with a Unix-seconds value containing either token expiry or a persistent subject cutoff. SurrealDB calls that field `stamp`. SQL Server stores namespace and identifiers as UTF-8 bytes with a nonzero terminator in `VARBINARY` columns, with an identifier limit of 512 bytes, so trailing spaces and NUL bytes remain distinct. Run initialization with DDL credentials during deployment; runtime credentials need read and write access. SurrealDB also requires permission to inspect table metadata with `INFO FOR TABLE`, which the checker uses to refuse an uninitialized schema. Run cleanup periodically to remove expired token records. Subject cutoffs persist so old credentials cannot become valid again.

The shared checker is available from the service's state. Pool agents connect asynchronously, so initialize after the selected pool is ready and before calling `serve()`:

```rust
let service = ServiceBuilder::new()
    .with_config(config)
    .with_routes(routes)
    .try_build()?;
let revocation = service.state().token_revocation()
    .ok_or_else(|| Error::Internal("Revocation is required".into()))?;
// PostgreSQL example. Use the selected backend's state accessor for other pools.
tokio::time::timeout(std::time::Duration::from_secs(30), async {
    while service.state().db().await.is_none() {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}).await.map_err(|_| Error::Internal("Revocation database startup deadline exceeded".into()))?;
revocation.initialize().await?;
service.serve().await?;
```

Applications with their own pool can construct `PgTokenRevocation`, `MssqlTokenRevocation`, `TursoTokenRevocation`, or `SurrealTokenRevocation` using that pool and a validated `RevocationNamespace`, initialize it, then inject an `Arc` with `ServiceBuilder::with_token_revocation`. Injection takes precedence over configuration.

### Revoke individual tokens and subjects

`revoke(jti, ttl_secs)` rejects an individual token through its remaining lifetime. Repeating revocation never shortens the stored expiration. A zero lifetime does not revoke an already expired token.

`revoke_subject(subject, not_before)` rejects every token issued at or before the given Unix timestamp, including tokens without `jti`. Repeating it cannot lower the cutoff. Exact timestamp equality is rejected because issue and deactivation times have second precision. A token missing `iat` is also rejected whenever its subject has a cutoff. Tokens with neither a revoked ID nor a subject cutoff keep their existing validation behavior.

```rust
let revocation = state.token_revocation()
    .ok_or_else(|| Error::Internal("Revocation is required".into()))?;
let now = chrono::Utc::now().timestamp();
revocation.revoke_subject(&claims.sub, now).await?;
if let Some(jti) = &claims.jti {
    let remaining = claims.exp.saturating_sub(now).max(0);
    revocation.revoke(jti, u64::try_from(remaining)
        .map_err(|error| Error::Internal(error.to_string()))?).await?;
}
```

Use stable, issuer-scoped subjects and keep the namespace identical on writers and validators. Before reporting deactivation or token retirement as complete, persist the revocation successfully. Subject cutoffs revoke existing tokens; the issuer must also prevent new issuance for deactivated principals.

Redis uses expiring token keys and a persistent hash of subject cutoffs. Configured prefixes are `token:revoked:{namespace_byte_length}:{namespace}:`; direct `RedisTokenRevocation::new` preserves `token:revoked:`. Redis reserves the token ID `subject-cutoffs` for the subject hash and refuses it. All participants must use the same prefix.

### Revocation Use Cases

- **User Logout**: Immediately invalidate tokens on explicit logout
- **Security Incidents**: Revoke compromised tokens without waiting for expiration
- **Permission Changes**: Force re-authentication when roles/permissions change
- **Account Suspension**: Revoke all user tokens immediately

## Audit Emission

When the `audit` feature is enabled, the PASETO and JWT middleware automatically emit audit events for the request lifecycle they manage. By default (`audit_auth_events: true`), every protected request produces one of these:

| Event Kind | When | Severity |
|---|---|---|
| `AuthLoginSuccess` | Token validated successfully | Notice |
| `AuthTokenMissing` | No bearer token, or token header malformed | Informational |
| `AuthTokenInvalid` | Bearer token present but failed validation | Warning |
| `AuthTokenRevoked` | Validated token denied by its token ID or subject cutoff | Warning |

`AuthTokenRevoked` includes the token's `jti` when present; subject-only revocation can have no token ID. SIEM rules and forensic queries that ask "which requests presented this revoked token" can anchor on that field directly.

The middleware no longer emits `AuthLoginFailed`. That event is reserved for application-level login handlers (typically `POST /auth/login`) where credentials are actually submitted. Emit it yourself from those handlers via `logger.log_auth(AuditEventKind::AuthLoginFailed, ...)`. Treating unauthenticated probes against protected routes as failed logins drowns out real signal — see the [audit docs](/docs/audit) for the rationale.

## gRPC Support

When `[token]` is configured, `ServiceBuilder` applies token authentication
to all registered gRPC services automatically — the same configuration that
protects HTTP routes protects gRPC methods, with no per-service wiring. The
`authorization` metadata is validated and `Claims` are injected into request
extensions where handlers (and Cedar) can read them. Failures are returned
as gRPC statuses (`UNAUTHENTICATED`), health
(`grpc.health.v1.Health`) and reflection services stay credential-free for
infrastructure probes, and `public_paths` prefixes are honored for
intentionally public methods (match against the full method path, e.g.
`"/hello.v1.HelloService/"`).

For manually composed stacks, `GrpcTokenAuthLayer` is the same layer as a
public type — it forwards `NamedService`, so a wrapped service registers
directly with `GrpcServicesBuilder::add_service`:

```rust
use acton_service::grpc::GrpcTokenAuthLayer;
use tower::Layer;

let services = GrpcServicesBuilder::new()
    .add_service(GrpcTokenAuthLayer::new(paseto_auth).layer(MyServiceServer::new(svc)))
    .build::<()>(None);
```

Tonic interceptors remain available for custom `with_interceptor` stacks.
They validate cryptography synchronously and cannot await storage-backed
revocation. Use `GrpcTokenAuthLayer` or the configured `ServiceBuilder` when
revocation is required:

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
policy_path = "/path/to/policies.cedar"
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
- The selected storage contains an unexpired token-ID revocation or a subject cutoff at or after the token's `iat`
- Tokens missing `iat` are denied if a subject cutoff exists
- Re-authenticate only when the issuer still permits the principal to receive new tokens

**Storage failure on protected requests**
- Confirm the configured pool is connected and the revocation schema was initialized
- Check storage credentials, namespace, and backend logs; failed lookups deny requests

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
