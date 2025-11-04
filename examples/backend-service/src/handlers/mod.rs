// Handler modules
pub mod health;
pub mod users;

// Re-export handlers for convenience
pub use health::{health, readiness};
pub use users::{create_user, get_user, list_users};
