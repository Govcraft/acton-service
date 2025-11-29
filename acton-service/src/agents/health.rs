//! Health monitoring agent for aggregating pool health status
//!
//! This agent subscribes to `PoolHealthUpdate` broadcasts from pool agents
//! and maintains aggregated health state. Health check handlers can query
//! this agent for current health without performing I/O.

use std::collections::HashMap;

use acton_reactive::prelude::*;

use super::messages::{
    AggregatedHealthResponse, ComponentHealth, GetAggregatedHealth, HealthStatus, PoolHealthUpdate,
};

/// State for the health monitor agent
#[derive(Debug, Default)]
pub struct HealthMonitorState {
    /// Health status by component name (database, redis, nats)
    components: HashMap<String, ComponentHealth>,
}

impl HealthMonitorState {
    /// Compute overall health status from all components
    fn is_overall_healthy(&self) -> bool {
        if self.components.is_empty() {
            return true; // No components registered yet
        }

        self.components
            .values()
            .all(|c| c.status == HealthStatus::Healthy)
    }

    /// Get aggregated health response
    fn get_aggregated_health(&self) -> AggregatedHealthResponse {
        AggregatedHealthResponse {
            overall_healthy: self.is_overall_healthy(),
            components: self.components.values().cloned().collect(),
        }
    }
}

/// Agent that monitors and aggregates health status from pool agents
///
/// The `HealthMonitorAgent` subscribes to `PoolHealthUpdate` broadcasts and
/// maintains a cached view of all pool health statuses. This enables fast
/// health check responses without querying each pool individually.
///
/// # Example
///
/// ```rust,ignore
/// use acton_service::agents::prelude::*;
///
/// let runtime = builder.with_agent_runtime();
///
/// // Spawn the health monitor
/// let health_monitor = HealthMonitorAgent::spawn(runtime).await?;
///
/// // Query aggregated health (no I/O - returns cached state)
/// let response = health_monitor
///     .send_and_wait::<GetAggregatedHealth, AggregatedHealthResponse>()
///     .await?;
///
/// if !response.overall_healthy {
///     // Handle unhealthy state
/// }
/// ```
pub struct HealthMonitorAgent;

impl HealthMonitorAgent {
    /// Spawn a new health monitor agent
    ///
    /// The agent will immediately begin listening for `PoolHealthUpdate` broadcasts.
    /// Pool agents should be spawned after this agent so their health updates are captured.
    pub async fn spawn(runtime: &mut AgentRuntime) -> anyhow::Result<AgentHandle> {
        let mut agent = runtime.new_agent::<HealthMonitorState>();

        // Handle pool health updates - update cached component state
        agent.mutate_on::<PoolHealthUpdate>(|agent, envelope| {
            let update = envelope.message();
            let component_health = ComponentHealth {
                name: update.pool_type.clone(),
                status: update.status.clone(),
                message: update.message.clone(),
            };

            agent
                .model
                .components
                .insert(update.pool_type.clone(), component_health);

            tracing::debug!(
                pool_type = %update.pool_type,
                status = ?update.status,
                "Health monitor received pool health update"
            );

            AgentReply::immediate()
        });

        // Handle aggregated health queries - read-only, returns cached state
        agent.act_on::<GetAggregatedHealth>(|agent, envelope| {
            let health = agent.model.get_aggregated_health();
            let reply_envelope = envelope.reply_envelope();

            AgentReply::from_async(async move {
                reply_envelope.send(health).await;
            })
        });

        // Log startup
        agent.after_start(|_agent| {
            tracing::info!("Health monitor agent started, listening for pool health updates");
            AgentReply::immediate()
        });

        // Log shutdown
        agent.before_stop(|agent| {
            let component_count = agent.model.components.len();
            tracing::info!(
                component_count,
                "Health monitor agent stopping, tracked {} components",
                component_count
            );
            AgentReply::immediate()
        });

        // Subscribe to pool health updates BEFORE starting
        agent.handle().subscribe::<PoolHealthUpdate>().await;
        agent.handle().subscribe::<GetAggregatedHealth>().await;

        let handle = agent.start().await;
        Ok(handle)
    }
}
