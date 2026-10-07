//! Driver-independent audit events, BLAKE3 chains and storage contracts.
pub mod chain;
pub mod config;
pub mod event;
pub mod id;
pub mod storage;
pub use chain::{verify_chain, AuditChain, ChainVerificationError};
pub use event::{AuditEvent, AuditEventKind, AuditSeverity, AuditSource};
pub use id::{AuditEventId, AuditEventIdError};
