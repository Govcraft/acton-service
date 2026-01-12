//! HTMX Task Manager Example
//!
//! A comprehensive example demonstrating HTMX patterns with acton-service:
//!
//! - **Askama Templates**: Server-side rendering with TemplateContext
//! - **SSE Real-Time Updates**: Live task updates across clients
//! - **Session Authentication**: Login/logout with AuthSession
//!
//! ## Running
//!
//! ```bash
//! cargo run --manifest-path=acton-service/Cargo.toml \
//!   --example task-manager --features htmx-full
//! ```
//!
//! Then open http://localhost:8080 in your browser.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use acton_service::prelude::*;
use acton_service::session::{create_memory_session_layer, AuthSession, FlashMessage, SessionConfig, TypedSession};
use axum::extract::Form;
use tokio::sync::RwLock;

// ============================================================================
// Data Models
// ============================================================================

/// A task in our task manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub completed: bool,
}

/// In-memory task storage.
#[derive(Debug, Default)]
pub struct TaskStore {
    tasks: Vec<Task>,
    next_id: u64,
}

impl TaskStore {
    fn add(&mut self, title: String) -> Task {
        self.next_id += 1;
        let task = Task {
            id: self.next_id,
            title,
            completed: false,
        };
        self.tasks.push(task.clone());
        task
    }

    fn get(&self, id: u64) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    fn update(&mut self, id: u64, title: String) -> Option<Task> {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.title = title;
            Some(task.clone())
        } else {
            None
        }
    }

    fn toggle(&mut self, id: u64) -> Option<Task> {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.completed = !task.completed;
            Some(task.clone())
        } else {
            None
        }
    }

    fn delete(&mut self, id: u64) -> bool {
        let len_before = self.tasks.len();
        self.tasks.retain(|t| t.id != id);
        self.tasks.len() < len_before
    }

    fn all(&self) -> Vec<Task> {
        self.tasks.clone()
    }

    fn stats(&self) -> (usize, usize, usize) {
        let total = self.tasks.len();
        let completed = self.tasks.iter().filter(|t| t.completed).count();
        let pending = total - completed;
        (total, completed, pending)
    }
}

type SharedStore = Arc<RwLock<TaskStore>>;

// ============================================================================
// Templates
// ============================================================================

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    ctx: TemplateContext,
    tasks: Vec<Task>,
    total_tasks: usize,
    completed_tasks: usize,
    pending_tasks: usize,
}

#[derive(Template)]
#[template(path = "tasks/item.html")]
struct TaskItemTemplate {
    task: Task,
}

#[derive(Template)]
#[template(path = "tasks/edit.html")]
struct TaskEditTemplate {
    task: Task,
}

#[derive(Template)]
#[template(path = "auth/login.html")]
struct LoginTemplate {
    ctx: TemplateContext,
}

// ============================================================================
// Form Data
// ============================================================================

#[derive(Debug, Deserialize)]
struct CreateTaskForm {
    title: String,
}

#[derive(Debug, Deserialize)]
struct UpdateTaskForm {
    title: String,
}

#[derive(Debug, Deserialize)]
struct LoginForm {
    username: String,
}

// ============================================================================
// Response Helpers
// ============================================================================

/// Render OOB stat updates as HTML.
fn render_stats_oob(total: usize, completed: usize, pending: usize) -> String {
    format!(
        r#"<span class="stat-value" id="total-count" hx-swap-oob="outerHTML">{}</span>
<span class="stat-value" id="pending-count" hx-swap-oob="outerHTML">{}</span>
<span class="stat-value" id="completed-count" hx-swap-oob="outerHTML">{}</span>"#,
        total, pending, completed
    )
}

// ============================================================================
// Handlers
// ============================================================================

/// Home page - shows task list.
async fn index(
    flash: FlashMessages,
    auth: TypedSession<AuthSession>,
    Extension(store): Extension<SharedStore>,
) -> impl IntoResponse {
    let tasks = store.read().await.all();
    let (total, completed, pending) = store.read().await.stats();

    let ctx = TemplateContext::new()
        .with_path("/")
        .with_auth(auth.data().user_id.clone())
        .with_flash(flash.into_messages());

    HtmlTemplate::page(IndexTemplate {
        ctx,
        tasks,
        total_tasks: total,
        completed_tasks: completed,
        pending_tasks: pending,
    })
}

/// Create a new task.
async fn create_task(
    Extension(store): Extension<SharedStore>,
    Form(form): Form<CreateTaskForm>,
) -> impl IntoResponse {
    let title = form.title.trim();
    if title.is_empty() {
        return Html("<div class=\"flash flash-error\">Task title cannot be empty</div>").into_response();
    }

    let task = store.write().await.add(title.to_string());

    // Return the new task item with OOB stats update
    let (total, completed, pending) = store.read().await.stats();
    let task_html = TaskItemTemplate { task }.render().unwrap_or_default();
    let stats_html = render_stats_oob(total, completed, pending);

    // Delete empty message if present
    let delete_empty = r#"<li id="empty-message" hx-swap-oob="delete"></li>"#;

    Html(format!("{}{}{}", task_html, stats_html, delete_empty)).into_response()
}

/// Get a single task (for cancel edit).
async fn get_task(
    Path(id): Path<u64>,
    Extension(store): Extension<SharedStore>,
) -> impl IntoResponse {
    match store.read().await.get(id).cloned() {
        Some(task) => HtmlTemplate::fragment(TaskItemTemplate { task }).into_response(),
        None => (StatusCode::NOT_FOUND, "Task not found").into_response(),
    }
}

/// Get task edit form.
async fn edit_task_form(
    Path(id): Path<u64>,
    Extension(store): Extension<SharedStore>,
) -> impl IntoResponse {
    match store.read().await.get(id).cloned() {
        Some(task) => HtmlTemplate::fragment(TaskEditTemplate { task }).into_response(),
        None => (StatusCode::NOT_FOUND, "Task not found").into_response(),
    }
}

/// Update a task.
async fn update_task(
    Path(id): Path<u64>,
    Extension(store): Extension<SharedStore>,
    Form(form): Form<UpdateTaskForm>,
) -> impl IntoResponse {
    let title = form.title.trim();
    if title.is_empty() {
        // Return the task unchanged
        if let Some(task) = store.read().await.get(id).cloned() {
            return HtmlTemplate::fragment(TaskItemTemplate { task }).into_response();
        }
        return (StatusCode::NOT_FOUND, "Task not found").into_response();
    }

    match store.write().await.update(id, title.to_string()) {
        Some(task) => HtmlTemplate::fragment(TaskItemTemplate { task }).into_response(),
        None => (StatusCode::NOT_FOUND, "Task not found").into_response(),
    }
}

/// Toggle task completion.
async fn toggle_task(
    Path(id): Path<u64>,
    Extension(store): Extension<SharedStore>,
) -> impl IntoResponse {
    // Use a block to ensure write lock is released before reading stats
    let toggle_result = { store.write().await.toggle(id) };

    match toggle_result {
        Some(task) => {
            // Update stats via OOB
            let (total, completed, pending) = store.read().await.stats();
            let task_html = TaskItemTemplate { task }.render().unwrap_or_default();
            let stats_html = render_stats_oob(total, completed, pending);

            Html(format!("{}{}", task_html, stats_html)).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Task not found").into_response(),
    }
}

/// Delete a task.
async fn delete_task(
    Path(id): Path<u64>,
    Extension(store): Extension<SharedStore>,
) -> impl IntoResponse {
    // Use a block to ensure write lock is released before reading stats
    let deleted = { store.write().await.delete(id) };

    if deleted {
        // Update stats via OOB - return empty content to remove the task
        let (total, completed, pending) = store.read().await.stats();
        let stats_html = render_stats_oob(total, completed, pending);

        Html(stats_html).into_response()
    } else {
        (StatusCode::NOT_FOUND, "Task not found").into_response()
    }
}

/// SSE endpoint for real-time updates.
async fn events(
    Extension(broadcaster): Extension<Arc<SseBroadcaster>>,
) -> Sse<impl Stream<Item = std::result::Result<SseEvent, Infallible>>> {
    let rx = broadcaster.subscribe();

    // Create a stream from the broadcast receiver
    let stream = stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(msg) => {
                let mut event = SseEvent::default().data(msg.data);
                if let Some(event_type) = msg.event_type {
                    event = event.event(event_type);
                }
                Some((Ok(event), rx))
            }
            Err(_) => None, // Channel closed
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Login page.
async fn login_page(flash: FlashMessages, auth: TypedSession<AuthSession>) -> impl IntoResponse {
    let ctx = TemplateContext::new()
        .with_path("/login")
        .with_auth(auth.data().user_id.clone())
        .with_flash(flash.into_messages());

    HtmlTemplate::page(LoginTemplate { ctx })
}

/// Handle login.
async fn login(mut auth: TypedSession<AuthSession>, Form(form): Form<LoginForm>) -> impl IntoResponse {
    let username = form.username.trim();
    if username.is_empty() {
        let _ = FlashMessages::push(auth.session(), FlashMessage::error("Username is required")).await;
        return axum::response::Redirect::to("/login").into_response();
    }

    // In a real app, you'd validate credentials here
    auth.data_mut()
        .login(username.to_string(), vec!["user".to_string()]);
    if let Err(e) = auth.save().await {
        tracing::error!("Failed to save session: {}", e);
    }

    let _ = FlashMessages::push(
        auth.session(),
        FlashMessage::success(format!("Welcome back, {}!", username)),
    ).await;

    axum::response::Redirect::to("/").into_response()
}

/// Handle logout.
async fn logout(mut auth: TypedSession<AuthSession>) -> impl IntoResponse {
    let _ = FlashMessages::push(auth.session(), FlashMessage::info("You have been logged out")).await;

    auth.data_mut().logout();
    if let Err(e) = auth.save().await {
        tracing::error!("Failed to save session: {}", e);
    }

    axum::response::Redirect::to("/").into_response()
}

// ============================================================================
// Application
// ============================================================================

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing (filter out noisy tower-sessions warnings)
    tracing_subscriber::fmt()
        .with_env_filter("info,tower_sessions_core=error")
        .init();

    // Initialize shared state
    let store: SharedStore = Arc::new(RwLock::new(TaskStore::default()));
    let broadcaster = Arc::new(SseBroadcaster::new());

    // Add some sample tasks
    {
        let mut s = store.write().await;
        s.add("Learn HTMX with acton-service".to_string());
        s.add("Build something awesome".to_string());
        s.add("Deploy to production".to_string());
    }

    // Create session layer
    let session_config = SessionConfig::default();
    let session_layer = create_memory_session_layer(&session_config);

    // Build routes
    let app = Router::new()
        // Pages
        .route("/", get(index))
        .route("/login", get(login_page))
        // Task CRUD
        .route("/tasks", post(create_task))
        .route(
            "/tasks/{id}",
            get(get_task).put(update_task).delete(delete_task),
        )
        .route("/tasks/{id}/edit", get(edit_task_form))
        .route("/tasks/{id}/toggle", post(toggle_task))
        // SSE
        .route("/events", get(events))
        // Auth
        .route("/login", post(login))
        .route("/logout", post(logout))
        // Extensions
        .layer(Extension(store))
        .layer(Extension(broadcaster))
        // Session layer
        .layer(session_layer);

    // Run server
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    tracing::info!("Starting HTMX Task Manager on http://{}", addr);
    tracing::info!("Open http://localhost:8080 in your browser");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
