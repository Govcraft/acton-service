//! Key rotation agent (acton-reactive actor)
//!
//! The `KeyRotationAgent` periodically checks whether the active signing key
//! needs rotation and retires expired draining keys. It follows the same
//! actor pattern as [`AuditAgent`](crate::audit::agent::AuditAgent).
//!
//! # Timer-Based Rotation
//!
//! On startup, the agent spawns a background tokio task with a
//! `tokio::time::interval` that fires every `check_interval_secs`. Each tick
//! runs the same logic as a manual `CheckRotation` message.
//!
//! # Audit Integration
//!
//! When the `audit` feature is active and an `AuditLogger` is provided,
//! rotation and retirement events are emitted to the audit trail.

use acton_reactive::prelude::*;

use super::config::KeyRotationConfig;
use super::manager::KeyManager;

#[cfg(feature = "audit")]
use crate::audit::event::{AuditEvent, AuditEventKind, AuditSeverity};
#[cfg(feature = "audit")]
use crate::audit::logger::AuditLogger;

// ---------------------------------------------------------------------------
// Agent state
// ---------------------------------------------------------------------------

/// State held by the key rotation agent actor
#[derive(Default)]
pub struct KeyRotationAgentState {
    /// Key manager with in-memory cache and storage backend
    pub key_manager: Option<KeyManager>,
    /// Key rotation configuration
    pub config: Option<KeyRotationConfig>,
    /// Audit logger for emitting rotation events (requires `audit` feature)
    #[cfg(feature = "audit")]
    pub audit_logger: Option<AuditLogger>,
}

impl std::fmt::Debug for KeyRotationAgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyRotationAgentState")
            .field("key_manager", &self.key_manager.is_some())
            .field("config", &self.config.is_some())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Manually trigger a rotation check (same logic as the periodic timer tick)
#[derive(Clone, Debug)]
pub struct CheckRotation;

/// Force an immediate key rotation regardless of key age
#[derive(Clone, Debug)]
pub struct ForceRotation;

// ---------------------------------------------------------------------------
// KeyRotationAgent
// ---------------------------------------------------------------------------

/// Acton-reactive actor that manages automated signing key rotation
///
/// Spawned during service startup when `KeyRotationConfig::enabled` is true.
/// Periodically checks whether the active key needs rotation, retires expired
/// draining keys, and optionally emits audit events for each lifecycle change.
///
/// # Messages
///
/// - [`CheckRotation`]: Manually trigger the same check logic as the periodic timer.
/// - [`ForceRotation`]: Force an immediate rotation regardless of key age.
pub struct KeyRotationAgent;

impl KeyRotationAgent {
    /// Spawn the key rotation agent
    ///
    /// Returns an `ActorHandle` that can be used to send `CheckRotation` or
    /// `ForceRotation` messages manually.
    pub async fn spawn(
        runtime: &mut ActorRuntime,
        key_manager: KeyManager,
        config: KeyRotationConfig,
        #[cfg(feature = "audit")] audit_logger: Option<AuditLogger>,
    ) -> anyhow::Result<ActorHandle> {
        let mut agent = runtime.new_actor::<KeyRotationAgentState>();

        // Populate state
        agent.model.config = Some(config.clone());
        agent.model.key_manager = Some(key_manager.clone());
        #[cfg(feature = "audit")]
        {
            agent.model.audit_logger = audit_logger.clone();
        }

        // ------------------------------------------------------------------
        // Handler: CheckRotation (manual trigger, same as timer tick)
        // ------------------------------------------------------------------
        agent.mutate_on::<CheckRotation>(|actor, _envelope| {
            let km = actor.model.key_manager.clone();
            let cfg = actor.model.config.clone();
            #[cfg(feature = "audit")]
            let audit = actor.model.audit_logger.clone();

            tokio::spawn(async move {
                if let (Some(km), Some(cfg)) = (km, cfg) {
                    perform_rotation_check(
                        &km,
                        &cfg,
                        false,
                        #[cfg(feature = "audit")]
                        audit.as_ref(),
                    )
                    .await;
                }
            });

            Reply::ready()
        });

        // ------------------------------------------------------------------
        // Handler: ForceRotation (force regardless of key age)
        // ------------------------------------------------------------------
        agent.mutate_on::<ForceRotation>(|actor, _envelope| {
            let km = actor.model.key_manager.clone();
            let cfg = actor.model.config.clone();
            #[cfg(feature = "audit")]
            let audit = actor.model.audit_logger.clone();

            tokio::spawn(async move {
                if let (Some(km), Some(cfg)) = (km, cfg) {
                    perform_rotation_check(
                        &km,
                        &cfg,
                        true,
                        #[cfg(feature = "audit")]
                        audit.as_ref(),
                    )
                    .await;
                }
            });

            Reply::ready()
        });

        // ------------------------------------------------------------------
        // after_start: spawn periodic timer task
        // ------------------------------------------------------------------
        let start_km = key_manager.clone();
        let start_config = config.clone();
        #[cfg(feature = "audit")]
        let start_audit = audit_logger.clone();

        agent.after_start(move |_agent| {
            let km = start_km.clone();
            let cfg = start_config.clone();
            #[cfg(feature = "audit")]
            let audit = start_audit.clone();

            tokio::spawn(async move {
                let period = std::time::Duration::from_secs(cfg.check_interval_secs);
                let mut interval = tokio::time::interval(period);
                // Skip the first immediate tick
                interval.tick().await;

                loop {
                    interval.tick().await;
                    tracing::debug!("Key rotation agent: periodic check");
                    perform_rotation_check(
                        &km,
                        &cfg,
                        false,
                        #[cfg(feature = "audit")]
                        audit.as_ref(),
                    )
                    .await;
                }
            });

            Reply::ready()
        });

        let handle = agent.start().await;
        Ok(handle)
    }
}

// ---------------------------------------------------------------------------
// Rotation check logic (shared between timer, CheckRotation, ForceRotation)
// ---------------------------------------------------------------------------

/// Perform a single rotation check cycle
///
/// 1. Retire expired draining keys
/// 2. Check if the active key needs rotation (or force rotation)
/// 3. Emit audit events if an audit logger is available
async fn perform_rotation_check(
    key_manager: &KeyManager,
    config: &KeyRotationConfig,
    force: bool,
    #[cfg(feature = "audit")] audit_logger: Option<&AuditLogger>,
) {
    // Step 1: Retire expired draining keys
    match key_manager.retire_expired().await {
        Ok(retired_count) => {
            if retired_count > 0 {
                tracing::info!(
                    count = retired_count,
                    service = %key_manager.service_name(),
                    "retired expired draining keys"
                );

                #[cfg(feature = "audit")]
                if let Some(logger) = audit_logger {
                    let event = AuditEvent::new(
                        AuditEventKind::AuthKeyRetired,
                        AuditSeverity::Informational,
                        logger.service_name().to_string(),
                    )
                    .with_metadata(serde_json::json!({
                        "retired_count": retired_count,
                        "service": key_manager.service_name(),
                    }));
                    logger.log(event).await;
                }
            }
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                service = %key_manager.service_name(),
                "failed to retire expired draining keys"
            );
        }
    }

    // Step 2: Check the active signing key
    let needs_rotation = if force {
        true
    } else {
        match key_manager.get_signing_key().await {
            Ok(Some(active_key)) => {
                // Check if the active key has exceeded its rotation period.
                // We look up the full metadata from storage via the kid to get
                // the activated_at timestamp (CachedKey doesn't store it).
                match key_manager.storage().get_key_by_kid(&active_key.kid).await {
                    Ok(Some(meta)) => {
                        if let Some(activated_at) = meta.activated_at {
                            let age_secs = (chrono::Utc::now() - activated_at).num_seconds();
                            age_secs >= 0 && (age_secs as u64) >= config.rotation_period_secs
                        } else {
                            // No activation time recorded -- treat as needing rotation
                            true
                        }
                    }
                    Ok(None) => {
                        // Key disappeared from storage -- needs rotation
                        true
                    }
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            kid = %active_key.kid,
                            "failed to fetch key metadata for age check"
                        );
                        false
                    }
                }
            }
            Ok(None) => {
                // No active key -- bootstrap
                tracing::info!(
                    service = %key_manager.service_name(),
                    "no active signing key found, bootstrapping"
                );
                true
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    service = %key_manager.service_name(),
                    "failed to get active signing key"
                );

                #[cfg(feature = "audit")]
                if let Some(logger) = audit_logger {
                    let event = AuditEvent::new(
                        AuditEventKind::AuthKeyRotationFailed,
                        AuditSeverity::Error,
                        logger.service_name().to_string(),
                    )
                    .with_metadata(serde_json::json!({
                        "error": e.to_string(),
                        "phase": "get_signing_key",
                        "service": key_manager.service_name(),
                    }));
                    logger.log(event).await;
                }

                false
            }
        }
    };

    // Step 3: Rotate if needed
    if needs_rotation {
        // Capture the old key kid for audit metadata
        let old_kid = match key_manager.get_signing_key().await {
            Ok(Some(k)) => Some(k.kid.clone()),
            _ => None,
        };

        match key_manager.rotate().await {
            Ok(new_key) => {
                tracing::info!(
                    new_kid = %new_key.kid,
                    old_kid = ?old_kid,
                    service = %key_manager.service_name(),
                    forced = force,
                    "signing key rotated"
                );

                #[cfg(feature = "audit")]
                if let Some(logger) = audit_logger {
                    let event = AuditEvent::new(
                        AuditEventKind::AuthKeyRotated,
                        AuditSeverity::Notice,
                        logger.service_name().to_string(),
                    )
                    .with_metadata(serde_json::json!({
                        "new_kid": new_key.kid,
                        "old_kid": old_kid,
                        "forced": force,
                        "service": key_manager.service_name(),
                    }));
                    logger.log(event).await;
                }
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    service = %key_manager.service_name(),
                    "key rotation failed"
                );

                #[cfg(feature = "audit")]
                if let Some(logger) = audit_logger {
                    let event = AuditEvent::new(
                        AuditEventKind::AuthKeyRotationFailed,
                        AuditSeverity::Error,
                        logger.service_name().to_string(),
                    )
                    .with_metadata(serde_json::json!({
                        "error": e.to_string(),
                        "phase": "rotate",
                        "service": key_manager.service_name(),
                    }));
                    logger.log(event).await;
                }
            }
        }
    }
}
