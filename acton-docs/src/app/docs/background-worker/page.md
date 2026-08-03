---
title: Background Worker
nextjs:
  metadata:
    title: Background Worker
    description: Managed background task execution with tracking, cancellation, and graceful shutdown support.
---

The `BackgroundWorker` provides a managed alternative to ad-hoc `tokio::spawn` calls. It offers named task tracking, graceful shutdown, status monitoring, and cancellation support.

{% callout type="note" title="When to Use BackgroundWorker" %}
Use `BackgroundWorker` when you need to:
- Track task status by name
- Cancel tasks on demand
- Ensure tasks complete gracefully on shutdown
- Monitor running tasks via health checks

For fire-and-forget tasks with no tracking needs, `tokio::spawn` remains appropriate.
{% /callout %}

---

## Quick Start

**1. Enable the background worker in `config.toml`:**

```toml
[background_worker]
enabled = true
```

**2. Access the worker in your handlers via `state.background_worker()`:**

```rust
use acton_service::prelude::*;
use acton_service::agents::TaskStatus;
use axum::{extract::State, Json};

async fn start_sync(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let worker = state.background_worker()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    worker.submit("data-sync", || async {
        sync_external_data().await?;
        Ok(())
    }).await;

    Ok(Json(serde_json::json!({ "status": "started" })))
}
```

`ServiceBuilder::build()` automatically spawns the `BackgroundWorker` when `[background_worker]` is configured with `enabled = true`. No manual runtime or spawn calls needed.

**3. (Optional) Submit startup tasks between `build()` and `serve()`:**

```rust
let service = ServiceBuilder::new()
    .with_routes(routes)
    .build();

// Submit tasks before serving — useful for cache warming, data sync, etc.
if let Some(worker) = service.state().background_worker() {
    worker.submit("cache-warm", || async {
        warm_cache().await?;
        Ok(())
    }).await;
}

service.serve().await?;
```

---

## Startup Tasks

Use `service.state()` to access the `BackgroundWorker` between `build()` and `serve()`. This is the recommended pattern for tasks that should run at application startup:

```rust
use acton_service::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let routes = VersionedApiBuilder::new()
        .add_version(ApiVersion::V1, |router| {
            router.route("/hello", get(hello))
        })
        .build_routes();

    let service = ServiceBuilder::new()
        .with_routes(routes)
        .build();

    // Submit startup tasks before serving
    if let Some(worker) = service.state().background_worker() {
        worker.submit("cache-warm", || async {
            load_cache_from_database().await?;
            Ok(())
        }).await;

        worker.submit("config-sync", || async {
            sync_remote_config().await?;
            Ok(())
        }).await;
    }

    service.serve().await?;
    Ok(())
}
```

{% callout type="note" title="Non-blocking" %}
Startup tasks run concurrently in the background. `serve()` does not wait for them to complete — the server starts accepting requests immediately. If your application requires a task to finish before serving traffic, await the work directly instead of submitting it to the worker.
{% /callout %}

---

## Configuration

All `BackgroundWorker` behavior is controlled via the `[background_worker]` section in `config.toml`:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | `bool` | `false` | Enable the background worker |
| `max_concurrent_tasks` | `usize` | `0` | Max concurrent tasks (0 = unlimited) |
| `task_shutdown_timeout_secs` | `u64` | `5` | Per-task shutdown timeout in seconds |
| `cleanup_interval_secs` | `u64` | `0` | Auto-cleanup interval in seconds (0 = disabled) |

### Full TOML Example

```toml
[background_worker]
enabled = true
max_concurrent_tasks = 10
task_shutdown_timeout_secs = 10
cleanup_interval_secs = 300
```

### Environment Variable Overrides

Each field can be overridden with environment variables using the `ACTON_` prefix:

```bash
export ACTON_BACKGROUND_WORKER_ENABLED=true
export ACTON_BACKGROUND_WORKER_MAX_CONCURRENT_TASKS=10
export ACTON_BACKGROUND_WORKER_TASK_SHUTDOWN_TIMEOUT_SECS=10
export ACTON_BACKGROUND_WORKER_CLEANUP_INTERVAL_SECS=300
```

---

## Task Lifecycle

Tasks progress through defined states:

```
┌─────────┐     submit()     ┌─────────┐     work completes     ┌───────────┐
│ Pending │ ───────────────► │ Running │ ────────────────────► │ Completed │
└─────────┘                  └────┬────┘                        └───────────┘
                                  │
                                  │ cancel() or              ┌──────────┐
                                  │ shutdown                 │ Failed   │
                                  ▼                          └──────────┘
                            ┌───────────┐
                            │ Cancelled │
                            └───────────┘
```

### Task States

| State | Description |
|-------|-------------|
| `Pending` | Task submitted but not yet started (brief) |
| `Running` | Task is actively executing |
| `Completed` | Task finished successfully |
| `Failed(String)` | Task returned an error |
| `Cancelled` | Task was cancelled before completion |

---

## Submitting Tasks

Use the `submit` method to add tasks to the worker:

```rust
// Basic task submission
worker.submit("my-task", || async {
    do_work().await?;
    Ok(())
}).await;

// Task with captured variables
let user_id = 42;
worker.submit(format!("user-sync-{}", user_id), move || async move {
    sync_user_data(user_id).await?;
    Ok(())
}).await;

// Long-running task with progress updates
worker.submit("report-generation", || async {
    for chunk in 0..100 {
        process_chunk(chunk).await?;
        tracing::info!(chunk, "Progress");
    }
    Ok(())
}).await;
```

### Task ID Best Practices

Choose task IDs that are:
- **Unique**: Avoid ID collisions with other tasks
- **Descriptive**: Make it easy to identify the task purpose
- **Queryable**: Include relevant identifiers for later lookup

```rust
// Good task IDs
worker.submit("daily-report-2024-01-15", || async { ... }).await;
worker.submit("user-sync-12345", || async { ... }).await;
worker.submit("email-campaign-welcome-series", || async { ... }).await;

// Avoid generic IDs
worker.submit("task1", || async { ... }).await;  // Hard to identify
worker.submit("work", || async { ... }).await;   // Too generic
```

---

## Concurrency Limiting

The `BackgroundWorker` supports semaphore-based concurrency limiting via the `max_concurrent_tasks` configuration. When set, `submit()` will wait until a slot is available before starting the task, providing built-in backpressure.

### Configuration

```toml
[background_worker]
enabled = true
max_concurrent_tasks = 5   # Only 5 tasks run at a time
```

### When to Use

- **Rate-limited APIs**: Prevent overwhelming an external service with too many concurrent requests
- **Resource-constrained tasks**: Limit CPU or memory-intensive work running in parallel
- **Database-heavy operations**: Cap concurrent queries to avoid connection pool exhaustion

### Example

```rust
// With max_concurrent_tasks = 2, only 2 tasks run concurrently.
// The third submit() will wait until a slot opens up.
for i in 0..10 {
    let worker = worker.clone();
    tokio::spawn(async move {
        worker.submit(format!("job-{}", i), move || async move {
            do_rate_limited_work(i).await?;
            Ok(())
        }).await;
    });
}
```

{% callout type="note" title="Unlimited Concurrency" %}
Set `max_concurrent_tasks = 0` (the default) for unlimited concurrency. All submitted tasks will start immediately.
{% /callout %}

---

## Monitoring Tasks

Task state lives inside the worker actor, so every query is a message round
trip: the methods are `async` and return `Result<_, AskError>`, which is `Err`
only if the actor cannot be reached or does not answer.

### Check Individual Task Status

`task_status` returns `None` when the worker has no record of that ID at all,
which is distinct from a task sitting in `TaskStatus::Pending`.

```rust
match worker.task_status("my-task").await? {
    Some(TaskStatus::Running) => println!("Still working..."),
    Some(TaskStatus::Completed) => println!("Done!"),
    Some(TaskStatus::Failed(error)) => println!("Failed: {}", error),
    Some(TaskStatus::Cancelled) => println!("Was cancelled"),
    Some(TaskStatus::Pending) => println!("Not started yet"),
    None => println!("No such task"),
}
```

### Wait for a Task to Finish

Rather than polling `task_status` in a loop, ask the worker to answer once the
task reaches a terminal state. The worker parks the request until the task
reports in.

```rust
let status = worker.wait_for_task("my-task").await?;
println!("Finished as {:?}", status);
```

### Check Task Counts

```rust
// Total tracked tasks (all states)
let total = worker.task_count().await?;

// Currently running tasks
let running = worker.running_task_count().await?;

println!("Tasks: {} total, {} running", total, running);
```

### Check Task Existence

```rust
if worker.has_task("my-task").await? {
    println!("Task exists (any state)");
}
```

---

## Cancelling Tasks

### Cancel Individual Task

```rust
// Cancel and wait for the task to actually stop
let final_status = worker.cancel("my-task").await?;
assert_eq!(final_status, Some(TaskStatus::Cancelled));

// The worker will:
// 1. Signal the task's cancellation token
// 2. Wait up to the configured shutdown timeout for the task to report in
// 3. Answer with the status the task finished in
```

`cancel` returns `None` if no such task is tracked. It does not return until the
task has genuinely stopped, so the status it hands back is the real outcome
rather than a guess — a task that completes before it notices the token comes
back as `Completed`, not `Cancelled`.

### Writing Cancellation-Aware Tasks

Tasks should check their cancellation token for cooperative cancellation:

```rust
use tokio_util::sync::CancellationToken;

worker.submit("long-task", || async {
    for i in 0..1000 {
        // Yield periodically so the worker's select! can observe cancellation.
        // Your task just needs to be yield-point aware - the worker handles
        // the cancellation token for you.
        tokio::task::yield_now().await;

        process_item(i).await?;
    }
    Ok(())
}).await;
```

The `BackgroundWorker` uses `tokio::select!` internally to handle cancellation:

```rust
// Internal implementation (for understanding)
tokio::select! {
    biased;
    () = cancel_token.cancelled() => {
        // Task was cancelled
        status = TaskStatus::Cancelled;
    }
    result = work() => {
        // Task completed normally
        match result {
            Ok(()) => status = TaskStatus::Completed,
            Err(e) => status = TaskStatus::Failed(e.to_string()),
        }
    }
}
```

---

## Cleanup

### Remove Finished Tasks

Over time, completed/failed/cancelled tasks accumulate. Clean them up:

```rust
// Remove all finished tasks; returns how many records were dropped
let dropped = worker.cleanup_finished_tasks().await?;

// Useful after waiting on a result
let status = worker.wait_for_task("batch-job").await?;
if matches!(status, Some(TaskStatus::Completed | TaskStatus::Failed(_))) {
    // Process result...
    worker.cleanup_finished_tasks().await?;
}
```

### Periodic Cleanup

For long-running services, enable automatic periodic cleanup via configuration:

```toml
[background_worker]
enabled = true
cleanup_interval_secs = 300   # Clean up every 5 minutes
```

When `cleanup_interval_secs` is set to a value greater than `0`, the `BackgroundWorker` automatically runs `cleanup_finished_tasks()` at the specified interval. No manual `tokio::spawn` loop needed.

---

## Graceful Shutdown

When the agent runtime shuts down, `BackgroundWorker`:

1. **Cancels all tasks** via root cancellation token
2. **Waits for completion** (up to the configured timeout per task, default: 5 seconds)
3. **Logs results** for each task

```rust
// Trigger shutdown
runtime.shutdown_all().await?;

// Log output:
// INFO BackgroundWorker stopping, cancelling all tasks...
// DEBUG Task "task-1" shutdown complete
// DEBUG Task "task-2" shutdown complete
// WARN Task "task-3" shutdown timed out
// INFO All background tasks stopped
```

### Kubernetes Integration

For Kubernetes deployments, ensure your preStop hook allows time for task completion:

```yaml
spec:
  containers:
    - name: myservice
      lifecycle:
        preStop:
          exec:
            command: ["sleep", "10"]  # Allow task cleanup
  terminationGracePeriodSeconds: 30    # Total shutdown time
```

---

## Integration with AppState

When `[background_worker]` is configured with `enabled = true`, `ServiceBuilder::build()` automatically spawns the worker and makes it available via `state.background_worker()`. No custom state wrapper needed.

```rust
use acton_service::prelude::*;
use acton_service::agents::TaskStatus;
use axum::{extract::{State, Path}, Json};

// Start a background job
async fn start_job(
    State(state): State<AppState>,
    Json(request): Json<JobRequest>,
) -> Result<Json<JobResponse>, StatusCode> {
    let worker = state.background_worker()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let job_id = format!("job-{}", uuid::Uuid::new_v4());

    worker.submit(job_id.clone(), move || async move {
        execute_job(request).await
    }).await;

    Ok(Json(JobResponse { job_id }))
}

// Check job status
async fn get_job_status(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobStatusResponse>, StatusCode> {
    let worker = state.background_worker()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let status = worker
        .task_status(&job_id)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    // `None` means the worker has never heard of this job — a 404, not a state.
    let status = status.ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(JobStatusResponse { job_id, status }))
}
```

{% callout type="note" title="Optional Access" %}
`state.background_worker()` returns `Option<&BackgroundWorker>`. It returns `None` if the worker is not enabled in configuration. Always handle the `None` case in your handlers.
{% /callout %}

---

## Error Handling

### Task Errors

When a task returns `Err`, the error message is stored:

```rust
worker.submit("failing-task", || async {
    Err(anyhow::anyhow!("Something went wrong"))
}).await;

// Later...
if let Some(TaskStatus::Failed(error)) = worker.wait_for_task("failing-task").await? {
    tracing::error!(%error, "Task failed");
    // error = "Something went wrong"
}
```

### Panic Handling

If a task panics, the worker logs the panic but continues running:

```rust
// Task that panics
worker.submit("panicking-task", || async {
    panic!("Unexpected error");
}).await;

// The worker logs:
// WARN task_id="panicking-task" error="..." "Task panicked during execution"

// Other tasks continue running normally
```

---

## Comparison with tokio::spawn

| Feature | `BackgroundWorker` | `tokio::spawn` |
|---------|-------------------|----------------|
| Task tracking | By name | By JoinHandle |
| Status queries | Yes | Manual |
| Cancellation | Built-in | Manual CancellationToken |
| Graceful shutdown | Automatic | Manual coordination |
| Concurrency limiting | Built-in semaphore | Manual implementation |
| Error storage | Yes (in status) | Via JoinHandle |
| Overhead | Slight | Minimal |

**Use `BackgroundWorker` when:**
- You need to query task status by name
- Multiple components need to cancel tasks
- Graceful shutdown is required
- You want centralized task management

**Use `tokio::spawn` when:**
- Fire-and-forget tasks
- Performance-critical scenarios
- Simple one-off operations
- You manage the JoinHandle yourself

---

## Next Steps

- **[Reactive Architecture](/docs/reactive-architecture)** - Understand the underlying agent system
- **[Health Checks](/docs/health-checks)** - Expose task status via health endpoints
- **[Observability](/docs/observability)** - Log and trace background task execution
