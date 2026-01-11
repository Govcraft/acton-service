//! OAuth provider implementations
//!
//! Built-in support for Google, GitHub, and custom OIDC providers.

pub mod google;
pub mod github;
pub mod custom;

pub use google::GoogleProvider;
pub use github::GitHubProvider;
pub use custom::{CustomOidcProvider, CustomOidcConfig};
