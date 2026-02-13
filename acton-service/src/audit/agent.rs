//! Audit agent (acton-reactive actor)
//!
//! The `AuditAgent` owns the BLAKE3 hash chain state and processes events
//! sequentially, guaranteeing correct chain ordering. It persists events to
//! the configured storage backend and dispatches to SIEM exporters.

use acton_reactive::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;

use super::chain::AuditChain;
use super::config::AuditConfig;
use super::event::AuditEvent;
use super::storage::AuditStorage;
use super::syslog::SyslogSender;

/// State held by the audit agent actor
#[derive(Default)]
pub struct AuditAgentState {
    /// BLAKE3 hash chain state
    pub chain: Option<AuditChain>,
    /// Persistent storage backend (if a DB feature is active)
    pub storage: Option<Arc<dyn AuditStorage>>,
    /// Syslog sender (if configured)
    pub syslog: Option<SyslogSender>,
    /// Audit configuration
    pub config: Option<AuditConfig>,
}

// Manual Debug impl since AuditChain and dyn AuditStorage don't impl Debug
impl std::fmt::Debug for AuditAgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditAgentState")
            .field("chain", &self.chain.is_some())
            .field("storage", &self.storage.is_some())
            .field("syslog", &self.syslog.is_some())
            .field("config", &self.config.is_some())
            .finish()
    }
}

/// Internal message: chain state loaded from storage
///
/// Sent by the spawned task in `after_start` back to the agent.
/// Must be Clone + Debug for ActonMessage.
#[derive(Clone, Debug)]
struct ChainLoaded {
    previous_hash: Option<String>,
    sequence: u64,
    service_name: String,
}

/// Internal message: triggers periodic retention cleanup
#[derive(Clone, Debug)]
struct CleanupTrigger;

/// Batch size for archival + purge operations
const CLEANUP_BATCH_SIZE: usize = 10_000;

/// Audit agent that manages the immutable audit trail
///
/// Follows the same actor pattern as `DatabasePoolAgent`, `RedisPoolAgent`, etc.
/// Spawned during `ServiceBuilder::build()` alongside other pool agents.
pub struct AuditAgent;

impl AuditAgent {
    /// Spawn the audit agent
    ///
    /// The agent loads chain state from storage (if available) in `after_start`,
    /// then processes `AuditEvent` messages sequentially.
    pub async fn spawn(
        runtime: &mut ActorRuntime,
        config: AuditConfig,
        storage: Option<Arc<dyn AuditStorage>>,
        service_name: String,
    ) -> anyhow::Result<ActorHandle> {
        let mut agent = runtime.new_actor::<AuditAgentState>();

        // Set up syslog sender if configured
        let syslog = if config.syslog.transport != "none" {
            match SyslogSender::new(&config.syslog) {
                Ok(sender) => Some(sender),
                Err(e) => {
                    tracing::warn!("Failed to initialize syslog sender: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Save retention config before moving config into agent model
        let retention_days = config.retention_days;
        let cleanup_interval_hours = config.cleanup_interval_hours;

        agent.model.config = Some(config);
        agent.model.storage = storage;
        agent.model.syslog = syslog;

        // Clone values needed for after_start closure
        let storage_for_start = agent.model.storage.clone();
        let service_name_for_start = service_name.clone();

        // Handle chain initialization (sent from after_start task)
        agent.mutate_on::<ChainLoaded>(|agent, envelope| {
            let msg = envelope.message().clone();
            let chain = if let Some(ref hash) = msg.previous_hash {
                AuditChain::resume(msg.service_name, hash.clone(), msg.sequence)
            } else {
                AuditChain::new(msg.service_name)
            };
            agent.model.chain = Some(chain);
            tracing::info!("Audit chain initialized at sequence {}", msg.sequence);
            Reply::ready()
        });

        // Handle incoming audit events
        agent.mutate_on::<AuditEvent>(|agent, envelope| {
            let event = envelope.message().clone();

            // Seal the event with the hash chain
            let sealed_event = if let Some(ref mut chain) = agent.model.chain {
                chain.seal(event)
            } else {
                tracing::warn!("Audit chain not initialized, dropping event");
                return Reply::ready();
            };

            // Clone what we need for the async work
            let storage = agent.model.storage.clone();
            let syslog = agent.model.syslog.clone();

            // Spawn async persistence/export work (not Sync-required)
            tokio::spawn(async move {
                // Persist to storage
                if let Some(ref store) = storage {
                    if let Err(e) = store.append(&sealed_event).await {
                        tracing::error!("Failed to persist audit event: {}", e);
                    }
                }

                // Send to syslog
                if let Some(ref sender) = syslog {
                    if let Err(e) = sender.send(&sealed_event).await {
                        tracing::warn!("Failed to send audit event to syslog: {}", e);
                    }
                }

                // OTLP export (when observability feature is active)
                #[cfg(feature = "observability")]
                {
                    super::otlp::emit_audit_log(&sealed_event);
                }
            });

            Reply::ready()
        });

        // Handle retention cleanup triggers
        agent.mutate_on::<CleanupTrigger>(|agent, _envelope| {
            let config = agent.model.config.clone();
            let storage = agent.model.storage.clone();

            tokio::spawn(async move {
                if let (Some(config), Some(storage)) = (config, storage) {
                    if let Err(e) = run_cleanup(&config, storage.as_ref()).await {
                        tracing::error!("Audit retention cleanup failed: {}", e);
                    }
                }
            });

            Reply::ready()
        });

        // Load chain state from storage on startup
        agent.after_start(move |agent| {
            let storage = storage_for_start.clone();
            let service_name = service_name_for_start.clone();
            let self_handle = agent.handle().clone();

            tokio::spawn(async move {
                let (previous_hash, sequence) = if let Some(ref store) = storage {
                    match store.latest().await {
                        Ok(Some(event)) => {
                            tracing::info!(
                                "Resuming audit chain at sequence {} for {}",
                                event.sequence,
                                service_name
                            );
                            (event.hash, event.sequence)
                        }
                        Ok(None) => {
                            tracing::info!("Starting new audit chain for {}", service_name);
                            (None, 0)
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to load audit chain state: {}. Starting fresh.",
                                e
                            );
                            (None, 0)
                        }
                    }
                } else {
                    tracing::info!(
                        "No audit storage configured, starting in-memory chain for {}",
                        service_name
                    );
                    (None, 0)
                };

                self_handle
                    .send(ChainLoaded {
                        previous_hash,
                        sequence,
                        service_name,
                    })
                    .await;
            });

            Reply::ready()
        });

        let handle = agent.start().await;

        // Start periodic cleanup if retention is configured
        if retention_days.is_some() {
            let cleanup_handle = handle.clone();
            let interval_hours = cleanup_interval_hours;
            tokio::spawn(async move {
                let period = std::time::Duration::from_secs(interval_hours as u64 * 3600);
                let mut interval = tokio::time::interval(period);
                // Skip the first immediate tick
                interval.tick().await;
                loop {
                    interval.tick().await;
                    tracing::debug!("Triggering audit retention cleanup");
                    cleanup_handle.send(CleanupTrigger).await;
                }
            });
        }

        Ok(handle)
    }
}

/// Run a single retention cleanup cycle
async fn run_cleanup(
    config: &AuditConfig,
    storage: &dyn AuditStorage,
) -> Result<(), crate::error::Error> {
    let retention_days = match config.retention_days {
        Some(days) => days,
        None => return Ok(()),
    };

    let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
    let archive_dir = config.archive_path.as_ref().map(PathBuf::from);
    let mut total_purged: u64 = 0;

    loop {
        let events = storage.query_before(cutoff, CLEANUP_BATCH_SIZE).await?;
        if events.is_empty() {
            break;
        }

        let batch_count = events.len();

        // Archive if configured — abort this cycle if archival fails
        if let Some(ref dir) = archive_dir {
            super::archive::archive_events(&events, dir).await?;
        }

        // Purge the batch
        let purged = storage.purge_before(cutoff).await?;
        total_purged += purged;

        tracing::info!(
            "Audit cleanup: purged {} events (batch had {})",
            purged,
            batch_count
        );

        // If we got fewer than the batch size, we're done
        if batch_count < CLEANUP_BATCH_SIZE {
            break;
        }
    }

    if total_purged > 0 {
        tracing::info!(
            "Audit retention cleanup complete: purged {} total events older than {} days",
            total_purged,
            retention_days
        );
    }

    Ok(())
}
