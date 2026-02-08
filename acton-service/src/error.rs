//! Error types and HTTP response conversion

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

// ============================================================================
// Structured Database Errors
// ============================================================================

/// Database operation being performed when the error occurred
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
pub enum DatabaseOperation {
    /// Establishing a database connection
    Connect,
    /// Executing a query
    Query,
    /// Inserting records
    Insert,
    /// Updating records
    Update,
    /// Deleting records
    Delete,
    /// Transaction operations (begin, commit, rollback)
    Transaction,
    /// Syncing data (e.g., Turso embedded replica sync)
    Sync,
    /// Running database migrations
    Migration,
    /// Acquiring a connection from the pool
    PoolAcquire,
}

#[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
impl fmt::Display for DatabaseOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect => write!(f, "connect"),
            Self::Query => write!(f, "query"),
            Self::Insert => write!(f, "insert"),
            Self::Update => write!(f, "update"),
            Self::Delete => write!(f, "delete"),
            Self::Transaction => write!(f, "transaction"),
            Self::Sync => write!(f, "sync"),
            Self::Migration => write!(f, "migration"),
            Self::PoolAcquire => write!(f, "pool_acquire"),
        }
    }
}

/// Category of database error
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
pub enum DatabaseErrorKind {
    /// Failed to establish connection
    ConnectionFailed,
    /// Record not found
    NotFound,
    /// Constraint violation (unique, foreign key, check)
    ConstraintViolation,
    /// Query execution failed
    QueryFailed,
    /// Transaction failed (begin, commit, or rollback)
    TransactionFailed,
    /// Type conversion error
    TypeConversion,
    /// Sync operation failed (Turso specific)
    SyncFailed,
    /// Configuration error
    Configuration,
    /// Operation timed out
    Timeout,
    /// Permission denied
    PermissionDenied,
    /// Connection pool exhausted
    PoolExhausted,
    /// Other/unknown error
    Other,
}

#[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
impl fmt::Display for DatabaseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionFailed => write!(f, "connection_failed"),
            Self::NotFound => write!(f, "not_found"),
            Self::ConstraintViolation => write!(f, "constraint_violation"),
            Self::QueryFailed => write!(f, "query_failed"),
            Self::TransactionFailed => write!(f, "transaction_failed"),
            Self::TypeConversion => write!(f, "type_conversion"),
            Self::SyncFailed => write!(f, "sync_failed"),
            Self::Configuration => write!(f, "configuration"),
            Self::Timeout => write!(f, "timeout"),
            Self::PermissionDenied => write!(f, "permission_denied"),
            Self::PoolExhausted => write!(f, "pool_exhausted"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Structured database error with operation context
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
pub struct DatabaseError {
    /// The operation being performed when the error occurred
    pub operation: DatabaseOperation,
    /// The category of error
    pub kind: DatabaseErrorKind,
    /// Human-readable error message
    pub message: String,
    /// Additional context (e.g., table name, query fragment)
    pub context: Option<String>,
}

#[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
impl DatabaseError {
    /// Create a new database error
    pub fn new(
        operation: DatabaseOperation,
        kind: DatabaseErrorKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            kind,
            message: message.into(),
            context: None,
        }
    }

    /// Create a new database error with context
    pub fn with_context(
        operation: DatabaseOperation,
        kind: DatabaseErrorKind,
        message: impl Into<String>,
        context: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            kind,
            message: message.into(),
            context: Some(context.into()),
        }
    }

    /// Create a "not found" error
    pub fn not_found(operation: DatabaseOperation, message: impl Into<String>) -> Self {
        Self::new(operation, DatabaseErrorKind::NotFound, message)
    }

    /// Create a connection failed error
    pub fn connection_failed(message: impl Into<String>) -> Self {
        Self::new(
            DatabaseOperation::Connect,
            DatabaseErrorKind::ConnectionFailed,
            message,
        )
    }

    /// Create a constraint violation error
    pub fn constraint_violation(operation: DatabaseOperation, message: impl Into<String>) -> Self {
        Self::new(operation, DatabaseErrorKind::ConstraintViolation, message)
    }

    /// Create a query failed error
    pub fn query_failed(message: impl Into<String>) -> Self {
        Self::new(
            DatabaseOperation::Query,
            DatabaseErrorKind::QueryFailed,
            message,
        )
    }

    /// Create a timeout error
    pub fn timeout(operation: DatabaseOperation, message: impl Into<String>) -> Self {
        Self::new(operation, DatabaseErrorKind::Timeout, message)
    }

    /// Create a pool exhausted error
    pub fn pool_exhausted(message: impl Into<String>) -> Self {
        Self::new(
            DatabaseOperation::PoolAcquire,
            DatabaseErrorKind::PoolExhausted,
            message,
        )
    }

    /// Create a transaction failed error
    pub fn transaction_failed(message: impl Into<String>) -> Self {
        Self::new(
            DatabaseOperation::Transaction,
            DatabaseErrorKind::TransactionFailed,
            message,
        )
    }

    /// Create a sync failed error (Turso specific)
    pub fn sync_failed(message: impl Into<String>) -> Self {
        Self::new(
            DatabaseOperation::Sync,
            DatabaseErrorKind::SyncFailed,
            message,
        )
    }

    /// Check if this error is retriable (transient errors that may succeed on retry)
    pub fn is_retriable(&self) -> bool {
        matches!(
            self.kind,
            DatabaseErrorKind::ConnectionFailed
                | DatabaseErrorKind::Timeout
                | DatabaseErrorKind::PoolExhausted
                | DatabaseErrorKind::SyncFailed
        )
    }

    /// Add context to an existing error
    pub fn add_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

#[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Database {} error during {}: {}",
            self.kind, self.operation, self.message
        )?;
        if let Some(ref ctx) = self.context {
            write!(f, " [context: {}]", ctx)?;
        }
        Ok(())
    }
}

#[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
impl std::error::Error for DatabaseError {}

/// Sanitize a database URL by removing credentials
#[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
pub fn sanitize_url(url: &str) -> String {
    // Handle standard database URLs like postgres://user:pass@host/db
    if let Some(at_pos) = url.find('@') {
        if let Some(scheme_end) = url.find("://") {
            let scheme = &url[..scheme_end + 3];
            let after_at = &url[at_pos + 1..];
            return format!("{}<redacted>@{}", scheme, after_at);
        }
    }
    // Handle Turso URLs like libsql://db-org.turso.io?authToken=xxx
    if url.contains("authToken=") || url.contains("auth_token=") {
        let base = url.split('?').next().unwrap_or(url);
        return format!("{}?<credentials redacted>", base);
    }
    url.to_string()
}

/// Result type alias using the framework error
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for the framework
///
/// Large error variants are boxed to reduce stack size
#[derive(Debug, Error)]
pub enum Error {
    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(Box<figment::Error>),

    /// Structured database error with operation context
    #[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
    #[error("{0}")]
    Database(DatabaseError),

    /// Redis error
    #[cfg(feature = "cache")]
    #[error("Redis error: {0}")]
    Redis(Box<redis::RedisError>),

    /// NATS error
    #[cfg(feature = "events")]
    #[error("NATS error: {0}")]
    Nats(String),

    /// PASETO error (default token format)
    #[error("PASETO error: {0}")]
    Paseto(String),

    /// Authentication error (password hashing, token generation, etc.)
    #[cfg(feature = "auth")]
    #[error("Auth error: {0}")]
    Auth(String),

    /// JWT error (requires `jwt` feature)
    #[cfg(feature = "jwt")]
    #[error("JWT error: {0}")]
    Jwt(Box<jsonwebtoken::errors::Error>),

    /// HTTP error
    #[error("HTTP error: {0}")]
    Http(Box<axum::http::Error>),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Authentication error
    #[error("Authentication failed: {0}")]
    Unauthorized(String),

    /// Authorization error
    #[error("Forbidden: {0}")]
    Forbidden(String),

    /// Resource not found
    #[error("Not found: {0}")]
    NotFound(String),

    /// Bad request
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    /// Resource conflict (409)
    #[error("Conflict: {0}")]
    Conflict(String),

    /// Validation error (422)
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// Not supported error (501)
    #[error("Not supported: {0}")]
    NotSupported(String),

    /// External service error (502)
    #[error("External service error: {0}")]
    External(String),

    /// Internal server error
    #[error("Internal server error: {0}")]
    Internal(String),

    /// Generic error
    #[error("{0}")]
    Other(String),

    /// Session error
    #[cfg(feature = "session")]
    #[error("Session error: {0}")]
    Session(String),

    /// Audit logging error
    #[cfg(feature = "audit")]
    #[error("Audit error: {0}")]
    Audit(String),
}

/// Error response body
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Error message
    pub error: String,

    /// Optional error code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// HTTP status code
    pub status: u16,
}

impl ErrorResponse {
    /// Create a new error response
    pub fn new(status: StatusCode, error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: None,
            status: status.as_u16(),
        }
    }

    /// Create error response with a code
    pub fn with_code(
        status: StatusCode,
        code: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            error: error.into(),
            code: Some(code.into()),
            status: status.as_u16(),
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, error_response) = match self {
            Error::Config(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorResponse::with_code(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "CONFIG_ERROR",
                    e.to_string(),
                ),
            ),

            #[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
            Error::Database(ref e) => {
                // Log with structured context
                tracing::error!(
                    operation = %e.operation,
                    kind = %e.kind,
                    context = ?e.context,
                    retriable = e.is_retriable(),
                    "Database error: {}", e.message
                );

                // Map error kind to HTTP status code
                let status = match e.kind {
                    DatabaseErrorKind::NotFound => StatusCode::NOT_FOUND,
                    DatabaseErrorKind::ConstraintViolation => StatusCode::CONFLICT,
                    DatabaseErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
                    DatabaseErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };

                // Generate error code from kind
                let code = format!("DATABASE_{}", e.kind.to_string().to_uppercase());

                // User-facing message (don't expose internal details)
                let user_message = match e.kind {
                    DatabaseErrorKind::NotFound => "Resource not found",
                    DatabaseErrorKind::ConstraintViolation => {
                        "Operation conflicts with existing data"
                    }
                    DatabaseErrorKind::Timeout => "Database operation timed out",
                    DatabaseErrorKind::PermissionDenied => "Database permission denied",
                    _ => "Database operation failed",
                };

                (status, ErrorResponse::with_code(status, code, user_message))
            }

            #[cfg(feature = "cache")]
            Error::Redis(e) => {
                tracing::error!("Redis error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::with_code(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "CACHE_ERROR",
                        "Cache operation failed",
                    ),
                )
            }

            #[cfg(feature = "events")]
            Error::Nats(e) => {
                tracing::error!("NATS error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::with_code(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "NATS_ERROR",
                        "Event system error",
                    ),
                )
            }

            Error::Paseto(msg) => (
                StatusCode::UNAUTHORIZED,
                ErrorResponse::with_code(StatusCode::UNAUTHORIZED, "INVALID_TOKEN", msg),
            ),

            #[cfg(feature = "auth")]
            Error::Auth(msg) => (
                StatusCode::UNAUTHORIZED,
                ErrorResponse::with_code(StatusCode::UNAUTHORIZED, "AUTH_ERROR", msg),
            ),

            #[cfg(feature = "jwt")]
            Error::Jwt(e) => (
                StatusCode::UNAUTHORIZED,
                ErrorResponse::with_code(StatusCode::UNAUTHORIZED, "INVALID_TOKEN", e.to_string()),
            ),

            Error::Http(e) => (
                StatusCode::BAD_REQUEST,
                ErrorResponse::with_code(StatusCode::BAD_REQUEST, "HTTP_ERROR", e.to_string()),
            ),

            Error::Io(e) => {
                tracing::error!("I/O error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::with_code(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "IO_ERROR",
                        "I/O operation failed",
                    ),
                )
            }

            Error::Unauthorized(msg) => (
                StatusCode::UNAUTHORIZED,
                ErrorResponse::with_code(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", msg),
            ),

            Error::Forbidden(msg) => (
                StatusCode::FORBIDDEN,
                ErrorResponse::with_code(StatusCode::FORBIDDEN, "FORBIDDEN", msg),
            ),

            Error::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                ErrorResponse::with_code(StatusCode::NOT_FOUND, "NOT_FOUND", msg),
            ),

            Error::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                ErrorResponse::with_code(StatusCode::BAD_REQUEST, "BAD_REQUEST", msg),
            ),

            Error::RateLimitExceeded => (
                StatusCode::TOO_MANY_REQUESTS,
                ErrorResponse::with_code(
                    StatusCode::TOO_MANY_REQUESTS,
                    "RATE_LIMIT_EXCEEDED",
                    "Too many requests",
                ),
            ),

            Error::Conflict(msg) => (
                StatusCode::CONFLICT,
                ErrorResponse::with_code(StatusCode::CONFLICT, "CONFLICT", msg),
            ),

            Error::ValidationError(msg) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorResponse::with_code(StatusCode::UNPROCESSABLE_ENTITY, "VALIDATION_ERROR", msg),
            ),

            Error::NotSupported(msg) => (
                StatusCode::NOT_IMPLEMENTED,
                ErrorResponse::with_code(StatusCode::NOT_IMPLEMENTED, "NOT_SUPPORTED", msg),
            ),

            Error::External(msg) => {
                tracing::error!("External service error: {}", msg);
                (
                    StatusCode::BAD_GATEWAY,
                    ErrorResponse::with_code(
                        StatusCode::BAD_GATEWAY,
                        "EXTERNAL_ERROR",
                        "External service unavailable",
                    ),
                )
            }

            Error::Internal(msg) => {
                tracing::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::with_code(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "INTERNAL_ERROR",
                        "Internal server error",
                    ),
                )
            }

            Error::Other(msg) => {
                tracing::error!("Unexpected error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "An unexpected error occurred",
                    ),
                )
            }

            #[cfg(feature = "session")]
            Error::Session(msg) => {
                tracing::error!("Session error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::with_code(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "SESSION_ERROR",
                        "Session operation failed",
                    ),
                )
            }

            #[cfg(feature = "audit")]
            Error::Audit(msg) => {
                tracing::error!("Audit error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::with_code(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "AUDIT_ERROR",
                        "Audit operation failed",
                    ),
                )
            }
        };

        (status, Json(error_response)).into_response()
    }
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

// Manual From implementations for boxed errors
impl From<figment::Error> for Error {
    fn from(err: figment::Error) -> Self {
        Error::Config(Box::new(err))
    }
}

// Conversion from sqlx::Error to DatabaseError
#[cfg(feature = "database")]
impl From<sqlx::Error> for DatabaseError {
    fn from(err: sqlx::Error) -> Self {
        use sqlx::Error as E;
        match err {
            E::RowNotFound => Self::not_found(DatabaseOperation::Query, "Row not found"),
            E::PoolTimedOut => Self::pool_exhausted("Connection pool timed out"),
            E::PoolClosed => Self::connection_failed("Connection pool is closed"),
            E::Protocol(msg) => Self::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::QueryFailed,
                msg,
            ),
            E::Configuration(e) => Self::new(
                DatabaseOperation::Connect,
                DatabaseErrorKind::Configuration,
                e.to_string(),
            ),
            E::Io(e) => Self::new(
                DatabaseOperation::Connect,
                DatabaseErrorKind::ConnectionFailed,
                e.to_string(),
            ),
            E::Tls(e) => Self::new(
                DatabaseOperation::Connect,
                DatabaseErrorKind::ConnectionFailed,
                format!("TLS error: {}", e),
            ),
            E::TypeNotFound { type_name } => Self::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::TypeConversion,
                format!("Type not found: {}", type_name),
            ),
            E::ColumnNotFound(col) => Self::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::QueryFailed,
                format!("Column not found: {}", col),
            ),
            E::ColumnIndexOutOfBounds { index, len } => Self::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::QueryFailed,
                format!("Column index {} out of bounds (len: {})", index, len),
            ),
            E::ColumnDecode { index, source } => Self::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::TypeConversion,
                format!("Failed to decode column {}: {}", index, source),
            ),
            E::Decode(e) => Self::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::TypeConversion,
                e.to_string(),
            ),
            E::AnyDriverError(e) => Self::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::QueryFailed,
                e.to_string(),
            ),
            E::Migrate(e) => Self::new(
                DatabaseOperation::Migration,
                DatabaseErrorKind::QueryFailed,
                e.to_string(),
            ),
            E::Database(db_err) => {
                // Parse database-specific errors - combine all constraint violations
                let kind = if db_err.is_unique_violation()
                    || db_err.is_foreign_key_violation()
                    || db_err.is_check_violation()
                {
                    DatabaseErrorKind::ConstraintViolation
                } else {
                    DatabaseErrorKind::QueryFailed
                };
                Self::new(DatabaseOperation::Query, kind, db_err.to_string())
            }
            E::WorkerCrashed => Self::connection_failed("Database worker crashed"),
            _ => Self::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::Other,
                err.to_string(),
            ),
        }
    }
}

#[cfg(feature = "database")]
impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        Error::Database(DatabaseError::from(err))
    }
}

// Conversion from DatabaseError to Error
#[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
impl From<DatabaseError> for Error {
    fn from(err: DatabaseError) -> Self {
        Error::Database(err)
    }
}

#[cfg(feature = "cache")]
impl From<redis::RedisError> for Error {
    fn from(err: redis::RedisError) -> Self {
        Error::Redis(Box::new(err))
    }
}

// Conversion from libsql::Error to DatabaseError
#[cfg(feature = "turso")]
impl From<libsql::Error> for DatabaseError {
    fn from(err: libsql::Error) -> Self {
        let msg = err.to_string();

        // Parse libsql error messages to determine kind and operation
        // Combine all constraint violations into a single branch
        let (kind, operation) = if msg.contains("UNIQUE constraint failed")
            || msg.contains("FOREIGN KEY constraint failed")
            || msg.contains("NOT NULL constraint failed")
            || msg.contains("CHECK constraint failed")
        {
            (
                DatabaseErrorKind::ConstraintViolation,
                DatabaseOperation::Insert,
            )
        } else if msg.contains("no such table") || msg.contains("no such column") {
            (DatabaseErrorKind::QueryFailed, DatabaseOperation::Query)
        } else if msg.contains("timeout") || msg.contains("timed out") {
            (DatabaseErrorKind::Timeout, DatabaseOperation::Query)
        } else if msg.contains("connection") || msg.contains("Connection") {
            (
                DatabaseErrorKind::ConnectionFailed,
                DatabaseOperation::Connect,
            )
        } else if msg.contains("permission denied") || msg.contains("Permission denied") {
            (
                DatabaseErrorKind::PermissionDenied,
                DatabaseOperation::Query,
            )
        } else if msg.contains("sync") || msg.contains("Sync") {
            (DatabaseErrorKind::SyncFailed, DatabaseOperation::Sync)
        } else {
            (DatabaseErrorKind::Other, DatabaseOperation::Query)
        };

        Self::new(operation, kind, msg)
    }
}

#[cfg(feature = "turso")]
impl From<libsql::Error> for Error {
    fn from(err: libsql::Error) -> Self {
        Error::Database(DatabaseError::from(err))
    }
}

// Conversion from surrealdb::Error to DatabaseError
#[cfg(feature = "surrealdb")]
impl From<surrealdb::Error> for DatabaseError {
    fn from(err: surrealdb::Error) -> Self {
        let msg = err.to_string();

        let (kind, operation) = if msg.contains("already exists")
            || msg.contains("unique")
            || msg.contains("duplicate")
        {
            (
                DatabaseErrorKind::ConstraintViolation,
                DatabaseOperation::Insert,
            )
        } else if msg.contains("not found") || msg.contains("no record") {
            (DatabaseErrorKind::NotFound, DatabaseOperation::Query)
        } else if msg.contains("timeout") || msg.contains("timed out") {
            (DatabaseErrorKind::Timeout, DatabaseOperation::Query)
        } else if msg.contains("connect") || msg.contains("Connection") {
            (
                DatabaseErrorKind::ConnectionFailed,
                DatabaseOperation::Connect,
            )
        } else if msg.contains("permission")
            || msg.contains("not allowed")
            || msg.contains("denied")
        {
            (
                DatabaseErrorKind::PermissionDenied,
                DatabaseOperation::Query,
            )
        } else if msg.contains("auth") || msg.contains("signin") || msg.contains("credentials") {
            (
                DatabaseErrorKind::ConnectionFailed,
                DatabaseOperation::Connect,
            )
        } else if msg.contains("parse") || msg.contains("syntax") {
            (DatabaseErrorKind::QueryFailed, DatabaseOperation::Query)
        } else {
            (DatabaseErrorKind::Other, DatabaseOperation::Query)
        };

        Self::new(operation, kind, msg)
    }
}

#[cfg(feature = "surrealdb")]
impl From<surrealdb::Error> for Error {
    fn from(err: surrealdb::Error) -> Self {
        Error::Database(DatabaseError::from(err))
    }
}

#[cfg(feature = "jwt")]
impl From<jsonwebtoken::errors::Error> for Error {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        Error::Jwt(Box::new(err))
    }
}

impl From<axum::http::Error> for Error {
    fn from(err: axum::http::Error) -> Self {
        Error::Http(Box::new(err))
    }
}

// Conversion from DatabaseError to RepositoryError
#[cfg(all(
    feature = "repository",
    any(feature = "database", feature = "turso", feature = "surrealdb")
))]
impl From<DatabaseError> for crate::repository::RepositoryError {
    fn from(err: DatabaseError) -> Self {
        use crate::repository::{RepositoryErrorKind, RepositoryOperation};

        let kind = match err.kind {
            DatabaseErrorKind::NotFound => RepositoryErrorKind::NotFound,
            DatabaseErrorKind::ConstraintViolation => RepositoryErrorKind::ConstraintViolation,
            DatabaseErrorKind::ConnectionFailed => RepositoryErrorKind::ConnectionFailed,
            DatabaseErrorKind::Timeout => RepositoryErrorKind::Timeout,
            DatabaseErrorKind::PoolExhausted => RepositoryErrorKind::ConnectionFailed,
            _ => RepositoryErrorKind::DatabaseError,
        };

        let operation = match err.operation {
            DatabaseOperation::Query => RepositoryOperation::FindAll,
            DatabaseOperation::Insert => RepositoryOperation::Create,
            DatabaseOperation::Update => RepositoryOperation::Update,
            DatabaseOperation::Delete => RepositoryOperation::Delete,
            _ => RepositoryOperation::FindById,
        };

        Self {
            operation,
            kind,
            message: err.message,
            entity_type: None,
            entity_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_response() {
        let err = ErrorResponse::new(StatusCode::NOT_FOUND, "User not found");
        assert_eq!(err.status, 404);
        assert_eq!(err.error, "User not found");
        assert!(err.code.is_none());
    }

    #[test]
    fn test_error_response_with_code() {
        let err = ErrorResponse::with_code(
            StatusCode::BAD_REQUEST,
            "INVALID_EMAIL",
            "Email format is invalid",
        );
        assert_eq!(err.status, 400);
        assert_eq!(err.error, "Email format is invalid");
        assert_eq!(err.code, Some("INVALID_EMAIL".to_string()));
    }

    // =========================================================================
    // DatabaseError Tests
    // =========================================================================

    #[cfg(any(feature = "database", feature = "turso", feature = "surrealdb"))]
    mod database_error_tests {
        use super::*;

        #[test]
        fn test_database_error_new() {
            let err = DatabaseError::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::QueryFailed,
                "Query failed",
            );
            assert_eq!(err.operation, DatabaseOperation::Query);
            assert_eq!(err.kind, DatabaseErrorKind::QueryFailed);
            assert_eq!(err.message, "Query failed");
            assert!(err.context.is_none());
        }

        #[test]
        fn test_database_error_with_context() {
            let err = DatabaseError::with_context(
                DatabaseOperation::Insert,
                DatabaseErrorKind::ConstraintViolation,
                "Unique constraint violated",
                "users.email",
            );
            assert_eq!(err.operation, DatabaseOperation::Insert);
            assert_eq!(err.kind, DatabaseErrorKind::ConstraintViolation);
            assert_eq!(err.message, "Unique constraint violated");
            assert_eq!(err.context, Some("users.email".to_string()));
        }

        #[test]
        fn test_not_found_convenience() {
            let err = DatabaseError::not_found(DatabaseOperation::Query, "User not found");
            assert_eq!(err.operation, DatabaseOperation::Query);
            assert_eq!(err.kind, DatabaseErrorKind::NotFound);
            assert_eq!(err.message, "User not found");
        }

        #[test]
        fn test_connection_failed_convenience() {
            let err = DatabaseError::connection_failed("Connection refused");
            assert_eq!(err.operation, DatabaseOperation::Connect);
            assert_eq!(err.kind, DatabaseErrorKind::ConnectionFailed);
            assert_eq!(err.message, "Connection refused");
        }

        #[test]
        fn test_constraint_violation_convenience() {
            let err =
                DatabaseError::constraint_violation(DatabaseOperation::Update, "FK constraint");
            assert_eq!(err.operation, DatabaseOperation::Update);
            assert_eq!(err.kind, DatabaseErrorKind::ConstraintViolation);
        }

        #[test]
        fn test_query_failed_convenience() {
            let err = DatabaseError::query_failed("Syntax error");
            assert_eq!(err.operation, DatabaseOperation::Query);
            assert_eq!(err.kind, DatabaseErrorKind::QueryFailed);
        }

        #[test]
        fn test_timeout_convenience() {
            let err = DatabaseError::timeout(DatabaseOperation::Query, "Query timed out");
            assert_eq!(err.operation, DatabaseOperation::Query);
            assert_eq!(err.kind, DatabaseErrorKind::Timeout);
        }

        #[test]
        fn test_pool_exhausted_convenience() {
            let err = DatabaseError::pool_exhausted("No connections available");
            assert_eq!(err.operation, DatabaseOperation::PoolAcquire);
            assert_eq!(err.kind, DatabaseErrorKind::PoolExhausted);
        }

        #[test]
        fn test_transaction_failed_convenience() {
            let err = DatabaseError::transaction_failed("Commit failed");
            assert_eq!(err.operation, DatabaseOperation::Transaction);
            assert_eq!(err.kind, DatabaseErrorKind::TransactionFailed);
        }

        #[test]
        fn test_sync_failed_convenience() {
            let err = DatabaseError::sync_failed("Sync with remote failed");
            assert_eq!(err.operation, DatabaseOperation::Sync);
            assert_eq!(err.kind, DatabaseErrorKind::SyncFailed);
        }

        #[test]
        fn test_is_retriable_transient_errors() {
            // Transient errors should be retriable
            assert!(DatabaseError::connection_failed("refused").is_retriable());
            assert!(DatabaseError::timeout(DatabaseOperation::Query, "timeout").is_retriable());
            assert!(DatabaseError::pool_exhausted("exhausted").is_retriable());
            assert!(DatabaseError::sync_failed("sync error").is_retriable());
        }

        #[test]
        fn test_is_retriable_permanent_errors() {
            // Permanent errors should not be retriable
            assert!(
                !DatabaseError::not_found(DatabaseOperation::Query, "not found").is_retriable()
            );
            assert!(
                !DatabaseError::constraint_violation(DatabaseOperation::Insert, "unique")
                    .is_retriable()
            );
            assert!(!DatabaseError::query_failed("syntax error").is_retriable());
            assert!(!DatabaseError::transaction_failed("rollback").is_retriable());
            assert!(!DatabaseError::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::PermissionDenied,
                "denied"
            )
            .is_retriable());
            assert!(!DatabaseError::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::TypeConversion,
                "type error"
            )
            .is_retriable());
            assert!(!DatabaseError::new(
                DatabaseOperation::Connect,
                DatabaseErrorKind::Configuration,
                "bad config"
            )
            .is_retriable());
        }

        #[test]
        fn test_add_context() {
            let err =
                DatabaseError::query_failed("Query failed").add_context("SELECT * FROM users");
            assert_eq!(err.context, Some("SELECT * FROM users".to_string()));
        }

        #[test]
        fn test_display_formatting() {
            let err = DatabaseError::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::QueryFailed,
                "Syntax error near 'FROM'",
            );
            let display = format!("{}", err);
            assert!(display.contains("query_failed"));
            assert!(display.contains("query"));
            assert!(display.contains("Syntax error near 'FROM'"));
        }

        #[test]
        fn test_display_formatting_with_context() {
            let err = DatabaseError::with_context(
                DatabaseOperation::Insert,
                DatabaseErrorKind::ConstraintViolation,
                "Unique constraint violated",
                "users.email",
            );
            let display = format!("{}", err);
            assert!(display.contains("[context: users.email]"));
        }

        #[test]
        fn test_database_operation_display() {
            assert_eq!(format!("{}", DatabaseOperation::Connect), "connect");
            assert_eq!(format!("{}", DatabaseOperation::Query), "query");
            assert_eq!(format!("{}", DatabaseOperation::Insert), "insert");
            assert_eq!(format!("{}", DatabaseOperation::Update), "update");
            assert_eq!(format!("{}", DatabaseOperation::Delete), "delete");
            assert_eq!(format!("{}", DatabaseOperation::Transaction), "transaction");
            assert_eq!(format!("{}", DatabaseOperation::Sync), "sync");
            assert_eq!(format!("{}", DatabaseOperation::Migration), "migration");
            assert_eq!(
                format!("{}", DatabaseOperation::PoolAcquire),
                "pool_acquire"
            );
        }

        #[test]
        fn test_database_error_kind_display() {
            assert_eq!(
                format!("{}", DatabaseErrorKind::ConnectionFailed),
                "connection_failed"
            );
            assert_eq!(format!("{}", DatabaseErrorKind::NotFound), "not_found");
            assert_eq!(
                format!("{}", DatabaseErrorKind::ConstraintViolation),
                "constraint_violation"
            );
            assert_eq!(
                format!("{}", DatabaseErrorKind::QueryFailed),
                "query_failed"
            );
            assert_eq!(
                format!("{}", DatabaseErrorKind::TransactionFailed),
                "transaction_failed"
            );
            assert_eq!(
                format!("{}", DatabaseErrorKind::TypeConversion),
                "type_conversion"
            );
            assert_eq!(format!("{}", DatabaseErrorKind::SyncFailed), "sync_failed");
            assert_eq!(
                format!("{}", DatabaseErrorKind::Configuration),
                "configuration"
            );
            assert_eq!(format!("{}", DatabaseErrorKind::Timeout), "timeout");
            assert_eq!(
                format!("{}", DatabaseErrorKind::PermissionDenied),
                "permission_denied"
            );
            assert_eq!(
                format!("{}", DatabaseErrorKind::PoolExhausted),
                "pool_exhausted"
            );
            assert_eq!(format!("{}", DatabaseErrorKind::Other), "other");
        }

        #[test]
        fn test_sanitize_url_postgres() {
            let url = "postgres://admin:secret123@localhost:5432/mydb";
            let sanitized = sanitize_url(url);
            assert_eq!(sanitized, "postgres://<redacted>@localhost:5432/mydb");
            assert!(!sanitized.contains("admin"));
            assert!(!sanitized.contains("secret123"));
        }

        #[test]
        fn test_sanitize_url_turso() {
            let url = "libsql://my-db-org.turso.io?authToken=eyJ0eXAi";
            let sanitized = sanitize_url(url);
            assert_eq!(
                sanitized,
                "libsql://my-db-org.turso.io?<credentials redacted>"
            );
            assert!(!sanitized.contains("eyJ0eXAi"));
        }

        #[test]
        fn test_sanitize_url_no_credentials() {
            let url = "libsql://localhost:8080";
            let sanitized = sanitize_url(url);
            assert_eq!(sanitized, "libsql://localhost:8080");
        }

        #[test]
        fn test_database_error_equality() {
            let err1 = DatabaseError::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::NotFound,
                "Not found",
            );
            let err2 = DatabaseError::new(
                DatabaseOperation::Query,
                DatabaseErrorKind::NotFound,
                "Not found",
            );
            assert_eq!(err1, err2);
        }

        #[test]
        fn test_database_error_clone() {
            let err = DatabaseError::with_context(
                DatabaseOperation::Insert,
                DatabaseErrorKind::ConstraintViolation,
                "Duplicate key",
                "users.id",
            );
            let cloned = err.clone();
            assert_eq!(err, cloned);
        }
    }
}
