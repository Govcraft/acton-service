---
title: HTMX Integration
nextjs:
  metadata:
    title: HTMX Integration
    description: Build hypermedia-driven applications with server-side rendering, session management, and real-time updates
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

**HTMX simplifies web architecture by eliminating the API layer**—your server returns HTML directly, not JSON that JavaScript must render. The browser swaps page fragments in place without full reloads, giving you interactive UIs without complex frontend frameworks.

acton-service provides first-class HTMX support through three integrated features:
- **`htmx`** - Type-safe request extractors (detect HTMX requests) and response helpers (redirect, refresh, trigger events)
- **`askama`** - Compile-time checked templates with automatic flash message and auth state aggregation
- **`sse`** - Server-Sent Events for pushing real-time updates to all connected browsers

Together with session management, you can build complete interactive applications—authentication, flash messages, live updates—without writing custom JavaScript.

## Quick Decision Guide

```text
What do you want to build?
├─ Server-rendered pages with templates     → Enable askama
├─ Interactive UI with HTMX attributes      → Enable htmx + askama
├─ Real-time updates (live data, notifications) → Enable sse
└─ Complete web application                 → Enable htmx-full (includes all)
```

The `htmx-full` convenience feature includes all HTMX-related features plus `session-memory` for development:

```toml
[dependencies]
acton-service = { version = "{{version}}", features = ["htmx-full"] }
```

See [Feature Flags](/docs/feature-flags#htmx-features) for detailed descriptions of each feature.

## Feature Overview

### `askama` - Type-Safe Templates

Askama provides Jinja2-like templates with compile-time checking. Template errors become compile errors—no more runtime "variable not found" failures in production.

```rust
use acton_service::prelude::*;

#[derive(Template)]
#[template(path = "tasks/list.html")]
struct TaskListTemplate {
    ctx: TemplateContext,
    tasks: Vec<Task>,
}

async fn list_tasks(flash: FlashMessages) -> impl IntoResponse {
    let ctx = TemplateContext::new()
        .with_path("/tasks")
        .with_flash(flash.into_messages());

    HtmlTemplate::page(TaskListTemplate { ctx, tasks })
}
```

`TemplateContext` aggregates common page data—authentication status, flash messages, CSRF tokens, and current path—so every template has consistent access to session state.

See [Askama Templates](/docs/askama) for the complete guide.

---

### `htmx` - HTMX Utilities

The `htmx` feature provides extractors for HTMX request headers and responders for HTMX-specific response patterns.

**Extractors** detect HTMX requests and access header values:

```rust
use acton_service::prelude::*;

async fn list_tasks(
    HxRequest(is_htmx): HxRequest,
    HxTarget(target): HxTarget,
) -> impl IntoResponse {
    if is_htmx {
        // Return just the fragment
        HtmlTemplate::fragment(TaskListFragment { tasks })
    } else {
        // Return full page
        HtmlTemplate::page(TaskListPage { tasks })
    }
}
```

**Responders** set HTMX response headers:

```rust
use acton_service::prelude::*;

async fn create_task(Form(data): Form<NewTask>) -> impl IntoResponse {
    // Create task...

    // Redirect via HTMX (replaces only hx-target, not full page)
    HxRedirect::to("/tasks")
}
```

**Out-of-band swaps** update multiple elements from a single response:

```rust
// Return new task HTML + update stats counter
let task_html = TaskItemTemplate { task }.render().unwrap();
let stats_oob = format!(
    r#"<span id="task-count" hx-swap-oob="outerHTML">{}</span>"#,
    total_count
);
Html(format!("{}{}", task_html, stats_oob))
```

---

### `sse` - Server-Sent Events

SSE provides real-time server-to-client updates. Unlike WebSockets, SSE is one-way (server to client only) and works over standard HTTP with automatic reconnection.

```rust
use acton_service::prelude::*;

async fn events(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = broadcaster.subscribe();

    let stream = stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(msg) => Some((Ok(msg.into_event()), rx)),
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

On the client, HTMX's SSE extension connects to your endpoint and automatically swaps content when events arrive:

```html
<div hx-ext="sse" sse-connect="/events">
    <div id="notifications" sse-swap="notification">
        <!-- New notifications appear here -->
    </div>
</div>
```

See [Server-Sent Events](/docs/sse) for broadcasting patterns and connection management.

---

### Frontend Routes with ServiceBuilder

{% callout type="note" title="New in 0.11.0" %}
The `htmx` feature now enables `with_frontend_routes()` on `VersionedApiBuilder`, allowing unversioned frontend routes alongside versioned API routes—all while using ServiceBuilder's batteries-included backend.
{% /callout %}

HTMX applications typically need unversioned routes (`/`, `/login`, `/tasks`) rather than API-style versioned routes (`/api/v1/users`). The `with_frontend_routes()` method lets you define these while still getting ServiceBuilder's automatic features:

```rust
use acton_service::prelude::*;
use acton_service::versioning::VersionedApiBuilder;

#[tokio::main]
async fn main() -> Result<()> {
    let routes = VersionedApiBuilder::new()
        // Optional: Add versioned API routes
        .with_base_path("/api")
        .add_version(ApiVersion::V1, |router| {
            router.route("/data", get(api_handler))
        })
        // Frontend routes (htmx feature required)
        .with_frontend_routes(|router| {
            router
                .route("/", get(index))
                .route("/login", get(login_page).post(login))
                .route("/tasks", post(create_task))
                .layer(session_layer)
        })
        .build_routes();

    // ServiceBuilder provides automatic:
    // - /health and /ready endpoints
    // - Tracing initialization
    // - Configuration loading
    // - Graceful shutdown
    ServiceBuilder::new()
        .with_routes(routes)
        .build()
        .serve()
        .await?;

    Ok(())
}
```

**Resulting routes:**
```text
GET  /health          # Auto-provided health check
GET  /ready           # Auto-provided readiness probe
GET  /                # Frontend index (unversioned)
GET  /login           # Frontend login (unversioned)
POST /login           # Frontend login handler (unversioned)
POST /tasks           # Frontend task creation (unversioned)
GET  /api/v1/data     # Versioned API endpoint
```

This pattern gives you the best of both worlds: clean frontend URLs for your HTMX UI, optional versioned API routes for programmatic access, and all of ServiceBuilder's production-ready features.

---

## Complete Example: Task Manager

The Task Manager example demonstrates all HTMX features working together—templates, flash messages, out-of-band swaps, real-time updates, and proper ServiceBuilder integration.

```bash
cargo run --manifest-path=acton-service/Cargo.toml --example task-manager --features htmx-full
```

Open http://localhost:3000 to see:

- **ServiceBuilder integration** with automatic health/ready endpoints and tracing
- **Session-based authentication** with login/logout
- **Flash messages** that survive redirects
- **Out-of-band swaps** updating task list and statistics simultaneously
- **Inline editing** with HTMX form handling
- **Real-time updates** via SSE when tasks change

Key patterns from the example:

**TemplateContext with flash messages:**
```rust
let ctx = TemplateContext::new()
    .with_path("/")
    .with_auth(auth.data().user_id.clone())
    .with_flash(flash.into_messages());
```

**Out-of-band statistics update:**
```rust
fn render_stats_oob(total: usize, completed: usize, pending: usize) -> String {
    format!(
        r#"<span class="stat-value" id="total-count" hx-swap-oob="outerHTML">{}</span>
<span class="stat-value" id="pending-count" hx-swap-oob="outerHTML">{}</span>
<span class="stat-value" id="completed-count" hx-swap-oob="outerHTML">{}</span>"#,
        total, pending, completed
    )
}
```

**ServiceBuilder with frontend routes:**
```rust
let routes = VersionedApiBuilder::new()
    .with_frontend_routes(|router| {
        router
            .route("/", get(index))
            .route("/login", get(login_page).post(login))
            .route("/tasks", post(create_task))
            .route("/events", get(events))
            .layer(Extension(store))
            .layer(session_layer)
    })
    .build_routes();

ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await?;
```

Explore the {% link href=githubUrl("/tree/main/acton-service/examples/htmx") %}complete source{% /link %} for implementation details.

---

## Getting Started

### 1. Add Feature Flags

For development, use `htmx-full` which includes everything:

```toml
[dependencies]
acton-service = { version = "{{version}}", features = ["htmx-full"] }
tokio = { version = "1", features = ["full"] }
```

For production with Redis sessions:

```toml
acton-service = { version = "{{version}}", features = [
    "htmx", "askama", "sse", "session-redis"
] }
```

### 2. Run the Example

```bash
cargo run --manifest-path=acton-service/Cargo.toml --example task-manager --features htmx-full
```

### 3. Read the Detailed Guides

- [Askama Templates](/docs/askama) - Template syntax, TemplateContext, flash messages
- [Server-Sent Events](/docs/sse) - Real-time updates with SseBroadcaster
- [Session Management](/docs/session) - Authentication, CSRF protection, flash storage

---

## Integration with Other Features

HTMX features work alongside acton-service's other capabilities:

| Feature | Integration |
|---------|-------------|
| **Session** | `session-memory` for dev, `session-redis` for production. Provides flash messages, CSRF tokens, and auth state. |
| **Auth** | Use `auth` feature for password hashing. Sessions store authentication state. |
| **Database** | `database` (PostgreSQL) or `turso` (SQLite) for persistent storage. |
| **Observability** | Full tracing support. Template rendering and HTMX requests are traced automatically. |

**Hybrid architectures** are supported: use sessions + HTMX for your admin UI while exposing a JWT-authenticated API for mobile clients or third-party integrations.

---

## When to Use HTMX vs. REST APIs

| Scenario | Recommendation |
|----------|---------------|
| Admin dashboards, internal tools | HTMX with sessions |
| Public-facing web apps | HTMX with sessions |
| Mobile app backend | REST API with JWT |
| Third-party integrations | REST API with JWT or API keys |
| Microservice communication | gRPC or REST with JWT |
| Hybrid (web + mobile) | Both—HTMX for web, API for mobile |

HTMX excels when the browser is your only client. If you need a JSON API anyway, consider whether HTMX adds value or just complexity.

---

## Next Steps

- [Askama Templates](/docs/askama) - Complete template guide with syntax reference
- [Server-Sent Events](/docs/sse) - Real-time updates and broadcasting patterns
- [Session Management](/docs/session) - Authentication, flash messages, CSRF
- [Feature Flags](/docs/feature-flags#htmx-features) - All HTMX-related feature options
- [Examples](/docs/examples#htmx) - Task Manager and other HTMX examples
