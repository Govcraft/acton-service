//! Immutable audit logging with SIEM export
//!
//! Provides tamper-evident audit trails using BLAKE3 hash chaining,
//! with export to Syslog (RFC 5424) and optionally OpenTelemetry Logs.
//!
//! # Architecture
//!
//! An acton-reactive actor (`AuditAgent`) processes all audit events sequentially,
//! guaranteeing correct hash chain ordering. Middleware and auth integrations send
//! events via fire-and-forget message passing, so audit logging never blocks
//! request handling.
//!
//! # Feature Interactions
//!
//! - `audit` alone: In-memory audit chain + syslog export
//! - `audit` + `database`/`turso`/`surrealdb`: Persistent append-only storage
//! - `audit` + `observability`: OTLP log export
//! - `audit` + `auth`: Automatic auth event emission

pub mod config;
pub mod event;
pub mod chain;
pub mod storage;
pub mod agent;
pub mod logger;
pub mod middleware;
pub mod syslog;

#[cfg(feature = "observability")]
pub mod otlp;

pub use config::{AuditConfig, SyslogConfig};
pub use event::{AuditEvent, AuditEventKind, AuditSeverity, AuditSource};
pub use chain::{AuditChain, ChainVerificationError, verify_chain};
pub use storage::AuditStorage;
pub use agent::AuditAgent;
pub use logger::AuditLogger;
pub use middleware::{audit_layer, AuditRoute};
