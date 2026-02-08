//! OAuth provider implementations
//!
//! Built-in support for Google, GitHub, and custom OIDC providers.

pub mod custom;
pub mod github;
pub mod google;

pub use custom::{CustomOidcConfig, CustomOidcProvider};
pub use github::GitHubProvider;
pub use google::GoogleProvider;
