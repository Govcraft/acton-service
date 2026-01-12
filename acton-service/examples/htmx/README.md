# HTMX Task Manager Example

A comprehensive example demonstrating how to build HTMX-powered applications with acton-service.

## What You'll Learn

- **Askama Templates** - Server-side rendering with compile-time checked templates
- **Flash Messages** - User feedback that persists across redirects
- **Session Authentication** - Login/logout with `AuthSession`
- **Out-of-Band Swaps** - Update multiple page elements from a single response
- **SSE Infrastructure** - Server-Sent Events setup for real-time updates

## Quick Start

```bash
# Run the example
cargo run --manifest-path=acton-service/Cargo.toml \
  --example task-manager --features htmx-full

# Open in browser
open http://localhost:8080
```

## Features Demonstrated

### 1. Askama Templates with TemplateContext

Templates use `TemplateContext` to pass common data like authentication status and flash messages:

```rust
let ctx = TemplateContext::new()
    .with_path("/")
    .with_auth(auth.data().user_id.clone())
    .with_flash(flash.into_messages());

HtmlTemplate::page(IndexTemplate { ctx, tasks, ... })
```

### 2. Flash Messages

Success/error feedback that survives redirects:

```rust
// Push a flash message before redirect
FlashMessages::push(
    auth.session(),
    FlashMessage::success("Welcome back!"),
).await;
```

Templates automatically display flash messages from `TemplateContext`:

```html
{% for flash in ctx.flash_messages %}
<div class="flash {{ flash.kind.css_class() }}">
    {{ flash.message }}
</div>
{% endfor %}
```

### 3. Out-of-Band Updates

Update multiple page elements from a single response using OOB swaps:

```rust
fn render_stats_oob(total: usize, completed: usize, pending: usize) -> String {
    format!(
        r#"<span class="stat-value" id="total-count" hx-swap-oob="outerHTML">{}</span>
<span class="stat-value" id="pending-count" hx-swap-oob="outerHTML">{}</span>
<span class="stat-value" id="completed-count" hx-swap-oob="outerHTML">{}</span>"#,
        total, pending, completed
    )
}

// Return task HTML + OOB stat updates
Html(format!("{}{}", task_html, stats_html)).into_response()
```

### 4. Session Authentication

Simple login/logout with `TypedSession<AuthSession>`:

```rust
// Login - stores user info in session
auth.data_mut().login(username.to_string(), vec!["user".to_string()]);
auth.save().await?;

// Check auth status in templates
if ctx.is_authenticated {
    // Show user menu
}

// Logout - clears session data
auth.data_mut().logout();
auth.save().await?;
```

### 5. SSE Infrastructure

The example includes SSE setup for real-time updates:

```rust
// Broadcaster for SSE messages
let broadcaster = Arc::new(SseBroadcaster::new());

// SSE endpoint
async fn events(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = broadcaster.subscribe();
    // ... stream setup
}
```

```html
<!-- Client connects to SSE -->
<div hx-ext="sse" sse-connect="/events">
    <!-- Content can be updated via SSE -->
</div>
```

## Project Structure

```
examples/htmx/
├── task-manager.rs          # Main application
├── README.md                # This file
└── templates/
    ├── base.html            # Layout with nav, flash messages
    ├── index.html           # Task list page
    ├── tasks/
    │   ├── item.html        # Single task item
    │   └── edit.html        # Inline edit form
    └── auth/
        └── login.html       # Login page
```

## Testing

### Browser Testing

1. Open http://localhost:8080
2. Add tasks using the form
3. Click checkboxes to complete tasks
4. Click "Edit" for inline editing
5. Click "Delete" to remove tasks
6. Login/logout to see flash messages

### curl Testing

```bash
# Get home page
curl http://localhost:8080

# Create task
curl -X POST http://localhost:8080/tasks \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "title=Test+Task"

# Toggle completion
curl -X POST http://localhost:8080/tasks/1/toggle

# Delete task
curl -X DELETE http://localhost:8080/tasks/1
```

## Key Patterns

### Askama Template Inheritance

Templates use Askama's inheritance:

```html
{% extends "base.html" %}
{% block content %}
<!-- Page content -->
{% endblock %}
```

### HTMX Attributes

Common patterns used:

- `hx-post`, `hx-put`, `hx-delete` - HTTP methods
- `hx-target` - Where to swap response
- `hx-swap` - How to swap (innerHTML, outerHTML, afterbegin, etc.)
- `hx-swap-oob` - Out-of-band swaps for updating multiple elements
- `hx-confirm` - Confirmation dialog
- `hx-on::after-request` - Post-request actions

### RwLock Deadlock Prevention

When using `RwLock` with async code, avoid holding locks across await points:

```rust
// Good: Release write lock before reading
let result = { store.write().await.toggle(id) };
let stats = store.read().await.stats();

// Bad: Deadlock - read inside match holding write lock
match store.write().await.toggle(id) {
    Some(task) => {
        let stats = store.read().await.stats(); // Deadlock!
    }
}
```

## Next Steps

- [HTMX Documentation](https://htmx.org/docs/)
- [Askama Templates](https://djc.github.io/askama/)
