pub mod cargo_toml;
pub mod config;
pub mod deployment;
pub mod handlers;
pub mod service;
pub mod worker;

use chrono::Datelike;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Template data for service generation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ServiceTemplate {
    pub name: String,
    pub pascal_name: String,
    pub snake_name: String,
    pub http: bool,
    pub grpc: bool,
    pub database: Option<String>,
    pub cache: Option<String>,
    pub events: Option<String>,
    pub auth: Option<String>,
    pub observability: bool,
    pub resilience: bool,
    pub rate_limit: bool,
    pub openapi: bool,
    pub audit: bool,
}

impl ServiceTemplate {
    #[allow(dead_code)]
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "name": self.name,
            "pascal_name": self.pascal_name,
            "snake_name": self.snake_name,
            "http": self.http,
            "grpc": self.grpc,
            "has_database": self.database.is_some(),
            "database": self.database,
            "has_cache": self.cache.is_some(),
            "cache": self.cache,
            "has_events": self.events.is_some(),
            "events": self.events,
            "has_auth": self.auth.is_some(),
            "auth": self.auth,
            "observability": self.observability,
            "resilience": self.resilience,
            "rate_limit": self.rate_limit,
            "openapi": self.openapi,
            "audit": self.audit,
            "year": chrono::Utc::now().year(),
        })
    }

    pub fn features(&self) -> Vec<String> {
        let mut features = vec![];

        if self.http {
            features.push("http".to_string());
        }

        if self.grpc {
            features.push("grpc".to_string());
        }

        if self.database.is_some() {
            features.push("database".to_string());
        }

        if self.cache.is_some() {
            features.push("cache".to_string());
        }

        if self.events.is_some() {
            features.push("events".to_string());
        }

        if self.observability {
            features.push("observability".to_string());
        }

        if self.resilience {
            features.push("resilience".to_string());
        }

        if self.rate_limit {
            features.push("rate-limit".to_string());
        }

        if self.audit {
            features.push("audit".to_string());
        }

        features
    }
}

impl ServiceTemplate {
    /// Calculate acton-service path from CLI manifest directory
    pub fn acton_service_path(&self) -> Option<String> {
        let cli_manifest_dir = env!("CARGO_MANIFEST_DIR");
        if cli_manifest_dir.ends_with("/acton-cli") {
            let workspace_root = cli_manifest_dir.strip_suffix("/acton-cli")?;
            Some(format!("{}/acton-service", workspace_root))
        } else {
            None
        }
    }
}
