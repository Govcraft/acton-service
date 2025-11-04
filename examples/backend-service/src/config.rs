use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub service: ServiceConfig,
    pub rate_limit: RateLimitConfig,
    pub otlp: OtlpConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceConfig {
    pub name: String,
    pub port: u16,
    pub log_level: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    pub per_client_rpm: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OtlpConfig {
    pub endpoint: String,
}

impl Config {
    pub fn load() -> Result<Self, Box<figment::Error>> {
        let config: Config = Figment::new()
            .merge(Toml::file("config.toml"))
            .merge(Env::prefixed("ACTON_"))
            .extract()
            .map_err(Box::new)?;

        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), Box<figment::Error>> {
        // Validate service name is not empty
        if self.service.name.is_empty() {
            return Err(Box::new(figment::Error::from(
                "service.name cannot be empty".to_string(),
            )));
        }

        // Validate port is in valid range
        if self.service.port == 0 {
            return Err(Box::new(figment::Error::from("service.port must be greater than 0")));
        }

        // Validate log level is valid
        let valid_log_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_log_levels.contains(&self.service.log_level.as_str()) {
            return Err(Box::new(figment::Error::from(format!(
                "service.log_level must be one of: {}",
                valid_log_levels.join(", ")
            ))));
        }

        // Validate rate limit is reasonable
        if self.rate_limit.per_client_rpm == 0 {
            return Err(Box::new(figment::Error::from(
                "rate_limit.per_client_rpm must be greater than 0",
            )));
        }

        Ok(())
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs(30)
    }
}
