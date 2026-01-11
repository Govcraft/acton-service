---
title: Authentication Overview
nextjs:
  metadata:
    title: Authentication Overview
    description: Navigate the five independent authentication capabilities in acton-service - token generation, session management, password hashing, API keys, and OAuth/OIDC.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

## Introduction

Authentication in acton-service provides five independent capabilities that work together or separately. Each capability solves a specific authentication problem: generating secure tokens, managing user sessions, hashing passwords, authenticating services, or integrating with OAuth providers. You compose these capabilities based on your application's requirements.

The `auth` module defaults to PASETO tokens (more secure than JWT) and supports three storage backends (Redis, PostgreSQL, Turso). All capabilities follow the same pattern: configure once, use the appropriate type, and let the implementation handle security details. Password hashing uses Argon2id with OWASP-recommended defaults. API keys follow the `{prefix}_{base32}` format familiar from Stripe and GitHub. OAuth integration normalizes user data across Google, GitHub, and custom OIDC providers.

**Key capabilities at a glance:**

- **Token Authentication**: Generate and validate PASETO or JWT tokens for stateless authentication
- **Session Management**: Manage refresh tokens with automatic rotation and reuse detection
- **Password Hashing**: Hash and verify passwords using Argon2id with configurable cost parameters
- **API Keys**: Generate and validate API keys for service-to-service authentication
- **OAuth/OIDC**: Integrate with Google, GitHub, or custom OIDC providers

This guide helps you navigate to the detailed documentation you need.

---

## The Five Capabilities

acton-service authentication divides into five independent capabilities. Each addresses a specific authentication need. You use only what you need.

### Token Authentication

Token authentication generates and validates cryptographic tokens for stateless authentication. The framework supports two token formats: PASETO V4 (the secure default) and JWT (feature-gated for compatibility).

**When to use**: Stateless authentication for web and mobile applications, single sign-on across services, short-lived access tokens with refresh token rotation.

**Core types**: `PasetoGenerator`, `JwtGenerator` (requires `jwt` feature), `TokenGenerator` trait

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["auth"] }
```

```rust
use acton_service::auth::{PasetoGenerator, TokenGenerator};
use acton_service::middleware::Claims;

let generator = PasetoGenerator::new(&paseto_config, &token_config)?;
let claims = Claims { sub: "user:123".to_string(), ..Default::default() };
let token = generator.generate_token(&claims)?;
```

**Details**: [Token Generation Guide](/docs/token-generation)

### Session Management

Session management handles refresh tokens with automatic rotation, reuse detection, and configurable storage. Refresh tokens let users obtain new access tokens without re-authenticating.

**When to use**: Long-lived sessions for web and mobile apps, token refresh without password re-entry, additional security through rotation and reuse detection.

**Core types**: `RefreshTokenStorage` trait, `RedisRefreshStorage`, `PgRefreshStorage`, `TursoRefreshStorage`

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["auth", "cache"] }
```

**Details**: [Token Generation Guide](/docs/token-generation) (refresh tokens section)

### Password Hashing

Password hashing uses Argon2id (the OWASP-recommended algorithm) with configurable cost parameters. The hasher generates cryptographically random salts and supports constant-time verification.

**When to use**: User registration and login, password reset flows, upgrading hashing parameters over time.

**Core types**: `PasswordHasher`, `PasswordConfig`

```rust
use acton_service::auth::PasswordHasher;

let hasher = PasswordHasher::default();
let hash = hasher.hash("user_password_123")?;
let is_valid = hasher.verify("user_password_123", &hash)?;
```

**Details**: [Password Hashing Guide](/docs/password-hashing)

### API Keys

API key authentication generates prefixed, high-entropy keys for service-to-service authentication. Keys follow the format `{prefix}_{base32}` (similar to Stripe and GitHub).

**When to use**: Service-to-service authentication, third-party API access, webhook authentication, CI/CD pipeline authentication.

**Core types**: `ApiKeyGenerator`, `ApiKey`, `ApiKeyStorage` trait

```rust
use acton_service::auth::ApiKeyGenerator;

let generator = ApiKeyGenerator::new("sk_live");
let (key, key_hash) = generator.generate();
// key = "sk_live_abc123..." - show to user ONCE
// key_hash = store in database
```

**Details**: [API Keys Guide](/docs/api-keys)

### OAuth and OIDC

OAuth integration provides authentication via external identity providers: Google, GitHub, and custom OIDC providers. The framework normalizes user data across providers and manages OAuth state with CSRF protection.

**When to use**: Social login (Sign in with Google/GitHub), enterprise SSO via OIDC, reducing password management burden.

**Core types**: `OAuthProvider` trait, `GoogleProvider`, `GitHubProvider`, `CustomOidcProvider`

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = ["auth", "oauth", "cache"] }
```

**Details**: [OAuth/OIDC Guide](/docs/oauth)

---

## Decision Guide

This flowchart helps you choose the right authentication approach:

```text
┌─────────────────────────────────────┐
│ What are you authenticating?        │
└──────────────┬──────────────────────┘
               │
       ┌───────┴────────┐
       │                │
  Human Users      Services/APIs
       │                │
       │                └──▶ API Keys
       │
       ▼
┌──────────────────────┐
│ Do you control the   │
│ identity provider?   │
└──────┬───────────────┘
       │
   ┌───┴────┐
   │        │
  Yes       No
   │        │
   │        └──▶ OAuth/OIDC
   │
   ▼
Password Hashing + Token Auth
```

**Common combinations:**

| Use Case | Capabilities | Features |
|----------|-------------|----------|
| REST API with user accounts | Password Hashing + Tokens + Sessions | `auth`, `cache` |
| Mobile app with social login | OAuth + Tokens + Sessions | `auth`, `oauth`, `cache` |
| Microservices communication | API Keys | `auth`, `cache` |
| SPA with refresh tokens | Tokens + Sessions | `auth`, `cache` |
| Third-party API access | API Keys | `auth`, `cache` |

---

## PASETO vs JWT

acton-service defaults to PASETO instead of JWT. Both formats carry claims and support expiration, but PASETO eliminates entire classes of vulnerabilities present in JWT.

**Why PASETO is the default:**

JWT's flexibility creates security risks. The `alg` header allows algorithm confusion attacks. PASETO eliminates these risks by design with two secure modes:

- **V4.local** (symmetric): Encrypts claims with XChaCha20-Poly1305
- **V4.public** (asymmetric): Signs claims with Ed25519

**When to use JWT:**

Use JWT only when integrating with systems that require it (third-party APIs, mobile SDKs that parse only JWT).

| Feature | PASETO V4 | JWT |
|---------|-----------|-----|
| Algorithm confusion | Impossible | Possible |
| Encryption support | Built-in (V4.local) | Requires JWE |
| Compatibility | Limited | Universal |

To enable JWT:

```toml
acton-service = { version = "{% version() %}", features = ["auth", "jwt"] }
```

---

## Storage Backends

Session management and API keys require persistent storage. The framework supports three backends:

### Redis

Fast storage with built-in TTL support. Best for high-traffic applications.

```toml
acton-service = { version = "{% version() %}", features = ["auth", "cache"] }
```

### PostgreSQL

Durable, transactional storage. Best for auditing requirements and complex queries.

```toml
acton-service = { version = "{% version() %}", features = ["auth", "database"] }
```

### Turso

Edge-deployed, globally replicated storage. Best for multi-region deployments.

```toml
acton-service = { version = "{% version() %}", features = ["auth", "turso"] }
```

**Choosing a backend:** Start with Redis for development. Use PostgreSQL when you need durability. Choose Turso for multi-region deployments.

---

## Composing Multiple Auth Types

Real applications often combine multiple authentication types. The framework's capabilities compose cleanly because they share core types and patterns.

**Example: Web application with user registration and API access**

```rust
use acton_service::auth::{
    PasswordHasher, PasetoGenerator, RedisRefreshStorage,
    ApiKeyGenerator, TokenGenerator,
};

// Registration: hash password
let hasher = PasswordHasher::default();
let password_hash = hasher.hash(&password)?;

// Login: verify password, generate tokens
let is_valid = hasher.verify(&password, &stored_hash)?;
let access_token = generator.generate_token(&claims)?;
let refresh_token = storage.create_refresh_token(&user_id, &session_id, ttl).await?;

// API access: generate API key for developers
let (key, key_hash) = api_generator.generate();
```

**Key patterns:**

- Password hashing stands alone (no dependency on tokens or sessions)
- Token generation accepts `Claims` from any source
- Refresh tokens store user_id and session_id, decoupled from authentication method
- API keys integrate with the same `Claims` extraction middleware as tokens

---

## Next Steps

Each capability has detailed documentation:

- [Password Hashing Guide](/docs/password-hashing) - Argon2id parameters, rehashing, migration
- [Token Generation Guide](/docs/token-generation) - PASETO/JWT, refresh tokens, claims structure
- [API Keys Guide](/docs/api-keys) - Key generation, scopes, rate limiting
- [OAuth/OIDC Guide](/docs/oauth) - Google, GitHub, custom OIDC, state management

**Related documentation:**

- [Token Authentication](/docs/token-auth) - Middleware for validating incoming tokens
- [Session Management](/docs/session) - Cookie-based sessions for HTMX/SSR applications
- [Feature Flags](/docs/feature-flags) - All available feature flags
