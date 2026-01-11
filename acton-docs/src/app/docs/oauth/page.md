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

The `OAuthProvider` trait abstracts provider differences, normalizing user information across Google, GitHub, and custom providers. State management with Redis prevents CSRF attacks during the OAuth flow. After authentication, you can generate your own tokens using the [Token Generation](/docs/token-generation) module.

**Key characteristics:**

- **Pre-built providers**: Google and GitHub with sensible default scopes
- **Custom OIDC**: Connect to any OIDC-compliant identity provider
- **Normalized user info**: Consistent data structure regardless of provider
- **CSRF protection**: Cryptographically secure state values with TTL expiration
- **Flexible scopes**: Default scopes with optional additional permissions

---

## Quick Start

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["auth", "oauth", "cache"] }
```

```rust
use acton_service::auth::oauth::{GoogleProvider, OAuthProvider};
use acton_service::auth::config::OAuthProviderConfig;

let config = OAuthProviderConfig {
    client_id: "your-client-id".to_string(),
    client_secret: "your-client-secret".to_string(),
    redirect_uri: "https://example.com/auth/google/callback".to_string(),
    scopes: vec![], // Use defaults: openid, email, profile
    ..Default::default()
};

let provider = GoogleProvider::new(&config)?;

// Generate authorization URL
let state = generate_state(); // Cryptographically random
let auth_url = provider.authorization_url(&state, &[]);
// Redirect user to auth_url

// In callback handler:
let tokens = provider.exchange_code(&authorization_code).await?;
let user_info = provider.get_user_info(&tokens.access_token).await?;
```

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
     │                      │    Store in Redis    │
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
    ..Default::default()
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
    ..Default::default()
};

let provider = GitHubProvider::new(&config)?;
```

**Default scopes**: `read:user`, `user:email`

**User info returned**:
- `provider_user_id`: GitHub's numeric user ID
- `email`: Primary verified email (fetched from `/user/emails` if needed)
- `email_verified`: Always `true` (GitHub only exposes verified emails)
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
    authorization_endpoint: "https://idp.example.com/authorize".to_string(),
    token_endpoint: "https://idp.example.com/token".to_string(),
    userinfo_endpoint: "https://idp.example.com/userinfo".to_string(),
};

let provider = CustomOidcProvider::new(&config)?;
```

---

## State Management

State values prevent CSRF attacks by ensuring the callback originated from a request your app initiated.

### Generate and Store State

```rust
use acton_service::auth::oauth::{
    generate_state, RedisOAuthStateManager, OAuthStateManager, StateData,
};
use chrono::Utc;

// Create state manager with 10-minute TTL
let state_manager = RedisOAuthStateManager::new(redis_pool, 600);

// Create state data
let state_data = StateData {
    provider: "google".to_string(),
    redirect_uri: Some("/dashboard".to_string()), // Where to go after auth
    created_at: Utc::now().timestamp(),
    extra: None, // Custom data if needed
};

// Store state and get token
let state = state_manager.create_state(&state_data).await?;

// Use in authorization URL
let auth_url = provider.authorization_url(&state, &[]);
```

### Validate in Callback

```rust
async fn callback(
    Query(params): Query<CallbackParams>,
    State(state_manager): State<RedisOAuthStateManager>,
    State(provider): State<GoogleProvider>,
) -> Result<Response, Error> {
    // Validate and consume state (one-time use)
    let state_data = state_manager.validate_state(&params.state).await?;

    // Exchange code for tokens
    let tokens = provider.exchange_code(&params.code).await?;

    // Get user info
    let user_info = provider.get_user_info(&tokens.access_token).await?;

    // Create or update user in your database
    // Generate your own session/tokens
    // Redirect to state_data.redirect_uri
}
```

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

## Complete OAuth Flow Example

```rust
use axum::{
    extract::{Query, State},
    response::{Redirect, IntoResponse},
    routing::get,
    Router,
};
use acton_service::auth::oauth::{
    GoogleProvider, OAuthProvider, RedisOAuthStateManager, OAuthStateManager, StateData,
};
use acton_service::auth::{PasetoGenerator, TokenGenerator, ClaimsBuilder};

#[derive(Clone)]
struct AppState {
    google: GoogleProvider,
    state_manager: RedisOAuthStateManager,
    token_generator: PasetoGenerator,
}

// Initiate OAuth flow
async fn login_google(State(state): State<AppState>) -> impl IntoResponse {
    let state_data = StateData {
        provider: "google".to_string(),
        redirect_uri: Some("/dashboard".to_string()),
        created_at: chrono::Utc::now().timestamp(),
        extra: None,
    };

    let oauth_state = state.state_manager.create_state(&state_data).await.unwrap();
    let auth_url = state.google.authorization_url(&oauth_state, &[]);

    Redirect::to(&auth_url)
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

// Handle OAuth callback
async fn callback_google(
    Query(params): Query<CallbackQuery>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, Error> {
    // 1. Validate state (CSRF protection)
    let state_data = state.state_manager
        .validate_state(&params.state)
        .await?;

    // 2. Exchange code for tokens
    let oauth_tokens = state.google
        .exchange_code(&params.code)
        .await?;

    // 3. Get user info
    let user_info = state.google
        .get_user_info(&oauth_tokens.access_token)
        .await?;

    // 4. Find or create user in your database
    let user = find_or_create_user(&user_info).await?;

    // 5. Generate your own tokens
    let claims = ClaimsBuilder::new()
        .user(&user.id)
        .email(user_info.email.as_deref().unwrap_or(""))
        .build()?;

    let token = state.token_generator.generate_token(&claims)?;

    // 6. Set token in cookie or return in response
    let redirect = state_data.redirect_uri.unwrap_or("/".to_string());

    Ok((
        [("Set-Cookie", format!("token={}; HttpOnly; Secure; Path=/", token))],
        Redirect::to(&redirect),
    ))
}

async fn find_or_create_user(info: &OAuthUserInfo) -> Result<User, Error> {
    // Look up by provider + provider_user_id
    if let Some(user) = find_user_by_oauth(
        &info.provider,
        &info.provider_user_id,
    ).await? {
        return Ok(user);
    }

    // Check if email exists (link accounts)
    if let Some(email) = &info.email {
        if let Some(user) = find_user_by_email(email).await? {
            // Link OAuth to existing account
            link_oauth_account(&user.id, &info.provider, &info.provider_user_id).await?;
            return Ok(user);
        }
    }

    // Create new user
    create_user(CreateUser {
        email: info.email.clone(),
        name: info.name.clone(),
        picture: info.picture.clone(),
        oauth_provider: Some(info.provider.clone()),
        oauth_provider_id: Some(info.provider_user_id.clone()),
    }).await
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/auth/google", get(login_google))
        .route("/auth/google/callback", get(callback_google))
        .with_state(state)
}
```

---

## Multiple Providers

Support multiple OAuth providers in the same application:

```rust
use std::collections::HashMap;
use acton_service::auth::oauth::{
    GoogleProvider, GitHubProvider, OAuthProvider,
};

// Store providers by name
let mut providers: HashMap<String, Box<dyn OAuthProvider>> = HashMap::new();
providers.insert("google".to_string(), Box::new(GoogleProvider::new(&google_config)?));
providers.insert("github".to_string(), Box::new(GitHubProvider::new(&github_config)?));

// Dynamic login endpoint
async fn login(
    Path(provider_name): Path<String>,
    State(state): State<AppState>,
) -> Result<Redirect, Error> {
    let provider = state.providers.get(&provider_name)
        .ok_or(Error::NotFound("Unknown provider".into()))?;

    let state_data = StateData {
        provider: provider_name,
        redirect_uri: Some("/dashboard".to_string()),
        created_at: chrono::Utc::now().timestamp(),
        extra: None,
    };

    let oauth_state = state.state_manager.create_state(&state_data).await?;
    let auth_url = provider.authorization_url(&oauth_state, &[]);

    Ok(Redirect::to(&auth_url))
}
```

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
let state_manager = RedisOAuthStateManager::new(redis_pool, 600); // 10 minutes
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
