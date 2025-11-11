//! NATS JetStream client management

#[cfg(feature = "events")]
use async_nats::Client;
use std::time::Duration;

use crate::{config::NatsConfig, error::{Error, Result}};

/// Create a NATS client with retry logic
///
/// This is an internal function used by AppStateBuilder.
/// It will retry connection attempts based on the configuration.
#[cfg(feature = "events")]
pub(crate) async fn create_client(config: &NatsConfig) -> Result<Client> {
    create_client_with_retries(config, config.max_retries).await
}

/// Create a NATS client with configurable retries
///
/// Uses exponential backoff strategy for retries
#[cfg(feature = "events")]
async fn create_client_with_retries(config: &NatsConfig, max_retries: u32) -> Result<Client> {
    let mut attempt = 0;
    let base_delay = Duration::from_secs(config.retry_delay_secs);

    loop {
        match try_create_client(config).await {
            Ok(client) => {
                if attempt > 0 {
                    tracing::info!(
                        "NATS connection established after {} attempt(s)",
                        attempt + 1
                    );
                } else {
                    tracing::info!("NATS client connected to {}", config.url);
                }
                return Ok(client);
            }
            Err(e) => {
                attempt += 1;

                if attempt > max_retries {
                    tracing::error!(
                        "Failed to connect to NATS after {} attempts: {}",
                        max_retries + 1,
                        e
                    );
                    return Err(e);
                }

                // Calculate exponential backoff
                let delay_multiplier = 2_u32.pow(attempt.saturating_sub(1));
                let delay = base_delay * delay_multiplier;

                tracing::warn!(
                    "NATS connection attempt {} failed: {}. Retrying in {:?}...",
                    attempt,
                    e,
                    delay
                );

                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Attempt to create a NATS client (single try)
#[cfg(feature = "events")]
async fn try_create_client(config: &NatsConfig) -> Result<Client> {
    let mut opts = async_nats::ConnectOptions::new();

    if let Some(name) = &config.name {
        opts = opts.name(name);
    }

    opts = opts.max_reconnects(Some(config.max_reconnects));

    let client = opts
        .connect(&config.url)
        .await
        .map_err(|e| {
            Error::Nats(format!(
                "Failed to connect to NATS server at '{}'\n\n\
                Troubleshooting:\n\
                1. Verify NATS server is running: nats-server --version\n\
                2. Check NATS server status: nats-server --signal status\n\
                3. Verify network connectivity: telnet <host> <port>\n\
                4. Check authentication if enabled (token, credentials, NKeys)\n\
                5. Review NATS server logs for connection errors\n\
                6. Verify firewall rules allow traffic on NATS ports\n\
                7. Check URL format: nats://host:port or nats://user:pass@host:port\n\n\
                Max reconnects: {}\n\
                Client name: {}\n\
                Error: {}",
                config.url,
                config.max_reconnects,
                config.name.as_deref().unwrap_or("<none>"),
                e
            ))
        })?;

    Ok(client)
}

/// Publish an event to NATS
#[cfg(feature = "events")]
pub async fn publish_event(
    client: &Client,
    subject: &str,
    payload: Vec<u8>,
) -> Result<()> {
    client
        .publish(subject.to_string(), payload.into())
        .await
        .map_err(|e| Error::Nats(format!("Failed to publish to {}: {}", subject, e)))?;

    Ok(())
}

/// Publish a JSON event to NATS
#[cfg(feature = "events")]
pub async fn publish_json<T: serde::Serialize>(
    client: &Client,
    subject: &str,
    payload: &T,
) -> Result<()> {
    let json = serde_json::to_vec(payload)
        .map_err(|e| Error::Internal(format!("Failed to serialize event: {}", e)))?;

    publish_event(client, subject, json).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nats_config() {
        let config = NatsConfig {
            url: "nats://localhost:4222".to_string(),
            name: Some("test-service".to_string()),
            max_reconnects: 10,
            max_retries: 5,
            retry_delay_secs: 2,
            optional: false,
            lazy_init: true,
        };

        assert_eq!(config.max_reconnects, 10);
        assert_eq!(config.name, Some("test-service".to_string()));
        assert_eq!(config.max_retries, 5);
        assert!(config.lazy_init);
    }
}
