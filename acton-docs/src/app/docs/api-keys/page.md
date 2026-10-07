---
title: API Keys
nextjs:
  metadata:
    title: API Keys
    description: Generate and validate API keys for service-to-service authentication with scope-based permissions and multiple storage backends.
---

{% callout type="note" title="Part of the Auth Module" %}
This guide covers API key authentication. See the [Authentication Overview](/docs/auth) for all auth capabilities, or jump to [Password Hashing](/docs/password-hashing), [Token Generation](/docs/token-generation), or [OAuth/OIDC](/docs/oauth).
{% /callout %}

---

## Introduction

API key authentication in acton-service provides machine-to-machine authentication for services, integrations, and third-party access. Keys follow the format `{prefix}_{random_base32}`, similar to Stripe (`sk_live_...`) and GitHub (`ghp_...`), making them recognizable and easy to manage.

Keys are hashed with keyed BLAKE3 before storage. Only the versioned digest is persisted; a separate server-side pepper protects it. The framework supports scope-based permissions, rate limiting per key, and efficient prefix-based lookup for validation. Storage backends include Redis, PostgreSQL, MSSQL, Turso, and SurrealDB.

**Key characteristics:**

- **High entropy**: 192 bits of randomness per key
- **Secure storage**: Keyed BLAKE3 with a persistent 256-bit server-side pepper
- **Prefix lookup**: First 8 characters indexed for efficient validation
- **Scope-based access**: Fine-grained permissions per key
- **Revocation**: Instant revocation without affecting other keys

---

## Quick Start

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["auth", "cache"] }
```

```rust
use acton_service::auth::{ApiKeyGenerator, ApiKeyPepper};

// Create generator with your prefix
let pepper = ApiKeyPepper::from_file("./secrets/api-key.pepper")?;
let generator = ApiKeyGenerator::new("sk_live", pepper);

// Generate a new API key
let (key, key_hash) = generator.generate()?;
// key = "sk_live_abc123..." - show to user ONCE
// key_hash = "$blake3-keyed$v1$..." - store in database

// Later, verify an incoming key
let is_valid = generator.verify(&incoming_key, &stored_hash)?;
```

Provision `./secrets/api-key.pepper` once with exactly 32 cryptographically random raw bytes. Keep it in a server secret store, separate from the API-key database, and restrict file access to the service. The file must contain no text encoding or trailing newline. The generator never creates a replacement secret automatically, and missing or incorrectly sized files return an error. Initialize the generator before serving requests and pass its clone to your storage constructor.

Key verification uses BLAKE3's constant-time digest comparison. API keys have 192 bits of random entropy and do not need password stretching; human passwords continue to use Argon2id.

## Upgrading existing keys and rotating the pepper

Version 0.47 stores digests as `$blake3-keyed$v1$` followed by 64 hexadecimal characters. Existing `$argon2id$` hashes and unknown formats are rejected immediately. The verifier does not run Argon2 as a compatibility fallback, so legacy requests cannot trigger password-hashing memory or CPU costs.

Before upgrading, revoke and reissue existing API keys under the new generator. An Argon2id hash cannot be converted into a keyed BLAKE3 digest. If a trusted offline process already holds the original high-entropy plaintext keys, it may instead call `generator.hash(&plaintext_key)` and replace the stored `key_hash`. Never hash the old Argon2 digest as if it were plaintext. The existing string column and lookup-prefix index need no schema change.

Persist the same pepper across restarts and share it among every issuer and verifier, including all service replicas. Replacing the pepper invalidates existing digests. Rotation therefore requires coordinated key reissuance or offline rehashing from securely held plaintext before switching all replicas. There is no old-pepper fallback or automatic rotation. A wrong pepper returns a verification mismatch; unreadable or malformed pepper files return initialization errors.

---

## Key Format

API keys use the format `{prefix}_{random}` where:

- **Prefix**: Identifies the key type and environment (e.g., `sk_live`, `sk_test`, `pk_live`)
- **Random**: 192 bits of entropy encoded as lowercase base32

```text
sk_live_abcdefghijklmnopqrstuvwxyz234567
└──┬──┘ └──────────────┬───────────────┘
 Prefix          Random (base32)
```

**Common prefix conventions:**

| Prefix | Use Case |
|--------|----------|
| `sk_live` | Secret key, production |
| `sk_test` | Secret key, testing |
| `pk_live` | Public key, production |
| `acton` | Generic service key |

---

## Generating Keys

```rust
use acton_service::auth::{ApiKeyGenerator, ApiKeyPepper};

let pepper = ApiKeyPepper::from_file("./secrets/api-key.pepper")?;
let generator = ApiKeyGenerator::new("sk_live", pepper);

// Generate returns (plaintext_key, keyed_blake3_digest)
let (key, hash) = generator.generate()?;

// IMPORTANT: Show the key to the user exactly ONCE
// After this, you can never recover it
println!("Your API key: {}", key);

// Store only the hash in your database
store_api_key(ApiKey {
    id: uuid::Uuid::new_v4().to_string(),
    user_id: "user:123".to_string(),
    name: "Production API Key".to_string(),
    prefix: ApiKeyGenerator::key_prefix_for_lookup(&key)
        .ok_or(Error::ValidationError("Invalid API-key format".into()))?,
    key_hash: hash,
    scopes: vec!["read:data".to_string(), "write:data".to_string()],
    rate_limit: Some(1000), // 1000 requests/minute
    is_revoked: false,
    last_used_at: None,
    expires_at: None,
    created_at: Utc::now(),
}).await?;
```

---

## Verifying Keys

Two approaches for validating incoming API keys:

### Direct Verification

If you already have the stored hash:

```rust
let pepper = ApiKeyPepper::from_file("./secrets/api-key.pepper")?;
let generator = ApiKeyGenerator::new("sk_live", pepper);

// Verify the key against stored hash
if generator.verify(&incoming_key, &stored_hash)? {
    // Key is valid
}
```

### With Storage Backend

Using the storage trait for complete validation:

```rust
use acton_service::auth::{ApiKeyStorage, RedisApiKeyStorage};

let storage = RedisApiKeyStorage::new(redis_pool, generator.clone());

// Get by full key (includes hash verification)
if let Some(api_key) = storage.get_by_key(&incoming_key).await? {
    if api_key.is_valid() {
        // Update last_used timestamp
        storage.update_last_used(&api_key.id).await?;

        // Check scopes
        if api_key.has_scope("write:data") {
            // Authorized for write operations
        }
    }
}
```

### Prefix-Based Lookup

For efficient validation, keys support prefix lookup using the first 8 characters of the random part:

```rust
// Extract lookup prefix from incoming key
let lookup_prefix = ApiKeyGenerator::key_prefix_for_lookup(&incoming_key);
// Returns: "sk_live_abcdefgh"

// Use for indexed database lookup
let api_key = storage.get_by_prefix(&lookup_prefix).await?;
```

This allows indexing on a short prefix rather than the full hash, improving lookup performance.

---

## ApiKey Structure

```rust
pub struct ApiKey {
    /// Database ID
    pub id: String,

    /// User/owner ID
    pub user_id: String,

    /// User-provided name for the key
    pub name: String,

    /// Indexed lookup prefix (e.g., "sk_live_abcd2345")
    pub prefix: String,

    /// Versioned keyed BLAKE3 digest
    pub key_hash: String,

    /// Allowed scopes/permissions
    pub scopes: Vec<String>,

    /// Rate limit (requests per minute, None = default)
    pub rate_limit: Option<u32>,

    /// Whether this key has been revoked
    pub is_revoked: bool,

    /// When this key was last used
    pub last_used_at: Option<DateTime<Utc>>,

    /// When this key expires (None = never)
    pub expires_at: Option<DateTime<Utc>>,

    /// When this key was created
    pub created_at: DateTime<Utc>,
}
```

**Helper methods:**

```rust
// Check if key is currently valid (not revoked, not expired)
api_key.is_valid() -> bool

// Check if key has a specific scope
api_key.has_scope("read:data") -> bool
```

---

## Scope-Based Permissions

Scopes provide fine-grained access control per API key:

```rust
// Create key with specific scopes
let api_key = ApiKey {
    scopes: vec![
        "read:users".to_string(),
        "write:users".to_string(),
        "read:orders".to_string(),
    ],
    ..Default::default()
};

// Check scope before allowing operation
async fn update_user(api_key: &ApiKey, user_id: &str, data: UserUpdate) -> Result<User, Error> {
    if !api_key.has_scope("write:users") {
        return Err(Error::Forbidden("Missing write:users scope".into()));
    }

    // Perform update...
}
```

**Common scope patterns:**

| Pattern | Description |
|---------|-------------|
| `read:{resource}` | Read access to resource |
| `write:{resource}` | Create/update access |
| `delete:{resource}` | Delete access |
| `admin` | Full administrative access |
| `*` | Wildcard (all permissions) |

---

## Storage Backends

### Redis

Fast lookups with automatic TTL handling:

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["auth", "cache"] }
```

```rust
use acton_service::auth::RedisApiKeyStorage;

let storage = RedisApiKeyStorage::new(redis_pool, generator.clone());
```

### PostgreSQL

Durable storage with queryable metadata:

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["auth", "database"] }
```

```rust
use acton_service::auth::PgApiKeyStorage;

let storage = PgApiKeyStorage::new(pg_pool, generator.clone());
```

**Database schema:**

```sql
CREATE TABLE api_keys (
    id VARCHAR(255) PRIMARY KEY,
    user_id VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    key_prefix VARCHAR(255) NOT NULL UNIQUE,
    key_hash TEXT NOT NULL,
    scopes JSONB NOT NULL DEFAULT '[]',
    rate_limit INTEGER,
    is_revoked BOOLEAN NOT NULL DEFAULT FALSE,
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_api_keys_user_id ON api_keys(user_id);
CREATE INDEX idx_api_keys_key_prefix ON api_keys(key_prefix);
```

### Turso

Edge-deployed storage for global distribution:

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["auth", "turso"] }
```

```rust
use acton_service::auth::TursoApiKeyStorage;

let storage = TursoApiKeyStorage::new(turso_conn, generator.clone());
```

### MSSQL and SurrealDB

These backends use the same configured generator and stored digest format. Enable `auth` with the corresponding `mssql` or `surrealdb` feature:

```rust
let mssql_storage = MssqlApiKeyStorage::new(mssql_pool, generator.clone()).await?;
let surreal_storage = SurrealDbApiKeyStorage::new(surreal_client, generator.clone());
```

Select one primary database backend per build. Each storage verifier must use the same persistent pepper as the generator that issued the keys.

---

## Storage API

```rust
#[async_trait]
pub trait ApiKeyStorage: Send + Sync {
    /// Get key by full key value (includes verification)
    async fn get_by_key(&self, key: &str) -> Result<Option<ApiKey>, Error>;

    /// Get key by prefix (for indexed lookup)
    async fn get_by_prefix(&self, prefix: &str) -> Result<Option<ApiKey>, Error>;

    /// Get key by database ID
    async fn get_by_id(&self, id: &str) -> Result<Option<ApiKey>, Error>;

    /// Store a new API key
    async fn create(&self, key: &ApiKey) -> Result<(), Error>;

    /// Update last_used_at timestamp
    async fn update_last_used(&self, id: &str) -> Result<(), Error>;

    /// Revoke an API key
    async fn revoke(&self, id: &str) -> Result<(), Error>;

    /// List all keys for a user
    async fn list_by_user(&self, user_id: &str) -> Result<Vec<ApiKey>, Error>;

    /// Permanently delete a key
    async fn delete(&self, id: &str) -> Result<(), Error>;
}
```

---

## Configuration

```rust
pub struct ApiKeyConfig {
    /// Enable API key authentication (default: true)
    pub enabled: bool,

    /// Key prefix (default: "sk_live")
    pub prefix: String,

    /// Path to a persistent raw 32-byte secret
    pub pepper_path: Option<std::path::PathBuf>,

    /// Header name for API key (default: "X-API-Key")
    pub header: String,

    /// Default rate limit per key (requests/minute)
    pub default_rate_limit: Option<u32>,

    /// Storage backend: "redis", "postgres", or "turso"
    pub storage: String,
}
```

**TOML configuration:**

```toml
[auth.api_keys]
enabled = true
prefix = "sk_live"
pepper_path = "./secrets/api-key.pepper"
header = "X-API-Key"
default_rate_limit = 1000
storage = "redis"
```

Configuration supplies the generator explicitly; API-key storage and authentication middleware are managed by the application:

```rust
use acton_service::auth::{ApiKeyConfig, RedisApiKeyStorage};

let config = ApiKeyConfig {
    pepper_path: Some("./secrets/api-key.pepper".into()),
    ..ApiKeyConfig::default()
};
let generator = config.generator()?;
let storage = RedisApiKeyStorage::new(redis_pool, generator.clone());
```

---

## Complete API Key Management

```rust
use acton_service::auth::{ApiKeyGenerator, ApiKey, ApiKeyStorage, RedisApiKeyStorage};
use chrono::Utc;
use uuid::Uuid;

// Create API key endpoint
async fn create_api_key(
    user_id: &str,
    request: CreateKeyRequest,
    storage: &RedisApiKeyStorage,
    generator: &ApiKeyGenerator,
) -> Result<CreateKeyResponse, Error> {
    let (key, hash) = generator.generate()?;

    let api_key = ApiKey {
        id: Uuid::new_v4().to_string(),
        user_id: user_id.to_string(),
        name: request.name,
        prefix: ApiKeyGenerator::key_prefix_for_lookup(&key)
            .ok_or(Error::Internal("Failed to extract prefix".into()))?,
        key_hash: hash,
        scopes: request.scopes,
        rate_limit: request.rate_limit,
        is_revoked: false,
        last_used_at: None,
        expires_at: request.expires_at,
        created_at: Utc::now(),
    };

    storage.create(&api_key).await?;

    // Return the key ONCE - it cannot be retrieved again
    Ok(CreateKeyResponse {
        id: api_key.id,
        key, // Show to user once!
        name: api_key.name,
        scopes: api_key.scopes,
    })
}

// List user's keys (without revealing the actual keys)
async fn list_api_keys(
    user_id: &str,
    storage: &RedisApiKeyStorage,
    generator: &ApiKeyGenerator,
) -> Result<Vec<KeyInfo>, Error> {
    let keys = storage.list_by_user(user_id).await?;

    Ok(keys.into_iter().map(|k| KeyInfo {
        id: k.id,
        name: k.name,
        prefix: k.prefix.chars().take(12).collect(), // Show partial prefix only
        scopes: k.scopes,
        last_used_at: k.last_used_at,
        created_at: k.created_at,
        is_revoked: k.is_revoked,
    }).collect())
}

// Revoke a key
async fn revoke_api_key(
    user_id: &str,
    key_id: &str,
    storage: &RedisApiKeyStorage,
    generator: &ApiKeyGenerator,
) -> Result<(), Error> {
    // Verify ownership
    let key = storage.get_by_id(key_id).await?
        .ok_or(Error::NotFound("API key not found".into()))?;

    if key.user_id != user_id {
        return Err(Error::Forbidden("Not your key".into()));
    }

    storage.revoke(key_id).await?;
    Ok(())
}
```

---

## Middleware Integration

Extract and validate API keys from incoming requests:

```rust
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

pub async fn api_key_auth(
    State(storage): State<RedisApiKeyStorage>,
    mut request: Request,
    next: Next,
) -> Result<Response, Error> {
    // Extract key from header
    let key = request
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::Auth("Missing API key".into()))?;

    // Validate and get key data
    let api_key = storage.get_by_key(key).await?
        .ok_or(Error::Auth("Invalid API key".into()))?;

    if !api_key.is_valid() {
        return Err(Error::Auth("API key revoked or expired".into()));
    }

    // Update last used (fire and forget)
    let id = api_key.id.clone();
    let storage_clone = storage.clone();
    tokio::spawn(async move {
        let _ = storage_clone.update_last_used(&id).await;
    });

    // Add key info to request extensions
    request.extensions_mut().insert(api_key);

    Ok(next.run(request).await)
}
```

---

## Security Best Practices

### Key Display

Show the full key exactly once during creation. After that, only display a masked version:

```rust
// During creation
"Your API key: sk_live_abcdefghijklmnopqrstuvwxyz234567"

// In key listing
"sk_live_abcd...7" // First 4 + last 1 characters of random part
```

### Key Rotation

Encourage users to rotate keys periodically:

```rust
async fn rotate_api_key(
    user_id: &str,
    old_key_id: &str,
    storage: &RedisApiKeyStorage,
    generator: &ApiKeyGenerator,
) -> Result<CreateKeyResponse, Error> {
    // Get old key details
    let old_key = storage.get_by_id(old_key_id).await?
        .ok_or(Error::NotFound("Key not found".into()))?;

    // Create new key with same settings
    let new_key = create_api_key(user_id, CreateKeyRequest {
        name: format!("{} (rotated)", old_key.name),
        scopes: old_key.scopes,
        rate_limit: old_key.rate_limit,
        expires_at: old_key.expires_at,
    }, storage).await?;

    // Revoke old key
    storage.revoke(old_key_id).await?;

    Ok(new_key)
}
```

### Rate Limiting

Combine API keys with rate limiting:

```rust
// Check rate limit before processing
if let Some(limit) = api_key.rate_limit {
    let key = format!("rate:{}:{}", api_key.id, current_minute());
    let count = redis.incr(&key).await?;

    if count == 1 {
        redis.expire(&key, 60).await?;
    }

    if count > limit as i64 {
        return Err(Error::RateLimited);
    }
}
```

---

## Next Steps

- [Token Generation](/docs/token-generation) - Generate access tokens for users
- [Rate Limiting](/docs/rate-limiting) - Apply rate limits to API keys
- [Authentication Overview](/docs/auth) - All auth capabilities
