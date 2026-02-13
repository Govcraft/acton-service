//! Audit agent (acton-reactive actor)
//!
//! The `AuditAgent` owns the BLAKE3 hash chain state and processes events
//! sequentially, guaranteeing correct chain ordering. It persists events to
//! the configured storage backend and dispatches to SIEM exporters.

use acton_reactive::prelude::*;
use std::sync::Arc;

use super::alert::AuditAlertHook;
use super::alert_webhook::WebhookAlertHook;
use super::chain::AuditChain;
use super::config::AuditConfig;
use super::event::AuditEvent;
use super::failure_tracker::FailureTracker;
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
    /// Storage failure tracker (if alerts are configured)
    pub(crate) failure_tracker: Option<Arc<FailureTracker>>,
}

// Manual Debug impl since AuditChain and dyn AuditStorage don't impl Debug
impl std::fmt::Debug for AuditAgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditAgentState")
            .field("chain", &self.chain.is_some())
            .field("storage", &self.storage.is_some())
            .field("syslog", &self.syslog.is_some())
            .field("config", &self.config.is_some())
            .field("failure_tracker", &self.failure_tracker.is_some())
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

        // Set up failure tracker if alert hooks are configured
        let failure_tracker = if let Some(ref alert_config) = config.alerts {
            if alert_config.enabled {
                let mut hooks: Vec<Arc<dyn AuditAlertHook>> = Vec::new();
                for wh in &alert_config.webhooks {
                    hooks.push(Arc::new(WebhookAlertHook::new(
                        wh.url.clone(),
                        std::time::Duration::from_secs(wh.timeout_secs),
                        wh.headers.clone(),
                    )));
                }
                Some(Arc::new(FailureTracker::new(
                    hooks,
                    alert_config.threshold_secs,
                    alert_config.cooldown_secs,
                    alert_config.notify_recovery,
                    service_name.clone(),
                )))
            } else {
                None
            }
        } else {
            None
        };

        agent.model.config = Some(config);
        agent.model.storage = storage;
        agent.model.syslog = syslog;
        agent.model.failure_tracker = failure_tracker;

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
            let tracker = agent.model.failure_tracker.clone();

            // Spawn async persistence/export work (not Sync-required)
            tokio::spawn(async move {
                // Persist to storage
                if let Some(ref store) = storage {
                    match store.append(&sealed_event).await {
                        Ok(()) => {
                            if let Some(ref t) = tracker {
                                t.record_success();
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to persist audit event: {}", e);
                            if let Some(ref t) = tracker {
                                t.record_failure(&e.to_string());
                            }
                        }
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
        Ok(handle)
    }
}
