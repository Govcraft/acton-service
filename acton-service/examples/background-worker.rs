//! Background Worker Example
//!
//! Demonstrates how to use the `BackgroundWorker` agent for managed background
//! task execution with tracking, cancellation, and graceful shutdown.
//!
//! Run with:
//! ```bash
//! cargo run --example background-worker
//! ```

use std::time::Duration;

use acton_reactive::prelude::*;
use acton_service::agents::{BackgroundWorker, TaskStatus};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing for logs
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Starting BackgroundWorker example");

    // Create an agent runtime using ActonApp::launch()
    let mut runtime = ActonApp::launch();

    // Spawn the background worker
    let worker = BackgroundWorker::spawn(&mut runtime).await?;
    tracing::info!("BackgroundWorker spawned");

    // Submit a quick task
    worker
        .submit("quick-task", || async {
            tracing::info!("Quick task started");
            tokio::time::sleep(Duration::from_millis(500)).await;
            tracing::info!("Quick task completed");
            Ok(())
        })
        .await;

    // Submit a longer-running task
    worker
        .submit("long-task", || async {
            tracing::info!("Long task started");
            for i in 1..=5 {
                tokio::time::sleep(Duration::from_millis(300)).await;
                tracing::info!("Long task progress: {}/5", i);
            }
            tracing::info!("Long task completed");
            Ok(())
        })
        .await;

    // Submit a task that will fail
    worker
        .submit("failing-task", || async {
            tracing::info!("Failing task started");
            tokio::time::sleep(Duration::from_millis(200)).await;
            Err(anyhow::anyhow!("Intentional failure for demonstration"))
        })
        .await;

    // Submit a task that we'll cancel
    worker
        .submit("cancellable-task", || async {
            tracing::info!("Cancellable task started");
            // This task runs for 10 seconds but will be cancelled
            for i in 1..=100 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if i % 10 == 0 {
                    tracing::info!("Cancellable task progress: {}%", i);
                }
            }
            tracing::info!("Cancellable task completed");
            Ok(())
        })
        .await;

    tracing::info!("Submitted {} tasks", worker.task_count());

    // Wait a bit then check statuses
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Check task statuses
    for task_id in ["quick-task", "long-task", "failing-task", "cancellable-task"] {
        let status = worker.get_task_status(task_id).await;
        tracing::info!("Task '{}' status: {:?}", task_id, status);
    }

    // Cancel the cancellable task
    tracing::info!("Cancelling 'cancellable-task'...");
    worker.cancel("cancellable-task").await;

    // Wait for it to be cancelled
    tokio::time::sleep(Duration::from_millis(500)).await;
    let status = worker.get_task_status("cancellable-task").await;
    tracing::info!("Task 'cancellable-task' status after cancel: {:?}", status);

    // Wait for remaining tasks to complete
    tracing::info!("Waiting for remaining tasks...");
    loop {
        let running = worker.running_task_count().await;
        if running == 0 {
            break;
        }
        tracing::info!("{} tasks still running...", running);
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Final status check
    tracing::info!("\n=== Final Task Statuses ===");
    for task_id in ["quick-task", "long-task", "failing-task", "cancellable-task"] {
        let status = worker.get_task_status(task_id).await;
        let status_str = match &status {
            TaskStatus::Pending => "Pending".to_string(),
            TaskStatus::Running => "Running".to_string(),
            TaskStatus::Completed => "Completed".to_string(),
            TaskStatus::Failed(err) => format!("Failed: {}", err),
            TaskStatus::Cancelled => "Cancelled".to_string(),
        };
        tracing::info!("  {}: {}", task_id, status_str);
    }

    // Clean up finished tasks
    worker.cleanup_finished_tasks().await;
    tracing::info!(
        "After cleanup: {} tasks tracked",
        worker.task_count()
    );

    // Graceful shutdown
    tracing::info!("Shutting down runtime...");
    runtime.shutdown_all().await?;
    tracing::info!("Shutdown complete");

    Ok(())
}
