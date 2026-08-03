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
//! The agent owns the task registry outright: it lives in
//! [`BackgroundWorkerState`] as a plain `HashMap`, reached only from handlers,
//! which the runtime already serializes. Nothing here is behind a lock, because
//! nothing else writes it.
//!
//! Spawned work reports its outcome back as a [`TaskCompleted`] message rather
//! than writing a shared status cell. Registration is sent before the task is
//! spawned, and mailboxes are FIFO, so the agent can never see a completion for
//! a task it has not yet registered.
//!
//! The one shared handle is a [`TaskTracker`], which exists so `before_stop` can
//! await in-flight work. That is a task registry, not decision state: the agent
//! never branches on it, and draining cannot be done through the mailbox because
//! the message loop is not running while `before_stop` is.
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
//! worker.submit("my-task", || async move {
//!     // Do background work
//!     tokio::time::sleep(Duration::from_secs(10)).await;
//!     Ok(())
//! }).await;
//!
//! // Check task status
//! let status = worker.task_status("my-task").await?;
//!
//! // Cancel a specific task and wait for it to stop
//! worker.cancel("my-task").await?;
//!
//! // Graceful shutdown cancels all remaining tasks
//! runtime.shutdown_all().await?;
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use acton_reactive::prelude::{Reply, *};
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::messages::{
    CancelTask, CleanupFinishedTasks, GetAllTaskStatuses, GetTaskStatus, RegisterTask,
    TaskCompleted, TaskStatusResponse, WaitForTask,
};

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

impl TaskStatus {
    /// Whether the task has reached a state it will not leave.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed(_) | Self::Cancelled)
    }
}

/// One tracked task, owned exclusively by the agent.
///
/// Plain fields rather than `Arc<Mutex<..>>`: the agent's message loop is the
/// mutual exclusion, so a lock here would guard against a writer that does not
/// exist.
#[derive(Debug)]
struct TaskRecord {
    /// Cancels this task specifically
    cancellation_token: CancellationToken,
    /// Current status, updated only by the `TaskCompleted` handler
    status: TaskStatus,
}

/// State for the background worker agent
#[derive(Debug, Default)]
pub struct BackgroundWorkerState {
    /// Every task the agent knows about, keyed by task ID
    tasks: HashMap<String, TaskRecord>,
    /// Callers blocked in `WaitForTask`, keyed by the task they are waiting on
    waiters: HashMap<String, Vec<OutboundEnvelope>>,
    /// Root cancellation token; cancelling it cancels every task's child token
    root_token: Option<CancellationToken>,
    /// Tracks spawned tasks so shutdown can await them
    tracker: Option<TaskTracker>,
    /// How long shutdown waits for in-flight tasks
    shutdown_timeout: Duration,
}

/// Service wrapper for the background worker agent
///
/// Provides a clean API for submitting and managing background tasks. Every
/// query goes through the agent, which is the only owner of task state.
#[derive(Clone)]
pub struct BackgroundWorker {
    /// Handle for sending messages to the agent
    agent_handle: ActorHandle,
    /// Root cancellation token for creating child tokens
    root_token: CancellationToken,
    /// Optional semaphore for concurrency limiting
    semaphore: Option<Arc<Semaphore>>,
    /// Tracks spawned tasks so shutdown can await them
    tracker: TaskTracker,
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
        let root_token = CancellationToken::new();
        let tracker = TaskTracker::new();
        let shutdown_timeout = Duration::from_secs(config.task_shutdown_timeout_secs);

        let semaphore = if config.max_concurrent_tasks > 0 {
            Some(Arc::new(Semaphore::new(config.max_concurrent_tasks)))
        } else {
            None
        };

        let mut agent = runtime.new_actor::<BackgroundWorkerState>();

        agent.model.root_token = Some(root_token.clone());
        agent.model.tracker = Some(tracker.clone());
        agent.model.shutdown_timeout = shutdown_timeout;

        // A task becomes visible to the agent here, before it is spawned, so a
        // completion can never arrive for an unknown ID.
        agent.mutate_on::<RegisterTask>(|agent, envelope| {
            let msg = envelope.message();
            agent.model.tasks.insert(
                msg.task_id.clone(),
                TaskRecord {
                    cancellation_token: msg.cancellation_token.clone(),
                    status: TaskStatus::Running,
                },
            );
            tracing::info!(task_id = %msg.task_id, "Background task submitted");
            Reply::ready()
        });

        // The spawned task reports its own outcome; the agent is the only
        // writer of status.
        agent.mutate_on::<TaskCompleted>(|agent, envelope| {
            let msg = envelope.message();
            if let Some(record) = agent.model.tasks.get_mut(&msg.task_id) {
                record.status = msg.status.clone();
            }

            tracing::debug!(
                task_id = %msg.task_id,
                status = ?msg.status,
                "Background task reached a terminal state"
            );

            // Release anyone blocked on this task finishing.
            let waiters = agent.model.waiters.remove(&msg.task_id).unwrap_or_default();
            let response = TaskStatusResponse {
                task_id: msg.task_id.clone(),
                status: msg.status.clone(),
            };

            Reply::pending(async move {
                for waiter in waiters {
                    waiter.send(Some(response.clone())).await;
                }
            })
        });

        // Cancellation is a request, not a wait: the task notices its token and
        // reports back through `TaskCompleted`. Callers who want to wait use
        // `WaitForTask`, which is what `BackgroundWorker::cancel` does.
        agent.mutate_on::<CancelTask>(|agent, envelope| {
            let task_id = &envelope.message().task_id;
            if let Some(record) = agent.model.tasks.get(task_id) {
                record.cancellation_token.cancel();
                tracing::info!(task_id = %task_id, "Task cancellation requested");
            } else {
                tracing::warn!(task_id = %task_id, "Task not found for cancellation");
            }
            Reply::ready()
        });

        // Answers now if the task is already finished or unknown, otherwise
        // parks the reply envelope until `TaskCompleted` arrives. Holding the
        // envelope is what lets a caller await completion without polling.
        agent.mutate_on::<WaitForTask>(|agent, envelope| {
            let task_id = envelope.message().task_id.clone();
            let reply = envelope.reply_envelope();

            let settled = match agent.model.tasks.get(&task_id) {
                None => Some(None),
                Some(record) if record.status.is_terminal() => Some(Some(TaskStatusResponse {
                    task_id: task_id.clone(),
                    status: record.status.clone(),
                })),
                Some(_) => None,
            };

            match settled {
                Some(response) => Reply::pending(async move {
                    reply.send(response).await;
                }),
                None => {
                    agent.model.waiters.entry(task_id).or_default().push(reply);
                    Reply::ready()
                }
            }
        });

        agent.act_on::<GetTaskStatus>(|agent, envelope| {
            let task_id = envelope.message().task_id.clone();
            let reply = envelope.reply_envelope();

            let response = agent
                .model
                .tasks
                .get(&task_id)
                .map(|record| TaskStatusResponse {
                    task_id,
                    status: record.status.clone(),
                });

            Reply::pending(async move {
                reply.send(response).await;
            })
        });

        agent.act_on::<GetAllTaskStatuses>(|agent, envelope| {
            let reply = envelope.reply_envelope();
            let statuses: Vec<TaskStatusResponse> = agent
                .model
                .tasks
                .iter()
                .map(|(task_id, record)| TaskStatusResponse {
                    task_id: task_id.clone(),
                    status: record.status.clone(),
                })
                .collect();

            Reply::pending(async move {
                reply.send(statuses).await;
            })
        });

        // Drop finished tasks so the registry does not grow without bound.
        agent.mutate_on::<CleanupFinishedTasks>(|agent, envelope| {
            let before = agent.model.tasks.len();
            agent
                .model
                .tasks
                .retain(|_, record| !record.status.is_terminal());
            let removed = before - agent.model.tasks.len();

            let reply = envelope.reply_envelope();
            Reply::pending(async move {
                reply.send(removed).await;
            })
        });

        // Graceful shutdown - cancel all tasks, then wait for them
        agent.before_stop(|agent| {
            let root_token = agent.model.root_token.clone();
            let tracker = agent.model.tracker.clone();
            let timeout = agent.model.shutdown_timeout;
            let task_count = agent.model.tasks.len();

            Reply::pending(async move {
                let Some(tracker) = tracker else {
                    return;
                };

                if task_count == 0 && tracker.is_empty() {
                    tracing::info!("BackgroundWorker stopping with no active tasks");
                    return;
                }

                tracing::info!(
                    task_count,
                    "BackgroundWorker stopping, cancelling all tasks..."
                );

                if let Some(token) = root_token {
                    token.cancel();
                }

                // Closing refuses new registrations so `wait` can terminate.
                tracker.close();
                if tokio::time::timeout(timeout, tracker.wait()).await.is_err() {
                    tracing::warn!(
                        remaining = tracker.len(),
                        "Background tasks did not stop within the shutdown timeout"
                    );
                } else {
                    tracing::info!("All background tasks stopped");
                }
            })
        });

        agent.after_start(|_agent| {
            tracing::info!("BackgroundWorker agent started");
            Reply::ready()
        });

        let handle = agent.start().await;

        let worker = Self {
            agent_handle: handle,
            root_token,
            semaphore,
            tracker,
            shutdown_timeout,
        };

        // Spawn periodic cleanup task if configured
        if config.cleanup_interval_secs > 0 {
            let cleanup_handle = worker.agent_handle.clone();
            let cleanup_token = worker.root_token.child_token();
            let interval = Duration::from_secs(config.cleanup_interval_secs);
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.tick().await;
                loop {
                    tokio::select! {
                        biased;
                        () = cleanup_token.cancelled() => break,
                        _ = ticker.tick() => {
                            cleanup_handle.send(CleanupFinishedTasks).await;
                            tracing::debug!("Periodic background task cleanup requested");
                        }
                    }
                }
            });
        }

        Ok(worker)
    }

    /// Submit a new background task
    ///
    /// The task is spawned and tracked by the agent. If a concurrency limit is
    /// configured, this method awaits a free slot, providing backpressure to
    /// callers.
    ///
    /// # Arguments
    ///
    /// * `task_id` - Unique identifier for the task
    /// * `work` - Closure producing the future that performs the work
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// worker.submit("cleanup-job", || async {
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

        let cancel_token = self.root_token.child_token();

        // Registered before the task exists. Mailboxes are FIFO, so the agent
        // processes this ahead of the completion the task will send.
        self.agent_handle
            .send(RegisterTask {
                task_id: task_id.clone(),
                cancellation_token: cancel_token.clone(),
            })
            .await;

        let agent_handle = self.agent_handle.clone();
        self.tracker.spawn(async move {
            let _permit = permit;

            let status = tokio::select! {
                biased;

                () = cancel_token.cancelled() => {
                    tracing::debug!(task_id = %task_id, "Task cancelled");
                    TaskStatus::Cancelled
                }
                result = work() => match result {
                    Ok(()) => {
                        tracing::debug!(task_id = %task_id, "Task completed successfully");
                        TaskStatus::Completed
                    }
                    Err(e) => {
                        tracing::warn!(task_id = %task_id, error = %e, "Task failed");
                        TaskStatus::Failed(e.to_string())
                    }
                },
            };

            agent_handle.send(TaskCompleted { task_id, status }).await;
        });
    }

    /// Cancel a task and wait for it to stop
    ///
    /// Returns the task's final status, or `None` if no such task is tracked.
    /// Waiting is bounded by the configured shutdown timeout.
    ///
    /// # Errors
    ///
    /// Returns [`AskError`] if the agent cannot be reached or does not answer
    /// within the timeout.
    pub async fn cancel(&self, task_id: impl Into<String>) -> Result<Option<TaskStatus>, AskError> {
        let task_id = task_id.into();
        self.agent_handle
            .send(CancelTask {
                task_id: task_id.clone(),
            })
            .await;

        let response = self
            .agent_handle
            .ask_with_timeout(WaitForTask { task_id }, self.shutdown_timeout)
            .await?;

        Ok(response.map(|r| r.status))
    }

    /// Wait for a task to reach a terminal state, returning its final status
    ///
    /// Returns `None` if no such task is tracked. Answers immediately for a
    /// task that has already finished.
    ///
    /// # Errors
    ///
    /// Returns [`AskError`] if the agent cannot be reached or does not answer.
    pub async fn wait_for_task(
        &self,
        task_id: impl Into<String>,
    ) -> Result<Option<TaskStatus>, AskError> {
        let response = self
            .agent_handle
            .ask(WaitForTask {
                task_id: task_id.into(),
            })
            .await?;
        Ok(response.map(|r| r.status))
    }

    /// Get the status of a specific task, or `None` if it is not tracked
    ///
    /// # Errors
    ///
    /// Returns [`AskError`] if the agent cannot be reached or does not answer.
    pub async fn task_status(
        &self,
        task_id: impl Into<String>,
    ) -> Result<Option<TaskStatus>, AskError> {
        let response = self
            .agent_handle
            .ask(GetTaskStatus {
                task_id: task_id.into(),
            })
            .await?;
        Ok(response.map(|r| r.status))
    }

    /// Get the status of every tracked task
    ///
    /// # Errors
    ///
    /// Returns [`AskError`] if the agent cannot be reached or does not answer.
    pub async fn all_task_statuses(&self) -> Result<Vec<TaskStatusResponse>, AskError> {
        self.agent_handle.ask(GetAllTaskStatuses).await
    }

    /// Get the count of tracked tasks
    ///
    /// # Errors
    ///
    /// Returns [`AskError`] if the agent cannot be reached or does not answer.
    pub async fn task_count(&self) -> Result<usize, AskError> {
        Ok(self.all_task_statuses().await?.len())
    }

    /// Get the count of tasks that have not yet reached a terminal state
    ///
    /// # Errors
    ///
    /// Returns [`AskError`] if the agent cannot be reached or does not answer.
    pub async fn running_task_count(&self) -> Result<usize, AskError> {
        Ok(self
            .all_task_statuses()
            .await?
            .into_iter()
            .filter(|r| r.status == TaskStatus::Running)
            .count())
    }

    /// Check if a task is tracked
    ///
    /// # Errors
    ///
    /// Returns [`AskError`] if the agent cannot be reached or does not answer.
    pub async fn has_task(&self, task_id: impl Into<String>) -> Result<bool, AskError> {
        Ok(self.task_status(task_id).await?.is_some())
    }

    /// Remove completed, failed and cancelled tasks from tracking
    ///
    /// Returns how many records were dropped.
    ///
    /// # Errors
    ///
    /// Returns [`AskError`] if the agent cannot be reached or does not answer.
    pub async fn cleanup_finished_tasks(&self) -> Result<usize, AskError> {
        self.agent_handle.ask(CleanupFinishedTasks).await
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

    #[test]
    fn terminal_statuses_are_classified() {
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());
        assert!(TaskStatus::Failed("boom".into()).is_terminal());
        assert!(!TaskStatus::Running.is_terminal());
        assert!(!TaskStatus::Pending.is_terminal());
    }

    async fn worker_with(config: BackgroundWorkerConfig) -> (ActorRuntime, BackgroundWorker) {
        let mut runtime = ActonApp::launch_async().await;
        let worker = BackgroundWorker::spawn(&mut runtime, &config)
            .await
            .unwrap();
        (runtime, worker)
    }

    #[tokio::test]
    async fn completed_task_reports_its_own_outcome() {
        let (mut runtime, worker) = worker_with(BackgroundWorkerConfig::default()).await;

        worker.submit("job", || async { Ok(()) }).await;

        // wait_for_task is the barrier: the agent answers it only once the
        // spawned task has reported completion.
        let status = worker.wait_for_task("job").await.unwrap();
        assert_eq!(status, Some(TaskStatus::Completed));

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn failing_task_records_its_error() {
        let (mut runtime, worker) = worker_with(BackgroundWorkerConfig::default()).await;

        worker
            .submit("doomed", || async { Err(anyhow::anyhow!("kaboom")) })
            .await;

        let status = worker.wait_for_task("doomed").await.unwrap();
        assert_eq!(status, Some(TaskStatus::Failed("kaboom".into())));

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn unknown_task_is_reported_as_absent() {
        let (mut runtime, worker) = worker_with(BackgroundWorkerConfig::default()).await;

        assert_eq!(worker.task_status("nope").await.unwrap(), None);
        assert!(!worker.has_task("nope").await.unwrap());

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn cancel_stops_a_running_task_and_waits_for_it() {
        let (mut runtime, worker) = worker_with(BackgroundWorkerConfig::default()).await;

        // A task that never finishes on its own, so only cancellation can end it.
        worker
            .submit("forever", || async {
                std::future::pending::<()>().await;
                Ok(())
            })
            .await;

        let status = worker.cancel("forever").await.unwrap();
        assert_eq!(
            status,
            Some(TaskStatus::Cancelled),
            "cancel should not return until the task has actually stopped"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn cancelling_an_unknown_task_reports_absence() {
        let (mut runtime, worker) = worker_with(BackgroundWorkerConfig::default()).await;

        assert_eq!(worker.cancel("ghost").await.unwrap(), None);

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn cleanup_drops_only_finished_tasks() {
        let (mut runtime, worker) = worker_with(BackgroundWorkerConfig::default()).await;

        for i in 0..3 {
            worker
                .submit(format!("done-{i}"), || async { Ok(()) })
                .await;
        }
        worker
            .submit("still-running", || async {
                std::future::pending::<()>().await;
                Ok(())
            })
            .await;

        for i in 0..3 {
            worker.wait_for_task(format!("done-{i}")).await.unwrap();
        }

        assert_eq!(worker.task_count().await.unwrap(), 4);
        assert_eq!(worker.cleanup_finished_tasks().await.unwrap(), 3);
        assert_eq!(worker.task_count().await.unwrap(), 1);
        assert!(
            worker.has_task("still-running").await.unwrap(),
            "a running task must survive cleanup"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn semaphore_caps_concurrent_tasks() {
        let (mut runtime, worker) = worker_with(BackgroundWorkerConfig {
            enabled: true,
            max_concurrent_tasks: 2,
            task_shutdown_timeout_secs: 5,
            cleanup_interval_secs: 0,
        })
        .await;

        let (release_tx, release_rx) = tokio::sync::watch::channel(false);
        let running = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let max_observed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Semaphore::new(0));

        // submit() blocks on the permit, so the submits must run concurrently
        // or the third would wait forever on a permit held by the first two.
        for i in 0..4 {
            let rx = release_rx.clone();
            let running = running.clone();
            let max_observed = max_observed.clone();
            let started = started.clone();
            let worker = worker.clone();
            tokio::spawn(async move {
                worker
                    .submit(format!("task-{i}"), move || async move {
                        let current = running.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        max_observed.fetch_max(current, std::sync::atomic::Ordering::SeqCst);
                        started.add_permits(1);

                        let mut rx = rx;
                        let _ = rx.wait_for(|v| *v).await;

                        running.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(())
                    })
                    .await;
            });
        }

        // Wait until the cap's worth of tasks are actually inside their bodies.
        // This is the barrier that makes the assertion meaningful rather than
        // merely early: two are running and the rest cannot start.
        let _ = started.acquire_many(2).await.unwrap();
        assert_eq!(
            max_observed.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "at most max_concurrent_tasks should run at once"
        );

        release_tx.send(true).unwrap();
        for i in 0..4 {
            worker.wait_for_task(format!("task-{i}")).await.unwrap();
        }
        assert_eq!(
            max_observed.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "the cap should hold for the whole run, not just at the start"
        );

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn test_configurable_shutdown_timeout() {
        let (mut runtime, worker) = worker_with(BackgroundWorkerConfig {
            enabled: true,
            max_concurrent_tasks: 0,
            task_shutdown_timeout_secs: 10,
            cleanup_interval_secs: 0,
        })
        .await;

        assert_eq!(worker.shutdown_timeout(), Duration::from_secs(10));

        runtime.shutdown_all().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_cancels_tasks_that_are_still_running() {
        let (mut runtime, worker) = worker_with(BackgroundWorkerConfig {
            enabled: true,
            max_concurrent_tasks: 0,
            task_shutdown_timeout_secs: 5,
            cleanup_interval_secs: 0,
        })
        .await;

        let observed_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = observed_cancel.clone();

        worker
            .submit("long-runner", move || async move {
                std::future::pending::<()>().await;
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .await;

        // shutdown_all runs before_stop, which cancels the root token and then
        // waits on the tracker. If cancellation did not reach the task this
        // would block until the 5s timeout instead of returning promptly.
        runtime.shutdown_all().await.unwrap();

        assert!(
            !observed_cancel.load(std::sync::atomic::Ordering::SeqCst),
            "the task body should have been cancelled, not run to completion"
        );
    }
}
