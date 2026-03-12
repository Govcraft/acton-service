//! Background Task Management Agent
//!
//! This module provides a managed alternative to ad-hoc `tokio::spawn` calls.
//! The `BackgroundWorker` agent offers:
//!
//! - **Named task tracking**: Each task has a unique ID for monitoring
//! - **Graceful shutdown**: Tasks are cancelled and awaited during agent shutdown
//! - **Status monitoring**: Query task status via message passing
//! - **Cancellation support**: Individual tasks can be cancelled on demand
//! - **Concurrency limiting**: Optional semaphore-based backpressure
//! - **Periodic cleanup**: Automatic removal of finished tasks
//!
//! # Architecture
//!
//! The `BackgroundWorker` uses a shared `DashMap` for concurrent task state tracking.
//! Tasks are spawned directly via the service API and tracked via the shared map.
//! The agent handles lifecycle coordination and cleanup during shutdown.
//!
//! # Example
//!
//! ```rust,ignore
//! use acton_service::agents::prelude::*;
//!
//! let mut runtime = ActonApp::launch();
//! let config = BackgroundWorkerConfig { enabled: true, ..Default::default() };
//! let worker = BackgroundWorker::spawn(&mut runtime, &config).await?;
//!
//! // Submit a background task
//! worker.submit("my-task", async move {
//!     // Do background work
//!     tokio::time::sleep(Duration::from_secs(10)).await;
//!     Ok(())
//! }).await;
//!
//! // Check task status
//! let status = worker.get_task_status("my-task").await;
//!
//! // Cancel a specific task
//! worker.cancel("my-task").await;
//!
//! // Graceful shutdown cancels all remaining tasks
//! runtime.shutdown_all().await?;
//! ```

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use acton_reactive::prelude::{Reply, *};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::messages::{CancelTask, GetAllTaskStatuses, GetTaskStatus, TaskStatusResponse};

fn default_task_shutdown_timeout_secs() -> u64 {
    5
}

/// Configuration for the background worker agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundWorkerConfig {
    /// Whether the background worker is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Maximum number of concurrent tasks (0 = unlimited)
    #[serde(default)]
    pub max_concurrent_tasks: usize,
    /// Timeout in seconds for individual task shutdown during cancellation
    #[serde(default = "default_task_shutdown_timeout_secs")]
    pub task_shutdown_timeout_secs: u64,
    /// Interval in seconds for automatic cleanup of finished tasks (0 = disabled)
    #[serde(default)]
    pub cleanup_interval_secs: u64,
}

impl Default for BackgroundWorkerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_concurrent_tasks: 0,
            task_shutdown_timeout_secs: 5,
            cleanup_interval_secs: 0,
        }
    }
}

/// Status of a background task
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task is queued but not yet started
    #[default]
    Pending,
    /// Task is currently running
    Running,
    /// Task completed successfully
    Completed,
    /// Task failed with an error
    Failed(String),
    /// Task was cancelled
    Cancelled,
}

/// Internal tracking information for a task
#[derive(Debug)]
pub(crate) struct TaskInfo {
    /// Unique task identifier
    task_id: String,
    /// Handle to the spawned task
    join_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// Token for cancelling this specific task
    cancellation_token: CancellationToken,
    /// Current status
    status: Arc<Mutex<TaskStatus>>,
}

/// State for the background worker agent
#[derive(Debug, Default)]
pub struct BackgroundWorkerState {
    /// Root cancellation token for creating child tokens
    pub(crate) root_token: Option<CancellationToken>,
}

/// Service wrapper for the background worker agent
///
/// Provides a clean API for submitting and managing background tasks.
/// Tasks are spawned directly and tracked in a shared DashMap for
/// efficient concurrent access.
#[derive(Clone)]
pub struct BackgroundWorker {
    /// Handle for sending messages to the agent
    agent_handle: ActorHandle,
    /// Shared task map for direct access
    tasks: Arc<DashMap<String, TaskInfo>>,
    /// Root cancellation token for creating child tokens
    root_token: CancellationToken,
    /// Optional semaphore for concurrency limiting
    semaphore: Option<Arc<Semaphore>>,
    /// Timeout for individual task shutdown
    shutdown_timeout: Duration,
}

impl BackgroundWorker {
    /// Spawn a new background worker agent
    ///
    /// The worker will manage background tasks with graceful shutdown support.
    /// Configuration controls concurrency limits, shutdown timeouts, and
    /// periodic cleanup behavior.
    pub async fn spawn(
        runtime: &mut ActorRuntime,
        config: &BackgroundWorkerConfig,
    ) -> anyhow::Result<Self> {
        let tasks: Arc<DashMap<String, TaskInfo>> = Arc::new(DashMap::new());
        let root_token = CancellationToken::new();
        let shutdown_timeout = Duration::from_secs(config.task_shutdown_timeout_secs);

        let semaphore = if config.max_concurrent_tasks > 0 {
            Some(Arc::new(Semaphore::new(config.max_concurrent_tasks)))
        } else {
            None
        };

        let tasks_for_shutdown = tasks.clone();
        let root_token_for_agent = root_token.clone();
        let shutdown_timeout_for_cancel = shutdown_timeout;
        let shutdown_timeout_for_stop = shutdown_timeout;

        let mut agent = runtime.new_actor::<BackgroundWorkerState>();

        // Store root token in agent state
        agent.model.root_token = Some(root_token.clone());

        // Handle task cancellation requests
        let tasks_for_cancel = tasks.clone();
        agent.mutate_on::<CancelTask>(move |_agent, envelope| {
            let msg = envelope.message().clone();
            let tasks = tasks_for_cancel.clone();
            let timeout = shutdown_timeout_for_cancel;

            Reply::pending(async move {
                if let Some(task_info) = tasks.get(&msg.task_id) {
                    task_info.cancellation_token.cancel();
                    tracing::info!(task_id = %msg.task_id, "Task cancellation requested");

                    // Wait for the task to complete with configured timeout
                    let mut handle_lock = task_info.join_handle.lock().await;
                    if let Some(handle) = handle_lock.take() {
                        let _ = tokio::time::timeout(timeout, handle).await;
                    }
                } else {
                    tracing::warn!(task_id = %msg.task_id, "Task not found for cancellation");
                }
            })
        });

        // Handle status queries (read-only)
        let tasks_for_status = tasks.clone();
        agent.act_on::<GetTaskStatus>(move |_agent, envelope| {
            let msg = envelope.message().clone();
            let tasks = tasks_for_status.clone();
            let reply = envelope.reply_envelope();

            Box::pin(async move {
                let status = if let Some(task_info) = tasks.get(&msg.task_id) {
                    task_info.status.lock().await.clone()
                } else {
                    TaskStatus::Pending // Task not found
                };

                reply
                    .send(TaskStatusResponse {
                        task_id: msg.task_id,
                        status,
                    })
                    .await;
            })
        });

        // Handle bulk status queries
        let tasks_for_all_status = tasks.clone();
        agent.act_on::<GetAllTaskStatuses>(move |_agent, envelope| {
            let tasks = tasks_for_all_status.clone();
            let reply = envelope.reply_envelope();

            Box::pin(async move {
                let mut statuses = Vec::new();

                for entry in tasks.iter() {
                    let status = entry.status.lock().await.clone();
                    statuses.push(TaskStatusResponse {
                        task_id: entry.task_id.clone(),
                        status,
                    });
                }

                reply.send(statuses).await;
            })
        });

        // Graceful shutdown - cancel all tasks
        agent.before_stop(move |_agent| {
            let tasks = tasks_for_shutdown.clone();
            let root_token = root_token_for_agent.clone();
            let timeout = shutdown_timeout_for_stop;

            Box::pin(async move {
                let task_count = tasks.len();
                if task_count == 0 {
                    tracing::info!("BackgroundWorker stopping with no active tasks");
                    return;
                }

                tracing::info!(
                    task_count,
                    "BackgroundWorker stopping, cancelling all tasks..."
                );

                // Cancel root token (all child tokens will be cancelled)
                root_token.cancel();

                // Wait for all tasks to complete with configured timeout
                for entry in tasks.iter() {
                    let mut handle_lock = entry.join_handle.lock().await;
                    if let Some(handle) = handle_lock.take() {
                        match tokio::time::timeout(timeout, handle).await {
                            Ok(Ok(())) => {
                                tracing::debug!(task_id = %entry.task_id, "Task shutdown complete");
                            }
                            Ok(Err(e)) => {
                                tracing::warn!(
                                    task_id = %entry.task_id,
                                    error = %e,
                                    "Task panicked during shutdown"
                                );
                            }
                            Err(_) => {
                                tracing::warn!(
                                    task_id = %entry.task_id,
                                    "Task shutdown timed out"
                                );
                            }
                        }
                    }
                }

                tracing::info!("All background tasks stopped");
            })
        });

        // Log startup
        agent.after_start(|_agent| {
            Box::pin(async move {
                tracing::info!("BackgroundWorker agent started");
            })
        });

        let handle = agent.start().await;

        let worker = Self {
            agent_handle: handle,
            tasks,
            root_token,
            semaphore,
            shutdown_timeout,
        };

        // Spawn periodic cleanup task if configured
        if config.cleanup_interval_secs > 0 {
            let cleanup_worker = worker.clone();
            let cleanup_token = worker.root_token.child_token();
            let interval = Duration::from_secs(config.cleanup_interval_secs);
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        biased;
                        () = cleanup_token.cancelled() => break,
                        () = tokio::time::sleep(interval) => {
                            cleanup_worker.cleanup_finished_tasks().await;
                            tracing::debug!("Periodic background task cleanup completed");
                        }
                    }
                }
            });
        }

        Ok(worker)
    }

    /// Submit a new background task
    ///
    /// The task will be spawned and tracked by the worker.
    /// If a concurrency limit is configured, this method will await until
    /// a slot is available, providing backpressure to callers.
    ///
    /// # Arguments
    ///
    /// * `task_id` - Unique identifier for the task
    /// * `work` - Async closure that performs the work
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// worker.submit("cleanup-job", async move {
    ///     do_cleanup().await?;
    ///     Ok(())
    /// }).await;
    /// ```
    pub async fn submit<F, Fut>(&self, task_id: impl Into<String>, work: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let task_id = task_id.into();

        // Acquire concurrency permit before spawning (provides backpressure)
        let permit = if let Some(ref sem) = self.semaphore {
            match sem.clone().acquire_owned().await {
                Ok(permit) => Some(permit),
                Err(_) => {
                    tracing::warn!(task_id = %task_id, "Semaphore closed, task not submitted");
                    return;
                }
            }
        } else {
            None
        };

        // Create child cancellation token for this task
        let cancel_token = self.root_token.child_token();
        let cancel_token_clone = cancel_token.clone();

        // Create status tracking
        let status = Arc::new(Mutex::new(TaskStatus::Running));
        let status_for_task = status.clone();

        let task_id_clone = task_id.clone();

        // Spawn the background task
        let handle = tokio::spawn(async move {
            // Hold the permit for the task's lifetime
            let _permit = permit;
            let task_id = task_id_clone;
            tokio::select! {
                biased;

                () = cancel_token_clone.cancelled() => {
                    tracing::debug!(task_id = %task_id, "Task cancelled");
                    let mut s = status_for_task.lock().await;
                    *s = TaskStatus::Cancelled;
                }
                result = work() => {
                    match result {
                        Ok(()) => {
                            tracing::debug!(task_id = %task_id, "Task completed successfully");
                            let mut s = status_for_task.lock().await;
                            *s = TaskStatus::Completed;
                        }
                        Err(e) => {
                            tracing::warn!(task_id = %task_id, error = %e, "Task failed");
                            let mut s = status_for_task.lock().await;
                            *s = TaskStatus::Failed(e.to_string());
                        }
                    }
                }
            }
        });

        // Store task info
        let task_info = TaskInfo {
            task_id: task_id.clone(),
            join_handle: Arc::new(Mutex::new(Some(handle))),
            cancellation_token: cancel_token,
            status,
        };

        self.tasks.insert(task_id.clone(), task_info);
        tracing::info!(task_id = %task_id, "Background task submitted");
    }

    /// Cancel a specific task by ID
    ///
    /// The task's cancellation token will be triggered, and the worker
    /// will wait up to the configured shutdown timeout for it to complete.
    pub async fn cancel(&self, task_id: impl Into<String>) {
        self.agent_handle
            .send(CancelTask {
                task_id: task_id.into(),
            })
            .await;
    }

    /// Get the status of a specific task
    ///
    /// Returns the current status directly from the shared task map.
    pub async fn get_task_status(&self, task_id: &str) -> TaskStatus {
        if let Some(task_info) = self.tasks.get(task_id) {
            task_info.status.lock().await.clone()
        } else {
            TaskStatus::Pending
        }
    }

    /// Get the count of tracked tasks
    #[must_use]
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// Get the count of running tasks
    pub async fn running_task_count(&self) -> usize {
        let mut count = 0;
        for entry in self.tasks.iter() {
            if *entry.status.lock().await == TaskStatus::Running {
                count += 1;
            }
        }
        count
    }

    /// Check if a task exists
    #[must_use]
    pub fn has_task(&self, task_id: &str) -> bool {
        self.tasks.contains_key(task_id)
    }

    /// Remove completed/failed/cancelled tasks from tracking
    ///
    /// This is useful to prevent the task map from growing indefinitely.
    pub async fn cleanup_finished_tasks(&self) {
        let mut to_remove = Vec::new();

        for entry in self.tasks.iter() {
            let status = entry.status.lock().await.clone();
            match status {
                TaskStatus::Completed | TaskStatus::Failed(_) | TaskStatus::Cancelled => {
                    to_remove.push(entry.task_id.clone());
                }
                _ => {}
            }
        }

        for task_id in to_remove {
            self.tasks.remove(&task_id);
        }
    }

    /// Get the agent handle for direct message sending
    #[must_use]
    pub fn handle(&self) -> &ActorHandle {
        &self.agent_handle
    }

    /// Get the configured shutdown timeout
    #[must_use]
    pub fn shutdown_timeout(&self) -> Duration {
        self.shutdown_timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = BackgroundWorkerConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_concurrent_tasks, 0);
        assert_eq!(config.task_shutdown_timeout_secs, 5);
        assert_eq!(config.cleanup_interval_secs, 0);
    }

    #[test]
    fn test_config_serde_empty_object() {
        let config: BackgroundWorkerConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.enabled);
        assert_eq!(config.max_concurrent_tasks, 0);
        assert_eq!(config.task_shutdown_timeout_secs, 5);
        assert_eq!(config.cleanup_interval_secs, 0);
    }

    #[test]
    fn test_config_serde_partial() {
        let config: BackgroundWorkerConfig =
            serde_json::from_str(r#"{"enabled": true, "max_concurrent_tasks": 10}"#).unwrap();
        assert!(config.enabled);
        assert_eq!(config.max_concurrent_tasks, 10);
        assert_eq!(config.task_shutdown_timeout_secs, 5);
        assert_eq!(config.cleanup_interval_secs, 0);
    }

    #[tokio::test]
    async fn test_semaphore_concurrency_limiting() {
        let mut runtime = ActonApp::launch_async().await;
        let config = BackgroundWorkerConfig {
            enabled: true,
            max_concurrent_tasks: 2,
            task_shutdown_timeout_secs: 5,
            cleanup_interval_secs: 0,
        };
        let worker = BackgroundWorker::spawn(&mut runtime, &config).await.unwrap();

        let (tx, rx) = tokio::sync::watch::channel(false);
        let running_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let max_observed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // Submit 4 tasks with max_concurrent_tasks = 2.
        // submit() blocks on the semaphore (backpressure), so we must
        // spawn submits concurrently — otherwise task 3 blocks forever
        // waiting for a permit held by tasks 1/2.
        for i in 0..4 {
            let rx = rx.clone();
            let running = running_count.clone();
            let max_obs = max_observed.clone();
            let w = worker.clone();
            tokio::spawn(async move {
                w.submit(format!("task-{i}"), move || {
                    let rx = rx;
                    let running = running;
                    let max_obs = max_obs;
                    async move {
                        let current =
                            running.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        max_obs.fetch_max(current, std::sync::atomic::Ordering::SeqCst);

                        // Wait for signal to complete
                        let mut rx = rx;
                        let _ = rx.wait_for(|v| *v).await;

                        running.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(())
                    }
                })
                .await;
            });
        }

        // Give tasks time to start (first 2 acquire permits, next 2 wait)
        tokio::time::sleep(Duration::from_millis(100)).await;

        let max = max_observed.load(std::sync::atomic::Ordering::SeqCst);
        assert!(max <= 2, "Max concurrent tasks was {max}, expected <= 2");

        // Signal tasks to complete
        tx.send(true).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn test_cleanup_finished_tasks() {
        let mut runtime = ActonApp::launch_async().await;
        let config = BackgroundWorkerConfig::default();
        let worker = BackgroundWorker::spawn(&mut runtime, &config).await.unwrap();

        // Submit tasks that complete immediately
        for i in 0..3 {
            worker
                .submit(format!("task-{i}"), || async { Ok(()) })
                .await;
        }

        // Wait for tasks to complete
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(worker.task_count(), 3);

        worker.cleanup_finished_tasks().await;

        assert_eq!(worker.task_count(), 0);

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn test_configurable_shutdown_timeout() {
        let mut runtime = ActonApp::launch_async().await;
        let config = BackgroundWorkerConfig {
            enabled: true,
            max_concurrent_tasks: 0,
            task_shutdown_timeout_secs: 10,
            cleanup_interval_secs: 0,
        };
        let worker = BackgroundWorker::spawn(&mut runtime, &config).await.unwrap();

        assert_eq!(worker.shutdown_timeout(), Duration::from_secs(10));

        runtime.shutdown_all().await.unwrap();
    }
}
