---
title: Session Management
nextjs:
  metadata:
    title: Session Management
    description: Cookie-based session state for HTMX and server-rendered applications
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

acton-service provides traditional HTTP session management for server-rendered applications. This is ideal for HTMX-based apps, form handling, and any application that needs server-side state.

## Quick Start

### 1. Enable the Feature

```toml
[dependencies]
acton-service = { version = "{% version() %}", features = [
    "http",
    "observability",
    "session-memory"    # For development
] }
```

For production with Redis:

```toml
acton-service = { version = "{% version() %}", features = [
    "http",
    "observability",
    "session-redis"     # For production
] }
```

### 2. Configure Sessions

```toml
# config.toml
[session]
cookie_name = "session_id"
expiry_secs = 86400          # 24 hours
secure = false               # true in production (HTTPS)
http_only = true
same_site = "lax"
storage = "memory"           # or "redis"
# redis_url = "redis://localhost:6379"  # Required for redis storage
```

### 3. Use Sessions in Handlers

```rust
use acton_service::prelude::*;
use acton_service::session::{Session, FlashMessage, FlashMessages};

async fn login(
    session: Session,
    Form(creds): Form<LoginForm>,
) -> impl IntoResponse {
    // Validate credentials...

    // Store user ID in session
    session.insert("user_id", &user.id).await?;

    // Regenerate session ID after login (security best practice)
    session.cycle_id().await?;

    // Add a flash message for the redirect
    FlashMessages::push(&session, FlashMessage::success("Logged in!")).await?;

    Redirect::to("/dashboard")
}

async fn dashboard(flash: FlashMessages) -> impl IntoResponse {
    // Flash messages are automatically consumed when read
    Html(render_page(flash.messages()))
}
```

---

## Feature Flags

| Feature | Description | Storage Backend |
|---------|-------------|-----------------|
| `session` | Base session support (included by storage features) | None |
| `session-memory` | In-memory session store | tower-sessions-memory-store |
| `session-redis` | Redis session store | tower-sessions-redis-store (fred) |

**Development**: Use `session-memory` for fast iteration without external dependencies.

**Production**: Use `session-redis` for distributed, persistent sessions across multiple instances.

---

## Configuration Reference

```toml
[session]
# Cookie Settings
cookie_name = "session_id"       # Name of the session cookie
cookie_path = "/"                # Cookie path
cookie_domain = "example.com"    # Optional: restrict to domain
secure = true                    # Require HTTPS (always true in production)
http_only = true                 # Prevent JavaScript access
same_site = "lax"               # "strict", "lax", or "none"

# Session Lifetime
expiry_secs = 86400             # Session duration (0 = browser session)
inactivity_timeout_secs = 3600  # Optional: expire on inactivity

# Storage
storage = "redis"               # "memory" or "redis"
redis_url = "redis://localhost:6379"  # Required for redis storage

# CSRF Protection
[session.csrf]
enabled = true                  # Enable CSRF validation
token_length = 32               # Token length in bytes
header_name = "X-CSRF-Token"    # Header for CSRF token
form_field_name = "_csrf"       # Form field name for token
```

---

## TypedSession - Type-Safe Sessions

For structured session data, use `TypedSession<T>` for automatic serialization:

```rust
use acton_service::session::TypedSession;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct CartSession {
    items: Vec<String>,
    total: f64,
}

async fn add_to_cart(
    mut session: TypedSession<CartSession>,
    Path(item_id): Path<String>,
) -> impl IntoResponse {
    // Access typed data
    session.data_mut().items.push(item_id);
    session.data_mut().total += 9.99;

    // Save changes
    session.save().await?;

    Ok::<_, Error>("Added to cart")
}

async fn view_cart(session: TypedSession<CartSession>) -> impl IntoResponse {
    let cart = session.data();
    Html(render_cart(&cart.items, cart.total))
}
```

### TypedSession Methods

| Method | Description |
|--------|-------------|
| `data()` | Get read-only reference to session data |
| `data_mut()` | Get mutable reference (call `save()` after) |
| `save()` | Persist changes to session store |
| `clear()` | Reset data to default |
| `destroy()` | Destroy entire session (logout) |
| `regenerate()` | Regenerate session ID (after login) |

---

## AuthSession - Built-in Authentication

For common authentication patterns, use the pre-built `AuthSession`:

```rust
use acton_service::session::{TypedSession, AuthSession};

async fn login(
    mut auth: TypedSession<AuthSession>,
    Form(creds): Form<LoginForm>,
) -> impl IntoResponse {
    // Validate credentials...

    // Login sets user_id, roles, and timestamp
    auth.data_mut().login(
        user.id.clone(),
        vec!["user".to_string(), "admin".to_string()]
    );

    // Save and regenerate session ID
    auth.save().await?;
    auth.regenerate().await?;

    Redirect::to("/dashboard")
}

async fn dashboard(auth: TypedSession<AuthSession>) -> impl IntoResponse {
    if !auth.data().is_authenticated() {
        return Redirect::to("/login").into_response();
    }

    let user_id = auth.data().user_id().unwrap();
    Html(format!("Welcome, {}!", user_id)).into_response()
}

async fn admin_only(auth: TypedSession<AuthSession>) -> impl IntoResponse {
    if !auth.data().has_role("admin") {
        return StatusCode::FORBIDDEN.into_response();
    }

    Html("Admin content").into_response()
}

async fn logout(mut auth: TypedSession<AuthSession>) -> impl IntoResponse {
    auth.data_mut().logout();
    auth.save().await?;
    Redirect::to("/")
}
```

### AuthSession Fields

| Field | Type | Description |
|-------|------|-------------|
| `user_id` | `Option<String>` | Authenticated user ID |
| `roles` | `Vec<String>` | User roles for authorization |
| `authenticated_at` | `Option<i64>` | Login timestamp |
| `extra` | `HashMap<String, String>` | Additional user data |

### AuthSession Methods

| Method | Description |
|--------|-------------|
| `is_authenticated()` | Check if user is logged in |
| `user_id()` | Get user ID if authenticated |
| `login(id, roles)` | Set user ID and roles |
| `logout()` | Clear all auth data |
| `has_role(role)` | Check for specific role |
| `has_any_role(roles)` | Check for any of the roles |
| `has_all_roles(roles)` | Check for all roles |

---

## Flash Messages

Flash messages are one-time messages stored in the session. They're automatically consumed when read, perfect for post-redirect-get patterns.

### Adding Flash Messages

```rust
use acton_service::session::{FlashMessage, FlashMessages};
use tower_sessions::Session;

async fn create_user(
    session: Session,
    Form(data): Form<CreateUser>,
) -> impl IntoResponse {
    // Create user...

    // Success message
    FlashMessages::push(&session, FlashMessage::success("User created!")).await?;

    Redirect::to("/users")
}

async fn delete_user(session: Session) -> impl IntoResponse {
    // Error message
    FlashMessages::push(&session, FlashMessage::error("Failed to delete user")).await?;

    Redirect::to("/users")
}
```

### Reading Flash Messages

```rust
async fn list_users(flash: FlashMessages) -> impl IntoResponse {
    // Messages are automatically removed from session
    let messages = flash.messages();

    Html(render_users_with_flash(messages))
}
```

### Flash Message Types

```rust
FlashMessage::success("Operation completed")
FlashMessage::info("Did you know...")
FlashMessage::warning("This action cannot be undone")
FlashMessage::error("Something went wrong")
```

### Flash Message Helpers

```rust
// Get CSS class for styling
let class = message.kind.css_class();  // "flash-success", "flash-error", etc.

// Get icon name
let icon = message.kind.icon();  // "check-circle", "x-circle", etc.

// Filter by type
let errors = flash.by_kind(FlashKind::Error);

// Check for specific types
if flash.has_errors() {
    // Show error banner
}
```

---

## CSRF Protection

CSRF (Cross-Site Request Forgery) protection is built-in for session-based applications.

### Setup

CSRF is enabled by default when sessions are configured. The middleware validates tokens on non-safe HTTP methods (POST, PUT, DELETE, PATCH).

### Getting the Token

```rust
use acton_service::session::CsrfToken;

async fn form_page(csrf: CsrfToken) -> impl IntoResponse {
    Html(format!(r#"
        <form method="post" action="/submit">
            {}
            <input type="text" name="data">
            <button type="submit">Submit</button>
        </form>
    "#, csrf.as_hidden_field()))
}
```

### HTMX Integration

For HTMX applications, include the CSRF token in the document head and configure HTMX to send it automatically:

```rust
async fn layout(csrf: CsrfToken) -> impl IntoResponse {
    Html(format!(r#"
        <!DOCTYPE html>
        <html>
        <head>
            {}
            <script src="https://unpkg.com/htmx.org"></script>
            <script>
                document.body.addEventListener('htmx:configRequest', (e) => {{
                    e.detail.headers['X-CSRF-Token'] =
                        document.querySelector('meta[name="csrf-token"]').content;
                }});
            </script>
        </head>
        <body>
            <button hx-post="/api/action" hx-swap="outerHTML">
                Click me
            </button>
        </body>
        </html>
    "#, csrf.as_meta_tag()))
}
```

### CsrfToken Methods

| Method | Description |
|--------|-------------|
| `token()` | Get raw token string |
| `as_hidden_field()` | Generate `<input type="hidden" name="_csrf" value="...">` |
| `as_hidden_field_named(name)` | Generate hidden field with custom name |
| `as_meta_tag()` | Generate `<meta name="csrf-token" content="...">` |

### Token Regeneration

Regenerate the CSRF token after login to prevent token fixation:

```rust
use acton_service::session::CsrfToken;

async fn login(session: Session) -> impl IntoResponse {
    // After successful login...
    session.cycle_id().await?;  // Regenerate session ID
    CsrfToken::regenerate(&session, 32).await?;  // Regenerate CSRF token

    Redirect::to("/dashboard")
}
```

---

## Session Security Best Practices

### 1. Regenerate Session ID After Login

Prevent session fixation attacks by regenerating the session ID after authentication:

```rust
session.cycle_id().await?;
```

### 2. Use Secure Cookies in Production

Always set `secure = true` in production to ensure cookies are only sent over HTTPS:

```toml
[session]
secure = true  # Require HTTPS
```

### 3. Set Appropriate SameSite

Use `strict` for maximum security, `lax` for usability with external links:

```toml
[session]
same_site = "strict"  # or "lax" for external links to work
```

### 4. Keep Sessions Short-Lived

Set reasonable expiration times:

```toml
[session]
expiry_secs = 3600  # 1 hour
inactivity_timeout_secs = 900  # 15 minutes of inactivity
```

### 5. Destroy Sessions on Logout

```rust
async fn logout(auth: TypedSession<AuthSession>) -> impl IntoResponse {
    auth.destroy().await?;  // Completely destroy session
    Redirect::to("/")
}
```

---

## Redis Session Storage

For production deployments with multiple instances, use Redis:

### Configuration

```toml
[session]
storage = "redis"
redis_url = "redis://localhost:6379"
# Or with authentication:
# redis_url = "redis://:password@redis-host:6379/0"
```

### Benefits

- **Distributed**: Sessions work across multiple application instances
- **Persistent**: Sessions survive application restarts
- **Scalable**: Redis handles high session volumes efficiently

### Redis URL Format

```text
redis://[username:password@]host:port[/database]

# Examples:
redis://localhost:6379
redis://:mypassword@redis.example.com:6379/0
redis://user:pass@redis-cluster.example.com:6379
```

---

## Session vs JWT

| Aspect | Sessions | JWT |
|--------|----------|-----|
| **State** | Server-side | Stateless |
| **Scalability** | Requires shared store (Redis) | No shared state needed |
| **Revocation** | Immediate | Requires token blacklist |
| **Best for** | HTMX, server-rendered apps | APIs, mobile apps, SPAs |

**Use Sessions when:**
- Building HTMX or server-rendered applications
- Need flash messages for form handling
- Want immediate session invalidation on logout
- Application runs on single server or has Redis

**Use JWT when:**
- Building stateless APIs
- Mobile app backends
- Single Page Applications (SPAs)
- Microservices without shared state

**Both can coexist**: You can enable both sessions (for admin UI) and JWT (for API) in the same application.

---

## Common Patterns

### Protected Routes with Redirect

```rust
async fn protected_page(auth: TypedSession<AuthSession>) -> Response {
    if !auth.data().is_authenticated() {
        return Redirect::to("/login").into_response();
    }

    Html("Protected content").into_response()
}
```

### Role-Based Access

```rust
async fn admin_dashboard(auth: TypedSession<AuthSession>) -> Response {
    let data = auth.data();

    if !data.is_authenticated() {
        return Redirect::to("/login").into_response();
    }

    if !data.has_role("admin") {
        return StatusCode::FORBIDDEN.into_response();
    }

    Html("Admin dashboard").into_response()
}
```

### Shopping Cart

```rust
#[derive(Default, Serialize, Deserialize)]
struct Cart {
    items: Vec<CartItem>,
}

async fn add_item(
    mut cart: TypedSession<Cart>,
    Json(item): Json<CartItem>,
) -> impl IntoResponse {
    cart.data_mut().items.push(item);
    cart.save().await?;
    Json(cart.data().items.len())
}
```

### Remember Me

```rust
async fn login(
    session: Session,
    Form(data): Form<LoginForm>,
) -> impl IntoResponse {
    // Validate credentials...

    if data.remember_me {
        // Set longer expiry (e.g., 30 days)
        // Note: This requires custom session configuration per-request
        // which is not yet supported. Use JWT for remember-me functionality.
    }

    session.insert("user_id", &user.id).await?;
    Redirect::to("/dashboard")
}
```

---

## Troubleshooting

### "Session not found in request extensions"

**Solution**: Ensure `SessionManagerLayer` is applied before your handlers. Check that:
1. Session configuration is present in `config.toml`
2. The correct session feature is enabled (`session-memory` or `session-redis`)

### "No session feature enabled"

**Solution**: Add a session storage feature to your `Cargo.toml`:
```toml
features = ["session-memory"]  # or "session-redis"
```

### CSRF validation failing

**Solution**: Ensure you're including the CSRF token in requests:
1. For forms: Use `csrf.as_hidden_field()`
2. For HTMX/fetch: Include `X-CSRF-Token` header

### Sessions not persisting across requests

**Solution**:
1. Check that cookies are being set (inspect browser dev tools)
2. Ensure `secure = false` for local development (no HTTPS)
3. For Redis: verify connection with `redis-cli ping`

### Flash messages not showing

**Solution**: Flash messages are consumed on read. Ensure you're reading them in the template **before** rendering. Don't read `FlashMessages` twice in the same request.

---

## Need More Help?

- [Feature Flags](/docs/feature-flags) - All available features
- [Configuration](/docs/configuration) - Full configuration reference
- [JWT Authentication](/docs/jwt-auth) - For API authentication
- [Examples](/docs/examples) - Working code examples
