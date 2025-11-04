pub mod config;
pub mod grpc_client;
pub mod handlers;
pub mod models;
pub mod services;

pub use config::Config;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
}
