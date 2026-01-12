---
title: Web App Integration
nextjs:
  metadata:
    title: Web App Integration
    description: Build web applications with HTMX, sessions, and server-side rendering using acton-service's built-in features
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

{% callout type="note" title="v0.10.0 Update" %}
acton-service v0.10.0 added first-class HTMX support with the `htmx`, `askama`, and `sse` features. This page has been updated to reflect built-in session and template support. See [HTMX Integration](/docs/htmx) for the complete guide.
{% /callout %}

acton-service supports both stateless API microservices (JWT-based) and traditional web applications (session-based). This guide helps you choose the right approach and shows how to build web applications with HTMX.

---

## Two Architecture Patterns

### Stateless APIs (JWT)

Best for: Mobile backends, SPAs, third-party integrations, microservices

```text
┌──────────────────┐     Authorization: Bearer <token>     ┌──────────────────┐
│   Mobile App     │ ──────────────────────────────────→   │  acton-service   │
│   SPA Client     │                                       │  (stateless)     │
│   External API   │ ←────────────────────────────────── │                  │
└──────────────────┘           JSON Response               └──────────────────┘
```

- JWT tokens in `Authorization` header
- No server-side session state
- Horizontal scaling without session affinity
- Inherent CSRF protection (custom headers trigger preflight)

### Server-Rendered Web Apps (Sessions)

Best for: Admin dashboards, internal tools, HTMX applications

```text
┌──────────────────┐     Cookie: session_id=abc123         ┌──────────────────┐
│   Browser        │ ──────────────────────────────────→   │  acton-service   │
│   (HTMX)         │                                       │  (sessions)      │
│                  │ ←────────────────────────────────── │                  │
└──────────────────┘           HTML Response               └──────────────────┘
```

- Session cookies for authentication
- Flash messages, CSRF protection built-in
- Compile-time checked templates
- Real-time updates via SSE

---

## Building HTMX Applications

### Quick Start

Enable the `htmx-full` feature for complete HTMX support:

```toml
[dependencies]
acton-service = { version = "{{version}}", features = ["htmx-full"] }
```

This includes:
- `htmx` - Request extractors and response helpers
- `askama` - Compile-time checked templates
- `sse` - Server-Sent Events for real-time updates
- `session-memory` - In-memory session storage (dev)

### Template-Based Rendering

Create Askama templates with `TemplateContext` for common page data:

```rust
use acton_service::prelude::*;

#[derive(Template)]
#[template(path = "tasks/list.html")]
struct TaskListTemplate {
    ctx: TemplateContext,
    tasks: Vec<Task>,
}

async fn list_tasks(
    flash: FlashMessages,
    auth: TypedSession<AuthSession>,
) -> impl IntoResponse {
    let ctx = TemplateContext::new()
        .with_path("/tasks")
        .with_auth(auth.data().user_id.clone())
        .with_flash(flash.into_messages());

    HtmlTemplate::page(TaskListTemplate { ctx, tasks })
}
```

### HTMX Request Detection

Return fragments for HTMX requests, full pages for direct navigation:

```rust
async fn list_tasks(
    HxRequest(is_htmx): HxRequest,
    Extension(store): Extension<SharedStore>,
) -> impl IntoResponse {
    let tasks = store.read().await.all();

    if is_htmx {
        HtmlTemplate::fragment(TaskListFragment { tasks })
    } else {
        let ctx = TemplateContext::new().with_path("/tasks");
        HtmlTemplate::page(TaskListPage { ctx, tasks })
    }
}
```

### Flash Messages

Feedback that survives redirects:

```rust
async fn create_task(
    session: Session,
    Form(data): Form<CreateTaskForm>,
) -> impl IntoResponse {
    // Create task...

    FlashMessages::push(&session, FlashMessage::success("Task created!")).await.ok();

    axum::response::Redirect::to("/tasks")
}
```

Display in templates:

```html
{​% if ctx.has_flash() %}
    {​% for flash in ctx.flash_messages %}
        <div class="flash {{ flash.kind.css_class() }}">
            {{ flash.message }}
        </div>
    {​% endfor %}
{​% endif %}
```

### Real-Time Updates with SSE

Broadcast events to all connected clients:

```rust
async fn create_task(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
    Extension(store): Extension<SharedStore>,
    Form(form): Form<CreateTaskForm>,
) -> impl IntoResponse {
    let task = store.write().await.add(form.title);

    // Broadcast to all SSE subscribers
    let html = TaskItemTemplate { task: &task }.render().unwrap();
    broadcaster.broadcast(BroadcastMessage::named("task-created", html)).ok();

    HxRedirect::to("/tasks")
}
```

Client connection:

```html
<div hx-ext="sse" sse-connect="/events">
    <ul id="task-list" sse-swap="task-created" hx-swap="beforeend">
        {​% for task in tasks %}
            {​% include "tasks/item.html" %}
        {​% endfor %}
    </ul>
</div>
```

---

## Session vs JWT: When to Use Which

| Scenario | Recommendation | Why |
|----------|---------------|-----|
| Admin dashboard | Sessions + HTMX | Server-rendered, immediate logout, flash messages |
| Public website | Sessions + HTMX | SEO-friendly, progressive enhancement |
| Mobile app backend | JWT | Stateless, works offline, no cookies |
| SPA frontend | JWT | JavaScript manages tokens |
| Third-party API | JWT or API keys | No browser session |
| Microservice calls | JWT | Service-to-service auth |

### Hybrid Architecture

You can use both patterns in the same application by organizing routes within versions:

```rust
let routes = VersionedApiBuilder::new()
    .with_base_path("/api")
    .add_version(ApiVersion::V1, |router| {
        router
            // HTMX routes with sessions
            .route("/", get(home))
            .route("/tasks", get(list_tasks).post(create_task))
            .layer(session_layer)
            // API routes with JWT
            .route("/api/tasks", get(api_list_tasks).post(api_create_task))
            .layer(jwt_layer)
    })
    .build_routes();

ServiceBuilder::new()
    .with_routes(routes)
    .build()
    .serve()
    .await?;
```

This pattern works well for applications that need both a web UI (admin dashboard) and an API (mobile app, integrations).

---

## Production Configuration

### Session Storage

Use Redis for multi-instance deployments:

```toml
acton-service = { version = "{{version}}", features = [
    "htmx", "askama", "sse", "session-redis"
] }
```

```toml
# config.toml
[session]
storage = "redis"
redis_url = "redis://localhost:6379"
secure = true        # Require HTTPS
http_only = true     # No JavaScript access
same_site = "lax"    # CSRF protection
```

### CSRF Protection

CSRF is enabled automatically with sessions. Include the token in HTMX requests:

```html
<!-- In <head> -->
{{ ctx.csrf_meta()|safe }}

<script>
document.body.addEventListener('htmx:configRequest', (e) => {
    e.detail.headers['X-CSRF-Token'] =
        document.querySelector('meta[name="csrf-token"]').content;
});
</script>
```

### Security Checklist

- [ ] Use `session-redis` in production (not `session-memory`)
- [ ] Set `secure = true` for HTTPS-only cookies
- [ ] Set `http_only = true` to prevent JavaScript access
- [ ] Use `same_site = "strict"` or `"lax"` for CSRF protection
- [ ] Regenerate session ID after login (`session.cycle_id()`)
- [ ] Set reasonable session expiry times

---

## Complete Example

The Task Manager example demonstrates all web application features:

```bash
cargo run --manifest-path=acton-service/Cargo.toml --example task-manager --features htmx-full
```

Features demonstrated:
- Session-based authentication
- Flash messages
- Askama templates with TemplateContext
- Out-of-band swaps
- Server-Sent Events
- CSRF protection

See {% link href=githubUrl("/tree/main/acton-service/examples/htmx") %}examples/htmx/{% /link %} for the complete source.

---

## Next Steps

- [HTMX Integration](/docs/htmx) - Complete HTMX feature overview
- [Askama Templates](/docs/askama) - Template syntax and patterns
- [Server-Sent Events](/docs/sse) - Real-time updates
- [Session Management](/docs/session) - Session configuration and API
- [Token Authentication](/docs/token-auth) - JWT/PASETO for APIs
