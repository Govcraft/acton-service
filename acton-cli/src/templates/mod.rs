pub mod service;
pub mod cargo_toml;
pub mod config;
pub mod handlers;
pub mod deployment;
pub mod worker;

use handlebars::Handlebars;
use serde_json::json;
use chrono::Datelike;

/// Template data for service generation
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

        features
    }
}

/// Get Handlebars renderer with all templates registered
#[allow(dead_code)]
pub fn get_renderer() -> Handlebars<'static> {
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    handlebars
}
