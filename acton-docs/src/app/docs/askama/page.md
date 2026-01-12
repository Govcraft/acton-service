---
title: Askama Templates
nextjs:
  metadata:
    title: Askama Templates
    description: Type-safe HTML templates with compile-time checking, inheritance, and HTMX integration
---

{% callout type="note" title="New to acton-service?" %}
Start with the [homepage](/) to understand what acton-service is, then explore [Core Concepts](/docs/concepts) for foundational explanations. See the [Glossary](/docs/glossary) for technical term definitions.
{% /callout %}

---

The `askama` feature provides compile-time checked HTML templates using [Askama](https://djc.github.io/askama/), a Jinja2-like template engine for Rust. Template errors become compile errors—no more runtime failures from typos or missing variables.

acton-service extends Askama with `TemplateContext` for common page data and `HtmlTemplate` for HTMX-aware response handling.

## Quick Start

### 1. Enable the Feature

```toml
[dependencies]
acton-service = { version = "{{version}}", features = ["askama", "session-memory"] }
```

Or use `htmx-full` for the complete HTMX stack:

```toml
acton-service = { version = "{{version}}", features = ["htmx-full"] }
```

### 2. Create a Template

Create `templates/index.html`:

```html
<!DOCTYPE html>
<html>
<head>
    <title>{{ title }}</title>
</head>
<body>
    <h1>Hello, {{ name }}!</h1>

    {​% if ctx.is_authenticated %}
        <p>Welcome back, {{ ctx.user_id.as_ref().unwrap() }}!</p>
    {​% else %}
        <p><a href="/login">Log in</a></p>
    {​% endif %}
</body>
</html>
```

### 3. Define the Template Struct

```rust
use acton_service::prelude::*;

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    ctx: TemplateContext,
    title: String,
    name: String,
}
```

### 4. Render in a Handler

```rust
async fn index(flash: FlashMessages, auth: TypedSession<AuthSession>) -> impl IntoResponse {
    let ctx = TemplateContext::new()
        .with_path("/")
        .with_auth(auth.data().user_id.clone())
        .with_flash(flash.into_messages());

    HtmlTemplate::page(IndexTemplate {
        ctx,
        title: "Home".to_string(),
        name: "World".to_string(),
    })
}
```

---

## Core Concepts

### TemplateContext

`TemplateContext` aggregates common data needed by most templates: authentication status, flash messages, CSRF tokens, and the current request path.

```rust
#[derive(Debug, Clone, Default)]
pub struct TemplateContext {
    pub flash_messages: Vec<FlashMessage>,  // requires session feature
    pub csrf_token: Option<String>,
    pub current_path: String,
    pub is_authenticated: bool,
    pub user_id: Option<String>,
    pub meta: HashMap<String, String>,
}
```

Build a context using the fluent builder pattern:

```rust
let ctx = TemplateContext::new()
    .with_path("/tasks")
    .with_auth(Some("user123".to_string()))
    .with_csrf("abc123def456".to_string())
    .with_flash(flash_messages)
    .with_meta("page_title", "Task List");
```

#### Builder Methods

| Method | Description |
|--------|-------------|
| `new()` | Create empty context |
| `with_path(path)` | Set current request path |
| `with_auth(user_id)` | Set authentication state |
| `with_csrf(token)` | Set CSRF token |
| `with_flash(messages)` | Add flash messages |
| `with_meta(key, value)` | Add custom metadata |

---

### HtmlTemplate

`HtmlTemplate` wraps your Askama template and converts it to an HTTP response with proper headers and status codes.

```rust
// Full page response (200 OK, text/html)
HtmlTemplate::page(MyTemplate { ctx, data })

// HTMX fragment response (200 OK, text/html)
HtmlTemplate::fragment(MyFragment { item })

// Custom status code
HtmlTemplate::new(ErrorTemplate { message })
    .with_status(StatusCode::BAD_REQUEST)

// With HTMX response headers
HtmlTemplate::fragment(ItemTemplate { item })
    .with_hx_trigger("itemCreated")
    .with_hx_push_url("/items/123")
```

#### Methods

| Method | Description |
|--------|-------------|
| `new(template)` | Create response with default settings |
| `page(template)` | Full page response |
| `fragment(template)` | HTMX fragment response |
| `with_status(code)` | Set HTTP status code |
| `with_header(name, value)` | Add custom header |
| `with_hx_trigger(event)` | Set HX-Trigger header |
| `with_hx_redirect(url)` | Set HX-Redirect header |
| `with_hx_refresh()` | Set HX-Refresh header |
| `with_hx_push_url(url)` | Set HX-Push-Url header |
| `with_hx_replace_url(url)` | Set HX-Replace-Url header |
| `with_hx_retarget(selector)` | Set HX-Retarget header |
| `with_hx_reswap(swap)` | Set HX-Reswap header |

---

### Template Helper Methods

`TemplateContext` provides helper methods you can call directly in templates.

#### Flash Message Helpers

```html
{​% if ctx.has_flash() %}
    <div class="flash-messages">
        {​% for flash in ctx.flash_messages %}
            <div class="flash {{ flash.kind.css_class() }}">
                {{ flash.message }}
            </div>
        {​% endfor %}
    </div>
{​% endif %}
```

| Method | Description |
|--------|-------------|
| `has_flash()` | Returns true if any flash messages exist |
| `success_messages()` | Filter to success messages only |
| `error_messages()` | Filter to error messages only |
| `warning_messages()` | Filter to warning messages only |
| `info_messages()` | Filter to info messages only |

#### CSRF Helpers

```html
<!-- In <head> for HTMX to pick up -->
{{ ctx.csrf_meta()|safe }}

<!-- In forms -->
<form method="post" action="/tasks">
    {{ ctx.csrf_field()|safe }}
    <!-- other fields -->
</form>
```

| Method | Description |
|--------|-------------|
| `csrf_field()` | Returns `<input type="hidden" name="_csrf" value="...">` |
| `csrf_meta()` | Returns `<meta name="csrf-token" content="...">` |

#### Navigation Helpers

```html
<nav>
    <a href="/" class="{​% if ctx.is_active('/') %}active{​% endif %}">Home</a>
    <a href="/tasks" class="{​% if ctx.is_active_prefix('/tasks') %}active{​% endif %}">Tasks</a>
</nav>
```

| Method | Description |
|--------|-------------|
| `is_active(path)` | True if current path equals given path |
| `is_active_prefix(prefix)` | True if current path starts with prefix |

---

## Template Syntax

Askama uses Jinja2-like syntax. Here's a quick reference.

### Variables and Expressions

```html
<!-- Simple variable -->
<p>{{ message }}</p>

<!-- Struct field -->
<p>{{ user.name }}</p>

<!-- Method call -->
<p>{{ items.len() }}</p>

<!-- Option handling -->
<p>{{ user_id.as_ref().unwrap_or(&"Guest".to_string()) }}</p>

<!-- Arithmetic -->
<p>Page {{ page + 1 }} of {{ total_pages }}</p>
```

### Control Flow

```html
<!-- If/else -->
{​% if items.is_empty() %}
    <p>No items found.</p>
{​% else %}
    <ul>
        {​% for item in items %}
            <li>{{ item.name }}</li>
        {​% endfor %}
    </ul>
{​% endif %}

<!-- Match -->
{​% match status %}
    {​% when Status::Active %}
        <span class="badge-active">Active</span>
    {​% when Status::Pending %}
        <span class="badge-pending">Pending</span>
    {​% when _ %}
        <span class="badge-unknown">Unknown</span>
{​% endmatch %}

<!-- Loop with index -->
{​% for item in items %}
    <tr class="{​% if loop.index % 2 == 0 %}even{​% else %}odd{​% endif %}">
        <td>{{ loop.index }}</td>
        <td>{{ item.name }}</td>
    </tr>
{​% endfor %}
```

### Template Inheritance

**Base template** (`templates/base.html`):

```html
<!DOCTYPE html>
<html>
<head>
    <title>{​% block title %}My App{​% endblock %}</title>
    {{ ctx.csrf_meta()|safe }}
    <script src="https://unpkg.com/htmx.org@2.0.4"></script>
    {​% block head %}{​% endblock %}
</head>
<body>
    {​% include "partials/nav.html" %}

    {​% if ctx.has_flash() %}
        {​% for flash in ctx.flash_messages %}
            <div class="flash {{ flash.kind.css_class() }}">
                {{ flash.message }}
            </div>
        {​% endfor %}
    {​% endif %}

    <main>
        {​% block content %}{​% endblock %}
    </main>
</body>
</html>
```

**Child template** (`templates/tasks/list.html`):

```html
{​% extends "base.html" %}

{​% block title %}Tasks - My App{​% endblock %}

{​% block content %}
<h1>Tasks</h1>
<ul id="task-list">
    {​% for task in tasks %}
        {​% include "tasks/item.html" %}
    {​% endfor %}
</ul>
{​% endblock %}
```

Define both template structs:

```rust
#[derive(Template)]
#[template(path = "base.html")]
struct BaseTemplate {
    ctx: TemplateContext,
}

#[derive(Template)]
#[template(path = "tasks/list.html")]
struct TaskListTemplate {
    ctx: TemplateContext,
    tasks: Vec<Task>,
}
```

### Includes

Include reusable fragments:

```html
{​% include "partials/header.html" %}

<main>
    {​% for task in tasks %}
        {​% include "tasks/item.html" %}
    {​% endfor %}
</main>

{​% include "partials/footer.html" %}
```

Included templates have access to the parent's variables. Define the `task` variable before the include loop.

### Filters

Transform values in templates:

```html
<!-- Built-in filters -->
<p>{{ name|upper }}</p>
<p>{{ description|truncate(100) }}</p>
<p>{{ content|safe }}</p>  <!-- Mark as safe HTML -->

<!-- Chained filters -->
<p>{{ title|lower|capitalize }}</p>
```

Common filters: `upper`, `lower`, `capitalize`, `trim`, `truncate`, `safe`, `escape`.

---

## Flash Messages

Flash messages are one-time messages stored in the session that survive redirects—perfect for form submission feedback.

### Creating Flash Messages

```rust
use acton_service::prelude::*;

async fn create_task(
    session: Session,
    Form(data): Form<CreateTaskForm>,
) -> impl IntoResponse {
    // Validate and create task...

    match create_task_in_db(&data).await {
        Ok(_) => {
            FlashMessages::push(
                &session,
                FlashMessage::success("Task created successfully!")
            ).await.ok();

            axum::response::Redirect::to("/tasks")
        }
        Err(e) => {
            FlashMessages::push(
                &session,
                FlashMessage::error(format!("Failed to create task: {}", e))
            ).await.ok();

            axum::response::Redirect::to("/tasks/new")
        }
    }
}
```

### Flash Message Types

```rust
FlashMessage::success("Operation completed")  // Green
FlashMessage::error("Something went wrong")   // Red
FlashMessage::warning("Please review")        // Yellow
FlashMessage::info("Did you know...")         // Blue
```

Each type has a `css_class()` method returning `flash-success`, `flash-error`, etc.

### Displaying Flash Messages

Flash messages are automatically consumed when read. Display them once in your base template:

```html
{​% if ctx.has_flash() %}
    <div class="flash-container">
        {​% for flash in ctx.flash_messages %}
            <div class="flash {{ flash.kind.css_class() }}">
                {{ flash.message }}
            </div>
        {​% endfor %}
    </div>
{​% endif %}
```

With CSS:

```css
.flash { padding: 1rem; margin-bottom: 1rem; border-radius: 0.25rem; }
.flash-success { background: #22c55e; color: white; }
.flash-error { background: #ef4444; color: white; }
.flash-warning { background: #f59e0b; color: white; }
.flash-info { background: #3b82f6; color: white; }
```

---

## HTMX Integration Patterns

### Full Pages vs. Fragments

When an HTMX request arrives, return just the fragment that changed. For non-HTMX requests (direct navigation), return the full page.

```rust
async fn list_tasks(
    HxRequest(is_htmx): HxRequest,
    Extension(store): Extension<SharedStore>,
) -> impl IntoResponse {
    let tasks = store.read().await.all();

    if is_htmx {
        // HTMX request: return just the list
        HtmlTemplate::fragment(TaskListFragment { tasks })
    } else {
        // Direct navigation: return full page
        let ctx = TemplateContext::new().with_path("/tasks");
        HtmlTemplate::page(TaskListPage { ctx, tasks })
    }
}
```

**Fragment template** (`tasks/list_fragment.html`):
```html
<ul id="task-list">
    {​% for task in tasks %}
        {​% include "tasks/item.html" %}
    {​% endfor %}
</ul>
```

**Full page template** (`tasks/list.html`):
```html
{​% extends "base.html" %}

{​% block content %}
<h1>Tasks</h1>
{​% include "tasks/list_fragment.html" %}
{​% endblock %}
```

### Out-of-Band Swaps

Update multiple elements from a single response using `hx-swap-oob`:

```rust
async fn create_task(
    Extension(store): Extension<SharedStore>,
    Form(form): Form<CreateTaskForm>,
) -> impl IntoResponse {
    let task = store.write().await.add(form.title);
    let (total, completed, pending) = store.read().await.stats();

    // Return task HTML + OOB updates for stats
    let task_html = TaskItemTemplate { task }.render().unwrap();
    let stats_oob = format!(
        r#"<span id="total-count" hx-swap-oob="outerHTML">{}</span>
<span id="pending-count" hx-swap-oob="outerHTML">{}</span>"#,
        total, pending
    );

    Html(format!("{}{}", task_html, stats_oob))
}
```

The main response swaps into the target element. Elements with `hx-swap-oob="outerHTML"` swap into matching IDs anywhere on the page.

**Client HTML with swap targets**:
```html
<div class="stats">
    <span id="total-count">{{ total_tasks }}</span> total
    <span id="pending-count">{{ pending_tasks }}</span> pending
</div>

<ul id="task-list" hx-target="beforeend">
    <!-- New tasks appear here via main response -->
</ul>
```

### Form Handling

Handle validation errors inline without losing form state:

```rust
async fn create_task(
    Form(form): Form<CreateTaskForm>,
) -> impl IntoResponse {
    // Validate
    if form.title.trim().is_empty() {
        return HtmlTemplate::fragment(TaskFormTemplate {
            error: Some("Title is required".to_string()),
            title: form.title,
        })
        .with_status(StatusCode::UNPROCESSABLE_ENTITY)
        .into_response();
    }

    // Create task and redirect
    // ...

    HxRedirect::to("/tasks").into_response()
}
```

**Form template**:
```html
<form hx-post="/tasks" hx-target="#task-form" hx-swap="outerHTML">
    <div id="task-form">
        {​% if error.is_some() %}
            <div class="error">{{ error.as_ref().unwrap() }}</div>
        {​% endif %}

        <input type="text" name="title" value="{{ title }}"
               placeholder="Task title" required>

        <button type="submit">Create Task</button>
    </div>
</form>
```

### Loading States

Add loading indicators to buttons:

```html
<button hx-post="/tasks/{{ task.id }}/complete"
        hx-target="closest li"
        hx-swap="outerHTML"
        hx-indicator="#spinner-{{ task.id }}">
    Complete
    <span id="spinner-{{ task.id }}" class="htmx-indicator">
        Loading...
    </span>
</button>
```

```css
.htmx-indicator { display: none; }
.htmx-request .htmx-indicator { display: inline; }
```

---

## Common Patterns

### Authentication-Aware Templates

```html
<nav>
    <a href="/">Home</a>
    {​% if ctx.is_authenticated %}
        <span>Welcome, {{ ctx.user_id.as_ref().unwrap() }}</span>
        <form hx-post="/logout" hx-target="body">
            <button type="submit">Logout</button>
        </form>
    {​% else %}
        <a href="/login">Login</a>
    {​% endif %}
</nav>
```

### Active Navigation

```html
<nav>
    <a href="/" class="{​% if ctx.is_active('/') %}active{​% endif %}">
        Home
    </a>
    <a href="/tasks" class="{​% if ctx.is_active_prefix('/tasks') %}active{​% endif %}">
        Tasks
    </a>
    <a href="/settings" class="{​% if ctx.is_active('/settings') %}active{​% endif %}">
        Settings
    </a>
</nav>
```

### Error Pages

```rust
async fn not_found() -> impl IntoResponse {
    let ctx = TemplateContext::new().with_path("/404");
    HtmlTemplate::new(NotFoundTemplate { ctx })
        .with_status(StatusCode::NOT_FOUND)
}
```

### Reusable Partials

**Task item partial** (`tasks/item.html`):
```html
<li id="task-{{ task.id }}" class="task-item {​% if task.completed %}completed{​% endif %}">
    <input type="checkbox"
           hx-post="/tasks/{{ task.id }}/toggle"
           hx-target="#task-{{ task.id }}"
           hx-swap="outerHTML"
           {​% if task.completed %}checked{​% endif %}>
    <span class="task-title">{{ task.title }}</span>
    <button hx-delete="/tasks/{{ task.id }}"
            hx-target="#task-{{ task.id }}"
            hx-swap="outerHTML"
            hx-confirm="Delete this task?">
        Delete
    </button>
</li>
```

Use in list templates:
```html
<ul id="task-list">
    {​% for task in tasks %}
        {​% include "tasks/item.html" %}
    {​% endfor %}
</ul>
```

---

## Configuration

### Template Directory

By default, Askama looks for templates in a `templates` directory at your crate root. Configure this in `askama.toml`:

```toml
[general]
dirs = ["templates"]
```

### Recommended Directory Structure

```text
templates/
├── base.html           # Base layout with head, nav, flash
├── partials/
│   ├── nav.html        # Navigation component
│   ├── flash.html      # Flash message display
│   └── footer.html     # Footer component
├── tasks/
│   ├── list.html       # Full task list page
│   ├── list_fragment.html  # Task list fragment for HTMX
│   ├── item.html       # Single task item
│   ├── form.html       # New/edit task form
│   └── edit.html       # Inline edit form
└── auth/
    ├── login.html      # Login page
    └── _user_menu.html # User dropdown fragment
```

---

## Troubleshooting

### "template not found"

Check that:
1. Template path in `#[template(path = "...")]` is relative to `templates/`
2. `askama.toml` exists and `dirs` is correct
3. Template file exists and has correct extension

### "field not found in this scope"

The struct field must match the variable name in the template:

```rust
// Struct has `items`
struct MyTemplate { items: Vec<Item> }

// Template uses `items`
{​% for item in items %}
```

### Compile errors in templates

Askama reports line numbers accurately. Read the full error—it points to the exact template line.

### HTMX not updating

Check:
1. `hx-target` selector matches an element ID
2. `hx-swap` strategy is correct (default is `innerHTML`)
3. Response is valid HTML (not JSON, not an error page)

### Flash messages not showing

Ensure:
1. Session feature is enabled (`session-memory` or `session-redis`)
2. Session middleware is applied to routes
3. `FlashMessages` is extracted in the handler displaying messages
4. `flash.into_messages()` is called and passed to `TemplateContext`

---

## Next Steps

- [HTMX Integration](/docs/htmx) - Overview of all HTMX features
- [Server-Sent Events](/docs/sse) - Real-time updates with SSE
- [Session Management](/docs/session) - Authentication and session state
- [Examples](/docs/examples#htmx) - Complete working examples
