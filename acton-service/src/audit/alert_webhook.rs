//! Webhook-based audit alert hook
//!
//! Posts JSON-serialized [`AuditAlertEvent`]s to a configured HTTP endpoint.
//! Errors are logged and dropped — no retries — to prevent cascading failures
//! in the alert system itself.

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

use super::alert::{AuditAlertEvent, AuditAlertHook};

/// Webhook alert hook that POSTs audit alert events as JSON
pub struct WebhookAlertHook {
    client: reqwest::Client,
    url: String,
    headers: HashMap<String, String>,
}

impl WebhookAlertHook {
    /// Create a new webhook alert hook
    ///
    /// # Arguments
    ///
    /// * `url` — Destination URL for POST requests
    /// * `timeout` — HTTP request timeout
    /// * `headers` — Additional headers to include (e.g., `Authorization`)
    pub fn new(url: String, timeout: Duration, headers: HashMap<String, String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_default();
        Self {
            client,
            url,
            headers,
        }
    }
}

#[async_trait]
impl AuditAlertHook for WebhookAlertHook {
    async fn on_alert(&self, event: AuditAlertEvent) {
        let mut request = self.client.post(&self.url).json(&event);
        for (key, value) in &self.headers {
            request = request.header(key.as_str(), value.as_str());
        }

        match request.send().await {
            Ok(response) => {
                if !response.status().is_success() {
                    tracing::warn!(
                        url = %self.url,
                        status = %response.status(),
                        "Audit alert webhook returned non-success status"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    url = %self.url,
                    error = %e,
                    "Failed to send audit alert webhook"
                );
            }
        }
    }
}
