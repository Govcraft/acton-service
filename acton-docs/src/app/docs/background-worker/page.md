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

```rust
use acton_service::agents::{BackgroundWorker, TaskStatus};
use acton_reactive::prelude::*;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create an agent runtime
    let mut runtime = AgentRuntime::new();

    // Spawn the background worker
    let worker = BackgroundWorker::spawn(&mut runtime).await?;

    // Submit a background task
    worker.submit("data-sync", || async {
        // Do background work
        sync_external_data().await?;
        Ok(())
    }).await;

    // Check task status
    let status = worker.get_task_status("data-sync").await;
    println!("Task status: {:?}", status);

    // Graceful shutdown cancels all running tasks
    runtime.shutdown_all().await?;
    Ok(())
}
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

## Monitoring Tasks

### Check Individual Task Status

```rust
let status = worker.get_task_status("my-task").await;

match status {
    TaskStatus::Running => println!("Still working..."),
    TaskStatus::Completed => println!("Done!"),
    TaskStatus::Failed(error) => println!("Failed: {}", error),
    TaskStatus::Cancelled => println!("Was cancelled"),
    TaskStatus::Pending => println!("Not started yet"),
}
```

### Check Task Counts

```rust
// Total tracked tasks (all states)
let total = worker.task_count();

// Currently running tasks
let running = worker.running_task_count().await;

println!("Tasks: {} total, {} running", total, running);
```

### Check Task Existence

```rust
if worker.has_task("my-task") {
    println!("Task exists (any state)");
}
```

---

## Cancelling Tasks

### Cancel Individual Task

```rust
// Request cancellation
worker.cancel("my-task").await;

// The worker will:
// 1. Signal the task's cancellation token
// 2. Wait up to 5 seconds for task completion
// 3. Update status to Cancelled
```

### Writing Cancellation-Aware Tasks

Tasks should check their cancellation token for cooperative cancellation:

```rust
use tokio_util::sync::CancellationToken;

worker.submit("long-task", || async {
    for i in 0..1000 {
        // Check for cancellation periodically
        if tokio::task::yield_now().await; {
            // The worker handles cancellation via select!
            // Your task just needs to be yield-point aware
        }

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
// Remove all non-running tasks
worker.cleanup_finished_tasks().await;

// Useful after checking results
let status = worker.get_task_status("batch-job").await;
if matches!(status, TaskStatus::Completed | TaskStatus::Failed(_)) {
    // Process result...
    worker.cleanup_finished_tasks().await;
}
```

### Periodic Cleanup

For long-running services, schedule periodic cleanup:

```rust
// In your service setup
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(300));
    loop {
        interval.tick().await;
        worker.cleanup_finished_tasks().await;
        tracing::debug!("Cleaned up finished tasks");
    }
});
```

---

## Graceful Shutdown

When the agent runtime shuts down, `BackgroundWorker`:

1. **Cancels all tasks** via root cancellation token
2. **Waits for completion** (up to 5 seconds per task)
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

For services using `ServiceBuilder`, you can store the `BackgroundWorker` in your application state:

```rust
use acton_service::prelude::*;
use acton_service::agents::BackgroundWorker;
use std::sync::Arc;

// Extended state with background worker
#[derive(Clone)]
pub struct MyAppState {
    inner: AppState,
    worker: Arc<BackgroundWorker>,
}

// In your handler
async fn start_job(
    State(state): State<MyAppState>,
    Json(request): Json<JobRequest>,
) -> Result<Json<JobResponse>, ApiError> {
    let job_id = format!("job-{}", uuid::Uuid::new_v4());

    state.worker.submit(job_id.clone(), move || async move {
        execute_job(request).await
    }).await;

    Ok(Json(JobResponse { job_id }))
}

// Check job status
async fn get_job_status(
    State(state): State<MyAppState>,
    Path(job_id): Path<String>,
) -> Result<Json<JobStatusResponse>, ApiError> {
    let status = state.worker.get_task_status(&job_id).await;
    Ok(Json(JobStatusResponse { job_id, status }))
}
```

---

## Error Handling

### Task Errors

When a task returns `Err`, the error message is stored:

```rust
worker.submit("failing-task", || async {
    Err(anyhow::anyhow!("Something went wrong"))
}).await;

// Later...
match worker.get_task_status("failing-task").await {
    TaskStatus::Failed(error) => {
        tracing::error!(%error, "Task failed");
        // error = "Something went wrong"
    }
    _ => {}
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
