---
title: Web App Integration
nextjs:
  metadata:
    title: Web App Integration
    description: Integrate acton-service with traditional web applications, HTMX frontends, and session-based authentication patterns.
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

acton-service is designed for stateless microservices with JWT-based authentication. This guide explains how to integrate with traditional web applications that require session cookies, including HTMX frontends.

## Design Philosophy

acton-service focuses on **stateless API microservices**:

- JWT bearer tokens in `Authorization` headers
- No server-side session state
- Horizontal scaling without session affinity
- Type-enforced API versioning

This design provides inherent CSRF protection—browsers don't automatically attach `Authorization` headers to cross-origin requests, and JSON APIs with custom headers trigger CORS preflight checks.

**Session cookies are not included** because they would conflict with these design goals:

| Concern | JWT (acton-service) | Session Cookies |
|---------|---------------------|-----------------|
| State | Stateless | Server-side state |
| Scaling | No session affinity | Requires sticky sessions or shared store |
| CSRF | Inherently protected | Requires CSRF tokens |
| Storage | Client holds token | Server manages sessions |

---

## HTMX and Traditional Web Apps

[HTMX](https://htmx.org) is a library for building dynamic web interfaces using HTML over the wire. While HTMX *can* use JWT authentication, session cookies are typically the better choice for several reasons:

### Why Cookies Work Better for HTMX

**Automatic transmission**: Cookies are sent with every request without JavaScript configuration.

**Initial page load**: The first HTML page load happens before any JavaScript runs—you need cookies to authenticate this request.

**XSS protection**: HttpOnly cookies can't be accessed by JavaScript, protecting against token theft.

**HTMX philosophy**: HTMX minimizes JavaScript. Adding JWT handling adds complexity that conflicts with this goal.

### JWT with HTMX (Possible but Complex)

HTMX supports custom headers via `hx-headers`:

```html
<!-- Per-element -->
<button hx-post="/api/action" hx-headers='{"Authorization": "Bearer ..."}'>
  Submit
</button>

<!-- Global configuration -->
<script>
document.body.addEventListener('htmx:configRequest', (e) => {
  const token = localStorage.getItem('access_token');
  if (token) {
    e.detail.headers['Authorization'] = 'Bearer ' + token;
  }
});
</script>
```

**Drawbacks:**
- Requires JavaScript boilerplate (contradicts HTMX philosophy)
- Token stored in localStorage is vulnerable to XSS
- Initial page load can't be authenticated
- Token refresh requires additional JavaScript logic

---

## Recommended Architecture

For web applications with HTMX frontends, use a **gateway pattern** that separates concerns:

```
┌─────────────────────────────────┐
│  Browser (HTMX Frontend)        │
│  - Receives HTML fragments      │
│  - Sends session cookie         │
└───────────────┬─────────────────┘
                │ Session Cookie
                ▼
┌─────────────────────────────────┐
│  Web Gateway (Axum + Sessions)  │
│  - Cookie-based authentication  │
│  - CSRF protection              │
│  - HTML rendering               │
│  - Issues JWT for backend calls │
└───────────────┬─────────────────┘
                │ JWT Bearer Token
                ▼
┌─────────────────────────────────┐
│  API Backend (acton-service)    │
│  - Stateless microservices      │
│  - JWT validation               │
│  - Business logic               │
└─────────────────────────────────┘
```

### Benefits of This Pattern

**Clean separation**: The gateway handles browser-specific concerns (cookies, CSRF, HTML), while acton-service stays focused on stateless APIs.

**Security**: Sensitive operations happen in the stateless backend where JWT provides strong authentication guarantees.

**Flexibility**: The same backend serves both web frontends and mobile/CLI clients without modification.

**Scaling**: The stateless backend scales horizontally without session coordination.

### Example Gateway Service

Use Axum with `tower-sessions` for the web gateway:

```rust
use axum::{Router, routing::get, response::Html};
use tower_sessions::{SessionManagerLayer, MemoryStore};

// Gateway handles sessions and renders HTML
async fn dashboard(session: Session) -> Html<String> {
    let user_id: Option<String> = session.get("user_id").await.unwrap();

    match user_id {
        Some(id) => {
            // Call acton-service backend with JWT
            let jwt = generate_jwt_for_user(&id);
            let data = call_backend_api(&jwt).await;
            Html(render_dashboard(data))
        }
        None => Html(render_login_page())
    }
}

#[tokio::main]
async fn main() {
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(true)
        .with_http_only(true)
        .with_same_site(SameSite::Strict);

    let app = Router::new()
        .route("/dashboard", get(dashboard))
        .layer(session_layer);

    // Gateway runs on port 3000, acton-service on 8080
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

---

## Direct Integration with tower-sessions

If you need sessions directly in acton-service (not recommended for most cases), you can integrate `tower-sessions` as custom middleware.

{% callout type="warning" title="Consider the Gateway Pattern First" %}
Direct session integration adds complexity and moves away from acton-service's stateless design. The gateway pattern above is preferred for most web applications.
{% /callout %}

### Adding tower-sessions

```bash
cargo add tower-sessions
cargo add tower-sessions-redis-store  # For production
```

### Configuration

```rust
use acton_service::prelude::*;
use tower_sessions::{SessionManagerLayer, RedisStore};
use tower_sessions::cookie::SameSite;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create Redis-backed session store
    let redis_client = redis::Client::open("redis://localhost:6379")?;
    let session_store = RedisStore::new(redis_client);

    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(true)        // HTTPS only
        .with_http_only(true)     // No JavaScript access
        .with_same_site(SameSite::Strict)  // CSRF protection
        .with_expiry(Expiry::OnInactivity(Duration::hours(24)));

    let routes = VersionedApiBuilder::new()
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router
                .route("/login", post(login))
                .route("/profile", get(profile))
        })
        .build_routes();

    ServiceBuilder::new()
        .with_routes(routes)
        .with_middleware(|router| router.layer(session_layer))
        .build()
        .serve()
        .await?;

    Ok(())
}
```

### CSRF Protection

When using session cookies, you **must** implement CSRF protection. Use `tower-csrf` or implement token validation manually:

```rust
use tower_csrf::{CsrfLayer, CsrfToken};

// Add CSRF layer after session layer
.with_middleware(|router| {
    router
        .layer(CsrfLayer::new())
        .layer(session_layer)
})

// In your HTML forms, include the CSRF token
async fn render_form(csrf: CsrfToken) -> Html<String> {
    Html(format!(r#"
        <form hx-post="/api/v1/submit">
            <input type="hidden" name="_csrf" value="{}">
            <button type="submit">Submit</button>
        </form>
    "#, csrf.token()))
}
```

---

## Why Not Built-in Sessions?

acton-service intentionally excludes session management:

**Architectural purity**: Sessions introduce server-side state, conflicting with stateless microservice design.

**Scope clarity**: The framework focuses on API microservices, not full-stack web applications.

**Ecosystem integration**: Rust has excellent session libraries (`tower-sessions`, `axum-sessions`) that integrate via Tower middleware.

**Security responsibility**: Session security requires careful configuration (SameSite, HttpOnly, Secure flags, CSRF). Making this opt-in ensures developers consciously choose and configure it.

---

## Security Considerations

### When Using Session Cookies

| Protection | Implementation |
|------------|----------------|
| **CSRF** | `SameSite=Strict` or CSRF tokens |
| **XSS** | `HttpOnly` flag prevents JavaScript access |
| **MITM** | `Secure` flag ensures HTTPS only |
| **Session fixation** | Regenerate session ID on login |
| **Session hijacking** | Short expiry, IP binding (optional) |

### When Using JWT

| Protection | Implementation |
|------------|----------------|
| **CSRF** | Inherently protected (not auto-sent) |
| **XSS** | Store in memory, not localStorage |
| **Token theft** | Short expiry + refresh tokens |
| **Replay** | Token revocation via Redis |

---

## Decision Guide

Use this guide to choose the right authentication pattern:

### Use JWT (acton-service default)

- Building REST/gRPC APIs consumed by mobile apps, CLI tools, or SPAs
- Microservice-to-microservice communication
- Need stateless horizontal scaling
- Clients can manage token refresh

### Use Gateway Pattern (recommended for web apps)

- Building HTMX or traditional server-rendered web apps
- Need session-based authentication for browsers
- Want to use acton-service for backend APIs
- Have both web and non-web clients

### Use Direct Sessions (advanced)

- Single monolithic application
- No separation between frontend and API
- Willing to manage session state and CSRF protection
- Understand the tradeoffs

---

## Next Steps

- [JWT Authentication](/docs/jwt-auth) - Configure token-based authentication
- [Middleware Overview](/docs/middleware) - Understand the middleware stack
- [Rate Limiting](/docs/rate-limiting) - Protect session endpoints from abuse
- [Production Checklist](/docs/production) - Security considerations for deployment
