---
title: Token Generation
nextjs:
  metadata:
    title: Token Generation
    description: Generate PASETO and JWT tokens for stateless authentication with refresh token rotation and multiple storage backends.
---

{% callout type="note" title="Part of the Auth Module" %}
This guide covers token generation. See the [Authentication Overview](/docs/auth) for all auth capabilities, or jump to [Password Hashing](/docs/password-hashing), [API Keys](/docs/api-keys), or [OAuth/OIDC](/docs/oauth).
{% /callout %}

---

## Introduction

Token generation in acton-service creates cryptographic tokens for stateless authentication. The module supports two token formats: PASETO V4 (the secure default) and JWT (feature-gated for compatibility). Refresh tokens enable long-lived sessions with automatic rotation and reuse detection.

The `TokenGenerator` trait abstracts token creation, with `PasetoGenerator` and `JwtGenerator` implementations. Claims include standard fields (subject, expiration, issuer) plus custom fields (roles, permissions, email). Storage backends (Redis, PostgreSQL, Turso) handle refresh token persistence with built-in security features.

**Key characteristics:**

- **PASETO by default**: Eliminates JWT algorithm confusion attacks
- **Refresh token rotation**: New token issued on each refresh, old token revoked
- **Reuse detection**: Detects stolen refresh tokens by tracking token families
- **Flexible storage**: Choose Redis, PostgreSQL, or Turso based on your needs

---

## Quick Start

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["auth"] }
```

```rust
use acton_service::auth::{PasetoGenerator, TokenGenerator, ClaimsBuilder};
use acton_service::auth::config::{PasetoGenerationConfig, TokenGenerationConfig};

// Create generator from configuration
let generator = PasetoGenerator::new(&paseto_config, &token_config)?;

// Build claims
let claims = ClaimsBuilder::new()
    .user("123")
    .email("user@example.com")
    .role("user")
    .build()?;

// Generate token
let token = generator.generate_token(&claims)?;
// Returns: v4.local.eyJzdWIiOiJ1c2VyOjEyMyIsLi4u...
```

---

## Token Formats

### PASETO (Default)

PASETO V4 tokens use modern cryptography and eliminate algorithm confusion attacks. Two modes are available:

| Mode | Cryptography | Use Case |
|------|--------------|----------|
| V4.local | XChaCha20-Poly1305 (symmetric) | Single service, shared secret |
| V4.public | Ed25519 (asymmetric) | Distributed services, public verification |

```rust
use acton_service::auth::PasetoGenerator;

// V4.local with symmetric key
let generator = PasetoGenerator::with_symmetric_key(key_bytes, config);

// V4.public with Ed25519 private key
let generator = PasetoGenerator::with_private_key(private_key_bytes, config);

// From configuration file
let generator = PasetoGenerator::new(&paseto_config, &token_config)?;
```

**Key generation:**

```bash
# V4.local: 32-byte symmetric key
head -c 32 /dev/urandom > keys/paseto.key

# V4.public: Ed25519 keypair (use openssl or a key generation tool)
```

### JWT (Feature-Gated)

JWT support requires the `jwt` feature flag. Use only when integrating with systems that require JWT.

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["auth", "jwt"] }
```

```rust
use acton_service::auth::JwtGenerator;

let generator = JwtGenerator::new(&jwt_config, &token_config)?;
let token = generator.generate_token(&claims)?;
// Returns: eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9...
```

Supported algorithms: RS256, RS384, RS512, ES256, ES384, HS256, HS384, HS512.

---

## Building Claims

The `ClaimsBuilder` provides an ergonomic API for creating token claims.

```rust
use acton_service::auth::ClaimsBuilder;

// User token
let claims = ClaimsBuilder::new()
    .user("123")                           // sub: "user:123"
    .email("user@example.com")
    .username("alice")
    .roles(["user", "admin"])
    .permissions(["read:docs", "write:docs"])
    .issuer("my-auth-service")
    .audience("my-api")
    .build()?;

// Service/client token
let claims = ClaimsBuilder::new()
    .client("api-service-abc")             // sub: "client:api-service-abc"
    .roles(["service"])
    .build()?;

// Direct subject
let claims = ClaimsBuilder::new()
    .subject("custom:identifier")          // sub: "custom:identifier"
    .build()?;
```

**Claims structure:**

| Field | Type | Description |
|-------|------|-------------|
| `sub` | String | Subject (required) |
| `email` | Option | User email |
| `username` | Option | Display name |
| `roles` | Vec | Role identifiers |
| `perms` | Vec | Permission identifiers |
| `exp` | i64 | Expiration (set by generator) |
| `iat` | Option | Issued at (set by generator) |
| `jti` | Option | Token ID (set by generator if configured) |
| `iss` | Option | Issuer |
| `aud` | Option | Audience |

---

## Token Expiration

Tokens expire based on configuration. Use custom expiration for special cases.

```rust
use std::time::Duration;
use acton_service::auth::TokenGenerator;

// Default expiration (from config, typically 15 minutes)
let token = generator.generate_token(&claims)?;

// Custom expiration
let token = generator.generate_token_with_expiry(
    &claims,
    Duration::from_secs(3600), // 1 hour
)?;

// Get default lifetime
let lifetime = generator.default_lifetime();
```

---

## Refresh Tokens

Refresh tokens enable long-lived sessions without storing long-lived access tokens. The framework implements automatic rotation and reuse detection.

### How Rotation Works

```text
1. User logs in → Access token (15 min) + Refresh token A (7 days)
2. Access token expires → Client sends Refresh token A
3. Server validates A → Revokes A → Issues Access + Refresh token B
4. Access token expires → Client sends Refresh token B
5. ... and so on
```

### Reuse Detection

If an attacker steals Refresh token A and tries to use it after rotation:

```text
1. Attacker sends stolen token A (already revoked)
2. Server detects reuse → Revokes entire token family
3. Legitimate user's token B is also revoked
4. User must re-authenticate
```

This limits the damage from stolen refresh tokens.

### Storage Backends

```toml
[dependencies]
# Redis (fast, TTL-based expiration)
acton-service = { version = "{% version() %}", features = ["auth", "cache"] }

# PostgreSQL (durable, queryable)
acton-service = { version = "{% version() %}", features = ["auth", "database"] }

# Turso (edge-deployed, globally replicated)
acton-service = { version = "{% version() %}", features = ["auth", "turso"] }
```

```rust
use acton_service::auth::{RedisRefreshStorage, RefreshTokenStorage};

// Redis storage
let storage = RedisRefreshStorage::new(redis_pool);

// PostgreSQL storage
use acton_service::auth::PgRefreshStorage;
let storage = PgRefreshStorage::new(pg_pool);

// Turso storage
use acton_service::auth::TursoRefreshStorage;
let storage = TursoRefreshStorage::new(turso_conn);
```

### Storage API

```rust
use acton_service::auth::{RefreshTokenStorage, RefreshTokenMetadata};
use chrono::{Utc, Duration};

// Store a new refresh token
let metadata = RefreshTokenMetadata {
    user_agent: Some("Mozilla/5.0...".to_string()),
    ip_address: Some("192.168.1.1".to_string()),
    device_id: None,
    created_at: Utc::now(),
};

storage.store(
    "token_id",
    "user_123",
    "family_abc",
    Utc::now() + Duration::days(7),
    &metadata,
).await?;

// Get token data
let data = storage.get("token_id").await?;

// Rotate: revoke old, create new atomically
storage.rotate(
    "old_token_id",
    "new_token_id",
    "user_123",
    "family_abc",
    Utc::now() + Duration::days(7),
    &metadata,
).await?;

// Revoke single token
storage.revoke("token_id").await?;

// Revoke all tokens in family (reuse detection)
let count = storage.revoke_family("family_abc").await?;

// Revoke all user tokens (logout everywhere)
let count = storage.revoke_all_for_user("user_123").await?;

// Cleanup expired (PostgreSQL/Turso only; Redis uses TTL)
let count = storage.cleanup_expired().await?;
```

### Database Schema (PostgreSQL)

```sql
CREATE TABLE refresh_tokens (
    id VARCHAR(255) PRIMARY KEY,
    user_id VARCHAR(255) NOT NULL,
    family_id VARCHAR(255) NOT NULL,
    is_revoked BOOLEAN NOT NULL DEFAULT FALSE,
    expires_at TIMESTAMPTZ NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_refresh_tokens_user_id ON refresh_tokens(user_id);
CREATE INDEX idx_refresh_tokens_family_id ON refresh_tokens(family_id);
CREATE INDEX idx_refresh_tokens_expires_at ON refresh_tokens(expires_at);
```

---

## Configuration

### TokenGenerationConfig

```rust
pub struct TokenGenerationConfig {
    /// Access token lifetime in seconds (default: 900 = 15 min)
    pub access_token_lifetime_secs: i64,

    /// Issuer claim
    pub issuer: Option<String>,

    /// Audience claim
    pub audience: Option<String>,

    /// Include jti (token ID) for revocation support (default: true)
    pub include_jti: bool,
}
```

### PasetoGenerationConfig

```rust
pub struct PasetoGenerationConfig {
    /// PASETO version (default: "v4")
    pub version: String,

    /// Token purpose: "local" (symmetric) or "public" (asymmetric)
    pub purpose: String,

    /// Path to key file
    pub key_path: PathBuf,

    /// Issuer (overrides TokenGenerationConfig.issuer)
    pub issuer: Option<String>,

    /// Audience (overrides TokenGenerationConfig.audience)
    pub audience: Option<String>,
}
```

### RefreshTokenConfig

```rust
pub struct RefreshTokenConfig {
    /// Enable refresh tokens (default: true)
    pub enabled: bool,

    /// Refresh token lifetime in seconds (default: 604800 = 7 days)
    pub lifetime_secs: i64,

    /// Enable token rotation on refresh (default: true)
    pub rotate_on_refresh: bool,

    /// Detect reuse of rotated tokens (default: true)
    pub detect_reuse: bool,

    /// Storage backend: "redis", "postgres", or "turso"
    pub storage: String,
}
```

### TOML Configuration

```toml
[auth.tokens]
access_token_lifetime_secs = 900
issuer = "my-auth-service"
audience = "my-api"
include_jti = true

[auth.paseto]
version = "v4"
purpose = "local"
key_path = "keys/paseto.key"

[auth.refresh_tokens]
enabled = true
lifetime_secs = 604800
rotate_on_refresh = true
detect_reuse = true
storage = "redis"
```

---

## Complete Login Flow

```rust
use acton_service::auth::{
    PasswordHasher, PasetoGenerator, TokenGenerator, ClaimsBuilder,
    RedisRefreshStorage, RefreshTokenStorage, RefreshTokenMetadata,
    TokenPair,
};
use chrono::{Utc, Duration};
use uuid::Uuid;

async fn login(
    credentials: LoginRequest,
    hasher: &PasswordHasher,
    generator: &PasetoGenerator,
    storage: &RedisRefreshStorage,
) -> Result<TokenPair, Error> {
    // 1. Verify password
    let user = find_user(&credentials.email).await?;
    if !hasher.verify(&credentials.password, &user.password_hash)? {
        return Err(Error::Auth("Invalid credentials".into()));
    }

    // 2. Build claims
    let claims = ClaimsBuilder::new()
        .user(&user.id)
        .email(&user.email)
        .roles(user.roles.clone())
        .build()?;

    // 3. Generate access token
    let access_token = generator.generate_token(&claims)?;

    // 4. Generate and store refresh token
    let refresh_token_id = Uuid::new_v4().to_string();
    let family_id = Uuid::new_v4().to_string();
    let metadata = RefreshTokenMetadata::default();

    storage.store(
        &refresh_token_id,
        &user.id,
        &family_id,
        Utc::now() + Duration::days(7),
        &metadata,
    ).await?;

    Ok(TokenPair::new(
        access_token,
        refresh_token_id,
        900,    // 15 min
        604800, // 7 days
    ))
}

async fn refresh(
    refresh_token_id: &str,
    generator: &PasetoGenerator,
    storage: &RedisRefreshStorage,
) -> Result<TokenPair, Error> {
    // 1. Get and validate refresh token
    let token_data = storage.get(refresh_token_id).await?
        .ok_or(Error::Auth("Invalid refresh token".into()))?;

    if token_data.is_revoked {
        // Potential token reuse - revoke entire family
        storage.revoke_family(&token_data.family_id).await?;
        return Err(Error::Auth("Token reuse detected".into()));
    }

    // 2. Load user and build new claims
    let user = find_user_by_id(&token_data.user_id).await?;
    let claims = ClaimsBuilder::new()
        .user(&user.id)
        .email(&user.email)
        .roles(user.roles.clone())
        .build()?;

    // 3. Generate new access token
    let access_token = generator.generate_token(&claims)?;

    // 4. Rotate refresh token
    let new_refresh_token_id = Uuid::new_v4().to_string();
    storage.rotate(
        refresh_token_id,
        &new_refresh_token_id,
        &user.id,
        &token_data.family_id,
        Utc::now() + Duration::days(7),
        &RefreshTokenMetadata::default(),
    ).await?;

    Ok(TokenPair::new(
        access_token,
        new_refresh_token_id,
        900,
        604800,
    ))
}
```

---

## API Reference

### TokenGenerator Trait

```rust
pub trait TokenGenerator: Send + Sync + Clone {
    /// Generate token with default expiration
    fn generate_token(&self, claims: &Claims) -> Result<String, Error>;

    /// Generate token with custom expiration
    fn generate_token_with_expiry(
        &self,
        claims: &Claims,
        expires_in: Duration,
    ) -> Result<String, Error>;

    /// Get default token lifetime
    fn default_lifetime(&self) -> Duration;
}
```

### RefreshTokenStorage Trait

```rust
#[async_trait]
pub trait RefreshTokenStorage: Send + Sync {
    async fn store(...) -> Result<(), Error>;
    async fn get(&self, token_id: &str) -> Result<Option<RefreshTokenData>, Error>;
    async fn revoke(&self, token_id: &str) -> Result<(), Error>;
    async fn revoke_family(&self, family_id: &str) -> Result<u64, Error>;
    async fn revoke_all_for_user(&self, user_id: &str) -> Result<u64, Error>;
    async fn rotate(...) -> Result<(), Error>;
    async fn cleanup_expired(&self) -> Result<u64, Error>;
}
```

---

## Next Steps

- [Token Authentication](/docs/token-auth) - Middleware for validating incoming tokens
- [Password Hashing](/docs/password-hashing) - Hash passwords before generating tokens
- [Authentication Overview](/docs/auth) - All auth capabilities
