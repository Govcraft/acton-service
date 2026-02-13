//! Automated signing key rotation (NIST SC-12)
//!
//! Provides lifecycle management for cryptographic signing keys used by
//! PASETO and JWT token generators. Keys progress through three states:
//!
//! ```text
//! Active --> Draining --> Retired
//! ```
//!
//! - **Active**: Signs new tokens AND validates existing tokens. One per service.
//! - **Draining**: No longer signs. Still validates during the drain window.
//! - **Retired**: Metadata retained for audit trail only.
//!
//! # Feature Interactions
//!
//! - `auth` alone: Key rotation types and configuration available
//! - `auth` + `database`: PostgreSQL key storage backend
//! - `auth` + `turso`: Turso/libsql key storage backend
//! - `auth` + `surrealdb`: SurrealDB key storage backend
//!
//! # Backward Compatibility
//!
//! When `KeyRotationConfig` is absent or `enabled = false`, static key behavior
//! is unchanged. Tokens issued before key rotation was enabled (no `kid`) still
//! validate via the static key fallback.

pub mod agent;
pub mod config;
pub mod key_metadata;
pub mod manager;
pub mod storage;

pub use config::KeyRotationConfig;
pub use key_metadata::{
    KeyFormat, KeyStatus, ParseKeyFormatError, ParseKeyStatusError, SigningKeyMetadata,
};
pub use agent::{CheckRotation, ForceRotation, KeyRotationAgent};
pub use manager::{CachedKey, KeyManager};
pub use storage::KeyRotationStorage;

#[cfg(feature = "database")]
pub use storage::pg::PgKeyRotationStorage;

#[cfg(feature = "turso")]
pub use storage::turso::TursoKeyRotationStorage;

#[cfg(feature = "surrealdb")]
pub use storage::surrealdb_impl::SurrealKeyRotationStorage;
