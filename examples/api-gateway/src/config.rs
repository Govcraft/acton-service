use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub service: ServiceConfig,
    pub jwt: JwtConfig,
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
pub struct JwtConfig {
    pub public_key_path: String,
    pub algorithm: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    pub per_user_rpm: u32,
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

    fn validate(&self) -> Result<(), Box<figment::Error>> {
        // Validate service name is not empty
        if self.service.name.is_empty() {
            return Err(Box::new(figment::Error::from("service.name cannot be empty")));
        }

        // Validate port is in valid range
        if self.service.port == 0 {
            return Err(Box::new(figment::Error::from("service.port must be greater than 0")));
        }

        // Validate JWT algorithm
        let valid_algorithms = ["RS256", "RS384", "RS512", "ES256", "ES384"];
        if !valid_algorithms.contains(&self.jwt.algorithm.as_str()) {
            return Err(Box::new(figment::Error::from(format!(
                "jwt.algorithm must be one of: {}",
                valid_algorithms.join(", ")
            ))));
        }

        // Validate rate limits
        if self.rate_limit.per_user_rpm == 0 {
            return Err(Box::new(figment::Error::from(
                "rate_limit.per_user_rpm must be greater than 0",
            )));
        }
        if self.rate_limit.per_client_rpm == 0 {
            return Err(Box::new(figment::Error::from(
                "rate_limit.per_client_rpm must be greater than 0",
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        let config = Config {
            service: ServiceConfig {
                name: "api-gateway".to_string(),
                port: 8081,
                log_level: "info".to_string(),
            },
            jwt: JwtConfig {
                public_key_path: "./keys/jwt-public.pem".to_string(),
                algorithm: "RS256".to_string(),
            },
            rate_limit: RateLimitConfig {
                per_user_rpm: 200,
                per_client_rpm: 1000,
            },
            otlp: OtlpConfig {
                endpoint: "http://localhost:4317".to_string(),
            },
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_algorithm() {
        let config = Config {
            service: ServiceConfig {
                name: "api-gateway".to_string(),
                port: 8081,
                log_level: "info".to_string(),
            },
            jwt: JwtConfig {
                public_key_path: "./keys/jwt-public.pem".to_string(),
                algorithm: "INVALID".to_string(),
            },
            rate_limit: RateLimitConfig {
                per_user_rpm: 200,
                per_client_rpm: 1000,
            },
            otlp: OtlpConfig {
                endpoint: "http://localhost:4317".to_string(),
            },
        };

        assert!(config.validate().is_err());
    }
}
