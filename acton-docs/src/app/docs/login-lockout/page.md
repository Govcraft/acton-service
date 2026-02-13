---
title: Login Lockout
nextjs:
  metadata:
    title: Login Lockout
    description: Progressive delay and account lockout for brute force protection
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

The `login-lockout` feature provides brute force protection for login endpoints. It tracks failed login attempts per identity in Redis, applies configurable progressive delays, and locks accounts after repeated failures.

**Feature flag**: `login-lockout` (depends on `auth` + `cache`)

```toml
{% $dep.loginLockout %}
```

---

## Quick Start

### 1. Add lockout configuration to `config.toml`

```toml
[lockout]
enabled = true
max_attempts = 5
window_secs = 900
lockout_duration_secs = 1800
```

### 2. Create the lockout service and use in your handler

```rust
use acton_service::prelude::*;

async fn login(
    State(lockout): State<LoginLockout>,
    Json(creds): Json<LoginRequest>,
) -> Result<Json<TokenPair>> {
    // Check if account is locked
    let status = lockout.check(&creds.email).await?;
    if status.locked {
        return Err(Error::AccountLocked {
            message: format!("Try again in {} seconds", status.lockout_remaining_secs),
            retry_after_secs: status.lockout_remaining_secs,
        });
    }

    // Attempt authentication
    match authenticate(&creds).await {
        Ok(tokens) => {
            lockout.record_success(&creds.email).await?;
            Ok(Json(tokens))
        }
        Err(_) => {
            let status = lockout.record_failure(&creds.email).await?;
            if status.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(status.delay_ms)).await;
            }
            Err(Error::Unauthorized("Invalid credentials".into()))
        }
    }
}
```

---

## Configuration Reference

All fields have sensible defaults. Only override what you need.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | `bool` | `true` | Whether lockout enforcement is active |
| `max_attempts` | `u32` | `5` | Failed attempts before account is locked |
| `window_secs` | `u64` | `900` (15 min) | Rolling window for counting failures |
| `lockout_duration_secs` | `u64` | `1800` (30 min) | How long a locked account stays locked |
| `progressive_delay_enabled` | `bool` | `true` | Apply exponential backoff delays |
| `base_delay_ms` | `u64` | `1000` (1s) | Initial delay after first failure |
| `max_delay_ms` | `u64` | `30000` (30s) | Maximum delay cap |
| `delay_multiplier` | `f64` | `2.0` | Exponential backoff multiplier |
| `warning_threshold` | `u32` | `3` | Fire warning notification after N failures (0 = disabled) |
| `key_prefix` | `String` | `"lockout"` | Redis key prefix (no `:` or whitespace) |

---

## Service API

### `LoginLockout`

The core service. Construct once at startup, share via axum `State` or `Extension`.

```rust
use acton_service::prelude::*;

// Create the service
let lockout_config = LockoutConfig::default();
let lockout = LoginLockout::new(lockout_config, redis_pool);

// Add to your router
let app = Router::new()
    .route("/login", post(login_handler))
    .with_state(lockout);
```

### Methods

#### `check(identity) -> Result<LockoutStatus>`

Check lockout status without recording a failure. Returns current attempt count, lock state, and recommended delay.

#### `record_failure(identity) -> Result<LockoutStatus>`

Record a failed login attempt. Increments the counter, fires notifications, and locks the account if the threshold is reached.

#### `record_success(identity) -> Result<()>`

Record a successful login. Clears all lockout state (attempt counter and lock flag) for the identity.

#### `unlock(identity) -> Result<()>`

Manually unlock an account (admin action). Clears both the attempt counter and lockout flag.

### `LockoutStatus`

Returned by `check()` and `record_failure()`:

| Field | Type | Description |
|-------|------|-------------|
| `locked` | `bool` | Whether the account is currently locked |
| `attempt_count` | `u32` | Number of failed attempts in the window |
| `max_attempts` | `u32` | Maximum allowed attempts |
| `lockout_remaining_secs` | `u64` | Seconds until lock expires (0 if not locked) |
| `delay_ms` | `u64` | Recommended delay before responding (0 if none) |

---

## Progressive Delay

When `progressive_delay_enabled` is true, each failed attempt increases the response delay exponentially:

```text
delay = min(base_delay_ms * delay_multiplier^(attempt - 1), max_delay_ms)
```

With default settings (`base=1000ms`, `multiplier=2.0`, `max=30000ms`):

| Attempt | Delay |
|---------|-------|
| 1 | 1,000 ms (1s) |
| 2 | 2,000 ms (2s) |
| 3 | 4,000 ms (4s) |
| 4 | 8,000 ms (8s) |
| 5 | 16,000 ms (16s) |
| 6+ | 30,000 ms (30s, capped) |

The delay is returned in `LockoutStatus.delay_ms`. The caller is responsible for applying the sleep:

```rust
let status = lockout.record_failure(&email).await?;
if status.delay_ms > 0 {
    tokio::time::sleep(Duration::from_millis(status.delay_ms)).await;
}
```

---

## Middleware Approach

For automatic enforcement without manual `check`/`record_failure`/`record_success` calls, use `LockoutMiddleware`:

```rust
use acton_service::prelude::*;

let lockout = LoginLockout::new(config, redis_pool);
let mw = LockoutMiddleware::new(lockout, "email"); // "email" = JSON field name

let app = Router::new()
    .route("/login", post(login_handler))
    .route_layer(axum::middleware::from_fn_with_state(
        mw,
        LockoutMiddleware::middleware,
    ));
```

The middleware:
1. Extracts the identity from the JSON request body (field name configurable)
2. If locked, returns **HTTP 423** with `Retry-After` header
3. Forwards the request to your handler
4. If the handler returns **401**, records a failure and applies progressive delay
5. If the handler returns **2xx**, records a success

Non-JSON requests pass through without enforcement.

---

## Notification Hooks

Register handlers to react to lockout lifecycle events:

```rust
use acton_service::prelude::*;

struct EmailNotifier { /* ... */ }

#[async_trait]
impl LockoutNotification for EmailNotifier {
    async fn on_event(&self, event: LockoutEvent) {
        match event {
            LockoutEvent::AccountLocked { identity, .. } => {
                // Send "your account has been locked" email
            }
            LockoutEvent::ApproachingThreshold { identity, remaining_attempts, .. } => {
                // Send "N attempts remaining" warning
            }
            _ => {}
        }
    }
}

let lockout = LoginLockout::new(config, redis_pool)
    .with_notification(Arc::new(EmailNotifier { /* ... */ }));
```

### Event Types

| Event | When | Key Fields |
|-------|------|------------|
| `FailedAttempt` | Every failed login | `identity`, `attempt_count`, `max_attempts` |
| `ApproachingThreshold` | `attempt_count == warning_threshold` | `identity`, `remaining_attempts` |
| `AccountLocked` | `attempt_count >= max_attempts` | `identity`, `lockout_duration_secs` |
| `AccountUnlocked` | Lock cleared (success, expiry, admin) | `identity`, `reason` |

Notifications are dispatched via `tokio::spawn` (fire-and-forget) and never block the login response.

---

## Audit Integration

When both `login-lockout` and `audit` features are active, use the built-in audit notification handler:

```rust
let lockout = LoginLockout::new(config, redis_pool)
    .with_audit(audit_logger);
```

This automatically emits:
- `auth.account.locked` when an account is locked
- `auth.account.unlocked` when an account is unlocked

Events include metadata with the identity, attempt count, and reason.

---

## Admin Unlock

To manually unlock an account (e.g., from an admin endpoint):

```rust
async fn admin_unlock(
    State(lockout): State<LoginLockout>,
    Path(user_id): Path<String>,
) -> Result<StatusCode> {
    lockout.unlock(&user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
```

---

## Best Practices

### Identity Normalization

Normalize identities before passing to lockout methods to prevent bypasses:

```rust
let email = creds.email.trim().to_lowercase();
let status = lockout.check(&email).await?;
```

### Redis Availability

The lockout service requires Redis. If Redis is unavailable, lockout operations will return errors. Consider:
- Setting `redis.optional = false` in config to fail fast
- Using the `resilience` feature for retry on transient Redis failures

### Testing

For unit tests that don't need Redis, test the configuration and delay logic directly:

```rust
let config = LockoutConfig {
    max_attempts: 3,
    base_delay_ms: 500,
    ..Default::default()
};
assert!(config.validate().is_ok());
```

For integration tests with Redis, use a test Redis instance and verify the full flow:

```rust
let lockout = LoginLockout::new(config, test_redis_pool);
lockout.record_failure("test@example.com").await?;
lockout.record_failure("test@example.com").await?;
let status = lockout.record_failure("test@example.com").await?;
assert!(status.locked);
```
