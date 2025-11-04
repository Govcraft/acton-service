pub mod config;
pub mod grpc_service;
pub mod handlers;
pub mod models;
pub mod services;

pub use config::Config;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub service_name: String,
    // TODO: Add your connection pools here when needed
    // pub db: PgPool,
    // pub redis: RedisPool,
    // pub nats: NatsClient,
}
