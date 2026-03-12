//! Background Worker Example
//!
//! Demonstrates how to use the `BackgroundWorker` agent for managed background
//! task execution with tracking, cancellation, and graceful shutdown.
//!
//! This example shows two patterns:
//! 1. **Startup tasks** — submitting work between `build()` and `serve()`
//!    via `service.state().background_worker()`
//! 2. **Handler tasks** — submitting work from route handlers
//!    via `state.background_worker()`
//!
//! Run with:
//! ```bash
//! cargo run --example background-worker
//! ```

use std::time::Duration;

use acton_service::agents::TaskStatus;
use acton_service::prelude::*;

async fn hello() -> &'static str {
    "Hello from background-worker example!"
}

/// Handler that submits a background task and returns immediately
async fn start_task(
    State(state): State<AppState>,
) -> std::result::Result<Json<serde_json::Value>, StatusCode> {
    let worker = state
        .background_worker()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    worker
        .submit("handler-task", || async {
            tracing::info!("Handler-submitted task running");
            tokio::time::sleep(Duration::from_secs(2)).await;
            tracing::info!("Handler-submitted task completed");
            Ok(())
        })
        .await;

    Ok(Json(serde_json::json!({ "status": "started" })))
}

/// Handler that checks task status by name
async fn check_status(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> std::result::Result<Json<serde_json::Value>, StatusCode> {
    let worker = state
        .background_worker()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let status = worker.get_task_status(&task_id).await;
    let status_str = match &status {
        TaskStatus::Pending => "pending",
        TaskStatus::Running => "running",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed(_) => "failed",
        TaskStatus::Cancelled => "cancelled",
    };

    Ok(Json(
        serde_json::json!({ "task_id": task_id, "status": status_str }),
    ))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Build versioned routes
    let routes = VersionedApiBuilder::new()
        .add_version(ApiVersion::V1, |router| {
            router
                .route("/hello", get(hello))
                .route("/tasks/start", post(start_task))
                .route("/tasks/{task_id}/status", get(check_status))
        })
        .build_routes();

    // Build the service — BackgroundWorker is automatically spawned
    // when [background_worker] is configured with enabled = true
    let service = ServiceBuilder::new().with_routes(routes).build();

    // Submit startup tasks between build() and serve().
    // This is useful for cache warming, data sync, migrations, etc.
    if let Some(worker) = service.state().background_worker() {
        worker
            .submit("startup-cache-warm", || async {
                tracing::info!("Warming caches at startup...");
                tokio::time::sleep(Duration::from_secs(1)).await;
                tracing::info!("Cache warming complete");
                Ok(())
            })
            .await;

        worker
            .submit("startup-data-sync", || async {
                tracing::info!("Syncing data at startup...");
                tokio::time::sleep(Duration::from_secs(2)).await;
                tracing::info!("Data sync complete");
                Ok(())
            })
            .await;

        tracing::info!("Submitted {} startup tasks", worker.task_count());
    }

    // Start serving — startup tasks continue running in the background
    service.serve().await?;

    Ok(())
}
