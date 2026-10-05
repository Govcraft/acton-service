---
title: OAuth/OIDC
nextjs:
  metadata:
    title: OAuth/OIDC Integration
    description: Integrate with Google, GitHub, and custom OIDC providers for social login and enterprise SSO with CSRF-protected state management.
---

{% callout type="note" title="Part of the Auth Module" %}
This guide covers OAuth/OIDC integration. See the [Authentication Overview](/docs/auth) for all auth capabilities, or jump to [Password Hashing](/docs/password-hashing), [Token Generation](/docs/token-generation), or [API Keys](/docs/api-keys).
{% /callout %}

---

## Introduction

OAuth integration in acton-service provides authentication through external identity providers. The framework includes pre-built providers for Google and GitHub, plus support for custom OIDC-compliant providers for enterprise SSO.

The `OAuthProvider` trait abstracts provider differences, normalizing user information across Google, GitHub, and custom providers. Single-use state storage prevents replay of OAuth callbacks. Use bounded in-memory storage for one process or Redis for multiple instances. After authentication, you can generate your own tokens using the [Token Generation](/docs/token-generation) module.

**Key characteristics:**

- **Pre-built providers**: Google and GitHub with sensible default scopes
- **Custom OIDC**: Connect to any OIDC-compliant identity provider
- **Normalized user info**: Consistent data structure regardless of provider
- **CSRF protection**: Cryptographically secure state values with TTL expiration
- **Configured provider registry**: Select providers from `[auth.oauth.providers]` at startup
- **Flexible scopes**: Default scopes with optional additional permissions

---

## Quick Start

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["oauth"] }
```

```rust
use acton_service::auth::config::{OAuthConfig, OAuthProviderConfig};
use acton_service::auth::oauth::{
    MemoryOAuthStateManager, OAuthProviderRegistry, OAuthStateManager, StateData,
};

let config = OAuthConfig {
    enabled: true,
    state_ttl_secs: 600,
    providers: [("google".to_string(), OAuthProviderConfig {
        client_id: "your-client-id".to_string(),
        client_secret: "your-client-secret".to_string(),
        redirect_uri: "https://example.com/auth/google/callback".to_string(),
        scopes: vec![],
        authorization_endpoint: None,
        token_endpoint: None,
        userinfo_endpoint: None,
    })].into(),
};
let providers = OAuthProviderRegistry::from_config(&config)?;
let states = MemoryOAuthStateManager::new(config.state_ttl_secs);
let provider = providers.get("google").ok_or_else(|| {
    acton_service::error::Error::NotFound("Unknown OAuth provider".to_string())
})?;

let state = states.create_state(&StateData {
    provider: "google".to_string(),
    redirect_uri: Some("/dashboard".to_string()),
    created_at: chrono::Utc::now().timestamp(),
    extra: None,
}).await?;
let auth_url = provider.authorization_url(&state, &[]);
// Redirect the browser to auth_url.

// In the callback handler, use the same states manager and consume state first.
let data = states.validate_state(&returned_state).await?;
let provider = providers.get(&data.provider).ok_or_else(|| {
    acton_service::error::Error::NotFound("Unknown OAuth provider".to_string())
})?;
let tokens = provider.exchange_code(&authorization_code).await?;
let user_info = provider.get_user_info(&tokens.access_token).await?;
```

Keep the registry and state manager in shared application state, typically `Arc`. Each process must use the same manager for its login and callback handlers. Applications own routes and account creation or linking policy.


---

## OAuth Flow

```text
┌─────────┐           ┌───────────┐           ┌──────────┐
│  User   │           │  Your App │           │ Provider │
└────┬────┘           └─────┬─────┘           └────┬─────┘
     │                      │                      │
     │  1. Click "Sign in"  │                      │
     │─────────────────────>│                      │
     │                      │                      │
     │                      │ 2. Generate state    │
     │                      │    Store state       │
     │                      │                      │
     │  3. Redirect to provider                    │
     │<─────────────────────│─────────────────────>│
     │                      │                      │
     │              4. User authenticates          │
     │<────────────────────────────────────────────│
     │                      │                      │
     │  5. Redirect with code + state             │
     │─────────────────────>│                      │
     │                      │                      │
     │                      │ 6. Validate state    │
     │                      │    Exchange code     │
     │                      │─────────────────────>│
     │                      │                      │
     │                      │ 7. Tokens + user info│
     │                      │<─────────────────────│
     │                      │                      │
     │  8. Session/token    │                      │
     │<─────────────────────│                      │
```

---

## Providers

### Google

```rust
use acton_service::auth::oauth::GoogleProvider;
use acton_service::auth::config::OAuthProviderConfig;

let config = OAuthProviderConfig {
    client_id: env::var("GOOGLE_CLIENT_ID")?,
    client_secret: env::var("GOOGLE_CLIENT_SECRET")?,
    redirect_uri: "https://example.com/auth/google/callback".to_string(),
    scopes: vec![], // Defaults: openid, email, profile
    authorization_endpoint: None,
    token_endpoint: None,
    userinfo_endpoint: None,
};

let provider = GoogleProvider::new(&config)?;
```

**Default scopes**: `openid`, `email`, `profile`

**User info returned**:
- `provider_user_id`: Google's unique user ID (`sub`)
- `email`: User's email address
- `email_verified`: Whether Google verified the email
- `name`: User's display name
- `picture`: Profile picture URL

### GitHub

```rust
use acton_service::auth::oauth::GitHubProvider;
use acton_service::auth::config::OAuthProviderConfig;

let config = OAuthProviderConfig {
    client_id: env::var("GITHUB_CLIENT_ID")?,
    client_secret: env::var("GITHUB_CLIENT_SECRET")?,
    redirect_uri: "https://example.com/auth/github/callback".to_string(),
    scopes: vec![], // Defaults: read:user, user:email
    authorization_endpoint: None,
    token_endpoint: None,
    userinfo_endpoint: None,
};

let provider = GitHubProvider::new(&config)?;
```

**Default scopes**: `read:user`, `user:email`

**User info returned**:
- `provider_user_id`: GitHub's numeric user ID
- `email`: Primary verified email from `/user/emails`, or the public profile email when that lookup fails
- `email_verified`: `true` only when the verified-emails lookup succeeded; profile fallback is `false`
- `name`: Display name or username
- `picture`: Avatar URL

{% callout type="warning" title="GitHub Refresh Tokens" %}
GitHub OAuth apps don't support refresh tokens. If you need long-lived access, consider using GitHub Apps instead.
{% /callout %}

### Custom OIDC

For enterprise SSO or other OIDC providers:

```rust
use acton_service::auth::oauth::{CustomOidcProvider, CustomOidcConfig};

let config = CustomOidcConfig {
    client_id: env::var("OIDC_CLIENT_ID")?,
    client_secret: env::var("OIDC_CLIENT_SECRET")?,
    redirect_uri: "https://example.com/auth/enterprise/callback".to_string(),
    scopes: vec!["openid".to_string(), "email".to_string(), "profile".to_string()],
    auth_url: "https://idp.example.com/authorize".to_string(),
    token_url: "https://idp.example.com/token".to_string(),
    userinfo_url: Some("https://idp.example.com/userinfo".to_string()),
    name: "enterprise".to_string(),
};

let provider = CustomOidcProvider::new(config)?;
```

---

## State Management

State values prevent CSRF attacks by ensuring the callback originated from a request your app initiated.

### Single Process: Memory Storage

`MemoryOAuthStateManager` requires only `oauth`. It stores at most 10,000 pending logins by default, consumes a state exactly once, and sweeps expired entries on each operation. At capacity, it evicts the oldest pending login. That user's callback fails, so they must start login again.

```rust
use acton_service::auth::oauth::{
    MemoryOAuthStateManager, OAuthStateManager, StateData,
};
use std::num::NonZeroUsize;

let state_manager = MemoryOAuthStateManager::new(600);
// Or explicitly bound the number of pending logins:
let bounded = MemoryOAuthStateManager::with_max_entries(
    600, NonZeroUsize::new(1_000).expect("nonzero capacity"),
);
let state = state_manager.create_state(&StateData {
    provider: "google".to_string(),
    redirect_uri: Some("/dashboard".to_string()),
    created_at: chrono::Utc::now().timestamp(),
    extra: None,
}).await?;
let auth_url = provider.authorization_url(&state, &[]);
```

TTL begins when the state is stored and uses a monotonic clock. `created_at` is application metadata and does not control expiry. A zero TTL refuses every callback. Memory state is lost when the process restarts.

### Multiple Processes: Redis Storage

Enable `cache` alongside `oauth`, then give every instance the same Redis backend and key prefix. Redis provides the shared state needed when login and callback reach different processes.

```rust
use acton_service::auth::oauth::RedisOAuthStateManager;

let state_manager = RedisOAuthStateManager::new(redis_pool, 600);
```

Both managers implement `OAuthStateManager` and return `Error::BadRequest` for invalid, expired, or reused state.

### Validate in Callback

```rust
// Consume state before contacting the provider.
let state_data = state_manager.validate_state(&params.state).await?;
if state_data.provider != provider.name() {
    return Err(Error::BadRequest("OAuth provider mismatch".to_string()));
}
let tokens = provider.exchange_code(&params.code).await?;
let user_info = provider.get_user_info(&tokens.access_token).await?;
```

Use `user_info` according to your application's account and session policy, then redirect to the validated application's destination.


---

## OAuthProvider Trait

All providers implement this trait:

```rust
#[async_trait]
pub trait OAuthProvider: Send + Sync {
    /// Get provider name (e.g., "google", "github")
    fn name(&self) -> &str;

    /// Generate authorization URL
    fn authorization_url(&self, state: &str, scopes: &[String]) -> String;

    /// Exchange authorization code for tokens
    async fn exchange_code(&self, code: &str) -> Result<OAuthTokens, Error>;

    /// Get user information using access token
    async fn get_user_info(&self, access_token: &str) -> Result<OAuthUserInfo, Error>;

    /// Refresh access token (if supported)
    async fn refresh_token(&self, refresh_token: &str) -> Result<OAuthTokens, Error>;
}
```

---

## Data Structures

### OAuthTokens

```rust
pub struct OAuthTokens {
    /// Access token from the provider
    pub access_token: String,

    /// Refresh token (if provided)
    pub refresh_token: Option<String>,

    /// Token lifetime in seconds
    pub expires_in: Option<i64>,

    /// Token type (usually "Bearer")
    pub token_type: String,

    /// ID token for OIDC providers
    pub id_token: Option<String>,
}
```

### OAuthUserInfo

Normalized user data across all providers:

```rust
pub struct OAuthUserInfo {
    /// Provider name (e.g., "google", "github")
    pub provider: String,

    /// User ID from the provider
    pub provider_user_id: String,

    /// User's email address
    pub email: Option<String>,

    /// Whether email is verified
    pub email_verified: bool,

    /// User's display name
    pub name: Option<String>,

    /// Profile picture URL
    pub picture: Option<String>,

    /// Raw provider response (for custom fields)
    pub raw: serde_json::Value,
}
```

### StateData

```rust
pub struct StateData {
    /// Provider name
    pub provider: String,

    /// Where to redirect after auth
    pub redirect_uri: Option<String>,

    /// Creation timestamp
    pub created_at: i64,

    /// Custom data
    pub extra: Option<serde_json::Value>,
}
```

---

## Configuration

```rust
pub struct OAuthProviderConfig {
    /// OAuth client ID
    pub client_id: String,

    /// OAuth client secret
    pub client_secret: String,

    /// Redirect URI after authentication
    pub redirect_uri: String,

    /// OAuth scopes to request
    pub scopes: Vec<String>,

    /// Custom authorization endpoint (for custom OIDC)
    pub authorization_endpoint: Option<String>,

    /// Custom token endpoint (for custom OIDC)
    pub token_endpoint: Option<String>,

    /// Custom userinfo endpoint (for custom OIDC)
    pub userinfo_endpoint: Option<String>,
}
```

**TOML configuration:**

```toml
[auth.oauth]
enabled = true
state_ttl_secs = 600

[auth.oauth.providers.google]
client_id = "${GOOGLE_CLIENT_ID}"
client_secret = "${GOOGLE_CLIENT_SECRET}"
redirect_uri = "https://example.com/auth/google/callback"
scopes = ["openid", "email", "profile"]

[auth.oauth.providers.github]
client_id = "${GITHUB_CLIENT_ID}"
client_secret = "${GITHUB_CLIENT_SECRET}"
redirect_uri = "https://example.com/auth/github/callback"
scopes = ["read:user", "user:email"]
```

---

## Provider Registry and Route Handlers

`OAuthProviderRegistry::from_config(&config)` builds every configured provider at startup. Exact keys `google` and `github` select built-in providers. Any other key requires `authorization_endpoint`, `token_endpoint`, and `userinfo_endpoint` as absolute HTTP(S) URLs. A missing or invalid custom endpoint fails construction before login starts. `get` returns a provider by name; `names` lists configured names in unspecified order.

When `audit` is enabled, use `OAuthProviderRegistry::from_config_audited(&config, audit_logger)` to emit `AuthOAuthCallback` on authorization-code exchanges, including failures. Applications still decide whether `config.enabled` permits mounting OAuth routes.

The following handlers show how the registry and state manager fit together. Your application decides how to create or link accounts and issue its own session after receiving `OAuthUserInfo`.

```rust
use std::sync::Arc;
use acton_service::prelude::*;
use acton_service::auth::oauth::{
    MemoryOAuthStateManager, OAuthProviderRegistry, OAuthStateManager,
    OAuthUserInfo, StateData,
};
use serde::Deserialize;

struct OAuthAppState {
    providers: OAuthProviderRegistry,
    states: MemoryOAuthStateManager,
}

async fn login(
    Path(provider_name): Path<String>,
    Extension(app): Extension<Arc<OAuthAppState>>,
) -> Result<Redirect, Error> {
    let provider = app.providers.get(&provider_name)
        .ok_or_else(|| Error::NotFound("Unknown OAuth provider".to_string()))?;
    let state = app.states.create_state(&StateData {
        provider: provider_name,
        redirect_uri: None,
        created_at: chrono::Utc::now().timestamp(),
        extra: None,
    }).await?;
    Ok(Redirect::to(&provider.authorization_url(&state, &[])))
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

async fn callback(
    Path(provider_name): Path<String>,
    Query(params): Query<CallbackQuery>,
    Extension(app): Extension<Arc<OAuthAppState>>,
) -> Result<Json<OAuthUserInfo>, Error> {
    let data = app.states.validate_state(&params.state).await?;
    if data.provider != provider_name {
        return Err(Error::BadRequest("OAuth provider mismatch".to_string()));
    }
    let provider = app.providers.get(&data.provider)
        .ok_or_else(|| Error::NotFound("Unknown OAuth provider".to_string()))?;
    let tokens = provider.exchange_code(&params.code).await?;
    let user_info = provider.get_user_info(&tokens.access_token).await?;
    Ok(Json(user_info))
}
```

Construct `OAuthAppState` once from your `OAuthConfig`, put it in `Arc`, and attach it with `Extension` to both handlers. Provider routes should match the configured redirect URI. Bind login intent to the browser session in your application and apply an explicit policy for account linking; an email address alone is not sufficient proof that two accounts belong to the same person.

These providers exchange OAuth authorization codes and normalize user information. They do not validate OIDC ID tokens or add PKCE automatically.

---

## Security Best Practices

### State Validation

Always validate state before processing callbacks:

```rust
// Correct: validate first
let state_data = state_manager.validate_state(&params.state).await?;
let tokens = provider.exchange_code(&params.code).await?;

// WRONG: skipping state validation
let tokens = provider.exchange_code(&params.code).await?; // Vulnerable to CSRF!
```

### HTTPS Only

OAuth redirect URIs should always use HTTPS in production:

```rust
// Production
redirect_uri: "https://example.com/auth/callback".to_string()

// Development only
redirect_uri: "http://localhost:3000/auth/callback".to_string()
```

### Short State TTL

Keep state TTL short (10 minutes or less) to limit the attack window:

```rust
let state_manager = MemoryOAuthStateManager::new(600); // 10 minutes
```

### Secure Token Storage

After OAuth authentication, generate your own tokens with appropriate expiration:

```rust
// Short-lived access token (15 min)
let access_token = generator.generate_token(&claims)?;

// Long-lived refresh token if needed
let refresh_token = storage.store(...).await?;
```

---

## Next Steps

- [Token Generation](/docs/token-generation) - Generate your own tokens after OAuth
- [Password Hashing](/docs/password-hashing) - Add password login alongside OAuth
- [Session Management](/docs/session) - Cookie-based sessions for SSR apps
- [Authentication Overview](/docs/auth) - All auth capabilities
